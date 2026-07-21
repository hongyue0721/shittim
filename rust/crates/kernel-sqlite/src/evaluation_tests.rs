//! Permission evaluation orchestration tests (slice 4b).

use super::*;
use chrono::{TimeZone, Utc};
use domain_policy::KernelInvariantState;
use kernel_authorization::ObservationEvidenceFactsV1;
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, ConfirmationModeV1, EntryPoint,
    InputContentOriginV1, InputContentOriginV1Kind, InputContentOriginV1ProducerRef,
    InputContentOriginV1ProducerRefKind, InputContentOriginV1SchemaVersion, InputTaskScopeV1,
    InputTaskScopeV1SchemaVersion, NormalizedRootTaskCreatePayloadV2Proposer,
    PermissionDecisionV2Decision, PolicyRuleV2, PolicyRuleV2ActionMatch, PolicyRuleV2ActorMatch,
    PolicyRuleV2Condition, PolicyRuleV2ConditionRateLimit, PolicyRuleV2ConditionRateLimitKeyScope,
    PolicyRuleV2ContentOriginMatch, PolicyRuleV2CreatedBy, PolicyRuleV2Effect,
    PolicyRuleV2ResourceMatch, PolicyRuleV2SchemaVersion, PolicyRuleV2Source,
    PolicyRuleV2UpdatedBy, RootTaskCreateAllocationV2, RootTaskCreateAllocationV2SchemaVersion,
    SideEffectClass, TaskCreateRequestV2, TaskCreateRequestV2SchemaVersion,
};
use serde_json::{json, Map};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

struct EvalDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl EvalDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("directory");
        Self {
            path: directory.path().join("eval.sqlite3"),
            _directory: directory,
            config: SqliteConfig::new(Duration::from_secs(2)).expect("config"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path, self.config).expect("open")
    }
}

#[test]
fn evaluation_default_allow_approves_and_writes_audit() {
    let database = EvalDatabase::new();
    let store = database.open();
    let (task, action) = seed_task_action(&store, 1);
    let result = store
        .with_write_transaction(|tx| tx.evaluate_action_permission(eval_command(&action, 1, 1)))
        .expect("eval");
    assert_eq!(
        result.decision.decision,
        PermissionDecisionV2Decision::Allow
    );
    assert!(result
        .decision
        .reason_codes
        .iter()
        .any(|c| c == "default_allow"));
    assert_eq!(result.decision.decision_revision, 1);
    assert_eq!(result.decision.policy_set_revision, 0);
    // The status edge went through the sole status-event authority: exactly one
    // `action.state_changed` event with action_transition causation, and the intent
    // reconciles as Committed (no silent CAS).
    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("read outbox");
    assert_eq!(events.len(), 2);
    let StoredEventEnvelope::ActiveV2(envelope) = &events[1].envelope;
    assert_eq!(envelope.aggregate_id, action);
    let transition_id = match &envelope.causation_ref {
        kernel_contracts::CausationRefV2::ActionTransition { transition_id, .. } => {
            transition_id.clone()
        }
        other => panic!("expected action_transition causation, got {other:?}"),
    };
    assert!(matches!(
        &envelope.payload,
        kernel_contracts::EventEnvelopeV2Payload::ActionStateChanged(_)
    ));
    let reconciled = store.reconcile_intent(&transition_id).expect("reconcile");
    assert!(matches!(
        reconciled,
        ReconcileIntentResult::Committed { .. }
    ));
    assert_eq!(
        result.action.status,
        kernel_contracts::ActionStatus::Approved
    );
    assert_eq!(result.action.revision, 2);
    assert_eq!(
        result.action.permission_decision_ref.as_deref(),
        Some(result.decision.id.as_str())
    );
    assert_eq!(
        result.audit.audit_type,
        kernel_contracts::AuditRecordV2AuditType::PermissionEvaluated
    );
    assert_eq!(
        result.audit.outcome,
        kernel_contracts::AuditRecordV2Outcome::Succeeded
    );
    assert!(result.audit.policy_context.is_some());
    // Fingerprints are real 64-hex, not placeholders.
    assert_eq!(result.decision.material_authorization_fingerprint.len(), 64);
    assert_eq!(result.decision.observation_evidence_fingerprint.len(), 64);
    assert_ne!(
        result.decision.material_authorization_fingerprint,
        "a".repeat(64)
    );
    let _ = task;
    store
        .validate_current_permission_decision_for_action(&action)
        .expect("bidirectional")
        .expect("pd");
}

