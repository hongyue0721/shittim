use super::*;
use chrono::{Duration as ChronoDuration, TimeZone};
use domain_policy::{
    evaluate_policy, KernelInvariantState, PolicyEvaluationContext, PolicyEvaluationResult,
    RateLimitConsume, RateLimitKey, RateLimitPort, RateLimitPreview, RateLimitRequest,
};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, AuditRecord,
    AuditRecordAuditType, AuditRecordExternalContentStatus, AuditRecordLevel, AuditRecordOutcome,
    AuditRecordRollbackCapability, AuditRecordSchemaVersion, CausationRef, CausationRefKind,
    EntryPoint, EventEnvelopeType, PolicyRule, PolicyRuleActionMatch, PolicyRuleActorMatch,
    PolicyRuleCondition, PolicyRuleConditionRateLimit, PolicyRuleConditionRateLimitKeyScope,
    PolicyRuleContentOriginMatch, PolicyRuleCreatedBy, PolicyRuleEffect, PolicyRuleResourceMatch,
    PolicyRuleSchemaVersion, PolicyRuleSource, PolicyRuleUpdatedBy, SideEffectClass,
};
use rusqlite::Connection;
use serde_json::json;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const TEST_TIMEOUT: Duration = Duration::from_secs(2);

struct TestDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl TestDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("kernel.sqlite3");
        Self {
            _directory: directory,
            path,
            config: SqliteConfig::new(TEST_TIMEOUT).expect("config"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path, self.config).expect("open store")
    }

    fn raw(&self) -> Connection {
        Connection::open(&self.path).expect("raw connection")
    }
}

#[test]
fn migration_is_idempotent_and_connection_pragmas_are_verified() {
    let database = TestDatabase::new();
    database.open();
    database.open();
    let store = database.open();
    let connection = store.lock_connection().expect("connection");
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("journal mode");
    let foreign_keys: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("foreign keys");
    let busy_timeout: i64 = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .expect("busy timeout");
    let migration_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("migration count");
    assert_eq!(journal.to_ascii_lowercase(), "wal");
    assert_eq!(foreign_keys, 1);
    assert_eq!(busy_timeout, 2_000);
    assert_eq!(migration_count, 2);
}

#[test]
fn concurrent_first_open_migrates_one_new_file_atomically() {
    let database = TestDatabase::new();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let path = database.path.clone();
        let config = database.config;
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            SqliteStore::open(path, config).expect("concurrent first open")
        }));
    }
    barrier.wait();
    let stores: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("join"))
        .collect();
    assert_eq!(stores.len(), 2);

    let connection = database.raw();
    let migration_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("migration count");
    assert_eq!(migration_count, 2);
    for table in [
        "aggregate_event_sequences",
        "outbox",
        "audit_records",
        "policy_rate_limit_consumptions",
        "content_origins",
        "content_origin_parent_refs",
        "task_scopes",
        "task_scope_source_refs",
        "tasks",
        "task_create_idempotency",
    ] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("table count");
        assert_eq!(count, 1, "missing table {table}");
    }
}

#[test]
fn migration_from_real_v1_preserves_audit_and_outbox_and_adds_task_tables() {
    let database = TestDatabase::new();
    let connection = database.raw();
    migration::create_v1_database_for_test(&connection).expect("create v1 database");
    let audit = valid_audit("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee");
    crate::audit::insert_audit(&connection, &audit).expect("v1 audit");
    let event = task_created_event(50, "v1-task");
    let expected_event = crate::outbox::append_event(&connection, event).expect("v1 event");
    drop(connection);

    let store = database.open();
    assert_eq!(
        store.get_audit(&audit.id).expect("upgraded audit"),
        Some(audit)
    );
    assert_eq!(
        store
            .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
            .expect("upgraded outbox"),
        vec![expected_event]
    );
    let connection = database.raw();
    let versions: Vec<i64> = {
        let mut statement = connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .expect("versions statement");
        statement
            .query_map([], |row| row.get(0))
            .expect("version rows")
            .collect::<Result<_, _>>()
            .expect("versions")
    };
    assert_eq!(versions, [1, 2]);
    for table in [
        "content_origins",
        "content_origin_parent_refs",
        "task_scopes",
        "task_scope_source_refs",
        "tasks",
        "task_create_idempotency",
    ] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("table count");
        assert_eq!(count, 1, "missing upgraded table {table}");
    }
}

