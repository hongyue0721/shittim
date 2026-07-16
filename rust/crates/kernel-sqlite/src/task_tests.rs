use super::*;
use chrono::{TimeZone, Timelike, Utc};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, AuditRecord,
    AuditRecordAuditType, AuditRecordExternalContentStatus, AuditRecordLevel, AuditRecordOutcome,
    AuditRecordRollbackCapability, AuditRecordSchemaVersion, AuditRecordTaskCreationContext,
    AuditRecordTaskCreationContextTaskRevision, CausationRef, CausationRefKind, EntryPoint,
    EventPayload, TaskCreateRequest, TaskCreatedPayloadSchemaVersion,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const FACT_TABLES: [&str; 8] = [
    "content_origins",
    "content_origin_parent_refs",
    "task_scopes",
    "task_scope_source_refs",
    "tasks",
    "task_create_idempotency",
    "audit_records",
    "outbox",
];

struct TaskDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl TaskDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        Self {
            path: directory.path().join("tasks.sqlite3"),
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

    fn open_raw_with_foreign_keys(&self) -> Connection {
        let connection = self.raw();
        connection
            .execute_batch("PRAGMA foreign_keys=ON")
            .expect("foreign keys");
        connection
    }
}

#[test]
fn migration_ledger_requires_a_continuous_binary_prefix() {
    let database = TaskDatabase::new();
    let connection = database.raw();
    connection
        .execute_batch(
            "CREATE TABLE schema_migrations(\
                version INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE,\
                checksum TEXT NOT NULL, applied_at TEXT NOT NULL\
             ) WITHOUT ROWID;",
        )
        .expect("ledger");
    connection
        .execute(
            "INSERT INTO schema_migrations(version,name,checksum,applied_at) \
             VALUES (2,'task_repository',?1,'2026-01-01T00:00:00Z')",
            ["0".repeat(64)],
        )
        .expect("only v2");
    drop(connection);
    assert_eq!(
        SqliteStore::open(&database.path, database.config)
            .expect_err("not a prefix")
            .code,
        StoreErrorCode::MigrationDrift
    );
}

#[test]
fn generated_unique_parent_key_and_deferred_cycle_work_in_bundled_sqlite() {
    let database = TaskDatabase::new();
    database.open();
    let connection = database.open_raw_with_foreign_keys();
    connection.execute_batch("BEGIN IMMEDIATE").expect("begin");
    let origin = canonical_document(json!({
        "schema_version":1,
        "id":"30000000-0000-4000-8000-000000000001",
        "kind":"user_input",
        "entry_point":"local_desktop",
        "source_uri":null,
        "upstream_stable_id":null,
        "producer_ref":{"kind":"actor","id":"actor"},
        "received_at":"2026-01-01T00:00:00Z",
        "carrier_ref":{"kind":"command_request","id":"10000000-0000-4000-8000-000000000001"},
        "parent_origin_refs":[],
        "kernel_receipt":{"receipt_id":"40000000-0000-4000-8000-000000000001","content_hash":"a".repeat(64),"recorded_at":"2026-01-01T00:00:00Z"}
    }));
    connection
        .execute(
            "INSERT INTO content_origins(record_json) VALUES (?1)",
            [&origin],
        )
        .expect("origin");
    let scope = canonical_document(json!({
        "id":"20000000-0000-4000-8000-000000000001","schema_version":1,"revision":1,
        "task_id":"00000000-0000-4000-8000-000000000001","resource_patterns":[],"exclusions":[],
        "allowed_capability_hints":[],"source_refs":["30000000-0000-4000-8000-000000000001"],
        "created_by":{"actor": serde_json::to_value(actor()).expect("actor"),"entry_point":"local_desktop"},
        "expires_at":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"
    }));
    let task = canonical_document(json!({
        "id":"00000000-0000-4000-8000-000000000001","origin_ref":"30000000-0000-4000-8000-000000000001",
        "actor":serde_json::to_value(actor()).expect("actor"),"proposer":"user","goal":"g","constraints":[],
        "success_criteria":[],"risk_hint":null,"capability_hints":[],"delegation_ref":null,
        "task_scope_ref":"20000000-0000-4000-8000-000000000001","parent_task_id":null,"status":"candidate",
        "plan_version":0,"schema_version":1,"revision":1,"created_at":"2026-01-01T00:00:00Z",
        "updated_at":"2026-01-01T00:00:00Z","failed_recovery_meta":null
    }));
    connection
        .execute("INSERT INTO task_scopes(record_json) VALUES (?1)", [&scope])
        .expect("deferred scope");
    connection
        .execute("INSERT INTO tasks(record_json) VALUES (?1)", [&task])
        .expect("task closes cycle");
    connection.execute_batch("COMMIT").expect("commit cycle");
    assert!(connection
        .execute(
            "INSERT INTO content_origins(record_json) VALUES (?1)",
            [&origin]
        )
        .is_err());
}

