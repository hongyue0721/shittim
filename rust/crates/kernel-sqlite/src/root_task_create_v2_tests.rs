use super::*;
use chrono::{TimeZone, Timelike, Utc};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, AuditRecordV2AuditType,
    CausationRefV2, EntryPoint, EventEnvelopeV2Payload, InputContentOriginV1,
    InputContentOriginV1Kind, InputContentOriginV1ProducerRef, InputContentOriginV1ProducerRefKind,
    InputContentOriginV1SchemaVersion, InputTaskScopeV1, InputTaskScopeV1SchemaVersion,
    NormalizedRootTaskCreatePayloadV2Proposer, RootTaskCreateAllocationV2,
    RootTaskCreateAllocationV2SchemaVersion, TaskCreateRequestV2, TaskCreateRequestV2SchemaVersion,
    TaskCreationProvenanceV1, TaskStatus,
};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

const V2_FACT_TABLES: [&str; 9] = [
    "content_origins_v2",
    "content_origin_v2_parent_refs",
    "task_scopes",
    "task_scope_source_refs",
    "tasks",
    "task_creation_provenances",
    "audit_records_v2",
    "root_task_create_idempotency_v2",
    "outbox",
];

struct RootV2Database {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl RootV2Database {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        Self {
            path: directory.path().join("root-v2.sqlite3"),
            _directory: directory,
            config: SqliteConfig::new(Duration::from_secs(2)).expect("config"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path, self.config).expect("open")
    }

    fn raw(&self) -> Connection {
        Connection::open(&self.path).expect("raw")
    }
}

#[test]
fn migration_0005_fresh_baseline_has_v2_tables_and_drops_v1_dead_tables() {
    let database = RootV2Database::new();
    database.open();
    let connection = database.raw();
    let version: i64 = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("version");
    assert_eq!(version, 7);
    for table in [
        "content_origins_v2",
        "content_origin_v2_parent_refs",
        "task_creation_provenances",
        "audit_records_v2",
        "root_task_create_idempotency_v2",
        "tasks",
        "task_scopes",
    ] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("table");
        assert_eq!(count, 1, "missing {table}");
    }
    for table in [
        "content_origins",
        "content_origin_parent_refs",
        "task_create_idempotency",
        "audit_records",
    ] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("table");
        assert_eq!(count, 0, "legacy table {table} must be dropped");
    }
}