#[test]
fn config_rejects_zero_timeout_and_memory_or_uri_paths() {
    assert_eq!(
        SqliteConfig::new(Duration::ZERO)
            .expect_err("zero timeout")
            .code,
        StoreErrorCode::SqliteConfigurationFailed
    );
    let config = SqliteConfig::new(TEST_TIMEOUT).expect("config");
    for path in [
        "",
        ":memory:",
        "file:test.sqlite3",
        "file:test.sqlite3?cache=shared",
        "file:test?mode=memory",
    ] {
        assert_eq!(
            SqliteStore::open(path, config)
                .expect_err("invalid path")
                .code,
            StoreErrorCode::InvalidDatabasePath
        );
    }
    let directory = tempfile::tempdir().expect("directory");
    assert_eq!(
        SqliteStore::open(directory.path(), config)
            .expect_err("directory path")
            .code,
        StoreErrorCode::InvalidDatabasePath
    );
    assert_eq!(
        SqliteStore::open(directory.path().join("missing/kernel.sqlite3"), config)
            .expect_err("missing parent")
            .code,
        StoreErrorCode::InvalidDatabasePath
    );
}

#[test]
fn foreign_keys_are_enforced_and_lock_timeout_maps_busy() {
    let database = TestDatabase::new();
    let first = database.open();
    let second = database.open();
    {
        let connection = first.lock_connection().expect("connection");
        connection
            .execute_batch(
                "CREATE TABLE test_parent(id INTEGER PRIMARY KEY);\
                 CREATE TABLE test_child(parent_id INTEGER NOT NULL REFERENCES test_parent(id));",
            )
            .expect("test foreign-key tables");
        let error = connection
            .execute("INSERT INTO test_child(parent_id) VALUES (1)", [])
            .expect_err("foreign key violation");
        assert_eq!(
            StoreError::sqlite(error, StoreErrorCode::InternalStoreError).code,
            StoreErrorCode::ConstraintViolation
        );
    }
    first
        .with_write_transaction(|_| {
            let error = second
                .with_write_transaction(|_| Ok(()))
                .expect_err("second writer must time out");
            assert_eq!(error.code, StoreErrorCode::SqliteBusy);
            Ok(())
        })
        .expect("first transaction");
}

#[test]
fn migration_checksum_drift_and_too_new_are_rejected() {
    let drift = TestDatabase::new();
    drift.open();
    drift
        .raw()
        .execute(
            "UPDATE schema_migrations SET checksum = ?1 WHERE version = 1",
            ["0".repeat(64)],
        )
        .expect("tamper checksum");
    assert_eq!(
        SqliteStore::open(&drift.path, drift.config)
            .expect_err("drift")
            .code,
        StoreErrorCode::MigrationDrift
    );

    let too_new = TestDatabase::new();
    too_new.open();
    too_new
        .raw()
        .execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at) \
             VALUES (3, 'future', ?1, '2026-01-01T00:00:00Z')",
            ["a".repeat(64)],
        )
        .expect("insert future migration");
    assert_eq!(
        SqliteStore::open(&too_new.path, too_new.config)
            .expect_err("too new")
            .code,
        StoreErrorCode::DatabaseSchemaTooNew
    );
}

#[test]
fn migration_unknown_version_is_rejected_as_drift() {
    let database = TestDatabase::new();
    let connection = database.raw();
    connection
        .execute_batch(
            "CREATE TABLE schema_migrations (\
                version INTEGER PRIMARY KEY,\
                name TEXT NOT NULL UNIQUE CHECK(length(name) > 0),\
                checksum TEXT NOT NULL CHECK(length(checksum) = 64),\
                applied_at TEXT NOT NULL\
            ) WITHOUT ROWID;",
        )
        .expect("test migration ledger");
    connection
        .execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at) \
             VALUES (0, 'unknown', ?1, '2026-01-01T00:00:00Z')",
            ["b".repeat(64)],
        )
        .expect("unknown migration");
    drop(connection);

    assert_eq!(
        SqliteStore::open(&database.path, database.config)
            .expect_err("unknown migration version")
            .code,
        StoreErrorCode::MigrationDrift
    );
}

