//! File-backed SQLite persistence base for the Kernel.
//!
//! This crate owns migrations, immutable AuditRecord JSON, atomic Event Outbox allocation,
//! publisher storage operations, transaction-bound policy rate-limit consumption, and the Task
//! create/get repository. It does not implement Task update/list, Action/PermissionDecision
//! repositories, KCP, `agentd`, networking, or a Publisher loop.

#![deny(missing_docs)]

mod audit;
mod config;
mod error;
mod migration;
mod outbox;
mod rate_limit;
mod task;

pub use config::SqliteConfig;
pub use error::{StoreError, StoreErrorCode};
pub use outbox::{
    MarkDeliveredResult, OutboxCursor, OutboxPosition, OutboxRecord, PageLimit, PendingEvent,
};
pub use rate_limit::TransactionRateLimitPort;
pub use task::{
    CreateTaskResult, TaskCreateAllocation, TaskCreateCommand, TaskCreateEnvelopeFacts,
};

use chrono::{DateTime, Utc};
use kernel_contracts::{AuditRecord, ContentOrigin, TaskScope, TaskSpec};
use rusqlite::Connection;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Thread-safe handle to one file-backed SQLite database.
#[derive(Debug)]
pub struct SqliteStore {
    connection: Mutex<Connection>,
    healthy: AtomicBool,
}

impl SqliteStore {
    /// Opens a file database, configures WAL/foreign keys/busy timeout, and applies migrations.
    pub fn open(path: impl AsRef<Path>, config: SqliteConfig) -> Result<Self, StoreError> {
        let path = config::validated_path(path.as_ref())?;
        let connection = config::open_connection(&path, config)?;
        config::initialize_wal(&connection, config.busy_timeout)?;
        migration::apply_migrations(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            healthy: AtomicBool::new(true),
        })
    }

    /// Runs a closure inside `BEGIN IMMEDIATE`; success commits and error or panic rolls back.
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
            };
            operation(&transaction)
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
                if connection.execute_batch("ROLLBACK").is_err() {
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
                if connection.execute_batch("ROLLBACK").is_err() {
                    self.mark_unhealthy();
                }
                drop(connection);
                resume_unwind(payload)
            }
        }
    }

    /// Reads an immutable AuditRecord and revalidates its stored JSON contract.
    pub fn get_audit(&self, id: &str) -> Result<Option<AuditRecord>, StoreError> {
        let connection = self.lock_connection()?;
        audit::get_audit(&connection, id)
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

    /// Reads a ContentOrigin and validates ordered parent mirrors and parent existence.
    pub fn get_content_origin(&self, id: &str) -> Result<Option<ContentOrigin>, StoreError> {
        let connection = self.lock_connection()?;
        task::get_content_origin(&connection, id)
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
    pub fn mark_delivered(
        &self,
        position: OutboxPosition,
        delivered_at: DateTime<Utc>,
    ) -> Result<MarkDeliveredResult, StoreError> {
        let connection = self.lock_connection()?;
        outbox::mark_delivered(&connection, position, delivered_at)
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
}

/// Restricted write surface borrowed from one active store transaction.
#[derive(Debug)]
pub struct WriteTransaction<'connection> {
    connection: &'connection Connection,
}

impl WriteTransaction<'_> {
    /// Validates, canonicalizes, and inserts an immutable AuditRecord.
    pub fn append_audit(&self, record: &AuditRecord) -> Result<(), StoreError> {
        audit::insert_audit(self.connection, record)
    }

    /// Allocates aggregate sequence/global position and validates the full typed EventEnvelope.
    pub fn append_event(&self, event: PendingEvent) -> Result<OutboxRecord, StoreError> {
        outbox::append_event(self.connection, event)
    }

    /// Borrows the production rate-limit authority bound to this transaction.
    pub fn rate_limit_port(&self) -> TransactionRateLimitPort<'_, '_> {
        TransactionRateLimitPort::new(self)
    }

    pub(crate) const fn connection(&self) -> &Connection {
        self.connection
    }
}

fn unhealthy_store_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::InternalStoreError,
        "SQLite connection is unusable after rollback failure",
    )
}

#[cfg(test)]
mod task_tests;
#[cfg(test)]
mod tests;
