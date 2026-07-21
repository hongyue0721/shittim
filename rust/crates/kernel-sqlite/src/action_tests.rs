//! Action repository + ActionTransitionIntent + action.state_changed producer tests (slice 4a).

use super::*;
use crate::action::TransitionActionCommand;
use chrono::{TimeZone, Timelike, Utc};
use domain_task::ActionEvidence;
use kernel_contracts::{
    ActionRequestV2Result, ActionStatus, ActionTransitionIntentV1,
    ActionTransitionIntentV1SchemaVersion, Actor, ActorAuthenticationLevel, ActorKind,
    ActorSchemaVersion, CausationRefV2, EntryPoint, EventEnvelopeV2Payload, InputContentOriginV1,
    InputContentOriginV1Kind, InputContentOriginV1ProducerRef, InputContentOriginV1ProducerRefKind,
    InputContentOriginV1SchemaVersion, InputTaskScopeV1, InputTaskScopeV1SchemaVersion,
    NormalizedRootTaskCreatePayloadV2Proposer, RootTaskCreateAllocationV2,
    RootTaskCreateAllocationV2SchemaVersion, SideEffectClass, TaskCreateRequestV2,
    TaskCreateRequestV2SchemaVersion,
};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

struct ActionDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl ActionDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        Self {
            path: directory.path().join("action.sqlite3"),
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
fn migration_0006_creates_action_tables() {
    let database = ActionDatabase::new();
    database.open();
    let connection = database.raw();
    let version: i64 = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("version");
    assert_eq!(version, 8);
    for table in ["actions", "action_transition_intents"] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("table");
        assert_eq!(count, 1, "missing {table}");
    }
}

#[test]
fn insert_pending_action_and_canonical_readback() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let command = pending_command(&task, 1);
    let action_id = command.action_id.clone();
    let created = store
        .with_write_transaction(|tx| tx.insert_pending_action(command.clone()))
        .expect("insert");
    assert_eq!(created.action_id, action_id);
    assert_eq!(created.status, ActionStatus::Pending);
    assert_eq!(created.revision, 1);
    assert!(created.permission_decision_ref.is_none());
    assert!(created.approval_chain_id.is_none());
    assert!(created.result.is_none());
    assert!(created.lease.is_none());

    let read = store.get_action(&action_id).expect("get").expect("exists");
    assert_eq!(read, created);
}

#[test]
fn insert_pending_requires_existing_task() {
    let database = ActionDatabase::new();
    let store = database.open();
    let command = pending_command("00000000-0000-4000-8000-000000000099", 1);
    assert_eq!(
        store
            .with_write_transaction(|tx| tx.insert_pending_action(command))
            .expect_err("missing task")
            .code,
        StoreErrorCode::NotFound
    );
}

#[test]
fn transition_pending_to_cancelled_with_cas() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let pending = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("insert");
    let updated = store
        .with_write_transaction(|tx| {
            tx.transition_with_expected_revision(TransitionActionCommand {
                action_id: pending.action_id.clone(),
                expected_revision: 1,
                expected_status: ActionStatus::Pending,
                target_status: ActionStatus::Cancelled,
                reason: "user_cancelled".into(),
                evidence: ActionEvidence::default(),
                result: None,
                permission_decision_ref: None,
                approval_chain_id: None,
                lease: None,
                recovery_meta: None,
                updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 1).unwrap(),
            })
        })
        .expect("transition");
    assert_eq!(updated.status, ActionStatus::Cancelled);
    assert_eq!(updated.revision, 2);
    let read = store
        .get_action(&pending.action_id)
        .expect("get")
        .expect("exists");
    assert_eq!(read, updated);
}

#[test]
fn transition_revision_conflict() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let pending = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("insert");
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.transition_with_expected_revision(TransitionActionCommand {
                    action_id: pending.action_id.clone(),
                    expected_revision: 99,
                    expected_status: ActionStatus::Pending,
                    target_status: ActionStatus::Cancelled,
                    reason: "stale".into(),
                    evidence: ActionEvidence::default(),
                    result: None,
                    permission_decision_ref: None,
                    approval_chain_id: None,
                    lease: None,
                    recovery_meta: None,
                    updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 1).unwrap(),
                })
            })
            .expect_err("conflict")
            .code,
        StoreErrorCode::ConstraintViolation
    );
    let still = store
        .get_action(&pending.action_id)
        .expect("get")
        .expect("exists");
    assert_eq!(still.revision, 1);
    assert_eq!(still.status, ActionStatus::Pending);
}

