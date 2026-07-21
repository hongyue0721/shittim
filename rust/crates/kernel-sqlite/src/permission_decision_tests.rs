//! PermissionDecisionV2 repository tests (slice 4b).

use super::*;
use chrono::{TimeZone, Utc};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, EntryPoint,
    InputContentOriginV1, InputContentOriginV1Kind, InputContentOriginV1ProducerRef,
    InputContentOriginV1ProducerRefKind, InputContentOriginV1SchemaVersion, InputTaskScopeV1,
    InputTaskScopeV1SchemaVersion, NormalizedRootTaskCreatePayloadV2Proposer, PermissionDecisionV2,
    PermissionDecisionV2Binding, PermissionDecisionV2Decision, PermissionDecisionV2SchemaVersion,
    RootTaskCreateAllocationV2, RootTaskCreateAllocationV2SchemaVersion, SideEffectClass,
    TaskCreateRequestV2, TaskCreateRequestV2SchemaVersion,
};
use serde_json::{json, Map};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

struct PdDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl PdDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("directory");
        Self {
            path: directory.path().join("pd.sqlite3"),
            _directory: directory,
            config: SqliteConfig::new(Duration::from_secs(2)).expect("config"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path, self.config).expect("open")
    }
}

#[test]
fn append_permission_decision_allocates_continuous_revision() {
    let database = PdDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 1);
    let action = insert_pending(&store, &task, 1);
    let mut first = sample_pd(&action, "d0000000-0000-4000-8000-000000000001", 0);
    let stored = store
        .with_write_transaction(|tx| tx.append_permission_decision(first.clone()))
        .expect("append1");
    assert_eq!(stored.decision_revision, 1);
    first.decision_revision = 1;
    assert_eq!(stored, first);

    let second = sample_pd(&action, "d0000000-0000-4000-8000-000000000002", 0);
    let stored2 = store
        .with_write_transaction(|tx| tx.append_permission_decision(second))
        .expect("append2");
    assert_eq!(stored2.decision_revision, 2);

    let current = store
        .get_current_permission_decision_for_action(&action)
        .expect("current")
        .expect("exists");
    assert_eq!(current.id, stored2.id);
    assert_eq!(current.decision_revision, 2);

    let list = store
        .list_permission_decisions_for_action(&action)
        .expect("list");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].decision_revision, 1);
    assert_eq!(list[1].decision_revision, 2);
}

#[test]
fn append_rejects_non_continuous_claimed_revision() {
    let database = PdDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 2);
    let action = insert_pending(&store, &task, 2);
    store
        .with_write_transaction(|tx| {
            tx.append_permission_decision(sample_pd(
                &action,
                "d0000000-0000-4000-8000-000000000011",
                0,
            ))
        })
        .expect("first");
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.append_permission_decision(sample_pd(
                    &action,
                    "d0000000-0000-4000-8000-000000000012",
                    5,
                ))
            })
            .expect_err("gap")
            .code,
        StoreErrorCode::ConstraintViolation
    );
}

#[test]
fn validate_current_requires_action_ref_consistency() {
    let database = PdDatabase::new();
    let store = database.open();
    let task = create_root_task(&store, 3);
    let action = insert_pending(&store, &task, 3);
    // No PD yet and Action ref null → Ok(None)
    assert!(store
        .validate_current_permission_decision_for_action(&action)
        .expect("validate")
        .is_none());

    let pd = store
        .with_write_transaction(|tx| {
            tx.append_permission_decision(sample_pd(
                &action,
                "d0000000-0000-4000-8000-000000000021",
                0,
            ))
        })
        .expect("append");
    // Action still null → inconsistent
    assert_eq!(
        store
            .validate_current_permission_decision_for_action(&action)
            .expect_err("mismatch")
            .code,
        StoreErrorCode::StoredDataInvalid
    );

    // Bind Action ref via evaluation-like CAS path: update record_json.
    store
        .with_write_transaction(|tx| {
            let mut action_doc = tx
                .connection()
                .query_row(
                    "SELECT record_json FROM actions WHERE id = ?1",
                    [&action],
                    |row| row.get::<_, String>(0),
                )
                .expect("row");
            // Use public evaluation path would be cleaner; here we only check validator after bind.
            let mut value: serde_json::Value = serde_json::from_str(&action_doc).expect("json");
            value["permission_decision_ref"] = json!(pd.id);
            value["revision"] = json!(2);
            action_doc = kernel_contracts::canonical_json_string(&value).expect("jcs");
            // Must re-validate Action schema.
            kernel_contracts::validate_json(
                "https://schemas.shittim.local/task/action_request/v2",
                &value,
            )
            .expect("schema");
            tx.connection()
                .execute(
                    "UPDATE actions SET record_json = ?1 WHERE id = ?2",
                    rusqlite::params![action_doc, action],
                )
                .expect("update");
            Ok(())
        })
        .expect("bind");

    let validated = store
        .validate_current_permission_decision_for_action(&action)
        .expect("ok")
        .expect("pd");
    assert_eq!(validated.id, pd.id);
}