#[test]
fn audit_is_canonical_immutable_validated_and_rollback_safe() {
    let database = TestDatabase::new();
    let store = database.open();
    let record = valid_audit("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    store
        .with_write_transaction(|transaction| transaction.append_audit(&record))
        .expect("append audit");
    assert_eq!(
        store.get_audit(&record.id).expect("get audit"),
        Some(record.clone())
    );

    let raw: String = database
        .raw()
        .query_row("SELECT record_json FROM audit_records", [], |row| {
            row.get(0)
        })
        .expect("stored json");
    assert_eq!(
        raw,
        kernel_contracts::canonical_json_string(&serde_json::to_value(&record).expect("value"))
            .expect("canonical")
    );
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.append_audit(&record))
            .expect_err("immutable duplicate")
            .code,
        StoreErrorCode::ConstraintViolation
    );
    let immutable_connection = database.raw();
    assert!(immutable_connection
        .execute("UPDATE audit_records SET record_json = record_json", [],)
        .is_err());
    assert!(immutable_connection
        .execute("DELETE FROM audit_records", [])
        .is_err());

    let mut invalid = valid_audit("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    invalid.external_content_status = AuditRecordExternalContentStatus::Sent;
    invalid.causation_ref = None;
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.append_audit(&invalid))
            .expect_err("sent support")
            .code,
        StoreErrorCode::ContractInvalid
    );
    assert!(store.get_audit(&invalid.id).expect("get invalid").is_none());

    let rollback = valid_audit("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    let error = store
        .with_write_transaction(|transaction| {
            transaction.append_audit(&rollback)?;
            Err::<(), _>(StoreError::new(StoreErrorCode::NotFound, "force rollback"))
        })
        .expect_err("rollback");
    assert_eq!(error.code, StoreErrorCode::NotFound);
    assert!(store
        .get_audit(&rollback.id)
        .expect("get rollback")
        .is_none());
}

#[test]
fn transaction_panic_rolls_back_and_does_not_poison_store() {
    let database = TestDatabase::new();
    let store = database.open();
    let audit = valid_audit("dddddddd-dddd-4ddd-8ddd-dddddddddddd");
    let instant = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let key = RateLimitKey("panic".into());
    let request = rate_request("panic-rule", 1, &key, instant, 1, 60);

    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = store.with_write_transaction(|transaction| -> Result<(), StoreError> {
            transaction.append_audit(&audit)?;
            transaction.append_event(task_created_event(50, "panic-task"))?;
            assert_eq!(
                transaction
                    .rate_limit_port()
                    .check_and_consume(&request)
                    .expect("consume"),
                RateLimitConsume::Consumed
            );
            panic!("transaction panic");
        });
    }));
    assert!(panic.is_err());
    assert!(store
        .get_audit(&audit.id)
        .expect("read after panic")
        .is_none());
    assert!(store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("outbox after panic")
        .is_empty());

    store
        .with_write_transaction(|transaction| {
            assert_eq!(
                transaction
                    .rate_limit_port()
                    .preview(&request)
                    .expect("preview after panic"),
                RateLimitPreview::Available
            );
            transaction.append_audit(&audit)?;
            transaction.append_event(task_created_event(51, "panic-task"))?;
            Ok(())
        })
        .expect("store remains writable");
    assert!(store
        .get_audit(&audit.id)
        .expect("audit after retry")
        .is_some());
    assert_eq!(
        store
            .latest_position()
            .expect("latest")
            .expect("position")
            .get(),
        1
    );
}

#[test]
fn audit_read_revalidates_tampered_storage() {
    let database = TestDatabase::new();
    database.open();
    database
        .raw()
        .execute(
            "INSERT INTO audit_records(record_json) VALUES (?1)",
            [json!({"id":"not-a-uuid","schema_version":1}).to_string()],
        )
        .expect("insert minimally checked malformed record");
    let store = database.open();
    assert_eq!(
        store
            .get_audit("not-a-uuid")
            .expect_err("contract revalidation")
            .code,
        StoreErrorCode::ContractInvalid
    );
}