#[test]
fn transition_illegal_edge_fail_closed() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let pending = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("insert");
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.transition_with_expected_revision(TransitionActionCommand {
                    action_id: pending.action_id.clone(),
                    expected_revision: 1,
                    expected_status: ActionStatus::Pending,
                    target_status: ActionStatus::Completed,
                    reason: "illegal".into(),
                    evidence: ActionEvidence::default(),
                    result: None,
                    permission_decision_ref: None,
                    approval_chain_id: None,
                    lease: None,
                    recovery_meta: None,
                    updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 1).unwrap(),
                })
            })
            .expect_err("illegal")
            .code,
        StoreErrorCode::ContractInvalid
    );
}

#[test]
fn transition_pending_to_approved_requires_permission_evidence() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let pending = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("insert");
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.transition_with_expected_revision(TransitionActionCommand {
                    action_id: pending.action_id.clone(),
                    expected_revision: 1,
                    expected_status: ActionStatus::Pending,
                    target_status: ActionStatus::Approved,
                    reason: "allow".into(),
                    evidence: ActionEvidence::default(),
                    result: None,
                    permission_decision_ref: None,
                    approval_chain_id: None,
                    lease: None,
                    recovery_meta: None,
                    updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 1).unwrap(),
                })
            })
            .expect_err("missing evidence")
            .code,
        StoreErrorCode::ContractInvalid
    );
}

#[test]
fn action_stored_corruption_fails_closed() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let pending = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("insert");
    let raw = database.raw();
    raw.execute_batch("DROP TRIGGER actions_identity_guard;")
        .expect("drop trigger");
    let stored: String = raw
        .query_row(
            "SELECT record_json FROM actions WHERE id = ?1",
            [&pending.action_id],
            |row| row.get(0),
        )
        .expect("stored");
    let mut value: Value = serde_json::from_str(&stored).expect("parse");
    value["status"] = json!("pending");
    // Non-canonical pretty JSON must fail closed.
    let pretty = serde_json::to_string_pretty(&value).expect("pretty");
    raw.execute(
        "UPDATE actions SET record_json = ?1 WHERE id = ?2",
        [&pretty, &pending.action_id],
    )
    .expect("tamper");
    assert_eq!(
        store
            .get_action(&pending.action_id)
            .expect_err("corruption")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
}

#[test]
fn insert_intent_and_replay_same_fact() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("action");
    let intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Cancelled,
    );

    let first = store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("insert");
    match first {
        InsertIntentResult::Inserted(value) => {
            assert_eq!(value.transition_id, intent.transition_id)
        }
        other => panic!("expected inserted, got {other:?}"),
    }
    let second = store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("replay");
    match second {
        InsertIntentResult::Replayed(value) => {
            assert_eq!(value.transition_id, intent.transition_id);
            assert_eq!(value.reason_code, intent.reason_code);
        }
        other => panic!("expected replayed, got {other:?}"),
    }
    let by_id = store
        .get_intent(&intent.transition_id)
        .expect("get")
        .expect("exists");
    assert_eq!(by_id.transition_id, intent.transition_id);
    let by_key = store
        .get_for_action_revision(
            &intent.action_id,
            intent.expected_action_revision,
            intent.execution_generation,
            intent.from_status,
            intent.to_status,
            &intent.reason_code,
        )
        .expect("by key")
        .expect("exists");
    assert_eq!(by_key.transition_id, intent.transition_id);
}

#[test]
fn insert_intent_dual_unique_key_conflict() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("action");
    let mut first = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Cancelled,
    );
    first.transition_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into();
    store
        .with_write_transaction(|tx| tx.insert_intent(first.clone()))
        .expect("first");

    // Same business unique key, different transition_id → conflict.
    let mut second = first.clone();
    second.transition_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into();
    second.correlation_id = "other-correlation".into();
    assert_eq!(
        store
            .with_write_transaction(|tx| tx.insert_intent(second))
            .expect_err("business key conflict")
            .code,
        StoreErrorCode::ConstraintViolation
    );

    // Same transition_id, different business key → conflict.
    let mut third = first;
    third.from_status = ActionStatus::Pending;
    third.to_status = ActionStatus::Approved;
    third.reason_code = "allow".into();
    assert_eq!(
        store
            .with_write_transaction(|tx| tx.insert_intent(third))
            .expect_err("transition_id conflict")
            .code,
        StoreErrorCode::ConstraintViolation
    );
}

#[test]
fn insert_intent_illegal_edge_fail_closed() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("action");
    let intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Completed,
    );
    assert_eq!(
        store
            .with_write_transaction(|tx| tx.insert_intent(intent))
            .expect_err("illegal")
            .code,
        StoreErrorCode::ContractInvalid
    );
}