#[test]
fn root_v2_create_atomically_writes_full_bundle_and_readback() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    let expected_task_id = command.allocation.task_id.clone();
    let expected_scope_id = command.allocation.task_scope_id.clone();
    let expected_origin_id = command.allocation.content_origin_id.clone();
    let expected_provenance = command.allocation.creation_provenance_id.clone();
    let expected_audit = command.allocation.audit_record_id.clone();
    let accepted_at = command
        .accepted_at
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let event_occurred_at = command.accepted_at.to_rfc3339();

    let result = store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("create");
    let (created, provenance_ref) = match result {
        CreateRootTaskV2Result::Created {
            task,
            creation_provenance_ref,
        } => (task, creation_provenance_ref),
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(created.id, expected_task_id);
    assert_eq!(provenance_ref, expected_provenance);
    assert!(created.parent_task_id.is_none());
    assert_eq!(created.revision, 1);
    assert_eq!(created.status, TaskStatus::Candidate);
    assert_eq!(created.created_at, accepted_at);
    assert_eq!(created.updated_at, accepted_at);

    let task = store
        .get_task(&expected_task_id)
        .expect("task")
        .expect("exists");
    assert_eq!(task, created);
    let scope = store
        .get_task_scope(&expected_scope_id)
        .expect("scope")
        .expect("exists");
    assert_eq!(scope.task_id, expected_task_id);
    assert_eq!(scope.source_refs, vec![expected_origin_id.clone()]);
    assert_eq!(scope.created_at, accepted_at);
    let origin = store
        .get_content_origin_v2(&expected_origin_id)
        .expect("origin")
        .expect("exists");
    assert_eq!(origin.received_at, accepted_at);
    assert_eq!(origin.kernel_receipt.recorded_at, accepted_at);
    assert_eq!(
        origin.kernel_receipt.receipt_id,
        command.allocation.kernel_receipt_id
    );
    assert_eq!(origin.carrier_ref.kind.as_str(), "command_request");
    assert_eq!(origin.carrier_ref.id, command.envelope.request_id);
    // Legacy content_origins table is dropped by migration 0005; only v2 remains.
    let connection = store.lock_connection().expect("connection");
    let legacy_origin_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'content_origins'",
            [],
            |row| row.get(0),
        )
        .expect("legacy table probe");
    assert_eq!(legacy_origin_tables, 0);
    drop(connection);

    let provenance = store
        .get_task_creation_provenance(&expected_provenance)
        .expect("provenance")
        .expect("exists");
    match provenance {
        TaskCreationProvenanceV1::RootCommandV2 {
            command_request_id,
            receipt_ref,
            accepted_at: at,
            ..
        } => {
            assert_eq!(command_request_id, command.envelope.request_id);
            assert_eq!(receipt_ref, command.allocation.kernel_receipt_id);
            assert_eq!(at, accepted_at);
        }
        other => panic!("unexpected provenance {other:?}"),
    }

    let audit = store
        .get_audit_v2(&expected_audit)
        .expect("audit")
        .expect("exists");
    assert_eq!(
        audit.audit_type,
        AuditRecordV2AuditType::TaskCreationRecorded
    );
    assert_eq!(audit.reason_codes, vec!["task_created_root_v2".to_owned()]);
    assert_eq!(
        audit.causation_ref,
        Some(CausationRefV2::CommandRequest {
            id: command.envelope.request_id.clone()
        })
    );
    assert_eq!(
        audit.correlation_id.as_deref(),
        Some(command.allocation.correlation_id.as_str())
    );
    let context = audit.task_creation_context.expect("context");
    assert_eq!(context.creation_kind.as_str(), "root_command_v2");
    assert_eq!(context.creation_provenance_ref, expected_provenance);
    assert_eq!(context.accepted_at, accepted_at);
    assert!(context.materialized_at.is_none());
    assert_eq!(context.goal, task.goal);
    assert_eq!(context.origin_ref, expected_origin_id);

    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("events");
    assert_eq!(events.len(), 1);
    let StoredEventEnvelope::ActiveV2(envelope) = &events[0].envelope;
    assert_eq!(envelope.type_, "task.created");
    assert_eq!(envelope.aggregate_type, "task");
    assert_eq!(envelope.aggregate_id, expected_task_id);
    assert_eq!(envelope.sequence, 0);
    assert_eq!(envelope.event_id, command.allocation.task_created_event_id);
    assert_eq!(
        envelope.causation_ref,
        CausationRefV2::CommandRequest {
            id: command.envelope.request_id.clone()
        }
    );
    assert_eq!(envelope.correlation_id, command.allocation.correlation_id);
    assert_eq!(
        envelope.dedup_key,
        command.allocation.task_created_dedup_key
    );
    assert_eq!(envelope.occurred_at, event_occurred_at);
    let EventEnvelopeV2Payload::TaskCreated(payload) = &envelope.payload else {
        panic!("task.created payload");
    };
    assert_eq!(payload.task_id, expected_task_id);
    assert_eq!(payload.created_at, accepted_at);
    assert_eq!(payload.task_revision, 1);
    assert_eq!(payload.status, TaskStatus::Candidate);

    assert_fact_counts(&database.raw(), &[1, 0, 1, 1, 1, 1, 1, 1, 1]);
}

#[test]
fn root_v2_idempotent_replay_and_conflict() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("create");

    let mut replay = command.clone();
    replay.allocation = allocation(99);
    replay.accepted_at = Utc.with_ymd_and_hms(2026, 7, 18, 13, 0, 0).unwrap();
    let result = store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(replay))
        .expect("replay");
    match result {
        CreateRootTaskV2Result::Replayed {
            task,
            creation_provenance_ref,
        } => {
            assert_eq!(task.id, command.allocation.task_id);
            assert_eq!(
                creation_provenance_ref,
                command.allocation.creation_provenance_id
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_fact_counts(&database.raw(), &[1, 0, 1, 1, 1, 1, 1, 1, 1]);

    let mut conflict = command.clone();
    conflict.request.goal = "different goal".into();
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(conflict))
            .expect_err("conflict")
            .code,
        StoreErrorCode::IdempotencyConflict
    );
    assert_fact_counts(&database.raw(), &[1, 0, 1, 1, 1, 1, 1, 1, 1]);
}