#[test]
fn outbox_allocates_sequences_positions_and_multiple_aggregates() {
    let database = TestDatabase::new();
    let store = database.open();
    let records = store
        .with_write_transaction(|transaction| {
            Ok(vec![
                transaction.append_event(task_created_event_for(1, "task-a", 1, 1))?,
                transaction.append_event(task_state_event(2, "task-a", 1))?,
                transaction.append_event(task_created_event_for(3, "task-b", 3, 3))?,
            ])
        })
        .expect("append events");
    assert_eq!(records[0].envelope.sequence, 0);
    assert_eq!(records[1].envelope.sequence, 1);
    assert_eq!(records[2].envelope.sequence, 0);
    assert_eq!(records[0].envelope.outbox_position, "1");
    assert_eq!(records[2].envelope.outbox_position, "3");
    assert_eq!(
        store
            .latest_position()
            .expect("latest")
            .expect("position")
            .get(),
        3
    );
}

#[test]
fn failed_event_rolls_back_sequence_and_position_without_gaps() {
    let database = TestDatabase::new();
    let store = database.open();
    let mut mismatch = task_created_event(1, "task-a");
    mismatch.event_type = EventEnvelopeType::TaskStateChanged;
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.append_event(mismatch))
            .expect_err("payload mismatch")
            .code,
        StoreErrorCode::ContractInvalid
    );
    let record = store
        .with_write_transaction(|transaction| {
            transaction.append_event(task_created_event(2, "task-a"))
        })
        .expect("valid event");
    assert_eq!(record.envelope.sequence, 0);
    assert_eq!(record.envelope.outbox_position, "1");
}

#[test]
fn contract_invalid_event_rolls_back_allocations() {
    let database = TestDatabase::new();
    let store = database.open();
    let mut invalid = task_created_event(1, "task-a");
    invalid.payload["goal"] = json!("");
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.append_event(invalid))
            .expect_err("invalid payload")
            .code,
        StoreErrorCode::ContractInvalid
    );
    let valid = store
        .with_write_transaction(|transaction| {
            transaction.append_event(task_created_event(2, "task-a"))
        })
        .expect("valid event");
    assert_eq!(valid.envelope.sequence, 0);
    assert_eq!(valid.envelope.outbox_position, "1");
}

#[test]
fn ignored_invalid_first_append_is_self_rolled_back() {
    let database = TestDatabase::new();
    let store = database.open();
    store
        .with_write_transaction(|transaction| {
            let mut invalid = task_created_event(10, "task-a");
            invalid.payload["goal"] = json!("");
            let error = transaction
                .append_event(invalid)
                .expect_err("invalid append");
            assert_eq!(error.code, StoreErrorCode::ContractInvalid);
            Ok(())
        })
        .expect("caller deliberately commits after handled error");
    assert!(store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("outbox")
        .is_empty());

    let valid = store
        .with_write_transaction(|transaction| {
            transaction.append_event(task_created_event(11, "task-a"))
        })
        .expect("valid append");
    assert_eq!(valid.envelope.sequence, 0);
    assert_eq!(valid.envelope.outbox_position, "1");
}

#[test]
fn ignored_invalid_second_append_preserves_prior_event_without_gaps() {
    let database = TestDatabase::new();
    let store = database.open();
    let first = store
        .with_write_transaction(|transaction| {
            let first = transaction.append_event(task_created_event(20, "task-a"))?;
            let mut invalid = task_created_event(21, "task-a");
            invalid.payload["goal"] = json!("");
            let error = transaction
                .append_event(invalid)
                .expect_err("invalid append");
            assert_eq!(error.code, StoreErrorCode::ContractInvalid);
            Ok(first)
        })
        .expect("commit first event");
    assert_eq!(first.envelope.sequence, 0);
    assert_eq!(first.envelope.outbox_position, "1");

    let next = store
        .with_write_transaction(|transaction| {
            transaction.append_event(task_created_event(22, "task-a"))
        })
        .expect("next append");
    assert_eq!(next.envelope.sequence, 1);
    assert_eq!(next.envelope.outbox_position, "2");
}