#[test]
fn task_create_fixture_normalizes_and_hashes_exactly() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/kcp/task_create_normalized_hash.v1.json"
    ))
    .expect("fixture");
    let envelope = &fixture["command_envelope"];
    let request: TaskCreateRequest =
        serde_json::from_value(envelope["payload"].clone()).expect("request");
    let command = command_from_fixture(envelope, request, 1);
    let prepared = super::task::prepare_create(&command).expect("prepare");
    assert_eq!(
        prepared.normalized_value_for_test(),
        fixture["normalized_payload"]
    );
    assert_eq!(
        prepared.receipt_hash_for_test(),
        fixture["receipt_content_hash"]
    );
    assert_eq!(
        prepared.projection_hash_for_test(),
        fixture["idempotency_projection_hash"]
    );
}

#[test]
fn task_create_success_exposes_complete_audit_and_event_then_replays() {
    let database = TaskDatabase::new();
    let store = database.open();
    let command = basic_command(1);
    let expected_task_id = command.allocation.task_id.clone();
    let expected_scope_id = command.allocation.task_scope_id.clone();
    let expected_origin_id = command.allocation.content_origin_id.clone();
    let result = store
        .with_write_transaction(|transaction| transaction.create_task(command.clone()))
        .expect("create");
    let created = match result {
        CreateTaskResult::Created { task } => task,
        other => panic!("unexpected result: {other:?}"),
    };
    let task = store
        .get_task(&expected_task_id)
        .expect("task")
        .expect("task exists");
    let scope = store
        .get_task_scope(&expected_scope_id)
        .expect("scope")
        .expect("scope exists");
    let origin = store
        .get_content_origin(&expected_origin_id)
        .expect("origin")
        .expect("origin exists");
    let timestamp = command.allocation.accepted_at.to_rfc3339();
    assert_eq!(task, created);
    assert_eq!(task.created_at, timestamp);
    assert_eq!(task.updated_at, timestamp);
    assert_eq!(scope.created_at, timestamp);
    assert_eq!(scope.updated_at, timestamp);
    assert_eq!(origin.received_at, timestamp);
    assert_eq!(origin.kernel_receipt.recorded_at, timestamp);
    assert_eq!(
        scope.source_refs.as_slice(),
        std::slice::from_ref(&origin.id)
    );

    let expected_audit = expected_creation_audit(&command, &task, &origin.id);
    assert_eq!(
        store
            .get_audit(&command.allocation.audit_id)
            .expect("audit read"),
        Some(expected_audit)
    );

    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("event read");
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.delivered_at, None);
    assert_eq!(event.envelope.event_id, command.allocation.event_id);
    assert_eq!(event.envelope.type_, "task.created");
    assert_eq!(event.envelope.aggregate_type, "task");
    assert_eq!(event.envelope.aggregate_id, task.id);
    assert_eq!(event.envelope.sequence, 0);
    assert_eq!(event.envelope.outbox_position, "1");
    assert_eq!(event.envelope.occurred_at, timestamp);
    assert_eq!(
        event.envelope.causation_ref,
        CausationRef {
            kind: CausationRefKind::CommandRequest,
            id: command.envelope.request_id.clone(),
        }
    );
    assert_eq!(
        event.envelope.correlation_id,
        command.allocation.correlation_id
    );
    assert_eq!(event.envelope.dedup_key, command.allocation.dedup_key);
    let EventPayload::TaskCreated(payload) = &event.envelope.payload else {
        panic!("task.created payload expected");
    };
    assert_eq!(payload.schema_version, TaskCreatedPayloadSchemaVersion);
    assert_eq!(payload.task_id, task.id);
    assert_eq!(payload.status, task.status);
    assert_eq!(payload.proposer.as_str(), task.proposer.as_str());
    assert_eq!(payload.goal, task.goal);
    assert_eq!(payload.task_revision, task.revision);
    assert_eq!(payload.created_at, task.created_at);
    assert_fact_counts(&database.raw(), &[1, 0, 1, 1, 1, 1, 1, 1]);

    let mut replay = command;
    replay.allocation = allocation(99);
    let result = store
        .with_write_transaction(|transaction| transaction.create_task(replay))
        .expect("replay");
    match result {
        CreateTaskResult::Replayed { task } => assert_eq!(task.id, expected_task_id),
        other => panic!("unexpected result: {other:?}"),
    }
    assert_fact_counts(&database.raw(), &[1, 0, 1, 1, 1, 1, 1, 1]);
}