#[test]
fn evaluation_deny_cancels_action() {
    let database = EvalDatabase::new();
    let store = database.open();
    let (_task, action) = seed_task_action(&store, 2);
    store
        .with_write_transaction(|tx| {
            tx.append_policy_rule_revision(sample_rule(
                "deny-all",
                1,
                PolicyRuleV2Effect::Deny,
                200,
                vec![],
                vec![],
                None,
                None,
            ))
        })
        .expect("rule");
    let result = store
        .with_write_transaction(|tx| tx.evaluate_action_permission(eval_command(&action, 2, 1)))
        .expect("eval");
    assert_eq!(result.decision.decision, PermissionDecisionV2Decision::Deny);
    assert_eq!(
        result.action.status,
        kernel_contracts::ActionStatus::Cancelled
    );
    assert_eq!(
        result.audit.outcome,
        kernel_contracts::AuditRecordV2Outcome::Blocked
    );
    assert_eq!(result.decision.policy_set_revision, 1);
    // Deny edge also went through the sole status-event authority.
    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("read outbox");
    assert_eq!(events.len(), 2);
    let StoredEventEnvelope::ActiveV2(envelope) = &events[1].envelope;
    assert!(matches!(
        &envelope.payload,
        kernel_contracts::EventEnvelopeV2Payload::ActionStateChanged(_)
    ));
    assert!(matches!(
        &envelope.causation_ref,
        kernel_contracts::CausationRefV2::ActionTransition { .. }
    ));
}

#[test]
fn evaluation_require_confirmation_defers_without_approval() {
    let database = EvalDatabase::new();
    let store = database.open();
    let (_task, action) = seed_task_action(&store, 3);
    store
        .with_write_transaction(|tx| {
            tx.append_policy_rule_revision(sample_rule(
                "confirm-generic",
                1,
                PolicyRuleV2Effect::Confirm,
                150,
                vec![],
                vec![],
                Some(ConfirmationModeV1::Generic),
                None,
            ))
        })
        .expect("rule");
    let result = store
        .with_write_transaction(|tx| tx.evaluate_action_permission(eval_command(&action, 3, 1)))
        .expect("eval");
    assert_eq!(
        result.decision.decision,
        PermissionDecisionV2Decision::RequireConfirmation
    );
    assert_eq!(
        result.action.status,
        kernel_contracts::ActionStatus::Pending
    );
    assert_eq!(result.action.revision, 2);
    assert!(result.action.approval_chain_id.is_none());
    assert_eq!(
        result.audit.outcome,
        kernel_contracts::AuditRecordV2Outcome::Deferred
    );
    assert!(result.decision.approval_requirement.is_some());
}

#[test]
fn evaluation_rate_limit_consumes_and_rolls_back_on_failure() {
    let database = EvalDatabase::new();
    let store = database.open();
    let (task, action) = seed_task_action(&store, 4);
    store
        .with_write_transaction(|tx| {
            tx.append_policy_rule_revision(sample_rule(
                "rate-allow",
                1,
                PolicyRuleV2Effect::Allow,
                300,
                vec!["kernel.task".into()],
                vec!["task.child.create".into()],
                None,
                Some(PolicyRuleV2ConditionRateLimit {
                    count: 1,
                    window_seconds: 3600,
                    key_scope: PolicyRuleV2ConditionRateLimitKeyScope::Actor,
                }),
            ))
        })
        .expect("rule");
    // First evaluation consumes the only slot and allows.
    let first = store
        .with_write_transaction(|tx| tx.evaluate_action_permission(eval_command(&action, 4, 1)))
        .expect("first");
    assert_eq!(first.decision.decision, PermissionDecisionV2Decision::Allow);
    assert_eq!(
        first.decision.matched_rule_ref.as_deref(),
        Some("rate-allow")
    );

    // Second Action with same actor should fall through after rate exceeded → default allow
    // (winner removed). Insert another pending action.
    let action2 = insert_pending(&store, &task, 40);
    let second = store
        .with_write_transaction(|tx| tx.evaluate_action_permission(eval_command(&action2, 40, 1)))
        .expect("second");
    // rate exceeded → rule removed → default allow
    assert_eq!(
        second.decision.decision,
        PermissionDecisionV2Decision::Allow
    );
    assert!(second.decision.matched_rule_ref.is_none());

    // Rollback: force revision conflict so rate-limit consume is undone.
    let action3 = insert_pending(&store, &task, 41);
    let before = count_rate_rows(&store);
    assert_eq!(
        store
            .with_write_transaction(|tx| {
                tx.evaluate_action_permission(EvaluateActionPermissionCommand {
                    expected_action_revision: 99,
                    ..eval_command(&action3, 41, 1)
                })
            })
            .expect_err("conflict")
            .code,
        StoreErrorCode::ConstraintViolation
    );
    assert_eq!(count_rate_rows(&store), before);
    assert!(store
        .get_current_permission_decision_for_action(&action3)
        .expect("pd")
        .is_none());
}