#[test]
fn cursor_parsing_ordering_and_limits_are_strict() {
    assert_eq!(
        "00042".parse::<OutboxCursor>().expect("cursor").to_string(),
        "42"
    );
    for invalid in ["", "+1", "-1", " 1", "1 ", "9223372036854775808"] {
        assert_eq!(
            invalid
                .parse::<OutboxCursor>()
                .expect_err("invalid cursor")
                .code,
            StoreErrorCode::InvalidCursor
        );
    }
    assert_eq!(
        PageLimit::new(0).expect_err("zero limit").code,
        StoreErrorCode::InvalidCursor
    );
    assert_eq!(
        PageLimit::new(501).expect_err("large limit").code,
        StoreErrorCode::InvalidCursor
    );

    let database = TestDatabase::new();
    let store = database.open();
    for number in 1..=3 {
        store
            .with_write_transaction(|transaction| {
                transaction.append_event(task_created_event_for(
                    number,
                    &format!("task-{number}"),
                    number,
                    number % 60,
                ))
            })
            .expect("append");
    }
    let page = store
        .read_after(
            OutboxCursor::new(1).expect("cursor"),
            PageLimit::new(1).expect("limit"),
        )
        .expect("page");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].envelope.outbox_position, "2");
}

#[test]
fn undelivered_reads_are_at_least_once_across_reopen() {
    let database = TestDatabase::new();
    let store = database.open();
    let record = store
        .with_write_transaction(|transaction| {
            transaction.append_event(task_created_event(1, "task-a"))
        })
        .expect("event");
    let limit = PageLimit::new(10).expect("limit");
    let first = store
        .read_undelivered(OutboxCursor::START, limit)
        .expect("first delivery read");
    let second = store
        .read_undelivered(OutboxCursor::START, limit)
        .expect("second delivery read");
    assert_eq!(first, vec![record.clone()]);
    assert_eq!(second, first);
    drop(store);

    let reopened = database.open();
    assert_eq!(
        reopened
            .read_undelivered(OutboxCursor::START, limit)
            .expect("delivery read after reopen"),
        vec![record.clone()]
    );
    let position = OutboxPosition::new(record.envelope.outbox_position.parse().expect("position"))
        .expect("position type");
    let delivered_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 5).unwrap();
    assert_eq!(
        reopened
            .mark_delivered(position, delivered_at)
            .expect("mark"),
        MarkDeliveredResult::Marked
    );
    assert!(reopened
        .read_undelivered(OutboxCursor::START, limit)
        .expect("after mark")
        .is_empty());
    let history = reopened
        .read_after(OutboxCursor::START, limit)
        .expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].envelope, record.envelope);
    assert_eq!(history[0].delivered_at, Some(delivered_at));
}

#[test]
fn delivered_marking_retains_first_time_and_history() {
    let database = TestDatabase::new();
    let store = database.open();
    let record = store
        .with_write_transaction(|transaction| {
            transaction.append_event(task_created_event(1, "task-a"))
        })
        .expect("event");
    let position = OutboxPosition::new(record.envelope.outbox_position.parse().expect("position"))
        .expect("position type");
    let first = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let second = first + ChronoDuration::seconds(10);
    assert_eq!(
        store.mark_delivered(position, first).expect("mark"),
        MarkDeliveredResult::Marked
    );
    assert_eq!(
        store.mark_delivered(position, second).expect("mark again"),
        MarkDeliveredResult::AlreadyMarked
    );
    assert!(store
        .read_undelivered(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("undelivered")
        .is_empty());
    let history = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("history");
    assert_eq!(history[0].delivered_at, Some(first));
    assert_eq!(
        store
            .mark_delivered(OutboxPosition::new(99).expect("position"), first)
            .expect("not found"),
        MarkDeliveredResult::NotFound
    );
}

#[test]
fn multiple_stores_serialize_sequence_and_position_allocation() {
    let database = TestDatabase::new();
    database.open();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for number in 1..=2 {
        let path = database.path.clone();
        let config = database.config;
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let store = SqliteStore::open(path, config).expect("thread store");
            barrier.wait();
            store
                .with_write_transaction(|transaction| {
                    transaction.append_event(task_created_event_for(
                        number,
                        "shared-task",
                        number,
                        number,
                    ))
                })
                .expect("thread append")
        }));
    }
    barrier.wait();
    let mut records: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("join"))
        .collect();
    records.sort_by_key(|record| record.envelope.sequence);
    assert_eq!(records[0].envelope.sequence, 0);
    assert_eq!(records[1].envelope.sequence, 1);
    assert_ne!(
        records[0].envelope.outbox_position,
        records[1].envelope.outbox_position
    );
}