#[test]
fn task_create_outer_panic_rolls_back_every_fact_and_retry_reuses_zero_allocations() {
    let database = TaskDatabase::new();
    let store = database.open();
    let command = basic_command(1);
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = store.with_write_transaction(|transaction| -> Result<(), StoreError> {
            let result = transaction.create_task(command.clone())?;
            assert!(matches!(result, CreateTaskResult::Created { .. }));
            panic!("panic after create");
        });
    }));
    assert!(panic.is_err());
    assert_fact_counts(&database.raw(), &[0; 8]);
    assert_table_count(&database.raw(), "aggregate_event_sequences", 0);
    assert!(store
        .get_task(&command.allocation.task_id)
        .expect("healthy task read")
        .is_none());
    assert!(store
        .get_audit(&command.allocation.audit_id)
        .expect("healthy audit read")
        .is_none());
    assert!(store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("healthy outbox read")
        .is_empty());

    let result = store
        .with_write_transaction(|transaction| transaction.create_task(command.clone()))
        .expect("retry");
    assert!(matches!(result, CreateTaskResult::Created { .. }));
    let event = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("outbox")
        .pop()
        .expect("event");
    assert_eq!(event.envelope.sequence, 0);
    assert_eq!(event.envelope.outbox_position, "1");
}

#[test]
fn task_create_conflict_and_ignored_error_leave_no_partial_facts() {
    let database = TaskDatabase::new();
    let store = database.open();
    let command = basic_command(1);
    store
        .with_write_transaction(|transaction| transaction.create_task(command.clone()))
        .expect("create");
    let mut conflict = command.clone();
    conflict.request.goal = "different goal".into();
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_task(conflict))
            .expect_err("conflict")
            .code,
        StoreErrorCode::IdempotencyConflict
    );

    let mut invalid = basic_command(2);
    invalid.allocation.event_id = command.allocation.event_id;
    store
        .with_write_transaction(|transaction| {
            let error = transaction
                .create_task(invalid)
                .expect_err("duplicate event");
            assert_eq!(error.code, StoreErrorCode::ConstraintViolation);
            Ok(())
        })
        .expect("outer commit after ignored error");
    assert!(store
        .get_task("00000000-0000-4000-8000-000000000002")
        .expect("read")
        .is_none());
}

