use super::*;
use chrono::{TimeZone, Utc};
use kernel_contracts::{
    CausationRefV2, EventEnvelopeV2Payload, TaskCreatedPayload, TaskCreatedPayloadProposer,
    TaskCreatedPayloadSchemaVersion, TaskStatus,
};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

struct SavepointDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl SavepointDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("directory");
        Self {
            path: directory.path().join("savepoint.sqlite3"),
            _directory: directory,
            config: SqliteConfig::new(Duration::from_secs(2)).expect("config"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path, self.config).expect("open")
    }
}

#[test]
fn release_failure_poison_prevents_commit_even_when_caller_swallows_error() {
    let database = SavepointDatabase::new();
    let store = database.open();
    let result = store.with_write_transaction(|transaction| {
        transaction.inject_savepoint_failure_for_test(SavepointFailureForTest::Release);
        let error = transaction
            .append_active_event_v2(event(1))
            .expect_err("release failure");
        assert_eq!(error.code, StoreErrorCode::InternalStoreError);
        Ok(())
    });
    assert_eq!(
        result.expect_err("poison prevents commit").code,
        StoreErrorCode::InternalStoreError
    );
    assert!(store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("read")
        .is_empty());
}

#[test]
fn rollback_cleanup_failure_poison_prevents_commit_when_original_error_is_swallowed() {
    let database = SavepointDatabase::new();
    let store = database.open();
    let result = store.with_write_transaction(|transaction| {
        transaction.inject_savepoint_failure_for_test(SavepointFailureForTest::Cleanup);
        let error = transaction
            .with_savepoint("poisoned_test", |connection| {
                connection
                    .execute(
                        "INSERT INTO aggregate_event_sequences VALUES ('task', 'x', 0)",
                        [],
                    )
                    .map_err(|error| {
                        StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
                    })?;
                Err::<(), _>(StoreError::new(
                    StoreErrorCode::ContractInvalid,
                    "injected operation failure",
                ))
            })
            .expect_err("cleanup failure");
        assert_eq!(error.code, StoreErrorCode::InternalStoreError);
        Ok(())
    });
    assert_eq!(
        result.expect_err("poison prevents commit").code,
        StoreErrorCode::InternalStoreError
    );
    assert!(store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("read")
        .is_empty());
}

#[test]
fn outer_rollback_failure_marks_store_unhealthy() {
    let database = SavepointDatabase::new();
    let store = database.open();
    store.inject_outer_rollback_failure_for_test();
    let error = store
        .with_write_transaction(|_| {
            Err::<(), _>(StoreError::new(
                StoreErrorCode::ContractInvalid,
                "force outer rollback",
            ))
        })
        .expect_err("rollback failure");
    assert_eq!(error.code, StoreErrorCode::InternalStoreError);
    assert_eq!(
        store
            .latest_position()
            .expect_err("unhealthy store fails closed")
            .code,
        StoreErrorCode::InternalStoreError
    );
}

fn event(number: u32) -> PendingActiveEventV2 {
    let task_id = Uuid::from_u128(0x1000_0000_0000_4000_8000_0000_0000_0000 + number as u128);
    let instant = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, number).unwrap();
    PendingActiveEventV2 {
        event_id: Uuid::from_u128(0x2000_0000_0000_4000_8000_0000_0000_0000 + number as u128),
        aggregate_id: EventAggregateId::Task(task_id),
        occurred_at: instant,
        causation_ref: CausationRefV2::CommandRequest {
            id: "30000000-0000-4000-8000-000000000001".to_owned(),
        },
        correlation_id: "savepoint-correlation".to_owned(),
        dedup_key: format!("savepoint-dedup-{number}"),
        payload: EventEnvelopeV2Payload::TaskCreated(Box::new(TaskCreatedPayload {
            created_at: instant.to_rfc3339(),
            goal: "savepoint test".to_owned(),
            proposer: TaskCreatedPayloadProposer::User,
            schema_version: TaskCreatedPayloadSchemaVersion,
            status: TaskStatus::Candidate,
            task_id: task_id.to_string(),
            task_revision: 1,
        })),
    }
}