#[test]
fn mark_committed_with_event_cas_and_causation() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 1)))
        .expect("action");
    let intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Cancelled,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("intent");

    let event_id = Uuid::parse_str("cccccccc-cccc-4ccc-8ccc-cccccccccccc").expect("uuid");
    let (updated, record) = store
        .with_write_transaction(|tx| {
            tx.mark_committed_with_event(MarkCommittedCommand {
                transition_id: intent.transition_id.clone(),
                event_id,
                dedup_key: "action-state-changed-1".into(),
                changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 2).unwrap(),
                evidence: ActionEvidence::default(),
                result: None,
                permission_decision_ref: None,
                approval_chain_id: None,
                approval_resolution_ref: None,
            })
        })
        .expect("commit");
    assert_eq!(updated.status, ActionStatus::Cancelled);
    assert_eq!(updated.revision, 2);

    let StoredEventEnvelope::ActiveV2(envelope) = &record.envelope;
    assert_eq!(envelope.event_id, event_id.to_string());
    assert_eq!(envelope.type_, "action.state_changed");
    assert_eq!(envelope.aggregate_type, "action");
    assert_eq!(envelope.aggregate_id, action.action_id);
    assert_eq!(envelope.sequence, 0);
    assert_eq!(
        envelope.causation_ref,
        CausationRefV2::ActionTransition {
            action_id: intent.action_id.clone(),
            transition_id: intent.transition_id.clone(),
        }
    );
    assert_eq!(envelope.correlation_id, intent.correlation_id);
    match &envelope.payload {
        EventEnvelopeV2Payload::ActionStateChanged(payload) => {
            assert_eq!(payload.from_status, ActionStatus::Pending);
            assert_eq!(payload.to_status, ActionStatus::Cancelled);
            assert_eq!(payload.action_revision, 2);
            assert_eq!(payload.reason_code, intent.reason_code);
            assert_eq!(payload.task_id, task);
        }
        other => panic!("unexpected payload {other:?}"),
    }

    match store
        .reconcile_intent(&intent.transition_id)
        .expect("reconcile")
    {
        ReconcileIntentResult::Committed {
            event_id: linked, ..
        } => assert_eq!(linked, event_id.to_string()),
        other => panic!("expected committed, got {other:?}"),
    }
}

#[test]
fn mark_committed_stale_revision_rolls_back_outbox() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 2);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 2)))
        .expect("action");
    let intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Cancelled,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("intent");
    // Concurrent transition wins CAS first.
    store
        .with_write_transaction(|tx| {
            tx.transition_with_expected_revision(TransitionActionCommand {
                action_id: action.action_id.clone(),
                expected_revision: 1,
                expected_status: ActionStatus::Pending,
                target_status: ActionStatus::Cancelled,
                reason: "race_winner".into(),
                evidence: ActionEvidence::default(),
                result: None,
                permission_decision_ref: None,
                approval_chain_id: None,
                lease: None,
                recovery_meta: None,
                updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 1).unwrap(),
            })
        })
        .expect("race");

    let before_position = store.latest_position().expect("pos");
    let sequences_before: i64 = database
        .raw()
        .query_row(
            "SELECT COALESCE(SUM(last_sequence + 1), 0) FROM aggregate_event_sequences \
             WHERE aggregate_type = 'action'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.mark_committed_with_event(MarkCommittedCommand {
                    transition_id: intent.transition_id.clone(),
                    event_id: Uuid::parse_str("dddddddd-dddd-4ddd-8ddd-dddddddddddd").unwrap(),
                    dedup_key: "stale-commit".into(),
                    changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 3).unwrap(),
                    evidence: ActionEvidence::default(),
                    result: None,
                    permission_decision_ref: None,
                    approval_chain_id: None,
                    approval_resolution_ref: None,
                })
            })
            .expect_err("stale")
            .code,
        StoreErrorCode::ConstraintViolation
    );
    assert_eq!(store.latest_position().expect("pos"), before_position);
    let sequences_after: i64 = database
        .raw()
        .query_row(
            "SELECT COALESCE(SUM(last_sequence + 1), 0) FROM aggregate_event_sequences \
             WHERE aggregate_type = 'action'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    assert_eq!(sequences_after, sequences_before);

    match store
        .reconcile_intent(&intent.transition_id)
        .expect("reconcile")
    {
        ReconcileIntentResult::Prepared { .. } => {}
        other => panic!("expected prepared, got {other:?}"),
    }
}