#[test]
fn duplicate_allocated_ids_each_rollback_to_the_baseline() {
    for duplicate in [
        DuplicateAllocation::Task,
        DuplicateAllocation::Scope,
        DuplicateAllocation::Origin,
        DuplicateAllocation::Receipt,
        DuplicateAllocation::Audit,
        DuplicateAllocation::Event,
    ] {
        assert_duplicate_allocation_rolls_back(duplicate);
    }
}

#[derive(Debug, Clone, Copy)]
enum DuplicateAllocation {
    Task,
    Scope,
    Origin,
    Receipt,
    Audit,
    Event,
}

fn assert_duplicate_allocation_rolls_back(duplicate: DuplicateAllocation) {
    let database = TaskDatabase::new();
    let store = database.open();
    let baseline = basic_command(1);
    store
        .with_write_transaction(|transaction| transaction.create_task(baseline.clone()))
        .expect("baseline");
    let mut command = basic_command(2);
    match duplicate {
        DuplicateAllocation::Task => {
            command.allocation.task_id = baseline.allocation.task_id.clone()
        }
        DuplicateAllocation::Scope => {
            command.allocation.task_scope_id = baseline.allocation.task_scope_id
        }
        DuplicateAllocation::Origin => {
            command.allocation.content_origin_id = baseline.allocation.content_origin_id
        }
        DuplicateAllocation::Receipt => {
            command.allocation.receipt_id = baseline.allocation.receipt_id
        }
        DuplicateAllocation::Audit => command.allocation.audit_id = baseline.allocation.audit_id,
        DuplicateAllocation::Event => command.allocation.event_id = baseline.allocation.event_id,
    }
    let new_task_id = command.allocation.task_id.clone();
    let error = store
        .with_write_transaction(|transaction| transaction.create_task(command))
        .expect_err("duplicate allocation");
    assert_eq!(
        error.code,
        StoreErrorCode::ConstraintViolation,
        "duplicate {duplicate:?}"
    );
    assert_fact_counts(&database.raw(), &[1, 0, 1, 1, 1, 1, 1, 1]);
    assert_table_count(&database.raw(), "aggregate_event_sequences", 1);
    if new_task_id != baseline.allocation.task_id {
        assert!(store
            .get_task(&new_task_id)
            .expect("new task lookup")
            .is_none());
    }
}

#[test]
fn invalid_source_uri_and_scope_patterns_fail_closed_with_stable_code() {
    for invalid in [
        InvalidUriInput::Source,
        InvalidUriInput::Resource,
        InvalidUriInput::Exclusion,
    ] {
        let database = TaskDatabase::new();
        let store = database.open();
        let mut command = basic_command(1);
        match invalid {
            InvalidUriInput::Source => command.request.origin.source_uri = Some("not a uri".into()),
            InvalidUriInput::Resource => {
                command.request.task_scope.resource_patterns = vec!["https://exa*mple.com/a".into()]
            }
            InvalidUriInput::Exclusion => {
                command.request.task_scope.exclusions = vec!["https://example.com/a/*/bad*".into()]
            }
        }
        assert_eq!(
            store
                .with_write_transaction(|transaction| transaction.create_task(command))
                .expect_err("invalid URI input")
                .code,
            StoreErrorCode::InvalidScopePattern,
            "invalid {invalid:?}"
        );
        assert_fact_counts(&database.raw(), &[0; 8]);
    }
}

#[derive(Debug, Clone, Copy)]
enum InvalidUriInput {
    Source,
    Resource,
    Exclusion,
}