#[test]
fn evaluation_stop_fence_fails_closed_without_pd() {
    let database = EvalDatabase::new();
    let store = database.open();
    let (_task, action) = seed_task_action(&store, 5);
    let mut command = eval_command(&action, 5, 1);
    command.kernel_invariant = KernelInvariantState::StopFence { generation: 7 };
    assert_eq!(
        store
            .with_write_transaction(|tx| tx.evaluate_action_permission(command))
            .expect_err("fence")
            .code,
        StoreErrorCode::ContractInvalid
    );
    assert!(store
        .get_current_permission_decision_for_action(&action)
        .expect("pd")
        .is_none());
    let action_doc = store.get_action(&action).expect("get").expect("exists");
    assert_eq!(action_doc.status, kernel_contracts::ActionStatus::Pending);
    assert_eq!(action_doc.revision, 1);
}

#[test]
fn evaluation_fingerprints_recompute_identically() {
    let database = EvalDatabase::new();
    let store = database.open();
    let (_task, action) = seed_task_action(&store, 6);
    let first = store
        .with_write_transaction(|tx| tx.evaluate_action_permission(eval_command(&action, 6, 1)))
        .expect("first");
    // Re-project observation NotApplicable should match stored fingerprint.
    let obs = kernel_authorization::project_observation_evidence(
        ObservationEvidenceFactsV1::NotApplicable,
    )
    .expect("obs");
    assert_eq!(first.decision.observation_evidence_fingerprint, obs.sha256);
}

fn seed_task_action(store: &SqliteStore, number: u32) -> (String, String) {
    let task = create_root_task(store, number);
    let action = insert_pending(store, &task, number);
    (task, action)
}

