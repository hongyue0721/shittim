//! File-backed SQLite persistence base for the Kernel.
//!
//! This crate owns migrations, atomic Event Outbox allocation (v2-only), publisher storage
//! operations, transaction-bound policy rate-limit consumption, strict Task/TaskScope/
//! ContentOrigin(v2) reads, and the active root TaskCreate v2 repository. Legacy v1 TaskCreate
//! write, AuditRecord v1 write, and Outbox v1 append were deleted under ADR-0009. It does not
//! implement Task update/list, Action/PermissionDecision repositories, KCP, `agentd`, networking,
//! or a Publisher loop.

#![deny(missing_docs)]

mod config;
mod error;
mod migration;
mod outbox;
mod rate_limit;
mod root_task_create_v2;
mod task;

pub use config::SqliteConfig;
pub use error::{StoreError, StoreErrorCode};
pub use outbox::{
    EventAggregateId, MarkDeliveredResult, OutboxCursor, OutboxPosition, OutboxRecord, PageLimit,
    PendingActiveEventV2, StoredEventEnvelope,
};
pub use rate_limit::TransactionRateLimitPort;
pub use root_task_create_v2::{
    CreateRootTaskV2Result, RootTaskCreateV2Command, RootTaskCreateV2EnvelopeFacts,
};

use chrono::{DateTime, Utc};
use kernel_contracts::{
    AuditRecordV2, ContentOriginV2, TaskCreationProvenanceV1, TaskScope, TaskSpec,
};
use rusqlite::Connection;
use std::cell::Cell;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Thread-safe handle to one file-backed SQLite database.
#[derive(Debug)]
pub struct SqliteStore {
    connection: Mutex<Connection>,
    healthy: AtomicBool,
    #[cfg(test)]
    outer_rollback_failure: AtomicBool,
}