#[test]
fn idempotency_projection_hash_noncanonical_json_or_created_task_tamper_fails_closed() {
    let hash_database = TaskDatabase::new();
    let hash_store = hash_database.open();
    let hash_command = basic_command(1);
    hash_store
        .with_write_transaction(|transaction| transaction.create_task(hash_command.clone()))
        .expect("create for hash tamper");
    hash_database
        .raw()
        .execute(
            "UPDATE task_create_idempotency SET projection_hash = ?1",
            ["f".repeat(64)],
        )
        .expect("tamper hash");
    assert_eq!(
        hash_store
            .with_write_transaction(|transaction| transaction.create_task(hash_command))
            .expect_err("hash mismatch")
            .code,
        StoreErrorCode::StoredDataInvalid
    );

    let json_database = TaskDatabase::new();
    let json_store = json_database.open();
    let json_command = basic_command(1);
    json_store
        .with_write_transaction(|transaction| transaction.create_task(json_command.clone()))
        .expect("create for JSON tamper");
    let raw = json_database.raw();
    let projection: String = raw
        .query_row(
            "SELECT projection_json FROM task_create_idempotency",
            [],
            |row| row.get(0),
        )
        .expect("projection");
    let pretty = serde_json::to_string_pretty(
        &serde_json::from_str::<Value>(&projection).expect("parse projection"),
    )
    .expect("pretty projection");
    raw.execute(
        "UPDATE task_create_idempotency SET projection_json = ?1",
        [&pretty],
    )
    .expect("tamper canonical projection");
    assert_eq!(
        json_store
            .with_write_transaction(|transaction| transaction.create_task(json_command))
            .expect_err("noncanonical projection")
            .code,
        StoreErrorCode::StoredDataInvalid
    );

    let task_database = TaskDatabase::new();
    let task_store = task_database.open();
    let task_command = basic_command(1);
    task_store
        .with_write_transaction(|transaction| transaction.create_task(task_command.clone()))
        .expect("create for task tamper");
    let raw = task_database.raw();
    raw.execute_batch("PRAGMA foreign_keys=OFF")
        .expect("disable foreign keys");
    raw.execute(
        "UPDATE task_create_idempotency SET created_task_id = ?1",
        ["90000000-0000-4000-8000-000000000001"],
    )
    .expect("tamper created task");
    assert_eq!(
        task_store
            .with_write_transaction(|transaction| transaction.create_task(task_command))
            .expect_err("missing replay task")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
}

#[test]
fn parent_refs_preserve_duplicates_and_missing_refs_or_delegation_fail() {
    let database = TaskDatabase::new();
    let store = database.open();
    let parent = basic_command(1);
    let parent_task_id = parent.allocation.task_id.clone();
    let parent_origin_id = parent.allocation.content_origin_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_task(parent))
        .expect("parent");

    let mut child = basic_command(2);
    child.request.parent_task_id = Some(parent_task_id.clone());
    child.request.origin.parent_origin_refs =
        vec![parent_origin_id.clone(), parent_origin_id.clone()];
    store
        .with_write_transaction(|transaction| transaction.create_task(child.clone()))
        .expect("child");
    let child_origin = store
        .get_content_origin(&child.allocation.content_origin_id)
        .expect("origin")
        .expect("origin exists");
    assert_eq!(
        child_origin.parent_origin_refs,
        [parent_origin_id.clone(), parent_origin_id]
    );

    let mut missing_task = basic_command(3);
    missing_task.request.parent_task_id = Some("90000000-0000-4000-8000-000000000001".into());
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_task(missing_task))
            .expect_err("missing parent task")
            .code,
        StoreErrorCode::ParentTaskNotFound
    );
    let mut missing_origin = basic_command(4);
    missing_origin.request.origin.parent_origin_refs =
        vec!["90000000-0000-4000-8000-000000000002".into()];
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_task(missing_origin))
            .expect_err("missing parent origin")
            .code,
        StoreErrorCode::ParentOriginNotFound
    );
    let mut delegated = basic_command(5);
    delegated.request.delegation_ref = Some("90000000-0000-4000-8000-000000000003".into());
    assert_eq!(
        store
            .with_write_transaction(|transaction| transaction.create_task(delegated))
            .expect_err("delegation unavailable")
            .code,
        StoreErrorCode::DelegationNotFound
    );
}