fn eval_command(
    action_id: &str,
    number: u32,
    expected_revision: i64,
) -> EvaluateActionPermissionCommand {
    EvaluateActionPermissionCommand {
        action_id: action_id.to_owned(),
        expected_action_revision: expected_revision,
        permission_decision_id: format!("d0000000-0000-4000-8000-{number:012}"),
        audit_record_id: format!("e0000000-0000-4000-8000-{number:012}"),
        correlation_id: Some(format!("corr-eval-{number}")),
        causation_ref: None,
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
        content_origins: vec![],
        content_origin_refs: vec![Uuid::parse_str(&format!(
            "30000000-0000-4000-8000-{number:012}"
        ))
        .unwrap()],
        kernel_invariant: KernelInvariantState::Clear,
        security_mode: "normal".into(),
        evaluated_at: Utc
            .with_ymd_and_hms(2026, 7, 21, 13, 0, number % 60)
            .unwrap(),
        target_kind: "kernel_action".into(),
        target_stable_ref: None,
        destination: None,
        protected_surface_labels: vec![],
        observation: ObservationEvidenceFactsV1::NotApplicable,
        child_task_delta_hash: None,
        task_proposal_hash: None,
        proposed_plan_version: None,
        proposed_plan_hash: None,
        delegation: None,
        local_presence: None,
        delegation_ref: None,
        delegation_authority_ref: None,
        delegation_revision: None,
        decision_expires_at: None,
        enforce_task_scope_containment: true,
        state_transition: EvaluateActionStateTransitionAllocation {
            transition_id: Uuid::parse_str(&format!("70000000-0000-4000-8000-{number:012}"))
                .unwrap(),
            event_id: Uuid::parse_str(&format!("e0000000-0000-4000-8000-{number:012}")).unwrap(),
            correlation_id: format!("corr-transition-{number}"),
            dedup_key: format!("dedup-transition-{number}"),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_rule(
    id: &str,
    revision: i64,
    effect: PolicyRuleV2Effect,
    priority: i64,
    capability_ids: Vec<String>,
    operation_patterns: Vec<String>,
    confirmation_mode: Option<ConfirmationModeV1>,
    rate_limit: Option<PolicyRuleV2ConditionRateLimit>,
) -> PolicyRuleV2 {
    let actor = Actor {
        authentication_level: ActorAuthenticationLevel::PlatformVerified,
        confidence: Some(0.9),
        id: "actor-policy".into(),
        kind: ActorKind::KnownUser,
        revision: 1,
        schema_version: ActorSchemaVersion,
        source: "actor-source://local/desktop".into(),
    };
    let now = "2026-07-21T12:00:00Z".to_owned();
    PolicyRuleV2 {
        id: id.into(),
        schema_version: PolicyRuleV2SchemaVersion,
        revision,
        name: id.into(),
        description: "eval test rule".into(),
        priority,
        enabled: true,
        actor_match: PolicyRuleV2ActorMatch {
            kind: None,
            source_patterns: None,
            entry_point: None,
            auth_level_min: None,
        },
        content_origin_match: PolicyRuleV2ContentOriginMatch {
            kinds: None,
            source_patterns: None,
        },
        resource_match: PolicyRuleV2ResourceMatch {
            scope_patterns: vec![],
            exclude_patterns: vec![],
        },
        action_match: PolicyRuleV2ActionMatch {
            capability_ids,
            operation_patterns,
            side_effect_max: None,
        },
        condition: PolicyRuleV2Condition {
            time_window: None,
            rate_limit,
            delegation_required: None,
            local_presence_required: None,
        },
        effect,
        confirmation_mode,
        expires_at: None,
        created_by: PolicyRuleV2CreatedBy {
            actor: actor.clone(),
            entry_point: EntryPoint::LocalDesktop,
        },
        updated_by: PolicyRuleV2UpdatedBy {
            actor,
            entry_point: EntryPoint::LocalDesktop,
        },
        created_at: now.clone(),
        updated_at: now,
        source: PolicyRuleV2Source::UserDefined,
    }
}

fn count_rate_rows(store: &SqliteStore) -> i64 {
    store
        .with_write_transaction(|tx| {
            let count: i64 = tx
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM policy_rate_limit_consumptions",
                    [],
                    |row| row.get(0),
                )
                .expect("count");
            Ok(count)
        })
        .expect("tx")
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
    // Action.task_scope_ref must equal the owning Task's scope (created with same number as task).
    // Extra actions on the same task reuse the task's scope id derived from task_id suffix.
    let task = store.get_task(task_id).expect("task").expect("exists");
    let command = InsertPendingActionCommand {
        action_id: format!("a0000000-0000-4000-8000-{number:012}"),
        task_id: task_id.to_owned(),
        step_id: None,
        parent_action_id: None,
        capability_id: "kernel.task".into(),
        operation: "task.child.create".into(),
        structured_arguments: Map::from_iter([("goal".into(), json!("child"))]),
        // Must fall within root task scope resource_patterns: HTTPS://Example.COM:443/a/**
        resource_refs: vec![format!("https://example.com/a/item-{number}")],
        task_scope_ref: task.task_scope_ref.clone(),
        side_effect_class: SideEffectClass::S1,
        idempotency_key: format!("eval-action-{number}"),
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
            idempotency_key: format!("root-for-eval-{number}"),
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
            correlation_id: format!("correlation-eval-{number}"),
            creation_provenance_id: format!("70000000-0000-4000-8000-{number:012}"),
            kernel_receipt_id: format!("40000000-0000-4000-8000-{number:012}"),
            schema_version: RootTaskCreateAllocationV2SchemaVersion,
            task_created_dedup_key: format!("dedup-eval-{number}"),
            task_created_event_id: format!("60000000-0000-4000-8000-{number:012}"),
            task_id: format!("00000000-0000-4000-8000-{number:012}"),
            task_scope_id: format!("20000000-0000-4000-8000-{number:012}"),
        },
        accepted_at: Utc
            .with_ymd_and_hms(2026, 7, 21, 8, 0, number % 60)
            .unwrap(),
    }
}