#[test]
fn append_requires_existing_action_and_unique_id() {
    let database = PdDatabase::new();
    let store = database.open();
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.append_permission_decision(sample_pd(
                    "a0000000-0000-4000-8000-000000000099",
                    "d0000000-0000-4000-8000-000000000031",
                    0,
                ))
            })
            .expect_err("missing action")
            .code,
        StoreErrorCode::NotFound
    );
    let task = create_root_task(&store, 4);
    let action = insert_pending(&store, &task, 4);
    let id = "d0000000-0000-4000-8000-000000000032";
    store
        .with_write_transaction(|tx| tx.append_permission_decision(sample_pd(&action, id, 0)))
        .expect("first");
    assert_eq!(
        store
            .with_write_transaction(|tx| tx.append_permission_decision(sample_pd(&action, id, 0)))
            .expect_err("dup id")
            .code,
        StoreErrorCode::ConstraintViolation
    );
}

fn sample_pd(action_id: &str, id: &str, decision_revision: i64) -> PermissionDecisionV2 {
    PermissionDecisionV2 {
        id: id.into(),
        schema_version: PermissionDecisionV2SchemaVersion,
        action_id: action_id.into(),
        decision: PermissionDecisionV2Decision::Allow,
        reason_codes: vec!["default_allow".into()],
        matched_rule_ref: None,
        decision_revision,
        evaluated_at: "2026-07-21T12:00:00Z".into(),
        policy_set_revision: 0,
        material_authorization_fingerprint: "a".repeat(64),
        observation_evidence_fingerprint: "b".repeat(64),
        binding: PermissionDecisionV2Binding {
            action_id: action_id.into(),
            action_revision: 1,
            task_id: "00000000-0000-4000-8000-000000000001".into(),
            plan_version: 0,
            capability_id: "kernel.task".into(),
            operation: "task.child.create".into(),
            side_effect_class: SideEffectClass::S1,
            resource_refs: vec![],
            key_params_hash: "c".repeat(64),
            delegation_authority_ref: None,
        },
        approval_requirement: None,
        expires_at: None,
        lease_ref: None,
    }
}

fn create_root_task(store: &SqliteStore, number: u32) -> String {
    let command = root_command(number);
    let task_id = command.allocation.task_id.clone();
    store
        .with_write_transaction(|tx| tx.create_root_task_v2(command))
        .expect("root");
    task_id
}

fn insert_pending(store: &SqliteStore, task_id: &str, number: u32) -> String {
    let command = InsertPendingActionCommand {
        action_id: format!("a0000000-0000-4000-8000-{number:012}"),
        task_id: task_id.to_owned(),
        step_id: None,
        parent_action_id: None,
        capability_id: "kernel.task".into(),
        operation: "task.child.create".into(),
        structured_arguments: Map::from_iter([("goal".into(), json!("child"))]),
        resource_refs: vec![format!("https://example.com/a/{number}")],
        task_scope_ref: format!("20000000-0000-4000-8000-{number:012}"),
        side_effect_class: SideEffectClass::S1,
        idempotency_key: format!("pd-action-{number}"),
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
    };
    let action_id = command.action_id.clone();
    store
        .with_write_transaction(|tx| tx.insert_pending_action(command))
        .expect("pending");
    action_id
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
            idempotency_key: format!("root-for-pd-{number}"),
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
            correlation_id: format!("correlation-pd-{number}"),
            creation_provenance_id: format!("70000000-0000-4000-8000-{number:012}"),
            kernel_receipt_id: format!("40000000-0000-4000-8000-{number:012}"),
            schema_version: RootTaskCreateAllocationV2SchemaVersion,
            task_created_dedup_key: format!("dedup-pd-{number}"),
            task_created_event_id: format!("60000000-0000-4000-8000-{number:012}"),
            task_id: format!("00000000-0000-4000-8000-{number:012}"),
            task_scope_id: format!("20000000-0000-4000-8000-{number:012}"),
        },
        accepted_at: Utc
            .with_ymd_and_hms(2026, 7, 21, 8, 0, number % 60)
            .unwrap(),
    }
}