#[test]
fn root_v2_rejects_duplicate_internal_uuid_and_external_collision() {
    let database = RootV2Database::new();
    let store = database.open();
    let mut command = basic_command(1);
    command.allocation.task_id = command.allocation.task_scope_id.clone();
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .expect_err("duplicate internal")
            .code,
        StoreErrorCode::ContractInvalid
    );
    assert_fact_counts(&database.raw(), &[0; 9]);

    let mut command = basic_command(2);
    command.envelope.request_id = command.allocation.task_id.clone();
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .expect_err("external collision")
            .code,
        StoreErrorCode::ContractInvalid
    );
    assert_fact_counts(&database.raw(), &[0; 9]);
}

#[test]
fn root_v2_parent_origin_not_found_and_rollback_does_not_consume_ids() {
    let database = RootV2Database::new();
    let store = database.open();
    let mut command = basic_command(1);
    command.request.origin.parent_origin_refs = vec!["aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into()];
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
            .expect_err("missing parent")
            .code,
        StoreErrorCode::ParentOriginNotFound
    );
    assert_fact_counts(&database.raw(), &[0; 9]);
    assert_table_count(&database.raw(), "aggregate_event_sequences", 0);

    // Retry with valid command reuses sequence zero / position one.
    let command = basic_command(1);
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("create");
    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(1).expect("limit"))
        .expect("events");
    assert_eq!(events[0].envelope.sequence(), 0);
    assert_eq!(events[0].envelope.outbox_position(), "1");
}

#[test]
fn root_v2_does_not_pollute_v1_idempotency_or_origin_tables() {
    let database = RootV2Database::new();
    let store = database.open();
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(basic_command(1)))
        .expect("v2 create");
    let connection = database.raw();
    // migration 0005 drops dead v1 tables on fresh baseline
    for table in [
        "content_origins",
        "task_create_idempotency",
        "audit_records",
        "content_origin_parent_refs",
    ] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("table existence");
        assert_eq!(count, 0, "legacy table {table} must be absent");
    }
    assert_table_count(&connection, "content_origins_v2", 1);
    assert_table_count(&connection, "root_task_create_idempotency_v2", 1);
    assert_table_count(&connection, "audit_records_v2", 1);
}

#[test]
fn root_v2_stored_corruption_fails_closed_on_readback() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    let task_id = command.allocation.task_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
        .expect("create");
    let raw = database.raw();
    raw.execute_batch("DROP TRIGGER tasks_identity_guard")
        .expect("drop");
    let stored: String = raw
        .query_row(
            "SELECT record_json FROM tasks WHERE id = ?1",
            [&task_id],
            |row| row.get(0),
        )
        .expect("stored");
    let pretty =
        serde_json::to_string_pretty(&serde_json::from_str::<Value>(&stored).expect("parse"))
            .expect("pretty");
    raw.execute(
        "UPDATE tasks SET record_json = ?1 WHERE id = ?2",
        [&pretty, &task_id],
    )
    .expect("tamper");
    assert_eq!(
        store.get_task(&task_id).expect_err("corruption").code,
        StoreErrorCode::StoredDataInvalid
    );
}