#[test]
fn mark_committed_idempotent_replay_same_event() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 3);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 3)))
        .expect("action");
    let intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Cancelled,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("intent");
    let event_id = Uuid::parse_str("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee").unwrap();
    let command = MarkCommittedCommand {
        transition_id: intent.transition_id.clone(),
        event_id,
        dedup_key: "idempotent-commit".into(),
        changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 4).unwrap(),
        evidence: ActionEvidence::default(),
        result: None,
        permission_decision_ref: None,
        approval_chain_id: None,
        approval_resolution_ref: None,
    };
    let (first_action, first_event) = store
        .with_write_transaction(|tx| tx.mark_committed_with_event(command.clone()))
        .expect("first");
    let (second_action, second_event) = store
        .with_write_transaction(|tx| tx.mark_committed_with_event(command))
        .expect("replay");
    assert_eq!(first_action, second_action);
    assert_eq!(
        first_event.envelope.outbox_position(),
        second_event.envelope.outbox_position()
    );
    // Only one action.state_changed for this action aggregate.
    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(50).expect("limit"))
        .expect("events");
    let action_events: Vec<_> = events
        .iter()
        .filter(|record| {
            matches!(
                &record.envelope,
                StoredEventEnvelope::ActiveV2(envelope)
                    if envelope.type_ == "action.state_changed"
                        && envelope.aggregate_id == action.action_id
            )
        })
        .collect();
    assert_eq!(action_events.len(), 1);
}

#[test]
fn reconcile_prepared_and_corrupt_missing_event() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 4);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 4)))
        .expect("action");
    let intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Cancelled,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("intent");
    match store
        .reconcile_intent(&intent.transition_id)
        .expect("prepared")
    {
        ReconcileIntentResult::Prepared { intent: value } => {
            assert_eq!(value.transition_id, intent.transition_id);
        }
        other => panic!("expected prepared, got {other:?}"),
    }

    // Force corrupt: mark committed_event_id without outbox row.
    database
        .raw()
        .execute(
            "UPDATE action_transition_intents SET committed_event_id = ?1 WHERE transition_id = ?2",
            [
                "ffffffff-ffff-4fff-8fff-ffffffffffff",
                intent.transition_id.as_str(),
            ],
        )
        .expect("force corrupt");
    match store
        .reconcile_intent(&intent.transition_id)
        .expect("corrupt")
    {
        ReconcileIntentResult::Corrupt { reason } => {
            assert!(
                reason.contains("missing")
                    || reason.contains("invalid")
                    || reason.contains("not match")
            );
        }
        other => panic!("expected corrupt, got {other:?}"),
    }
}

#[test]
fn mark_committed_post_append_failure_does_not_consume_sequence() {
    // Use a forced path: commit succeeds only if whole savepoint succeeds. We simulate by
    // using an illegal approval_resolution without permission (Schema-level) if possible.
    // Schema requires: approval_resolution_ref!=null ⇒ permission_decision_ref!=null.
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 5);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 5)))
        .expect("action");
    let intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Cancelled,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("intent");

    let before = store.latest_position().expect("pos");
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.mark_committed_with_event(MarkCommittedCommand {
                    transition_id: intent.transition_id.clone(),
                    event_id: Uuid::parse_str("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").unwrap(),
                    dedup_key: "schema-fail".into(),
                    changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 5).unwrap(),
                    evidence: ActionEvidence::default(),
                    result: None,
                    permission_decision_ref: None,
                    approval_chain_id: None,
                    // Schema: approval_resolution_ref non-null requires permission_decision_ref.
                    approval_resolution_ref: Some("11111111-1111-4111-8111-111111111111".into()),
                })
            })
            .expect_err("schema")
            .code,
        StoreErrorCode::ContractInvalid
    );
    assert_eq!(store.latest_position().expect("pos"), before);
    // Action must remain pending revision 1.
    let still = store
        .get_action(&action.action_id)
        .expect("get")
        .expect("exists");
    assert_eq!(still.revision, 1);
    assert_eq!(still.status, ActionStatus::Pending);
    match store
        .reconcile_intent(&intent.transition_id)
        .expect("prepared")
    {
        ReconcileIntentResult::Prepared { .. } => {}
        other => panic!("expected prepared, got {other:?}"),
    }
}

