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
fn migration_0003_preserves_legacy_bytes_positions_delivery_and_sequence() {
    let database = MigrationDatabase::new();
    let connection = database.raw();
    migration::create_v2_database_for_test(&connection).expect("v2 database");
    let event = legacy_event(1, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let expected =
        outbox::append_legacy_v1_storage_for_test(&connection, event).expect("legacy append");
    connection
        .execute(
            "UPDATE outbox SET delivered_at = ?1 WHERE outbox_position = 1",
            ["2026-01-01T00:00:05+00:00"],
        )
        .expect("delivery");
    let payload_before: String = connection
        .query_row("SELECT payload_json FROM outbox", [], |row| row.get(0))
        .expect("payload before");
    drop(connection);

    let store = SqliteStore::open(&database.path, database.config).expect("upgrade");
    let record = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("read")
        .pop()
        .expect("row");
    assert_eq!(record.envelope, expected.envelope);
    assert_eq!(
        record.delivered_at,
        Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 5).unwrap())
    );
    let connection = database.raw();
    let (payload_after, causation, position, sequence): (String, String, i64, i64) = connection
        .query_row(
            "SELECT payload_json, causation_json, outbox_position, sequence FROM outbox",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("post migration row");
    assert_eq!(payload_after, payload_before);
    assert_eq!(
        causation,
        r#"{"id":"11111111-1111-4111-8111-111111111111","kind":"command_request"}"#
    );
    assert_eq!((position, sequence), (1, 0));
    let sqlite_sequence: i64 = connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'outbox'",
            [],
            |row| row.get(0),
        )
        .expect("sqlite sequence");
    assert_eq!(sqlite_sequence, 1);
}

#[test]
fn migration_corruption_rolls_back_ledger_columns_replacement_and_row() {
    let database = MigrationDatabase::new();
    let connection = database.raw();
    migration::create_v2_database_for_test(&connection).expect("v2 database");
    outbox::append_legacy_v1_storage_for_test(
        &connection,
        legacy_event(1, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
    )
    .expect("legacy append");
    connection
        .execute(
            "UPDATE outbox SET payload_json = ?1 WHERE outbox_position = 1",
            [r#"{"schema_version":1}"#],
        )
        .expect("corrupt payload");
    drop(connection);

    assert_eq!(
        SqliteStore::open(&database.path, database.config)
            .expect_err("migration must fail")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
    let connection = database.raw();
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
    let migration_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("migration count");
    assert_eq!(migration_count, 2);
    let replacement_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master \
             WHERE type = 'table' AND name = 'outbox_versioned_replacement'",
            [],
            |row| row.get(0),
        )
        .expect("replacement count");
    assert_eq!(replacement_count, 0);
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
             VALUES (4, 'future', ?1, '2026-01-01T00:00:00Z')",
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
fn concurrent_upgrade_to_0003_with_legacy_data_leaves_both_stores_writable() {
    // 回归守护：竞态中输家在锁内看到 0003 已应用时，曾带着未关闭的
    // BEGIN IMMEDIATE 返回，使该连接后续所有写事务失败。多跑几轮以降低
    // “输家未真正进入 already-applied 分支”的偶然性。
    for round in 0..5 {
        let database = MigrationDatabase::new();
        let connection = database.raw();
        migration::create_v2_database_for_test(&connection).expect("v2 database");
        outbox::append_legacy_v1_storage_for_test(
            &connection,
            legacy_event(1, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
        )
        .expect("legacy append");
        drop(connection);

        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let path = database.path.clone();
            let config = database.config;
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                SqliteStore::open(&path, config).expect("concurrent upgrade")
            }));
        }
        barrier.wait();
        let stores: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("join"))
            .collect();

        let connection = database.raw();
        let (migration_rows, outbox_rows): (i64, i64) = connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM schema_migrations WHERE version = 3), \
                 (SELECT COUNT(*) FROM outbox)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("post race counts");
        assert_eq!((migration_rows, outbox_rows), (1, 1), "round {round}");
        drop(connection);

        for (index, store) in stores.iter().enumerate() {
            let number = index as u32 + 2;
            let record = store
                .with_write_transaction(|transaction| {
                    transaction.append_legacy_event_v1(legacy_event(
                        number,
                        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                    ))
                })
                .expect("store writable after migration race");
            assert_eq!(
                record.envelope.outbox_position(),
                number.to_string(),
                "round {round}"
            );
        }

        let events = stores[0]
            .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
            .expect("read after race");
        assert_eq!(events.len(), 3, "round {round}");
        assert_eq!(
            events
                .iter()
                .map(|record| record.envelope.sequence())
                .collect::<Vec<_>>(),
            vec![0, 1, 2],
            "round {round}"
        );
    }
}

fn legacy_event(number: u32, aggregate_id: &str) -> PendingLegacyEventV1 {
    let instant = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, number).unwrap();
    PendingLegacyEventV1 {
        event_id: format!("{number:08x}-0000-4000-8000-{number:012x}"),
        event_type: kernel_contracts::EventEnvelopeType::TaskCreated,
        aggregate_type: "task".to_owned(),
        aggregate_id: aggregate_id.to_owned(),
        occurred_at: instant,
        causation_ref: kernel_contracts::CausationRef {
            kind: kernel_contracts::CausationRefKind::CommandRequest,
            id: "11111111-1111-4111-8111-111111111111".to_owned(),
        },
        correlation_id: format!("correlation-{number}"),
        dedup_key: format!("dedup-{number}"),
        payload: json!({
            "schema_version": 1,
            "task_id": aggregate_id,
            "status": "candidate",
            "proposer": "user",
            "goal": "migration test goal",
            "task_revision": 1,
            "created_at": instant.to_rfc3339(),
        }),
    }
}

#[allow(dead_code)]
fn active_task_created_payload(task_id: Uuid) -> EventEnvelopeV2Payload {
    EventEnvelopeV2Payload::TaskCreated(Box::new(TaskCreatedPayload {
        created_at: "2026-01-01T00:00:00+00:00".to_owned(),
        goal: "active task".to_owned(),
        proposer: TaskCreatedPayloadProposer::User,
        schema_version: TaskCreatedPayloadSchemaVersion,
        status: TaskStatus::Candidate,
        task_id: task_id.to_string(),
        task_revision: 1,
    }))
}

#[allow(dead_code)]
fn active_causation() -> CausationRefV2 {
    CausationRefV2::CommandRequest {
        id: "11111111-1111-4111-8111-111111111111".to_owned(),
    }
}