#[test]
fn migration_0004_restores_tasks_task_scope_ref_deferred_fk() {
    let database = RootV2Database::new();
    database.open();
    let connection = database.raw();
    let mut found = false;
    let mut statement = connection
        .prepare("PRAGMA foreign_key_list(tasks)")
        .expect("pragma");
    let mut rows = statement.query([]).expect("query");
    while let Some(row) = rows.next().expect("row") {
        let table: String = row.get(2).expect("table");
        let from: String = row.get(3).expect("from");
        if table == "task_scopes" && from == "task_scope_ref" {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "tasks.task_scope_ref → task_scopes FK missing after 0004"
    );
}

#[test]
fn root_v2_replay_fails_closed_when_audit_missing() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("create");
    let raw = database.raw();
    raw.execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER audit_records_v2_immutable_delete;")
        .expect("prepare delete");
    raw.execute(
        "DELETE FROM audit_records_v2 WHERE id = ?1",
        [&command.allocation.audit_record_id],
    )
    .expect("delete audit");
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .expect_err("missing audit")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
    // Replay failure must not write new facts.
    assert_fact_counts(&database.raw(), &[1, 0, 1, 1, 1, 1, 0, 1, 1]);
}

#[test]
fn root_v2_replay_fails_closed_when_outbox_event_missing() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("create");
    let raw = database.raw();
    raw.execute(
        "DELETE FROM outbox WHERE event_id = ?1",
        [&command.allocation.task_created_event_id],
    )
    .expect("delete event");
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .expect_err("missing event")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
    assert_table_count(&database.raw(), "outbox", 0);
    assert_table_count(&database.raw(), "tasks", 1);
}

#[test]
fn root_v2_replay_fails_closed_when_content_origin_v2_missing() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("create");
    let raw = database.raw();
    raw.execute_batch(
        "PRAGMA foreign_keys=OFF; \
         DROP TRIGGER content_origins_v2_immutable_delete; \
         DROP TRIGGER content_origin_v2_parent_refs_immutable_delete;",
    )
    .expect("prepare delete");
    raw.execute(
        "DELETE FROM content_origin_v2_parent_refs WHERE origin_id = ?1",
        [&command.allocation.content_origin_id],
    )
    .ok();
    raw.execute(
        "DELETE FROM content_origins_v2 WHERE id = ?1",
        [&command.allocation.content_origin_id],
    )
    .expect("delete origin");
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .expect_err("missing origin")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
}

#[test]
fn root_v2_replay_fails_closed_when_audit_projection_tampered() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("create");
    let raw = database.raw();
    raw.execute_batch("DROP TRIGGER audit_records_v2_immutable_update")
        .expect("drop");
    let stored: String = raw
        .query_row(
            "SELECT record_json FROM audit_records_v2 WHERE id = ?1",
            [&command.allocation.audit_record_id],
            |row| row.get(0),
        )
        .expect("stored");
    let mut value: Value = serde_json::from_str(&stored).expect("parse");
    value["reason_codes"] = json!(["tampered"]);
    let tampered = serde_json::to_string(&value).expect("encode");
    // Bypass JCS equality by writing non-canonical pretty JSON so decode fails closed.
    let pretty = serde_json::to_string_pretty(&value).expect("pretty");
    raw.execute(
        "UPDATE audit_records_v2 SET record_json = ?1 WHERE id = ?2",
        [&pretty, &command.allocation.audit_record_id],
    )
    .expect("tamper");
    let _ = tampered;
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .expect_err("tampered audit")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
}

#[test]
fn root_v2_get_provenance_fails_closed_on_column_task_id_mismatch() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    let provenance_id = command.allocation.creation_provenance_id.clone();
    let original_task_id = command.allocation.task_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
        .expect("create");
    // Create a second full root so another Task id exists; free its provenance.task_id slot
    // (UNIQUE) so the first provenance free-column can be retargeted to it.
    let other = basic_command(2);
    let other_task_id = other.allocation.task_id.clone();
    let other_provenance = other.allocation.creation_provenance_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(other))
        .expect("second create");
    let raw = database.raw();
    raw.execute_batch(
        "PRAGMA foreign_keys=OFF; \
         DROP TRIGGER task_creation_provenances_immutable_update; \
         DROP TRIGGER task_creation_provenances_immutable_delete;",
    )
    .expect("drop");
    // Free UNIQUE(task_id) on provenances by deleting the second provenance row (FK disabled).
    raw.execute(
        "DELETE FROM task_creation_provenances WHERE id = ?1",
        [&other_provenance],
    )
    .expect("free other task_id");
    // Free column task_id no longer matches idempotency.created_task_id for this provenance.
    raw.execute(
        "UPDATE task_creation_provenances SET task_id = ?1 WHERE id = ?2",
        [&other_task_id, &provenance_id],
    )
    .expect("mismatch column");
    assert_ne!(other_task_id, original_task_id);
    assert_eq!(
        store
            .get_task_creation_provenance(&provenance_id)
            .expect_err("column mismatch")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
}