#[test]
fn content_origin_parent_refs_reject_late_insert_without_locking_scope_updates() {
    let database = TaskDatabase::new();
    let store = database.open();
    let parent = basic_command(1);
    let parent_origin_id = parent.allocation.content_origin_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_task(parent))
        .expect("parent");
    let child = basic_command(2);
    let child_origin_id = child.allocation.content_origin_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_task(child))
        .expect("child");
    let connection = database.open_raw_with_foreign_keys();
    let error = connection
        .execute(
            "INSERT INTO content_origin_parent_refs(origin_id, ordinal, parent_origin_id) \
             VALUES (?1, 0, ?2)",
            [&child_origin_id, &parent_origin_id],
        )
        .expect_err("late insert rejected");
    assert_eq!(
        StoreError::sqlite(error, StoreErrorCode::InternalStoreError).code,
        StoreErrorCode::ConstraintViolation
    );
}

#[test]
fn parent_relation_delete_or_ordinal_tamper_is_detected_by_strict_read() {
    for tamper in [ParentTamper::Delete, ParentTamper::Ordinal] {
        let database = TaskDatabase::new();
        let store = database.open();
        let parent = basic_command(1);
        let parent_origin_id = parent.allocation.content_origin_id.clone();
        store
            .with_write_transaction(|transaction| transaction.create_task(parent))
            .expect("parent");
        let mut child = basic_command(2);
        child.request.origin.parent_origin_refs =
            vec![parent_origin_id.clone(), parent_origin_id.clone()];
        let child_origin_id = child.allocation.content_origin_id.clone();
        let child_task_id = child.allocation.task_id.clone();
        store
            .with_write_transaction(|transaction| transaction.create_task(child))
            .expect("child");
        let raw = database.raw();
        raw.execute_batch("DROP TRIGGER content_origin_parent_refs_immutable_delete")
            .expect("drop delete trigger for corruption fixture");
        match tamper {
            ParentTamper::Delete => {
                raw.execute(
                    "DELETE FROM content_origin_parent_refs WHERE origin_id = ?1 AND ordinal = 1",
                    [&child_origin_id],
                )
                .expect("delete parent relation");
            }
            ParentTamper::Ordinal => {
                raw.execute(
                    "DELETE FROM content_origin_parent_refs WHERE origin_id = ?1 AND ordinal = 0",
                    [&child_origin_id],
                )
                .expect("delete first relation");
            }
        }
        assert_eq!(
            store
                .get_content_origin(&child_origin_id)
                .expect_err("origin relation mismatch")
                .code,
            StoreErrorCode::StoredDataInvalid
        );
        assert_eq!(
            store
                .get_task(&child_task_id)
                .expect_err("task relation mismatch")
                .code,
            StoreErrorCode::StoredDataInvalid
        );
    }
}

#[derive(Clone, Copy)]
enum ParentTamper {
    Delete,
    Ordinal,
}

#[test]
fn multiple_stores_same_scope_different_hash_create_once_and_conflict_once() {
    let database = TaskDatabase::new();
    database.open();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for (goal, allocation_number) in [("goal A", 1_u32), ("goal B", 2_u32)] {
        let path = database.path.clone();
        let config = database.config;
        let barrier = Arc::clone(&barrier);
        let mut command = basic_command(1);
        command.request.goal = goal.into();
        command.allocation = allocation(allocation_number);
        handles.push(thread::spawn(move || {
            let store = SqliteStore::open(path, config)?;
            barrier.wait();
            store.with_write_transaction(|transaction| transaction.create_task(command))
        }));
    }
    barrier.wait();
    let results: Vec<Result<CreateTaskResult, StoreError>> = handles
        .into_iter()
        .map(|handle| handle.join().expect("join"))
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(CreateTaskResult::Created { .. })))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(error) if error.code == StoreErrorCode::IdempotencyConflict))
            .count(),
        1
    );
    assert_fact_counts(&database.raw(), &[1, 0, 1, 1, 1, 1, 1, 1]);
    assert_table_count(&database.raw(), "aggregate_event_sequences", 1);
    let store = database.open();
    let winner_id = results
        .iter()
        .find_map(|result| match result {
            Ok(CreateTaskResult::Created { task }) => Some(task.id.as_str()),
            _ => None,
        })
        .expect("winner");
    let loser_id = if winner_id == allocation(1).task_id {
        allocation(2).task_id
    } else {
        allocation(1).task_id
    };
    assert!(store.get_task(winner_id).expect("winner read").is_some());
    assert!(store.get_task(&loser_id).expect("loser read").is_none());
}

