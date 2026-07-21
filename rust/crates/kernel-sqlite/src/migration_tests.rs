use super::*;
use chrono::{TimeZone, Utc};
use kernel_contracts::{
    CausationRefV2, EventEnvelopeV2Payload, TaskCreatedPayload, TaskCreatedPayloadProposer,
    TaskCreatedPayloadSchemaVersion, TaskStatus,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

struct MigrationDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl MigrationDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("migration.sqlite3");
        Self {
            _directory: directory,
            path,
            config: SqliteConfig::new(Duration::from_secs(2)).expect("config"),
        }
    }

    fn raw(&self) -> Connection {
        Connection::open(&self.path).expect("connection")
    }
}

#[test]
fn descriptor_v1_bytes_are_jcs_lf_and_hash_matches_ledger() {
    let bytes = migration::migration_0003_descriptor_bytes_for_test();
    assert!(bytes.ends_with(b"\n"));
    assert!(!bytes.ends_with(b"\n\n"));
    assert!(!bytes.contains(&b'\r'));
    let without_lf = &bytes[..bytes.len() - 1];
    let descriptor: Value = serde_json::from_slice(without_lf).expect("descriptor JSON");
    assert_eq!(
        kernel_contracts::canonical_json_string(&descriptor).expect("JCS"),
        std::str::from_utf8(without_lf).expect("UTF-8")
    );
    assert_eq!(descriptor["migration_version"], 3);
    assert_eq!(descriptor["name"], "versioned_event_outbox");
    assert_eq!(
        descriptor["sql_assets"][0]["path"],
        "rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql"
    );
    assert_eq!(
        descriptor["transform"],
        json!({
            "algorithm_id": "shittim.kernel-sqlite.outbox-v1-to-versioned-v1",
            "implementation_id": "kernel_sqlite::migration::outbox_v1_to_versioned_v1",
            "version": 1
        })
    );

    let database = MigrationDatabase::new();
    SqliteStore::open(&database.path, database.config).expect("open");
    let (checksum, descriptor_hash, format): (String, String, i64) = database
        .raw()
        .query_row(
            "SELECT checksum, descriptor_hash, descriptor_format_version \
             FROM schema_migrations WHERE version = 3",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("ledger row");
    let expected = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(checksum, expected);
    assert_eq!(descriptor_hash, expected);
    assert_eq!(format, 1);
}

#[test]
fn migration_0003_refuses_nonempty_legacy_outbox_with_reinitialize_required() {
    let database = MigrationDatabase::new();
    let connection = database.raw();
    migration::create_v2_database_for_test(&connection).expect("v2 database");
    connection
        .execute(
            "INSERT INTO outbox(\
                event_id, event_type, schema_version, aggregate_type, aggregate_id, sequence, \
                occurred_at, causation_kind, causation_id, correlation_id, dedup_key, payload_json\
             ) VALUES (?1, 'task.created', 1, 'task', ?2, 0, ?3, 'command_request', ?4, 'c', 'd', ?5)",
            rusqlite::params![
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "2026-01-01T00:00:00+00:00",
                "11111111-1111-4111-8111-111111111111",
                r#"{"schema_version":1,"task_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","status":"candidate","proposer":"user","goal":"legacy","task_revision":1,"created_at":"2026-01-01T00:00:00+00:00"}"#,
            ],
        )
        .expect("seed legacy outbox row");
    drop(connection);

    let error = SqliteStore::open(&database.path, database.config).expect_err("must refuse");
    assert_eq!(error.code, StoreErrorCode::StoredDataInvalid);
    assert!(
        error.message.contains("reinitialize-required"),
        "message={}",
        error.message
    );

    let connection = database.raw();
    let migration_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("migration count");
    assert_eq!(migration_count, 2);
    let columns: Vec<String> = {
        let mut statement = connection
            .prepare("PRAGMA table_info(schema_migrations)")
            .expect("columns");
        statement
            .query_map([], |row| row.get(1))
            .expect("rows")
            .collect::<Result<_, _>>()
            .expect("collect")
    };
    assert!(!columns.iter().any(|column| column == "descriptor_hash"));
}

#[test]
fn migration_0003_empty_outbox_upgrades_and_fresh_baseline_reaches_0008() {
    let database = MigrationDatabase::new();
    let connection = database.raw();
    migration::create_v2_database_for_test(&connection).expect("v2 database");
    drop(connection);

    let store = SqliteStore::open(&database.path, database.config).expect("upgrade empty");
    let connection = database.raw();
    let versions: Vec<i64> = {
        let mut statement = connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .expect("versions");
        statement
            .query_map([], |row| row.get(0))
            .expect("rows")
            .collect::<Result<_, _>>()
            .expect("collect")
    };
    assert_eq!(versions, [1, 2, 3, 4, 5, 6, 7, 8]);
    for table in [
        "content_origins",
        "content_origin_parent_refs",
        "audit_records",
        "task_create_idempotency",
    ] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("table count");
        assert_eq!(count, 0, "legacy table {table} must be dropped");
    }
    for table in [
        "content_origins_v2",
        "audit_records_v2",
        "root_task_create_idempotency_v2",
        "task_creation_provenances",
        "tasks",
        "task_scopes",
        "outbox",
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
    drop(connection);

    let task_id = Uuid::from_u128(0x1000_0000_0000_4000_8000_0000_0000_0001);
    let record = store
        .with_write_transaction(|transaction| {
            transaction.append_active_event_v2(active_task_created(task_id, 1))
        })
        .expect("v2-only append after upgrade");
    assert_eq!(record.envelope.sequence(), 0);
    assert_eq!(record.envelope.outbox_position(), "1");
}

#[test]
fn open_refuses_schema_version_1_outbox_rows_as_reinitialize_required() {
    let database = MigrationDatabase::new();
    SqliteStore::open(&database.path, database.config).expect("fresh open");
    database
        .raw()
        .execute(
            "INSERT INTO outbox(\
                event_id, event_type, schema_version, aggregate_type, aggregate_id, sequence, \
                occurred_at, causation_json, correlation_id, dedup_key, payload_json\
             ) VALUES (?1, 'task.created', 1, 'task', ?2, 0, ?3, ?4, 'c', 'd', ?5)",
            rusqlite::params![
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "2026-01-01T00:00:00+00:00",
                r#"{"id":"11111111-1111-4111-8111-111111111111","kind":"command_request"}"#,
                r#"{"created_at":"2026-01-01T00:00:00+00:00","goal":"legacy","proposer":"user","schema_version":1,"status":"candidate","task_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","task_revision":1}"#,
            ],
        )
        .expect("force schema_version=1 row past CHECK if possible or fail");
    // If CHECK rejected the insert, re-open with PRAGMA ignore.
    let inserted: i64 = database
        .raw()
        .query_row(
            "SELECT COUNT(*) FROM outbox WHERE schema_version = 1",
            [],
            |row| row.get(0),
        )
        .expect("count");
    if inserted == 0 {
        database
            .raw()
            .execute_batch(
                "PRAGMA ignore_check_constraints=ON; \
                 INSERT INTO outbox(\
                    event_id, event_type, schema_version, aggregate_type, aggregate_id, sequence, \
                    occurred_at, causation_json, correlation_id, dedup_key, payload_json\
                 ) VALUES (\
                    'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa','task.created',1,'task',\
                    'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',0,'2026-01-01T00:00:00+00:00',\
                    '{\"id\":\"11111111-1111-4111-8111-111111111111\",\"kind\":\"command_request\"}',\
                    'c','d',\
                    '{\"created_at\":\"2026-01-01T00:00:00+00:00\",\"goal\":\"legacy\",\"proposer\":\"user\",\"schema_version\":1,\"status\":\"candidate\",\"task_id\":\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\",\"task_revision\":1}'\
                 ); \
                 PRAGMA ignore_check_constraints=OFF;",
            )
            .expect("force insert");
    }
    let error = SqliteStore::open(&database.path, database.config).expect_err("refuse v1 outbox");
    assert_eq!(error.code, StoreErrorCode::StoredDataInvalid);
    assert!(error.message.contains("reinitialize-required"));
}

#[test]
fn migration_0005_refuses_nonempty_content_origins_and_parent_refs() {
    assert_0005_refuses_nonempty_legacy_table(
        "content_origins",
        |connection| {
            // parent_refs must be inserted before the origin row (late-insert trigger).
            connection
                .execute_batch("PRAGMA foreign_keys=OFF")
                .expect("fk off");
            connection
                .execute(
                    "INSERT INTO content_origin_parent_refs(origin_id, ordinal, parent_origin_id) \
                     VALUES (?1, 0, ?2)",
                    rusqlite::params![
                        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                        "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                    ],
                )
                .expect("seed parent_refs");
            connection
                .execute(
                    "INSERT INTO content_origins(record_json) VALUES (?1)",
                    [r#"{"id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","schema_version":1}"#],
                )
                .expect("seed content_origins");
        },
        "legacy content_origins",
    );
}

#[test]
fn migration_0005_refuses_nonempty_audit_records() {
    assert_0005_refuses_nonempty_legacy_table(
        "audit_records",
        |connection| {
            connection
                .execute(
                    "INSERT INTO audit_records(record_json) VALUES (?1)",
                    [r#"{"id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","schema_version":1}"#],
                )
                .expect("seed audit_records");
        },
        "legacy audit_records",
    );
}

#[test]
fn migration_0005_refuses_nonempty_task_create_idempotency() {
    assert_0005_refuses_nonempty_legacy_table(
        "task_create_idempotency",
        |connection| {
            // Seed after 0004 so the rebuilt table is present and 0005 is the next unit.
            connection
                .execute_batch("PRAGMA foreign_keys=OFF")
                .expect("fk off");
            connection
                .execute(
                    "INSERT INTO task_create_idempotency(\
                        projection_json, idempotency_key, projection_hash, created_task_id, accepted_at\
                     ) VALUES (?1, 'key', ?2, ?3, '2026-01-01T00:00:00+00:00')",
                    rusqlite::params![
                        r#"{"actor":{"id":"actor"},"entry_point":"local_desktop","command_type":"task.create"}"#,
                        "a".repeat(64),
                        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                    ],
                )
                .expect("seed task_create_idempotency");
        },
        "legacy task_create_idempotency",
    );
}

#[test]
fn open_refuses_nonempty_content_origins_after_0005() {
    assert_open_refuses_recreated_legacy_table("content_origins", |connection| {
        connection
            .execute_batch(
                "CREATE TABLE content_origins (
                    record_json TEXT NOT NULL
                );",
            )
            .expect("recreate content_origins");
        connection
            .execute(
                "INSERT INTO content_origins(record_json) VALUES ('legacy-row')",
                [],
            )
            .expect("seed content_origins");
    });
}

#[test]
fn open_refuses_nonempty_audit_records_after_0005() {
    assert_open_refuses_recreated_legacy_table("audit_records", |connection| {
        connection
            .execute_batch(
                "CREATE TABLE audit_records (
                    record_json TEXT NOT NULL
                );",
            )
            .expect("recreate audit_records");
        connection
            .execute(
                "INSERT INTO audit_records(record_json) VALUES ('legacy-row')",
                [],
            )
            .expect("seed audit_records");
    });
}

#[test]
fn open_refuses_nonempty_task_create_idempotency_after_0005() {
    assert_open_refuses_recreated_legacy_table("task_create_idempotency", |connection| {
        connection
            .execute_batch(
                "CREATE TABLE task_create_idempotency (
                    projection_json TEXT NOT NULL
                );",
            )
            .expect("recreate task_create_idempotency");
        connection
            .execute(
                "INSERT INTO task_create_idempotency(projection_json) VALUES ('legacy-row')",
                [],
            )
            .expect("seed task_create_idempotency");
    });
}

fn assert_0005_refuses_nonempty_legacy_table(
    table: &str,
    seed: impl FnOnce(&Connection),
    expected_label_fragment: &str,
) {
    let database = MigrationDatabase::new();
    let connection = database.raw();
    // Reach post-0004 so dead v1 tables still exist and 0005 is the next unit.
    // Seeding before 0004 is unsafe for task_create_idempotency (FK rebuild).
    migration::create_through_0004_for_test(&connection).expect("through 0004");
    seed(&connection);
    let seeded: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("seed count");
    assert!(seeded > 0, "{table} must be non-empty before open");
    drop(connection);

    let error = SqliteStore::open(&database.path, database.config).expect_err("must refuse");
    assert_eq!(error.code, StoreErrorCode::StoredDataInvalid);
    assert!(
        error.message.starts_with("reinitialize-required:"),
        "message={}",
        error.message
    );
    assert!(
        error.message.contains(expected_label_fragment),
        "message={}",
        error.message
    );

    let versions = applied_versions(&database);
    assert_eq!(
        versions,
        [1, 2, 3, 4],
        "0005 must not advance the ledger when refusing non-empty {table}"
    );
    assert!(
        table_exists_raw(&database, table),
        "{table} must remain after refused 0005"
    );
}

fn assert_open_refuses_recreated_legacy_table(
    table: &str,
    recreate_and_seed: impl FnOnce(&Connection),
) {
    let database = MigrationDatabase::new();
    SqliteStore::open(&database.path, database.config).expect("fresh baseline");
    assert_eq!(applied_versions(&database), [1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(
        !table_exists_raw(&database, table),
        "{table} must be dropped on fresh baseline"
    );

    let connection = database.raw();
    recreate_and_seed(&connection);
    let seeded: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("seed count");
    assert!(seeded > 0, "{table} must be non-empty before reopen");
    drop(connection);

    let error = SqliteStore::open(&database.path, database.config).expect_err("must refuse reopen");
    assert_eq!(error.code, StoreErrorCode::StoredDataInvalid);
    assert!(
        error.message.starts_with("reinitialize-required:"),
        "message={}",
        error.message
    );
    assert!(
        error.message.contains(table),
        "message must name {table}; got {}",
        error.message
    );
    assert_eq!(
        applied_versions(&database),
        [1, 2, 3, 4, 5, 6, 7, 8],
        "open refuse must not advance ledger"
    );
}

fn applied_versions(database: &MigrationDatabase) -> Vec<i64> {
    let connection = database.raw();
    let mut statement = connection
        .prepare("SELECT version FROM schema_migrations ORDER BY version")
        .expect("versions");
    statement
        .query_map([], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("collect")
}

fn table_exists_raw(database: &MigrationDatabase, table: &str) -> bool {
    let count: i64 = database
        .raw()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .expect("table probe");
    count == 1
}

#[test]
fn descriptor_half_shape_is_drift_but_too_new_has_priority() {
    let half = MigrationDatabase::new();
    let connection = half.raw();
    migration::create_v2_database_for_test(&connection).expect("v2 database");
    connection
        .execute_batch("ALTER TABLE schema_migrations ADD COLUMN descriptor_hash TEXT")
        .expect("half shape");
    drop(connection);
    assert_eq!(
        SqliteStore::open(&half.path, half.config)
            .expect_err("half shape")
            .code,
        StoreErrorCode::MigrationDrift
    );

    let too_new = MigrationDatabase::new();
    let connection = too_new.raw();
    migration::create_v2_database_for_test(&connection).expect("v2 database");
    connection
        .execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at) \
             VALUES (9, 'future', ?1, '2026-01-01T00:00:00Z')",
            ["a".repeat(64)],
        )
        .expect("future row");
    connection
        .execute_batch("ALTER TABLE schema_migrations ADD COLUMN descriptor_hash TEXT")
        .expect("half shape");
    drop(connection);
    assert_eq!(
        SqliteStore::open(&too_new.path, too_new.config)
            .expect_err("too new")
            .code,
        StoreErrorCode::DatabaseSchemaTooNew
    );
}

#[test]
fn raw_sql_constraints_reject_invalid_version_mapping_and_duplicate_dedup() {
    let database = MigrationDatabase::new();
    SqliteStore::open(&database.path, database.config).expect("open");
    let connection = database.raw();
    let base = (
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "task.created",
        "task",
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    );
    let mut next_event_id = 10_000_u128;
    let mut insert = |schema_version: i64, event_type: &str, aggregate_type: &str, dedup: &str| {
        next_event_id += 1;
        connection.execute(
            "INSERT INTO outbox(\
                event_id, event_type, schema_version, aggregate_type, aggregate_id, sequence, \
                occurred_at, causation_json, correlation_id, dedup_key, payload_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, 'correlation', ?8, ?9)",
            rusqlite::params![
                Uuid::from_u128(0x1000_0000_0000_4000_8000_0000_0000_0000 + next_event_id)
                    .to_string(),
                event_type,
                schema_version,
                aggregate_type,
                base.3,
                "2026-01-01T00:00:00+00:00",
                r#"{"id":"11111111-1111-4111-8111-111111111111","kind":"command_request"}"#,
                dedup,
                r#"{"schema_version":1}"#,
            ],
        )
    };
    assert!(insert(3, base.1, base.2, "bad-version").is_err());
    assert!(insert(1, "action.state_changed", "action", "bad-matrix").is_err());
    assert!(insert(2, "action.state_changed", "task", "bad-aggregate").is_err());
    insert(2, base.1, base.2, "same-dedup").expect("first dedup");
    assert!(insert(2, base.1, base.2, "same-dedup").is_err());
}

#[test]
fn concurrent_first_open_of_empty_file_reaches_0008_once() {
    for round in 0..5 {
        let database = MigrationDatabase::new();
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let path = database.path.clone();
            let config = database.config;
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                SqliteStore::open(&path, config).expect("concurrent open")
            }));
        }
        barrier.wait();
        let stores: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("join"))
            .collect();

        let connection = database.raw();
        let migration_rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(migration_rows, 8, "round {round}");
        drop(connection);

        for (index, store) in stores.iter().enumerate() {
            let task_id =
                Uuid::from_u128(0x2000_0000_0000_4000_8000_0000_0000_0000 + index as u128);
            let record = store
                .with_write_transaction(|transaction| {
                    transaction
                        .append_active_event_v2(active_task_created(task_id, index as u32 + 1))
                })
                .expect("writable after race");
            assert!(
                record
                    .envelope
                    .outbox_position()
                    .parse::<i64>()
                    .expect("pos")
                    > 0
            );
        }
    }
}

fn active_task_created(task_id: Uuid, number: u32) -> PendingActiveEventV2 {
    PendingActiveEventV2 {
        event_id: Uuid::from_u128(0x3000_0000_0000_4000_8000_0000_0000_0000 + number as u128),
        aggregate_id: EventAggregateId::Task(task_id),
        occurred_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, number).unwrap(),
        causation_ref: CausationRefV2::CommandRequest {
            id: "11111111-1111-4111-8111-111111111111".to_owned(),
        },
        correlation_id: format!("correlation-{number}"),
        dedup_key: format!("dedup-{number}"),
        payload: EventEnvelopeV2Payload::TaskCreated(Box::new(TaskCreatedPayload {
            created_at: Utc
                .with_ymd_and_hms(2026, 1, 1, 0, 0, number)
                .unwrap()
                .to_rfc3339(),
            goal: "migration test goal".to_owned(),
            proposer: TaskCreatedPayloadProposer::User,
            schema_version: TaskCreatedPayloadSchemaVersion,
            status: TaskStatus::Candidate,
            task_id: task_id.to_string(),
            task_revision: 1,
        })),
    }
}