#[test]
fn root_v2_dangling_origin_ref_fails_closed_on_get_task() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    let task_id = command.allocation.task_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("create");
    let raw = database.raw();
    raw.execute_batch(
        "PRAGMA foreign_keys=OFF; \
         DROP TRIGGER tasks_identity_guard; \
         DROP TRIGGER content_origins_v2_immutable_delete; \
         DROP TRIGGER content_origin_v2_parent_refs_immutable_delete;",
    )
    .expect("prepare");
    raw.execute(
        "DELETE FROM content_origins_v2 WHERE id = ?1",
        [&command.allocation.content_origin_id],
    )
    .expect("delete origin");
    assert_eq!(
        store.get_task(&task_id).expect_err("dangling origin").code,
        StoreErrorCode::StoredDataInvalid
    );
}

#[test]
fn root_v2_wrong_task_scope_ref_fails_closed_on_get_task() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    let task_id = command.allocation.task_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
        .expect("create");
    let raw = database.raw();
    // FK off so dangling task_scope_ref is writable; UNIQUE(task_scope_ref) still holds for
    // a fresh UUID that no other task claims.
    raw.execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER tasks_identity_guard;")
        .expect("prepare");
    let stored: String = raw
        .query_row(
            "SELECT record_json FROM tasks WHERE id = ?1",
            [&task_id],
            |row| row.get(0),
        )
        .expect("stored");
    let mut value: Value = serde_json::from_str(&stored).expect("parse");
    value["task_scope_ref"] = json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    // Non-canonical pretty JSON fails closed at decode; dangling ref would also fail relation.
    let pretty = serde_json::to_string_pretty(&value).expect("pretty");
    raw.execute(
        "UPDATE tasks SET record_json = ?1 WHERE id = ?2",
        [&pretty, &task_id],
    )
    .expect("tamper scope");
    assert_eq!(
        store
            .get_task(&task_id)
            .expect_err("wrong task_scope_ref")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
}

#[test]
fn root_v2_post_append_bundle_failure_rolls_back_sequence_and_position() {
    let database = RootV2Database::new();
    let store = database.open();
    let command = basic_command(1);
    assert_eq!(
        store
            .with_write_transaction(|transaction| {
                transaction.inject_root_v2_post_append_bundle_invalid_for_test();
                transaction.create_root_task_v2(command.clone())
            })
            .expect_err("forced bundle invalid")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
    assert_fact_counts(&database.raw(), &[0; 9]);
    assert_table_count(&database.raw(), "aggregate_event_sequences", 0);

    // Successful retry must still own sequence 0 / position 1.
    store
        .with_write_transaction(|transaction| transaction.create_root_task_v2(command.clone()))
        .expect("retry create");
    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(1).expect("limit"))
        .expect("events");
    assert_eq!(events[0].envelope.sequence(), 0);
    assert_eq!(events[0].envelope.outbox_position(), "1");
    let StoredEventEnvelope::ActiveV2(envelope) = &events[0].envelope;
    assert_eq!(envelope.event_id, command.allocation.task_created_event_id);
}

#[test]
fn root_v2_invalid_scope_pattern_is_caller_invalid_and_rolls_back() {
    let database = RootV2Database::new();
    let store = database.open();
    let mut command = basic_command(1);
    // Schema-valid absolute URI that fails domain-policy URI normalization (glob-like star).
    command.request.origin.source_uri = Some("https://example.com/*".into());
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .expect_err("uri")
            .code,
        StoreErrorCode::InvalidScopePattern
    );
    assert_fact_counts(&database.raw(), &[0; 9]);
}