impl SqliteStore {
    /// Opens a file database, configures WAL/foreign keys/busy timeout, and applies migrations.
    ///
    /// WAL journal-mode setup and migration ledger bootstrap are infrastructure initialization,
    /// not public business write APIs under ADR-0004's `BEGIN IMMEDIATE` business-write surface.
    /// Pending migration application still uses its own `BEGIN IMMEDIATE` unit.
    pub fn open(path: impl AsRef<Path>, config: SqliteConfig) -> Result<Self, StoreError> {
        let path = config::validated_path(path.as_ref())?;
        let connection = config::open_connection(&path, config)?;
        // Non-business write exception: connection/journal bootstrap before any business API.
        config::initialize_wal(&connection, config.busy_timeout)?;
        // Non-business write exception: schema bootstrap + pending migration units.
        migration::apply_migrations(&connection)?;
        // ADR-0009: refuse any remaining v1 business facts after migrations.
        migration::reject_legacy_v1_business_data(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            healthy: AtomicBool::new(true),
            #[cfg(test)]
            outer_rollback_failure: AtomicBool::new(false),
        })
    }

    /// Runs a closure inside `BEGIN IMMEDIATE`; success commits and error or panic rolls back.
    ///
    /// This is the sole public business write entry for multi-statement work. Store convenience
    /// writers such as [`Self::mark_delivered`] also delegate here so callers never observe a
    /// committed business side effect without a successful `COMMIT`.
    ///
    /// The closure must contain database work only. External calls must happen after commit.
    pub fn with_write_transaction<T>(
        &self,
        operation: impl FnOnce(&WriteTransaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let transaction = WriteTransaction {
                connection: &connection,
                poisoned: Cell::new(false),
                #[cfg(test)]
                savepoint_failure: Cell::new(None),
                #[cfg(test)]
                force_root_v2_post_append_bundle_invalid: Cell::new(false),
            };
            let outcome = operation(&transaction);
            if transaction.poisoned.get() {
                Err(StoreError::new(
                    StoreErrorCode::InternalStoreError,
                    "transaction is poisoned after savepoint cleanup failure",
                ))
            } else {
                outcome
            }
        }));

        match result {
            Ok(Ok(value)) => match connection.execute_batch("COMMIT") {
                Ok(()) => Ok(value),
                Err(error) => {
                    let commit_error =
                        StoreError::sqlite(error, StoreErrorCode::InternalStoreError);
                    if connection.execute_batch("ROLLBACK").is_err() {
                        self.mark_unhealthy();
                    }
                    Err(commit_error)
                }
            },
            Ok(Err(error)) => {
                let rollback_failed = connection.execute_batch("ROLLBACK").is_err()
                    || self.consume_outer_rollback_failure_for_test();
                if rollback_failed {
                    self.mark_unhealthy();
                    return Err(StoreError::new(
                        StoreErrorCode::InternalStoreError,
                        format!(
                            "transaction failed with {} and rollback also failed",
                            error.code.as_str()
                        ),
                    ));
                }
                Err(error)
            }
            Err(payload) => {
                let rollback_failed = connection.execute_batch("ROLLBACK").is_err()
                    || self.consume_outer_rollback_failure_for_test();
                if rollback_failed {
                    self.mark_unhealthy();
                }
                drop(connection);
                resume_unwind(payload)
            }
        }
    }

    /// Reads a Task and validates its ContentOrigin/TaskScope relation closure.
    pub fn get_task(&self, id: &str) -> Result<Option<TaskSpec>, StoreError> {
        let connection = self.lock_connection()?;
        task::get_task(&connection, id)
    }

    /// Reads a TaskScope and validates ordered source mirrors and its owning Task.
    pub fn get_task_scope(&self, id: &str) -> Result<Option<TaskScope>, StoreError> {
        let connection = self.lock_connection()?;
        task::get_task_scope(&connection, id)
    }

    /// Reads an active ContentOriginV2 and validates ordered parent mirrors and parent existence.
    pub fn get_content_origin_v2(&self, id: &str) -> Result<Option<ContentOriginV2>, StoreError> {
        let connection = self.lock_connection()?;
        task::get_content_origin_v2(&connection, id)
    }

    /// Reads an immutable AuditRecordV2 and revalidates its stored JSON contract.
    pub fn get_audit_v2(&self, id: &str) -> Result<Option<AuditRecordV2>, StoreError> {
        let connection = self.lock_connection()?;
        root_task_create_v2::get_audit_v2(&connection, id)
    }

    /// Reads a TaskCreationProvenanceV1 and revalidates its stored JSON contract.
    pub fn get_task_creation_provenance(
        &self,
        id: &str,
    ) -> Result<Option<TaskCreationProvenanceV1>, StoreError> {
        let connection = self.lock_connection()?;
        root_task_create_v2::get_provenance(&connection, id)
    }

    /// Reads all historical Outbox rows strictly after a cursor.
    pub fn read_after(
        &self,
        cursor: OutboxCursor,
        limit: PageLimit,
    ) -> Result<Vec<OutboxRecord>, StoreError> {
        let connection = self.lock_connection()?;
        outbox::read_after(&connection, cursor, limit)
    }

    /// Reads publisher-pending rows in global position order.
    pub fn read_undelivered(
        &self,
        cursor: OutboxCursor,
        limit: PageLimit,
    ) -> Result<Vec<OutboxRecord>, StoreError> {
        let connection = self.lock_connection()?;
        outbox::read_undelivered(&connection, cursor, limit)
    }

    /// Returns the latest allocated position, or `None` when the Outbox is empty.
    pub fn latest_position(&self) -> Result<Option<OutboxPosition>, StoreError> {
        let connection = self.lock_connection()?;
        outbox::latest_position(&connection)
    }

    /// Stores the first successful publisher completion time without overwriting it.
    ///
    /// Convenience API: still enters the unified `BEGIN IMMEDIATE` write-transaction boundary via
    /// [`Self::with_write_transaction`]. Only a successful `COMMIT` can return
    /// [`MarkDeliveredResult::Marked`], [`MarkDeliveredResult::AlreadyMarked`], or
    /// [`MarkDeliveredResult::NotFound`]. The crate-private Outbox helper is transaction-bound and
    /// is not exposed as `WriteTransaction::mark_delivered`.
    pub fn mark_delivered(
        &self,
        position: OutboxPosition,
        delivered_at: DateTime<Utc>,
    ) -> Result<MarkDeliveredResult, StoreError> {
        self.with_write_transaction(|transaction| {
            outbox::mark_delivered(transaction, position, delivered_at)
        })
    }

    fn lock_connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        if !self.healthy.load(Ordering::Acquire) {
            return Err(unhealthy_store_error());
        }
        let connection = self.connection.lock().map_err(|_| {
            StoreError::new(
                StoreErrorCode::InternalStoreError,
                "SQLite connection lock is poisoned",
            )
        })?;
        if !self.healthy.load(Ordering::Acquire) {
            return Err(unhealthy_store_error());
        }
        Ok(connection)
    }

    fn mark_unhealthy(&self) {
        self.healthy.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn inject_outer_rollback_failure_for_test(&self) {
        self.outer_rollback_failure.store(true, Ordering::Release);
    }

    #[cfg(test)]
    fn consume_outer_rollback_failure_for_test(&self) -> bool {
        self.outer_rollback_failure.swap(false, Ordering::AcqRel)
    }

    #[cfg(not(test))]
    fn consume_outer_rollback_failure_for_test(&self) -> bool {
        false
    }

    /// Test-only fail-closed seam for unhealthy-store coverage; not a production API.
    #[cfg(test)]
    pub(crate) fn mark_unhealthy_for_test(&self) {
        self.mark_unhealthy();
    }
}