#[test]
fn pending_to_approved_with_permission_then_intent_commit() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 6);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 6)))
        .expect("action");
    let pd = "22222222-2222-4222-8222-222222222222".to_string();
    let approved = store
        .with_write_transaction(|tx| {
            tx.transition_with_expected_revision(TransitionActionCommand {
                action_id: action.action_id.clone(),
                expected_revision: 1,
                expected_status: ActionStatus::Pending,
                target_status: ActionStatus::Approved,
                reason: "policy_allow".into(),
                evidence: ActionEvidence {
                    permission_decision_ref: Some(pd.clone()),
                    ..ActionEvidence::default()
                },
                result: None,
                permission_decision_ref: Some(pd.clone()),
                approval_chain_id: None,
                lease: None,
                recovery_meta: None,
                updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 6).unwrap(),
            })
        })
        .expect("approve");
    assert_eq!(approved.status, ActionStatus::Approved);
    assert_eq!(
        approved.permission_decision_ref.as_deref(),
        Some(pd.as_str())
    );

    // Status event path for approved→cancelled via intent producer.
    let intent = sample_intent(
        &action.action_id,
        2,
        ActionStatus::Approved,
        ActionStatus::Cancelled,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("intent");
    let (final_action, record) = store
        .with_write_transaction(|tx| {
            tx.mark_committed_with_event(MarkCommittedCommand {
                transition_id: intent.transition_id.clone(),
                event_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
                dedup_key: "approved-cancelled".into(),
                changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 0, 7).unwrap(),
                evidence: ActionEvidence::default(),
                result: None,
                permission_decision_ref: Some(pd),
                approval_chain_id: None,
                approval_resolution_ref: None,
            })
        })
        .expect("commit");
    assert_eq!(final_action.status, ActionStatus::Cancelled);
    assert_eq!(final_action.revision, 3);
    let StoredEventEnvelope::ActiveV2(envelope) = &record.envelope;
    assert_eq!(envelope.sequence, 0);
}

#[test]
fn reconcile_still_committed_after_later_legal_advance() {
    let database = ActionDatabase::new();
    let store = database.open();
    // Single Action lifecycle: pending→approved (mark) then approved→cancelled (mark).
    // First committed intent must stay Committed after the second advance.
    let task = create_root_task(&store, 17);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 17)))
        .expect("action");
    let pd = "27272727-2727-4272-8272-272727272727".to_string();
    let mut approve_intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Approved,
    );
    approve_intent.transition_id = "17171717-aaaa-4aaa-8aaa-171717171717".into();
    store
        .with_write_transaction(|tx| tx.insert_intent(approve_intent.clone()))
        .expect("approve intent");
    let approve_event = Uuid::parse_str("17171717-1717-4171-8171-171717171717").unwrap();
    store
        .with_write_transaction(|tx| {
            tx.mark_committed_with_event(MarkCommittedCommand {
                transition_id: approve_intent.transition_id.clone(),
                event_id: approve_event,
                dedup_key: "approve-17".into(),
                changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 1, 1).unwrap(),
                evidence: ActionEvidence {
                    permission_decision_ref: Some(pd.clone()),
                    ..ActionEvidence::default()
                },
                result: None,
                permission_decision_ref: Some(pd.clone()),
                approval_chain_id: None,
                approval_resolution_ref: None,
            })
        })
        .expect("commit approve");

    let mut cancel_intent = sample_intent(
        &action.action_id,
        2,
        ActionStatus::Approved,
        ActionStatus::Cancelled,
    );
    cancel_intent.transition_id = "18181818-bbbb-4bbb-8bbb-181818181818".into();
    store
        .with_write_transaction(|tx| tx.insert_intent(cancel_intent.clone()))
        .expect("cancel intent");
    store
        .with_write_transaction(|tx| {
            tx.mark_committed_with_event(MarkCommittedCommand {
                transition_id: cancel_intent.transition_id.clone(),
                event_id: Uuid::parse_str("18181818-1818-4181-8181-181818181818").unwrap(),
                dedup_key: "cancel-17".into(),
                changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 1, 2).unwrap(),
                evidence: ActionEvidence::default(),
                result: None,
                permission_decision_ref: Some(pd),
                approval_chain_id: None,
                approval_resolution_ref: None,
            })
        })
        .expect("commit cancel");

    // After later legal advance, first intent remains Committed (not Corrupt).
    match store
        .reconcile_intent(&approve_intent.transition_id)
        .expect("reconcile first")
    {
        ReconcileIntentResult::Committed {
            event_id: linked, ..
        } => assert_eq!(linked, approve_event.to_string()),
        other => panic!("expected committed after later advance, got {other:?}"),
    }
    // Sanity: cancelled head is beyond the first transition revision.
    let head = store
        .get_action(&action.action_id)
        .expect("get")
        .expect("exists");
    assert_eq!(head.status, ActionStatus::Cancelled);
    assert_eq!(head.revision, 3);
}