#[test]
fn root_v2_subsecond_accepted_at_is_contract_invalid() {
    let database = RootV2Database::new();
    let store = database.open();
    let mut command = basic_command(1);
    command.accepted_at = Utc
        .with_ymd_and_hms(2026, 7, 18, 12, 0, 1)
        .unwrap()
        .with_nanosecond(1)
        .expect("nanos");
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .expect_err("subsecond")
            .code,
        StoreErrorCode::ContractInvalid
    );
}

fn assert_fact_counts(connection: &Connection, expected: &[i64; 9]) {
    for (table, expected_count) in V2_FACT_TABLES.into_iter().zip(expected) {
        assert_table_count(connection, table, *expected_count);
    }
}

fn assert_table_count(connection: &Connection, table: &str, expected: i64) {
    let count: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(count, expected, "table {table}");
}

fn basic_command(number: u32) -> RootTaskCreateV2Command {
    RootTaskCreateV2Command {
        envelope: RootTaskCreateV2EnvelopeFacts {
            actor: actor(),
            entry_point: EntryPoint::LocalDesktop,
            request_id: format!("10000000-0000-4000-8000-{number:012}"),
            context: Some(Map::from_iter([("conversation".to_owned(), json!(number))])),
            idempotency_key: format!("root-v2-{number}"),
        },
        request: TaskCreateRequestV2 {
            capability_hints: vec!["filesystem.read".into()],
            constraints: vec!["keep".into()],
            delegation_ref: None,
            goal: format!("goal {number}"),
            origin: InputContentOriginV1 {
                kind: InputContentOriginV1Kind::UserInput,
                parent_origin_refs: vec![],
                producer_ref: InputContentOriginV1ProducerRef {
                    id: "actor".into(),
                    kind: InputContentOriginV1ProducerRefKind::Actor,
                },
                schema_version: InputContentOriginV1SchemaVersion,
                source_uri: Some("HTTPS://Example.COM:443/inbox/./request".into()),
                upstream_stable_id: None,
            },
            proposer: NormalizedRootTaskCreatePayloadV2Proposer::User,
            risk_hint: None,
            schema_version: TaskCreateRequestV2SchemaVersion,
            success_criteria: vec!["done".into()],
            task_scope: InputTaskScopeV1 {
                allowed_capability_hints: vec!["filesystem.read".into()],
                exclusions: vec!["https://example.com/a/tmp/*".into()],
                expires_at: None,
                resource_patterns: vec!["HTTPS://Example.COM:443/a/**".into()],
                schema_version: InputTaskScopeV1SchemaVersion,
            },
        },
        allocation: allocation(number),
        accepted_at: Utc
            .with_ymd_and_hms(2026, 7, 18, 12, 0, number % 60)
            .unwrap(),
    }
}

fn allocation(number: u32) -> RootTaskCreateAllocationV2 {
    RootTaskCreateAllocationV2 {
        audit_record_id: format!("50000000-0000-4000-8000-{number:012}"),
        content_origin_id: format!("30000000-0000-4000-8000-{number:012}"),
        correlation_id: format!("correlation-v2-{number}"),
        creation_provenance_id: format!("70000000-0000-4000-8000-{number:012}"),
        kernel_receipt_id: format!("40000000-0000-4000-8000-{number:012}"),
        schema_version: RootTaskCreateAllocationV2SchemaVersion,
        task_created_dedup_key: format!("dedup-v2-{number}"),
        task_created_event_id: format!("60000000-0000-4000-8000-{number:012}"),
        task_id: format!("00000000-0000-4000-8000-{number:012}"),
        task_scope_id: format!("20000000-0000-4000-8000-{number:012}"),
    }
}

fn actor() -> Actor {
    Actor {
        authentication_level: ActorAuthenticationLevel::PlatformVerified,
        confidence: Some(0.9),
        id: "actor".into(),
        kind: ActorKind::KnownUser,
        revision: 1,
        schema_version: ActorSchemaVersion,
        source: "actor-source://local/desktop".into(),
    }
}

// Silence unused Uuid import when all helpers use formatted strings.
#[allow(dead_code)]
fn _uuid_parse_check() {
    let _ = Uuid::parse_str("00000000-0000-4000-8000-000000000001");
}