/// Restricted write surface borrowed from one active store transaction.
#[derive(Debug)]
pub struct WriteTransaction<'connection> {
    connection: &'connection Connection,
    poisoned: Cell<bool>,
    #[cfg(test)]
    savepoint_failure: Cell<Option<SavepointFailureForTest>>,
    /// When set, root TaskCreate v2 forces stored_data_invalid after a successful event append
    /// so tests can prove sequence/position roll back with the outer savepoint.
    #[cfg(test)]
    force_root_v2_post_append_bundle_invalid: Cell<bool>,
}

impl<'connection> WriteTransaction<'connection> {
    /// Derives active type/aggregate facts and appends an EventEnvelope v2.
    pub fn append_active_event_v2(
        &self,
        event: PendingActiveEventV2,
    ) -> Result<OutboxRecord, StoreError> {
        outbox::append_active_event_v2(self, event)
    }

    /// Borrows the production rate-limit authority bound to this transaction.
    pub fn rate_limit_port(&self) -> TransactionRateLimitPort<'_, '_> {
        TransactionRateLimitPort::new(self)
    }

    pub(crate) const fn connection(&self) -> &Connection {
        self.connection
    }

    pub(crate) fn with_savepoint<T>(
        &self,
        name: &'static str,
        operation: impl FnOnce(&Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        debug_assert!(name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_'));
        self.connection
            .execute_batch(&format!("SAVEPOINT {name}"))
            .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
        let result = operation(self.connection);
        #[cfg(test)]
        if self.savepoint_failure.get() == Some(SavepointFailureForTest::Release) {
            let _ = self
                .connection
                .execute_batch(&format!("RELEASE SAVEPOINT {name}"));
        }
        match result {
            Ok(value) => match self
                .connection
                .execute_batch(&format!("RELEASE SAVEPOINT {name}"))
            {
                Ok(()) => Ok(value),
                Err(error) => {
                    let original = StoreError::sqlite(error, StoreErrorCode::InternalStoreError);
                    if self.savepoint_cleanup_must_fail_for_test()
                        || self
                            .connection
                            .execute_batch(&format!(
                                "ROLLBACK TO SAVEPOINT {name}; RELEASE SAVEPOINT {name}"
                            ))
                            .is_err()
                    {
                        self.poisoned.set(true);
                        return Err(StoreError::new(
                            StoreErrorCode::InternalStoreError,
                            "savepoint release failed and cleanup poisoned the transaction",
                        ));
                    }
                    Err(original)
                }
            },
            Err(error) => {
                if self.savepoint_cleanup_must_fail_for_test()
                    || self
                        .connection
                        .execute_batch(&format!(
                            "ROLLBACK TO SAVEPOINT {name}; RELEASE SAVEPOINT {name}"
                        ))
                        .is_err()
                {
                    self.poisoned.set(true);
                    return Err(StoreError::new(
                        StoreErrorCode::InternalStoreError,
                        format!(
                            "operation failed with {} and savepoint cleanup poisoned the transaction",
                            error.code.as_str()
                        ),
                    ));
                }
                Err(error)
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn inject_savepoint_failure_for_test(&self, failure: SavepointFailureForTest) {
        self.savepoint_failure.set(Some(failure));
    }

    /// Forces root TaskCreate v2 to fail closed after outbox append, before commit.
    #[cfg(test)]
    pub(crate) fn inject_root_v2_post_append_bundle_invalid_for_test(&self) {
        self.force_root_v2_post_append_bundle_invalid.set(true);
    }

    #[cfg(test)]
    pub(crate) fn root_v2_post_append_bundle_invalid_for_test(&self) -> bool {
        self.force_root_v2_post_append_bundle_invalid.get()
    }

    #[cfg(test)]
    fn savepoint_cleanup_must_fail_for_test(&self) -> bool {
        self.savepoint_failure.get() == Some(SavepointFailureForTest::Cleanup)
    }

    #[cfg(not(test))]
    fn savepoint_cleanup_must_fail_for_test(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SavepointFailureForTest {
    Release,
    Cleanup,
}

fn unhealthy_store_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::InternalStoreError,
        "SQLite connection is unusable after rollback failure",
    )
}

#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod outbox_tests;
#[cfg(test)]
mod root_task_create_v2_tests;
#[cfg(test)]
mod savepoint_tests;
#[cfg(test)]
mod tests;