#[test]
fn rate_limit_preview_consume_boundary_isolation_and_rollback() {
    let database = TestDatabase::new();
    let store = database.open();
    let instant = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 10).unwrap();
    let key = RateLimitKey("actor-a".into());
    let request = rate_request("rule", 1, &key, instant, 2, 10);
    store
        .with_write_transaction(|transaction| {
            let port = transaction.rate_limit_port();
            assert_eq!(
                port.preview(&request).expect("preview"),
                RateLimitPreview::Available
            );
            assert_eq!(
                port.preview(&request).expect("preview"),
                RateLimitPreview::Available
            );
            assert_eq!(
                port.check_and_consume(&request).expect("consume"),
                RateLimitConsume::Consumed
            );
            assert_eq!(
                port.check_and_consume(&request).expect("consume"),
                RateLimitConsume::Consumed
            );
            assert_eq!(
                port.check_and_consume(&request).expect("exceeded"),
                RateLimitConsume::Exceeded
            );
            Ok(())
        })
        .expect("rate transaction");

    let boundary = rate_request(
        "rule",
        1,
        &key,
        instant + ChronoDuration::seconds(10),
        1,
        10,
    );
    store
        .with_write_transaction(|transaction| {
            assert_eq!(
                transaction
                    .rate_limit_port()
                    .preview(&boundary)
                    .expect("boundary"),
                RateLimitPreview::Available
            );
            let other_revision = rate_request("rule", 2, &key, instant, 1, 10);
            let other_key = RateLimitKey("actor-b".into());
            let other_key_request = rate_request("rule", 1, &other_key, instant, 1, 10);
            assert_eq!(
                transaction
                    .rate_limit_port()
                    .preview(&other_revision)
                    .expect("revision"),
                RateLimitPreview::Available
            );
            assert_eq!(
                transaction
                    .rate_limit_port()
                    .preview(&other_key_request)
                    .expect("key"),
                RateLimitPreview::Available
            );
            Ok(())
        })
        .expect("isolation");

    let rollback_key = RateLimitKey("rollback".into());
    let rollback_request = rate_request("rule", 1, &rollback_key, instant, 1, 10);
    store
        .with_write_transaction(|transaction| {
            assert_eq!(
                transaction
                    .rate_limit_port()
                    .check_and_consume(&rollback_request)
                    .expect("consume"),
                RateLimitConsume::Consumed
            );
            Err::<(), _>(StoreError::new(StoreErrorCode::NotFound, "force rollback"))
        })
        .expect_err("rollback");
    store
        .with_write_transaction(|transaction| {
            assert_eq!(
                transaction
                    .rate_limit_port()
                    .preview(&rollback_request)
                    .expect("preview"),
                RateLimitPreview::Available
            );
            Ok(())
        })
        .expect("rollback verified");
}

#[test]
fn rate_limit_supports_same_microsecond_and_rejects_bad_facts() {
    let database = TestDatabase::new();
    let store = database.open();
    let instant = Utc.timestamp_micros(1_700_000_000_000_000).unwrap();
    let key = RateLimitKey("same-microsecond".into());
    let request = rate_request("rule", 1, &key, instant, 3, 10);
    store
        .with_write_transaction(|transaction| {
            let port = transaction.rate_limit_port();
            for _ in 0..3 {
                assert_eq!(
                    port.check_and_consume(&request).expect("consume"),
                    RateLimitConsume::Consumed
                );
            }
            assert_eq!(
                port.check_and_consume(&request).expect("exceeded"),
                RateLimitConsume::Exceeded
            );
            let invalid_key = RateLimitKey(String::new());
            let invalid = rate_request("rule", 1, &invalid_key, instant, 1, 10);
            assert_eq!(
                port.preview(&invalid).expect_err("invalid").code,
                domain_policy::PolicyErrorCode::RateLimitFailed
            );
            Ok(())
        })
        .expect("transaction");
}