#[test]
fn mark_committed_idempotent_replay_after_later_advance() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 8);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 8)))
        .expect("action");
    let pd = "28282828-2828-4282-8282-282828282828".to_string();
    let approve_intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Approved,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(approve_intent.clone()))
        .expect("approve intent");
    let approve_event = Uuid::parse_str("08080808-0808-4080-8080-080808080808").unwrap();
    let approve_cmd = MarkCommittedCommand {
        transition_id: approve_intent.transition_id.clone(),
        event_id: approve_event,
        dedup_key: "approve-8".into(),
        changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 2, 0).unwrap(),
        evidence: ActionEvidence {
            permission_decision_ref: Some(pd.clone()),
            ..ActionEvidence::default()
        },
        result: None,
        permission_decision_ref: Some(pd.clone()),
        approval_chain_id: None,
        approval_resolution_ref: None,
    };
    store
        .with_write_transaction(|tx| tx.mark_committed_with_event(approve_cmd.clone()))
        .expect("commit approve");

    // Later legal advance moves head past the first transition.
    let cancel_intent = sample_intent(
        &action.action_id,
        2,
        ActionStatus::Approved,
        ActionStatus::Cancelled,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(cancel_intent.clone()))
        .expect("cancel intent");
    store
        .with_write_transaction(|tx| {
            tx.mark_committed_with_event(MarkCommittedCommand {
                transition_id: cancel_intent.transition_id.clone(),
                event_id: Uuid::parse_str("09090909-0909-4090-8090-090909090909").unwrap(),
                dedup_key: "cancel-8".into(),
                changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 2, 1).unwrap(),
                evidence: ActionEvidence::default(),
                result: None,
                permission_decision_ref: Some(pd),
                approval_chain_id: None,
                approval_resolution_ref: None,
            })
        })
        .expect("commit cancel");

    // Same-event idempotent replay of the earlier commit must still succeed.
    let (replayed_action, replayed_event) = store
        .with_write_transaction(|tx| tx.mark_committed_with_event(approve_cmd))
        .expect("idempotent replay after advance");
    assert_eq!(replayed_action.status, ActionStatus::Cancelled);
    assert_eq!(replayed_action.revision, 3);
    let StoredEventEnvelope::ActiveV2(envelope) = &replayed_event.envelope;
    assert_eq!(envelope.event_id, approve_event.to_string());
    assert_eq!(envelope.aggregate_id, action.action_id);
}

#[test]
fn mark_committed_pending_to_approved_without_pd_fail_closed() {
    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 9);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 9)))
        .expect("action");
    let intent = sample_intent(
        &action.action_id,
        1,
        ActionStatus::Pending,
        ActionStatus::Approved,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(intent.clone()))
        .expect("intent");
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.mark_committed_with_event(MarkCommittedCommand {
                    transition_id: intent.transition_id.clone(),
                    event_id: Uuid::parse_str("19191919-1919-4191-8191-191919191919").unwrap(),
                    dedup_key: "approve-no-pd".into(),
                    changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 3, 0).unwrap(),
                    evidence: ActionEvidence::default(),
                    result: None,
                    permission_decision_ref: None,
                    approval_chain_id: None,
                    approval_resolution_ref: None,
                })
            })
            .expect_err("missing pd")
            .code,
        StoreErrorCode::ContractInvalid
    );
    let still = store
        .get_action(&action.action_id)
        .expect("get")
        .expect("exists");
    assert_eq!(still.status, ActionStatus::Pending);
    assert_eq!(still.revision, 1);
    match store
        .reconcile_intent(&intent.transition_id)
        .expect("prepared")
    {
        ReconcileIntentResult::Prepared { .. } => {}
        other => panic!("expected prepared, got {other:?}"),
    }
}