#[test]
fn strict_reads_fail_on_relation_or_noncanonical_tampering() {
    let database = TaskDatabase::new();
    let store = database.open();
    let command = basic_command(1);
    let task_id = command.allocation.task_id.clone();
    let scope_id = command.allocation.task_scope_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_task(command))
        .expect("create");
    let raw = database.raw();
    raw.execute_batch("PRAGMA foreign_keys=OFF")
        .expect("disable FK");
    raw.execute(
        "DELETE FROM task_scope_source_refs WHERE scope_id = ?1",
        [&scope_id],
    )
    .expect("tamper relation");
    assert_eq!(
        store
            .get_task(&task_id)
            .expect_err("relation mismatch")
            .code,
        StoreErrorCode::StoredDataInvalid
    );

    let second = basic_command(2);
    let second_task = second.allocation.task_id.clone();
    store
        .with_write_transaction(|transaction| transaction.create_task(second))
        .expect("second create");
    raw.execute_batch("DROP TRIGGER tasks_identity_guard")
        .expect("drop guard for corruption fixture");
    let stored: String = raw
        .query_row(
            "SELECT record_json FROM tasks WHERE id = ?1",
            [&second_task],
            |row| row.get(0),
        )
        .expect("stored");
    let pretty = serde_json::to_string_pretty(
        &serde_json::from_str::<Value>(&stored).expect("parse stored"),
    )
    .expect("pretty");
    raw.execute(
        "UPDATE tasks SET record_json = ?1 WHERE id = ?2",
        [&pretty, &second_task],
    )
    .expect("tamper canonical form");
    assert_eq!(
        store.get_task(&second_task).expect_err("noncanonical").code,
        StoreErrorCode::StoredDataInvalid
    );
}

fn expected_creation_audit(
    command: &TaskCreateCommand,
    task: &kernel_contracts::TaskSpec,
    origin_id: &str,
) -> AuditRecord {
    AuditRecord {
        action_id: None,
        actor: Some(command.envelope.actor.clone()),
        approval_record_ref: None,
        artifact_refs: vec![],
        audit_type: AuditRecordAuditType::TaskCreationRecorded,
        causation_ref: Some(CausationRef {
            id: command.envelope.request_id.clone(),
            kind: CausationRefKind::CommandRequest,
        }),
        content_origin_refs: vec![origin_id.into()],
        correlation_id: Some(command.allocation.correlation_id.clone()),
        delegation_ref: None,
        details: json!({}),
        entry_point: command.envelope.entry_point,
        extension_id: None,
        external_content_status: AuditRecordExternalContentStatus::NotSent,
        id: command.allocation.audit_id.clone(),
        level: AuditRecordLevel::UserActivity,
        model_call_refs: vec![],
        occurred_at: command.allocation.accepted_at.to_rfc3339(),
        outcome: AuditRecordOutcome::Succeeded,
        payload_manifest_refs: vec![],
        permission_decision_ref: None,
        policy_context: None,
        provider_id: None,
        reason_codes: vec!["task_created".into()],
        recovery_attempt_ref: None,
        resource_refs: vec![],
        rollback_capability: AuditRecordRollbackCapability::Unknown,
        schema_version: AuditRecordSchemaVersion,
        stop_fence_generation: None,
        summary: None,
        task_creation_context: Some(AuditRecordTaskCreationContext {
            goal: task.goal.clone(),
            origin_ref: task.origin_ref.clone(),
            proposer: serde_json::from_value(
                serde_json::to_value(task.proposer).expect("proposer value"),
            )
            .expect("audit proposer"),
            task_revision: AuditRecordTaskCreationContextTaskRevision,
        }),
        task_id: Some(task.id.clone()),
        verification_result_refs: vec![],
    }
}