#[test]
fn multiple_stores_compete_for_last_rate_limit_slot() {
    let database = TestDatabase::new();
    database.open();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let path = database.path.clone();
        let config = database.config;
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let store = SqliteStore::open(path, config).expect("store");
            let key = RateLimitKey("shared".into());
            let instant = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
            barrier.wait();
            store
                .with_write_transaction(|transaction| {
                    transaction
                        .rate_limit_port()
                        .check_and_consume(&rate_request("rule", 1, &key, instant, 1, 60))
                        .map_err(|_| {
                            StoreError::new(StoreErrorCode::InternalStoreError, "rate limit")
                        })
                })
                .expect("rate transaction")
        }));
    }
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("join"))
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|&&value| value == RateLimitConsume::Consumed)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|&&value| value == RateLimitConsume::Exceeded)
            .count(),
        1
    );
}

#[test]
fn policy_matcher_consumes_only_the_winner() {
    let database = TestDatabase::new();
    let store = database.open();
    let instant = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let mut winner = rate_limited_rule("winner", 2);
    winner.effect = PolicyRuleEffect::Deny;
    let loser = rate_limited_rule("loser", 1);
    let context = policy_context(instant);
    store
        .with_write_transaction(|transaction| {
            match evaluate_policy(&[loser, winner], &context, &transaction.rate_limit_port()) {
                PolicyEvaluationResult::Denied(draft) => {
                    assert_eq!(draft.matched_rule_ref.as_deref(), Some("winner"));
                }
                other => panic!("unexpected result: {other:?}"),
            }
            let loser_key = RateLimitKey("loser".into());
            let loser_request = rate_request("loser", 1, &loser_key, instant, 1, 60);
            assert_eq!(
                transaction
                    .rate_limit_port()
                    .preview(&loser_request)
                    .expect("loser preview"),
                RateLimitPreview::Available
            );
            Ok(())
        })
        .expect("matcher transaction");
}

fn valid_audit(id: &str) -> AuditRecord {
    AuditRecord {
        action_id: None,
        actor: Some(actor()),
        approval_record_ref: None,
        artifact_refs: vec![],
        audit_type: AuditRecordAuditType::CommandAccepted,
        causation_ref: Some(CausationRef {
            id: "77777777-7777-4777-8777-777777777777".into(),
            kind: CausationRefKind::CommandRequest,
        }),
        content_origin_refs: vec![],
        correlation_id: Some("correlation".into()),
        delegation_ref: None,
        details: json!({"b": 2, "a": 1}),
        entry_point: EntryPoint::LocalDesktop,
        extension_id: None,
        external_content_status: AuditRecordExternalContentStatus::NotSent,
        id: id.into(),
        level: AuditRecordLevel::Security,
        model_call_refs: vec![],
        occurred_at: "2026-01-01T00:00:00Z".into(),
        outcome: AuditRecordOutcome::Observed,
        payload_manifest_refs: vec![],
        permission_decision_ref: None,
        policy_context: None,
        provider_id: None,
        reason_codes: vec!["accepted".into()],
        recovery_attempt_ref: None,
        resource_refs: vec![],
        rollback_capability: AuditRecordRollbackCapability::Unknown,
        schema_version: AuditRecordSchemaVersion,
        stop_fence_generation: None,
        summary: Some("accepted".into()),
        task_creation_context: None,
        task_id: None,
        verification_result_refs: vec![],
    }
}

fn actor() -> Actor {
    Actor {
        authentication_level: ActorAuthenticationLevel::Asserted,
        confidence: None,
        id: "actor".into(),
        kind: ActorKind::KnownUser,
        revision: 1,
        schema_version: ActorSchemaVersion,
        source: "actor-source://local/test".into(),
    }
}

fn task_created_event(number: u32, aggregate_id: &str) -> PendingEvent {
    task_created_event_for(number, aggregate_id, number, number)
}

fn task_created_event_for(
    event_number: u32,
    aggregate_id: &str,
    task_number: u32,
    second: u32,
) -> PendingEvent {
    let task_id = format!("00000000-0000-4000-8000-{task_number:012}");
    PendingEvent {
        event_id: format!("10000000-0000-4000-8000-{event_number:012}"),
        event_type: EventEnvelopeType::TaskCreated,
        aggregate_type: "task".into(),
        aggregate_id: aggregate_id.into(),
        occurred_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap(),
        causation_ref: CausationRef {
            id: format!("20000000-0000-4000-8000-{event_number:012}"),
            kind: CausationRefKind::CommandRequest,
        },
        correlation_id: format!("correlation-{event_number}"),
        dedup_key: format!("dedup-{event_number}"),
        payload: json!({
            "schema_version": 1,
            "task_id": task_id,
            "status": "candidate",
            "proposer": "user",
            "goal": "test goal",
            "task_revision": 1,
            "created_at": format!("2026-01-01T00:00:{second:02}Z")
        }),
    }
}