#[test]
fn mark_committed_leased_exits_require_evidence_and_effects_fail_closed() {
    use domain_task::DispatchCertainty;

    let database = ActionDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 10);
    let action = store
        .with_write_transaction(|tx| tx.insert_pending_action(pending_command(&task, 10)))
        .expect("action");
    let pd = "2a2a2a2a-2a2a-42a2-82a2-2a2a2a2a2a2a".to_string();

    // Drive pending → approved → leased via internal CAS helper (no status event required here).
    store
        .with_write_transaction(|tx| {
            tx.transition_with_expected_revision(TransitionActionCommand {
                action_id: action.action_id.clone(),
                expected_revision: 1,
                expected_status: ActionStatus::Pending,
                target_status: ActionStatus::Approved,
                reason: "policy_allow".into(),
                evidence: ActionEvidence {
                    permission_decision_ref: Some(pd.clone()),
                    ..ActionEvidence::default()
                },
                result: None,
                permission_decision_ref: Some(pd.clone()),
                approval_chain_id: None,
                lease: None,
                recovery_meta: None,
                updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 4, 0).unwrap(),
            })
        })
        .expect("approve");
    store
        .with_write_transaction(|tx| {
            tx.transition_with_expected_revision(TransitionActionCommand {
                action_id: action.action_id.clone(),
                expected_revision: 2,
                expected_status: ActionStatus::Approved,
                target_status: ActionStatus::Leased,
                reason: "lease_acquired".into(),
                evidence: ActionEvidence::default(),
                result: None,
                permission_decision_ref: Some(pd.clone()),
                approval_chain_id: None,
                lease: None,
                recovery_meta: None,
                updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 4, 1).unwrap(),
            })
        })
        .expect("lease");

    // leased → cancelled without dispatch_certainty fails closed (missing evidence).
    let cancel_intent = sample_intent(
        &action.action_id,
        3,
        ActionStatus::Leased,
        ActionStatus::Cancelled,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(cancel_intent.clone()))
        .expect("cancel intent");
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.mark_committed_with_event(MarkCommittedCommand {
                    transition_id: cancel_intent.transition_id.clone(),
                    event_id: Uuid::parse_str("1a1a1a1a-1a1a-41a1-81a1-1a1a1a1a1a1a").unwrap(),
                    dedup_key: "leased-cancel-no-evidence".into(),
                    changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 4, 2).unwrap(),
                    evidence: ActionEvidence::default(),
                    result: None,
                    permission_decision_ref: Some(pd.clone()),
                    approval_chain_id: None,
                    approval_resolution_ref: None,
                })
            })
            .expect_err("missing dispatch evidence")
            .code,
        StoreErrorCode::ContractInvalid
    );

    // leased → cancelled with NotStarted evidence requires lease release effects → fail closed.
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.mark_committed_with_event(MarkCommittedCommand {
                    transition_id: cancel_intent.transition_id.clone(),
                    event_id: Uuid::parse_str("1b1b1b1b-1b1b-41b1-81b1-1b1b1b1b1b1b").unwrap(),
                    dedup_key: "leased-cancel-effects".into(),
                    changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 4, 3).unwrap(),
                    evidence: ActionEvidence {
                        dispatch_certainty: Some(DispatchCertainty::NotStarted),
                        ..ActionEvidence::default()
                    },
                    result: None,
                    permission_decision_ref: Some(pd.clone()),
                    approval_chain_id: None,
                    approval_resolution_ref: None,
                })
            })
            .expect_err("lease effects not implemented")
            .code,
        StoreErrorCode::ContractInvalid
    );

    // completed evidence gate via mark path: drive to in_flight then attempt completed without verification.
    store
        .with_write_transaction(|tx| {
            tx.transition_with_expected_revision(TransitionActionCommand {
                action_id: action.action_id.clone(),
                expected_revision: 3,
                expected_status: ActionStatus::Leased,
                target_status: ActionStatus::InFlight,
                reason: "dispatch".into(),
                evidence: ActionEvidence::default(),
                result: None,
                permission_decision_ref: Some(pd.clone()),
                approval_chain_id: None,
                lease: None,
                recovery_meta: None,
                updated_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 4, 4).unwrap(),
            })
        })
        .expect("in_flight");
    let complete_intent = sample_intent(
        &action.action_id,
        4,
        ActionStatus::InFlight,
        ActionStatus::Completed,
    );
    store
        .with_write_transaction(|tx| tx.insert_intent(complete_intent.clone()))
        .expect("complete intent");
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.mark_committed_with_event(MarkCommittedCommand {
                    transition_id: complete_intent.transition_id.clone(),
                    event_id: Uuid::parse_str("1c1c1c1c-1c1c-41c1-81c1-1c1c1c1c1c1c").unwrap(),
                    dedup_key: "complete-no-verification".into(),
                    changed_at: Utc.with_ymd_and_hms(2026, 7, 21, 10, 4, 5).unwrap(),
                    evidence: ActionEvidence::default(),
                    result: None,
                    permission_decision_ref: Some(pd),
                    approval_chain_id: None,
                    approval_resolution_ref: None,
                })
            })
            .expect_err("missing verification")
            .code,
        StoreErrorCode::ContractInvalid
    );

    let still = store
        .get_action(&action.action_id)
        .expect("get")
        .expect("exists");
    assert_eq!(still.status, ActionStatus::InFlight);
    assert_eq!(still.revision, 4);
}

// --- helpers ---

fn create_root_task(store: &SqliteStore, number: u32) -> String {
    let command = root_command(number);
    let task_id = command.allocation.task_id.clone();
    store
        .with_write_transaction(|tx| tx.create_root_task_v2(command))
        .expect("root");
    task_id
}