fn assert_fact_counts(connection: &Connection, expected: &[i64; 8]) {
    for (table, expected_count) in FACT_TABLES.into_iter().zip(expected) {
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

fn basic_command(number: u32) -> TaskCreateCommand {
    let request: TaskCreateRequest = serde_json::from_value(json!({
        "schema_version": 1,
        "proposer": "user",
        "goal": format!("goal {number}"),
        "constraints": ["keep"],
        "success_criteria": ["done"],
        "risk_hint": null,
        "capability_hints": ["filesystem.read"],
        "task_scope": {
            "schema_version": 1,
            "resource_patterns": ["HTTPS://Example.COM:443/a/**"],
            "exclusions": ["https://example.com/a/tmp/*"],
            "allowed_capability_hints": ["filesystem.read"],
            "expires_at": null
        },
        "delegation_ref": null,
        "parent_task_id": null,
        "origin": {
            "schema_version": 1,
            "kind": "user_input",
            "source_uri": "HTTPS://Example.COM:443/inbox/./request",
            "upstream_stable_id": null,
            "producer_ref": {"kind": "actor", "id": "actor"},
            "parent_origin_refs": []
        }
    }))
    .expect("request");
    TaskCreateCommand {
        envelope: TaskCreateEnvelopeFacts {
            actor: actor(),
            entry_point: EntryPoint::LocalDesktop,
            request_id: format!("10000000-0000-4000-8000-{number:012}"),
            envelope_task_id: None,
            context: Some(json!({"conversation": number})),
            expected_revision: None,
            idempotency_key: format!("task-create-{number}"),
        },
        request,
        allocation: allocation(number),
    }
}

fn allocation(number: u32) -> TaskCreateAllocation {
    TaskCreateAllocation {
        task_id: format!("00000000-0000-4000-8000-{number:012}"),
        task_scope_id: format!("20000000-0000-4000-8000-{number:012}"),
        content_origin_id: format!("30000000-0000-4000-8000-{number:012}"),
        receipt_id: format!("40000000-0000-4000-8000-{number:012}"),
        audit_id: format!("50000000-0000-4000-8000-{number:012}"),
        event_id: format!("60000000-0000-4000-8000-{number:012}"),
        correlation_id: format!("correlation-{number}"),
        dedup_key: format!("dedup-{number}"),
        accepted_at: Utc
            .with_ymd_and_hms(2026, 7, 18, 12, 0, number % 60)
            .unwrap()
            .with_nanosecond(123_456_789)
            .expect("nanos"),
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

fn command_from_fixture(
    envelope: &Value,
    request: TaskCreateRequest,
    number: u32,
) -> TaskCreateCommand {
    TaskCreateCommand {
        envelope: TaskCreateEnvelopeFacts {
            actor: serde_json::from_value(envelope["actor"].clone()).expect("actor"),
            entry_point: serde_json::from_value(envelope["entry_point"].clone()).expect("entry"),
            request_id: envelope["request_id"].as_str().expect("request id").into(),
            envelope_task_id: serde_json::from_value(envelope["task_id"].clone()).expect("task id"),
            context: serde_json::from_value(envelope["context"].clone()).expect("context"),
            expected_revision: serde_json::from_value(envelope["expected_revision"].clone())
                .expect("revision"),
            idempotency_key: envelope["idempotency_key"].as_str().expect("key").into(),
        },
        request,
        allocation: allocation(number),
    }
}

fn canonical_document(value: Value) -> String {
    kernel_contracts::canonical_json_string(&value).expect("canonical")
}