fn task_state_event(number: u32, aggregate_id: &str, task_number: u32) -> PendingEvent {
    let task_id = format!("00000000-0000-4000-8000-{task_number:012}");
    PendingEvent {
        event_id: format!("30000000-0000-4000-8000-{number:012}"),
        event_type: EventEnvelopeType::TaskStateChanged,
        aggregate_type: "task".into(),
        aggregate_id: aggregate_id.into(),
        occurred_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, number).unwrap(),
        causation_ref: CausationRef {
            id: format!("40000000-0000-4000-8000-{number:012}"),
            kind: CausationRefKind::Event,
        },
        correlation_id: format!("correlation-{number}"),
        dedup_key: format!("dedup-state-{number}"),
        payload: json!({
            "schema_version": 1,
            "task_id": task_id,
            "from_status": "candidate",
            "to_status": "planned",
            "task_revision": 2,
            "reason_code": "planning_started",
            "changed_at": format!("2026-01-01T00:01:{number:02}Z")
        }),
    }
}

fn rate_request<'a>(
    rule_id: &'a str,
    revision: i64,
    key: &'a RateLimitKey,
    instant: chrono::DateTime<Utc>,
    count: i64,
    window_seconds: i64,
) -> RateLimitRequest<'a> {
    RateLimitRequest {
        rule_id,
        rule_revision: revision,
        key,
        window_seconds,
        count,
        instant,
    }
}

fn rate_limited_rule(id: &str, priority: i64) -> PolicyRule {
    let actor = actor();
    PolicyRule {
        action_match: PolicyRuleActionMatch {
            capability_ids: vec![],
            operation_patterns: vec![],
            side_effect_max: None,
        },
        actor_match: PolicyRuleActorMatch {
            auth_level_min: None,
            entry_point: None,
            kind: None,
            source_patterns: None,
        },
        condition: PolicyRuleCondition {
            delegation_required: None,
            local_presence_required: None,
            rate_limit: Some(PolicyRuleConditionRateLimit {
                count: 1,
                key_scope: PolicyRuleConditionRateLimitKeyScope::Rule,
                window_seconds: 60,
            }),
            time_window: None,
        },
        confirmation_mode: None,
        content_origin_match: PolicyRuleContentOriginMatch {
            kinds: None,
            source_patterns: None,
        },
        created_at: "2026-01-01T00:00:00Z".into(),
        created_by: PolicyRuleCreatedBy {
            actor: actor.clone(),
            entry_point: EntryPoint::SystemInternal,
        },
        description: String::new(),
        effect: PolicyRuleEffect::Allow,
        enabled: true,
        expires_at: None,
        id: id.into(),
        name: id.into(),
        priority,
        resource_match: PolicyRuleResourceMatch {
            exclude_patterns: vec![],
            scope_patterns: vec![],
        },
        revision: 1,
        schema_version: PolicyRuleSchemaVersion,
        source: PolicyRuleSource::UserDefined,
        updated_at: "2026-01-01T00:00:00Z".into(),
        updated_by: PolicyRuleUpdatedBy {
            actor,
            entry_point: EntryPoint::SystemInternal,
        },
    }
}

fn policy_context(instant: chrono::DateTime<Utc>) -> PolicyEvaluationContext {
    PolicyEvaluationContext {
        actor: actor(),
        entry_point: EntryPoint::LocalDesktop,
        content_origins: vec![],
        task_id: None,
        action_id: None,
        plan_version: 0,
        resource_refs: vec![],
        capability_id: "test".into(),
        operation: "test.run".into(),
        side_effect_class: SideEffectClass::S0,
        structured_arguments: json!({}),
        delegation: None,
        local_presence: None,
        evaluation_instant: instant,
        security_mode: "normal".into(),
        kernel_invariant: KernelInvariantState::Clear,
    }
}