fn pending_command(task_id: &str, number: u32) -> InsertPendingActionCommand {
    InsertPendingActionCommand {
        action_id: format!("a0000000-0000-4000-8000-{number:012}"),
        task_id: task_id.to_owned(),
        step_id: None,
        parent_action_id: None,
        capability_id: "kernel.task".into(),
        operation: "task.child.create".into(),
        structured_arguments: Map::from_iter([("goal".into(), json!("child"))]),
        resource_refs: vec![format!("task://{task_id}")],
        task_scope_ref: format!("20000000-0000-4000-8000-{number:012}"),
        side_effect_class: SideEffectClass::S1,
        idempotency_key: format!("action-idem-{number}"),
        execution_generation: 0,
        verification_policy: ActionRequestV2VerificationPolicyInput {
            strategy: "kernel_local".into(),
            expected_outcome: json!({"ok": true}),
            timeout: "PT30S".into(),
        },
        rollback_policy: None,
        recovery_meta: None,
        created_at: Utc
            .with_ymd_and_hms(2026, 7, 21, 9, 0, number % 60)
            .unwrap(),
    }
}

fn status_code(status: ActionStatus) -> i64 {
    match status {
        ActionStatus::Pending => 1,
        ActionStatus::Approved => 2,
        ActionStatus::Leased => 3,
        ActionStatus::InFlight => 4,
        ActionStatus::Completed => 5,
        ActionStatus::Failed => 6,
        ActionStatus::UnknownSideEffect => 7,
        ActionStatus::RollingBack => 8,
        ActionStatus::RolledBack => 9,
        ActionStatus::RollbackFailed => 10,
        ActionStatus::Cancelled => 11,
    }
}

fn sample_intent(
    action_id: &str,
    expected_revision: i64,
    from: ActionStatus,
    to: ActionStatus,
) -> ActionTransitionIntentV1 {
    ActionTransitionIntentV1 {
        schema_version: ActionTransitionIntentV1SchemaVersion,
        transition_id: format!(
            "b0000000-0000-4000-8000-{:012}",
            expected_revision * 100 + status_code(from)
        ),
        action_id: action_id.to_owned(),
        expected_action_revision: expected_revision,
        execution_generation: 0,
        from_status: from,
        to_status: to,
        reason_code: match (from, to) {
            (ActionStatus::Pending, ActionStatus::Cancelled) => "user_cancelled",
            (ActionStatus::Approved, ActionStatus::Cancelled) => "cancelled_before_lease",
            (ActionStatus::Pending, ActionStatus::Approved) => "policy_allow",
            _ => "transition",
        }
        .into(),
        correlation_id: format!("corr-action-{expected_revision}"),
        created_at: "2026-07-21T10:00:00Z".into(),
    }
}

fn root_command(number: u32) -> RootTaskCreateV2Command {
    RootTaskCreateV2Command {
        envelope: RootTaskCreateV2EnvelopeFacts {
            actor: Actor {
                authentication_level: ActorAuthenticationLevel::PlatformVerified,
                confidence: Some(0.9),
                id: "actor".into(),
                kind: ActorKind::KnownUser,
                revision: 1,
                schema_version: ActorSchemaVersion,
                source: "actor-source://local/desktop".into(),
            },
            entry_point: EntryPoint::LocalDesktop,
            request_id: format!("10000000-0000-4000-8000-{number:012}"),
            context: Some(Map::from_iter([("conversation".to_owned(), json!(number))])),
            idempotency_key: format!("root-for-action-{number}"),
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
        allocation: RootTaskCreateAllocationV2 {
            audit_record_id: format!("50000000-0000-4000-8000-{number:012}"),
            content_origin_id: format!("30000000-0000-4000-8000-{number:012}"),
            correlation_id: format!("correlation-action-{number}"),
            creation_provenance_id: format!("70000000-0000-4000-8000-{number:012}"),
            kernel_receipt_id: format!("40000000-0000-4000-8000-{number:012}"),
            schema_version: RootTaskCreateAllocationV2SchemaVersion,
            task_created_dedup_key: format!("dedup-action-{number}"),
            task_created_event_id: format!("60000000-0000-4000-8000-{number:012}"),
            task_id: format!("00000000-0000-4000-8000-{number:012}"),
            task_scope_id: format!("20000000-0000-4000-8000-{number:012}"),
        },
        accepted_at: Utc
            .with_ymd_and_hms(2026, 7, 21, 8, 0, number % 60)
            .unwrap(),
    }
}

// Silence unused helpers for compile stability when tests evolve.
#[allow(dead_code)]
fn _unused_result() -> ActionRequestV2Result {
    ActionRequestV2Result {
        materialized_child_task_ref: None,
        verification_result_refs: vec![],
    }
}

#[allow(dead_code)]
fn _unused_timelike() {
    let _ = Utc
        .with_ymd_and_hms(2026, 7, 21, 0, 0, 0)
        .unwrap()
        .with_nanosecond(0);
}
