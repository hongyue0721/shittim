//! Action permission evaluation orchestration (V2InitialBuildActive slice 4b).
//!
//! Single-transaction flow (one savepoint):
//! 1. Load Action (must be `pending`) + Task + TaskScope + PolicySet snapshot
//! 2. Optional TaskScope resource containment (fail closed)
//! 3. Convert enabled PolicyRuleV2 heads → domain-policy matcher `PolicyRule` surface
//! 4. `domain_policy::evaluate_policy` with transaction-bound `RateLimitPort`
//! 5. Project material + observation fingerprints via `kernel-authorization`
//! 6. Append immutable PermissionDecisionV2 (continuous `decision_revision`)
//! 7. Append `permission.evaluated` AuditRecordV2 (`policy_context` consistent with PD)
//! 8. Project Action via `domain_task::apply_policy_evaluation_outcome` + CAS
//!
//! Approval creation is **not** in this slice: `require_*` leaves Action `pending` with
//! `permission_decision_ref` bound (deferred). No `action.state_changed` Outbox write here.
//! Failures roll back the whole savepoint: no PD, no audit, no rate-limit consume, no CAS.

use crate::action::{format_time as format_action_time, get_action};
use crate::action_transition::MarkCommittedCommand;
use crate::permission_decision::{get_current_for_action, get_permission_decision};
use crate::policy_rule::{get_policy_set_revision, list_enabled_current_policy_rules};
use crate::root_task_create_v2::get_audit_v2 as read_audit_v2;
use crate::task::{encode_contract_document, get_task, get_task_scope};
use crate::{StoreError, StoreErrorCode, WriteTransaction};
use chrono::{DateTime, SecondsFormat, Utc};
use domain_policy::{
    evaluate_policy, resource_refs_within_task_scope, KernelInvariantBlock, KernelInvariantState,
    PermissionDecisionDraft, PolicyEvaluationContext, PolicyEvaluationResult,
};
use domain_task::{
    apply_policy_evaluation_outcome, ActionEvidence, PolicyEvaluationEffect,
    PolicyEvaluationOutcome,
};
use kernel_authorization::{
    material_policy_set_revision_for_projection, project_material_authorization,
    project_observation_evidence, MaterialAuthorizationFactsV1, ObservationEvidenceFactsV1,
};
use kernel_contracts::{
    ActionStatus, ActionTransitionIntentV1, ActionTransitionIntentV1SchemaVersion, Actor,
    AuditRecordV2, AuditRecordV2AuditType, AuditRecordV2ExternalContentStatus, AuditRecordV2Level,
    AuditRecordV2Outcome, AuditRecordV2PolicyContext, AuditRecordV2RollbackCapability,
    AuditRecordV2SchemaVersion, CausationRefV2, ConfirmationModeV1, ContentOrigin, EntryPoint,
    PermissionDecisionDecision, PermissionDecisionV2, PermissionDecisionV2ApprovalRequirement,
    PermissionDecisionV2Binding, PermissionDecisionV2Decision, PermissionDecisionV2SchemaVersion,
    PolicyRule, PolicyRuleActionMatch, PolicyRuleActorMatch, PolicyRuleActorMatchAuthLevelMin,
    PolicyRuleActorMatchKind, PolicyRuleCondition, PolicyRuleConditionRateLimit,
    PolicyRuleConditionRateLimitKeyScope, PolicyRuleConditionTimeWindow,
    PolicyRuleConditionTimeWindowWeekdaysItem, PolicyRuleConfirmationMode,
    PolicyRuleContentOriginMatch, PolicyRuleContentOriginMatchKindsItem, PolicyRuleCreatedBy,
    PolicyRuleEffect, PolicyRuleResourceMatch, PolicyRuleSchemaVersion, PolicyRuleSource,
    PolicyRuleUpdatedBy, PolicyRuleV2, PolicyRuleV2ActorMatchAuthLevelMin,
    PolicyRuleV2ActorMatchKind, PolicyRuleV2ConditionRateLimitKeyScope,
    PolicyRuleV2ConditionTimeWindowWeekdaysItem, PolicyRuleV2ContentOriginMatchKindsItem,
    PolicyRuleV2Effect, PolicyRuleV2Source,
};
use rusqlite::params;
use serde_json::{Map, Value};
use uuid::Uuid;

const EVALUATION_SAVEPOINT: &str = "kernel_sqlite_evaluate_action_permission";
const AUDIT_RECORD_V2_SCHEMA: &str = "https://schemas.shittim.local/audit/audit_record/v2";
const ACTION_SCHEMA: &str = "https://schemas.shittim.local/task/action_request/v2";

/// Caller-allocated identifiers for an evaluation-originated status edge.
///
/// Allow/deny evaluation outcomes change Action status and must emit `action.state_changed`
/// through the sole status-event authority (transition intent + `mark_committed_with_event`).
/// All identifiers must be globally unique; evaluation never allocates IDs itself.
#[derive(Debug, Clone)]
pub struct EvaluateActionStateTransitionAllocation {
    /// ActionTransitionIntent UUID.
    pub transition_id: Uuid,
    /// `action.state_changed` Event UUID.
    pub event_id: Uuid,
    /// Non-empty correlation id for the transition intent and event.
    pub correlation_id: String,
    /// Non-empty dedup key for the event.
    pub dedup_key: String,
}

/// Caller-injected facts for one Action permission evaluation.
///
/// Repository does not invent Actor, observation, material labels, timestamps, or IDs.
#[derive(Debug, Clone)]
pub struct EvaluateActionPermissionCommand {
    /// Action UUID (must exist, status pending).
    pub action_id: String,
    /// Expected Action revision for CAS.
    pub expected_action_revision: i64,
    /// Caller-allocated PermissionDecision UUID.
    pub permission_decision_id: String,
    /// Caller-allocated state-transition bundle for allow/deny edges.
    pub state_transition: EvaluateActionStateTransitionAllocation,
    /// Caller-allocated AuditRecord UUID for `permission.evaluated`.
    pub audit_record_id: String,
    /// Optional correlation id for the audit record.
    pub correlation_id: Option<String>,
    /// Optional causation (e.g. command_request that triggered evaluation).
    pub causation_ref: Option<CausationRefV2>,
    /// Actor snapshot used for policy matching and material fingerprint.
    pub actor: Actor,
    /// Invocation entry point.
    pub entry_point: EntryPoint,
    /// Content origins relevant to evaluation (matcher input).
    pub content_origins: Vec<ContentOrigin>,
    /// ContentOrigin UUIDs for material fingerprint.
    pub content_origin_refs: Vec<Uuid>,
    /// Kernel invariant state (Stop Fence / Recovery before ordinary rules).
    pub kernel_invariant: KernelInvariantState,
    /// Security-mode context string (no hidden behavior).
    pub security_mode: String,
    /// Evaluation instant (PD.evaluated_at / Action.updated_at / Audit.occurred_at).
    pub evaluated_at: DateTime<Utc>,
    /// Material target kind (required by material projection).
    pub target_kind: String,
    /// Optional stable target ref.
    pub target_stable_ref: Option<String>,
    /// Optional destination facts for material projection.
    pub destination: Option<kernel_authorization::DestinationFactsV1>,
    /// Material protected-surface labels.
    pub protected_surface_labels: Vec<kernel_authorization::ProtectedSurfaceLabelFactsV1>,
    /// Observation facts (`NotApplicable` or `Observed`).
    pub observation: ObservationEvidenceFactsV1,
    /// Optional child delta hash for material projection.
    pub child_task_delta_hash: Option<String>,
    /// Optional task proposal hash.
    pub task_proposal_hash: Option<String>,
    /// Optional proposed plan version.
    pub proposed_plan_version: Option<u64>,
    /// Optional proposed plan hash.
    pub proposed_plan_hash: Option<String>,
    /// Optional Delegation coverage for matcher.
    pub delegation: Option<domain_policy::DelegationCoverageEvidence>,
    /// Optional local presence for matcher.
    pub local_presence: Option<domain_policy::LocalPresenceEvidence>,
    /// Optional Delegation UUID for material projection.
    pub delegation_ref: Option<Uuid>,
    /// Optional Delegation authority ref.
    pub delegation_authority_ref: Option<String>,
    /// Optional Delegation revision.
    pub delegation_revision: Option<u64>,
    /// Optional PD expires_at.
    pub decision_expires_at: Option<DateTime<Utc>>,
    /// Whether to enforce TaskScope resource containment before matching.
    pub enforce_task_scope_containment: bool,
}

/// Successful evaluation result.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluateActionPermissionResult {
    /// Immutable PermissionDecisionV2 just appended.
    pub decision: PermissionDecisionV2,
    /// Action snapshot after CAS (`permission_decision_ref` bound; status may change).
    pub action: kernel_contracts::ActionRequestV2,
    /// AuditRecordV2 `permission.evaluated`.
    pub audit: AuditRecordV2,
    /// PolicySet revision snapshot used for this evaluation.
    pub policy_set_revision: i64,
}

impl WriteTransaction<'_> {
    /// Evaluates policy for a pending Action and persists PD + Audit + Action CAS atomically.
    pub fn evaluate_action_permission(
        &self,
        command: EvaluateActionPermissionCommand,
    ) -> Result<EvaluateActionPermissionResult, StoreError> {
        self.with_savepoint(EVALUATION_SAVEPOINT, |_| evaluate_inside(self, &command))
    }
}

fn evaluate_inside(
    transaction: &WriteTransaction<'_>,
    command: &EvaluateActionPermissionCommand,
) -> Result<EvaluateActionPermissionResult, StoreError> {
    validate_command(command)?;
    let connection = transaction.connection();

    let action = get_action(connection, &command.action_id)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::NotFound,
            "action was not found for permission evaluation",
        )
    })?;
    if action.status != kernel_contracts::ActionStatus::Pending {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "permission evaluation requires pending action",
        ));
    }
    if action.revision != command.expected_action_revision {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action expected revision does not match stored revision",
        ));
    }

    let task = get_task(connection, &action.task_id)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::StoredDataInvalid,
            "owning task missing for permission evaluation",
        )
    })?;
    let scope = get_task_scope(connection, &action.task_scope_ref)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::StoredDataInvalid,
            "task scope missing for permission evaluation",
        )
    })?;
    if scope.task_id != task.id || action.task_scope_ref != scope.id {
        return Err(stored_invalid());
    }

    if command.enforce_task_scope_containment {
        let within = resource_refs_within_task_scope(
            &scope.resource_patterns,
            &scope.exclusions,
            &action.resource_refs,
        )
        .map_err(|error| {
            StoreError::new(
                StoreErrorCode::ContractInvalid,
                format!(
                    "task scope containment check failed: {}",
                    error.code.as_str()
                ),
            )
        })?;
        if !within {
            return Err(StoreError::new(
                StoreErrorCode::ContractInvalid,
                "action resource_refs are outside task scope",
            ));
        }
    }

    let policy_set_revision = get_policy_set_revision(connection)?;
    let rule_heads = list_enabled_current_policy_rules(connection)?;

    // domain-policy matcher still consumes PolicyRule v1 surface. remote_signature is
    // ConfirmationModeV1-only on PolicyRuleV2 and has no v1 closed-set equivalent; such
    // rules are excluded from production decisions until the matcher is upgraded to v2
    // (fail closed: they never match, rather than being approximated).
    let mut matcher_rules = Vec::with_capacity(rule_heads.len());
    for rule in &rule_heads {
        if matches!(
            rule.confirmation_mode,
            Some(ConfirmationModeV1::RemoteSignature)
        ) {
            continue;
        }
        matcher_rules.push(policy_rule_v2_to_matcher(rule)?);
    }

    let context = PolicyEvaluationContext {
        actor: command.actor.clone(),
        entry_point: command.entry_point,
        content_origins: command.content_origins.clone(),
        task_id: Some(task.id.clone()),
        action_id: Some(action.action_id.clone()),
        plan_version: task.plan_version,
        resource_refs: action.resource_refs.clone(),
        capability_id: action.capability_id.clone(),
        operation: action.operation.clone(),
        side_effect_class: action.side_effect_class,
        structured_arguments: action.structured_arguments.clone(),
        delegation: command.delegation.clone(),
        local_presence: command.local_presence.clone(),
        evaluation_instant: command.evaluated_at,
        security_mode: command.security_mode.clone(),
        kernel_invariant: command.kernel_invariant.clone(),
    };

    let rate_port = transaction.rate_limit_port();
    let evaluation = evaluate_policy(&matcher_rules, &context, &rate_port);

    let (draft, effect, confirmation_mode) = match evaluation {
        PolicyEvaluationResult::Allowed(draft) => (
            draft,
            PolicyEvaluationEffect::Allow,
            None::<ConfirmationModeV1>,
        ),
        PolicyEvaluationResult::Denied(draft) => (draft, PolicyEvaluationEffect::Deny, None),
        PolicyEvaluationResult::RequiresConfirmation(mode, draft) => {
            let mode_v2 = confirmation_mode_from_v1(mode)?;
            (draft, PolicyEvaluationEffect::Confirm, Some(mode_v2))
        }
        PolicyEvaluationResult::BlockedByKernelInvariant(block) => {
            return Err(StoreError::new(
                StoreErrorCode::ContractInvalid,
                match block {
                    KernelInvariantBlock::StopFence { generation } => {
                        format!("stop_fence_active generation={generation}")
                    }
                    KernelInvariantBlock::Recovery { reason_code } => {
                        format!("recovery_invariant_blocked:{reason_code}")
                    }
                },
            ));
        }
        PolicyEvaluationResult::Error(error) => {
            return Err(StoreError::new(
                StoreErrorCode::ContractInvalid,
                format!("policy evaluation failed: {}", error.code.as_str()),
            ));
        }
    };

    let mut decision_enum = map_draft_decision(draft.decision)?;
    let final_effect = effect;
    let final_mode = confirmation_mode;

    if let Some(mode) = final_mode {
        decision_enum = decision_from_confirmation_mode(mode);
    }

    finalize_decision(
        transaction,
        command,
        &action,
        &task,
        policy_set_revision,
        draft,
        decision_enum,
        final_effect,
        final_mode,
    )
}

#[allow(clippy::too_many_arguments)]
fn finalize_decision(
    transaction: &WriteTransaction<'_>,
    command: &EvaluateActionPermissionCommand,
    action: &kernel_contracts::ActionRequestV2,
    task: &kernel_contracts::TaskSpec,
    policy_set_revision: i64,
    draft: PermissionDecisionDraft,
    decision_enum: PermissionDecisionV2Decision,
    effect: PolicyEvaluationEffect,
    confirmation_mode: Option<ConfirmationModeV1>,
) -> Result<EvaluateActionPermissionResult, StoreError> {
    let connection = transaction.connection();
    let evaluated_at = format_action_time(command.evaluated_at);

    let material_fp = project_material_fingerprint(command, action, task, policy_set_revision)?;
    let observation_fp = project_observation_fingerprint(&command.observation)?;

    let approval_requirement =
        confirmation_mode.map(|mode| PermissionDecisionV2ApprovalRequirement {
            confirmation_mode: mode,
            approval_chain_id: None,
            reusable_resolution_ref: None,
        });

    let binding = PermissionDecisionV2Binding {
        action_id: action.action_id.clone(),
        action_revision: action.revision,
        task_id: action.task_id.clone(),
        plan_version: task.plan_version,
        capability_id: action.capability_id.clone(),
        operation: action.operation.clone(),
        side_effect_class: action.side_effect_class,
        resource_refs: draft.binding.resource_refs.clone(),
        key_params_hash: draft.binding.key_params_hash.clone(),
        delegation_authority_ref: command.delegation_authority_ref.clone(),
    };

    let pd_draft = PermissionDecisionV2 {
        id: command.permission_decision_id.clone(),
        schema_version: PermissionDecisionV2SchemaVersion,
        action_id: action.action_id.clone(),
        decision: decision_enum,
        reason_codes: draft.reason_codes.clone(),
        matched_rule_ref: draft.matched_rule_ref.clone(),
        decision_revision: 0,
        evaluated_at: evaluated_at.clone(),
        policy_set_revision,
        material_authorization_fingerprint: material_fp,
        observation_evidence_fingerprint: observation_fp,
        binding,
        approval_requirement,
        expires_at: command
            .decision_expires_at
            .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true)),
        lease_ref: None,
    };

    let pd = transaction.append_permission_decision(pd_draft)?;

    let audit_outcome = match effect {
        PolicyEvaluationEffect::Allow => AuditRecordV2Outcome::Succeeded,
        PolicyEvaluationEffect::Deny => AuditRecordV2Outcome::Blocked,
        PolicyEvaluationEffect::Confirm => AuditRecordV2Outcome::Deferred,
    };
    let audit = build_permission_evaluated_audit(command, action, &pd, audit_outcome)?;
    let audit_json = encode_contract_document(AUDIT_RECORD_V2_SCHEMA, &audit)?;
    connection
        .execute(
            "INSERT INTO audit_records_v2(record_json) VALUES (?1)",
            params![audit_json],
        )
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    let stored_audit = read_audit_v2(connection, &audit.id)?.ok_or_else(stored_invalid)?;
    if stored_audit != audit {
        return Err(stored_invalid());
    }
    let ctx = stored_audit
        .policy_context
        .as_ref()
        .ok_or_else(stored_invalid)?;
    if ctx.matched_rule_ref != pd.matched_rule_ref
        || ctx.policy_set_revision != pd.policy_set_revision
        || ctx.permission_decision_revision != pd.decision_revision
        || ctx.material_authorization_fingerprint != pd.material_authorization_fingerprint
        || ctx.observation_evidence_fingerprint != pd.observation_evidence_fingerprint
    {
        return Err(stored_invalid());
    }

    // v2: confirm deferral is bound to the real PermissionDecision; no approval reference
    // exists or may be fabricated before slice 4c. Action.approval_chain_id stays null.
    let outcome = PolicyEvaluationOutcome {
        effect,
        permission_decision_ref: pd.id.clone(),
        approval_record_ref: None,
        reason: draft_reason(&pd),
    };
    let domain = apply_policy_evaluation_outcome(
        action.action_id.clone(),
        action.parent_action_id.clone(),
        action.status,
        action.revision as u64,
        Some(command.expected_action_revision as u64),
        &outcome,
    )
    .map_err(crate::action::map_domain_error)?;

    let updated_action = if domain.status_changed {
        // Allow/deny evaluation edges change Action status and must emit
        // `action.state_changed` through the sole status-event authority
        // (transition intent + mark_committed_with_event), never a silent CAS.
        let reason_code = match domain.new_status {
            ActionStatus::Approved => "policy_allow",
            ActionStatus::Cancelled => "policy_deny",
            other => {
                return Err(StoreError::new(
                    StoreErrorCode::ContractInvalid,
                    format!("policy evaluation produced unsupported status edge {other:?}"),
                ))
            }
        };
        let intent = ActionTransitionIntentV1 {
            schema_version: ActionTransitionIntentV1SchemaVersion,
            transition_id: command.state_transition.transition_id.to_string(),
            action_id: action.action_id.clone(),
            expected_action_revision: command.expected_action_revision,
            execution_generation: action.execution_generation,
            from_status: action.status,
            to_status: domain.new_status,
            reason_code: reason_code.to_owned(),
            correlation_id: command.state_transition.correlation_id.clone(),
            created_at: format_action_time(command.evaluated_at),
        };
        transaction.insert_intent(intent.clone())?;
        let (updated, _event_record) =
            transaction.mark_committed_with_event(MarkCommittedCommand {
                transition_id: intent.transition_id.clone(),
                event_id: command.state_transition.event_id,
                dedup_key: command.state_transition.dedup_key.clone(),
                changed_at: command.evaluated_at,
                evidence: ActionEvidence {
                    permission_decision_ref: Some(pd.id.clone()),
                    ..ActionEvidence::default()
                },
                result: None,
                permission_decision_ref: Some(pd.id.clone()),
                approval_chain_id: None,
                approval_resolution_ref: None,
            })?;
        updated
    } else {
        cas_pending_metadata(
            connection,
            action,
            command.expected_action_revision,
            domain.new_revision as i64,
            &pd.id,
            command.evaluated_at,
        )?
    };

    if updated_action.permission_decision_ref.as_deref() != Some(pd.id.as_str()) {
        return Err(stored_invalid());
    }
    let current_pd =
        get_current_for_action(connection, &action.action_id)?.ok_or_else(stored_invalid)?;
    if current_pd.id != pd.id {
        return Err(stored_invalid());
    }
    let _ = get_permission_decision(connection, &pd.id)?.ok_or_else(stored_invalid)?;

    Ok(EvaluateActionPermissionResult {
        decision: pd,
        action: updated_action,
        audit: stored_audit,
        policy_set_revision,
    })
}

fn cas_pending_metadata(
    connection: &rusqlite::Connection,
    current: &kernel_contracts::ActionRequestV2,
    expected_revision: i64,
    new_revision: i64,
    permission_decision_id: &str,
    updated_at: DateTime<Utc>,
) -> Result<kernel_contracts::ActionRequestV2, StoreError> {
    if current.revision != expected_revision {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action expected revision does not match stored revision",
        ));
    }
    if current.status != kernel_contracts::ActionStatus::Pending {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "confirm metadata CAS requires pending action",
        ));
    }
    if new_revision != expected_revision + 1 {
        return Err(StoreError::new(
            StoreErrorCode::InternalStoreError,
            "domain outcome revision mismatch",
        ));
    }
    let mut next = current.clone();
    next.revision = new_revision;
    next.permission_decision_ref = Some(permission_decision_id.to_owned());
    next.updated_at = format_action_time(updated_at);
    let record_json = encode_contract_document(ACTION_SCHEMA, &next)?;
    let changed = connection
        .execute(
            "UPDATE actions SET record_json = ?1 \
             WHERE id = ?2 AND revision = ?3 AND status = 'pending'",
            params![record_json, current.action_id, expected_revision],
        )
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    if changed != 1 {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action expected revision does not match stored revision",
        ));
    }
    get_action(connection, &current.action_id)?.ok_or_else(stored_invalid)
}

fn build_permission_evaluated_audit(
    command: &EvaluateActionPermissionCommand,
    action: &kernel_contracts::ActionRequestV2,
    pd: &PermissionDecisionV2,
    outcome: AuditRecordV2Outcome,
) -> Result<AuditRecordV2, StoreError> {
    Ok(AuditRecordV2 {
        action_id: Some(action.action_id.clone()),
        actor: Some(command.actor.clone()),
        approval_resolution_ref: None,
        artifact_refs: vec![],
        audit_type: AuditRecordV2AuditType::PermissionEvaluated,
        causation_ref: command.causation_ref.clone(),
        content_origin_refs: command
            .content_origin_refs
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        correlation_id: command.correlation_id.clone(),
        delegation_ref: command.delegation_ref.map(|id| id.to_string()),
        details: serde_json::json!({}),
        entry_point: command.entry_point,
        extension_id: None,
        external_content_status: AuditRecordV2ExternalContentStatus::NotSent,
        id: command.audit_record_id.clone(),
        level: AuditRecordV2Level::Security,
        model_call_refs: vec![],
        occurred_at: pd.evaluated_at.clone(),
        outcome,
        payload_manifest_refs: vec![],
        permission_decision_ref: Some(pd.id.clone()),
        policy_context: Some(AuditRecordV2PolicyContext {
            authentication_evidence_refs: vec![],
            child_task_delta_hash: command.child_task_delta_hash.clone(),
            matched_rule_ref: pd.matched_rule_ref.clone(),
            material_authorization_fingerprint: pd.material_authorization_fingerprint.clone(),
            observation_evidence_fingerprint: pd.observation_evidence_fingerprint.clone(),
            permission_decision_revision: pd.decision_revision,
            policy_set_revision: pd.policy_set_revision,
            reused_approval_resolution_ref: None,
        }),
        provider_id: None,
        reason_codes: pd.reason_codes.clone(),
        recovery_attempt_ref: None,
        resource_refs: pd.binding.resource_refs.clone(),
        rollback_capability: AuditRecordV2RollbackCapability::Unknown,
        schema_version: AuditRecordV2SchemaVersion,
        stop_fence_generation: match &command.kernel_invariant {
            KernelInvariantState::StopFence { generation } => Some(*generation as i64),
            _ => None,
        },
        summary: None,
        task_creation_context: None,
        task_id: Some(action.task_id.clone()),
        verification_result_refs: vec![],
    })
}

fn project_material_fingerprint(
    command: &EvaluateActionPermissionCommand,
    action: &kernel_contracts::ActionRequestV2,
    task: &kernel_contracts::TaskSpec,
    policy_set_revision: i64,
) -> Result<String, StoreError> {
    // policy_set_revision is authoritative PolicySet metadata: 0 is the bootstrap empty set.
    // The projection preimage must use the same value stored on the PD (no 0→1 rewrite);
    // re-verification callers share the same helper.
    let projection_revision = material_policy_set_revision_for_projection(policy_set_revision)
        .map_err(|_| contract_error())?;

    let key_params = match &action.structured_arguments {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };

    let task_id = Uuid::parse_str(&task.id).map_err(|_| contract_error())?;
    let action_id = Uuid::parse_str(&action.action_id).map_err(|_| contract_error())?;
    let scope_id = Uuid::parse_str(&action.task_scope_ref).map_err(|_| contract_error())?;

    let facts = MaterialAuthorizationFactsV1 {
        actor: command.actor.clone(),
        entry_point: command.entry_point,
        task_id,
        task_revision: task.revision as u64,
        task_plan_version: task.plan_version as u64,
        action_id,
        action_revision: action.revision as u64,
        capability_id: action.capability_id.clone(),
        operation: action.operation.clone(),
        side_effect_class: action.side_effect_class,
        normalized_key_params: key_params,
        task_scope_ref: scope_id,
        resource_refs: action.resource_refs.clone(),
        child_task_delta_hash: command.child_task_delta_hash.clone(),
        delegation_ref: command.delegation_ref,
        delegation_authority_ref: command.delegation_authority_ref.clone(),
        delegation_revision: command.delegation_revision,
        policy_set_revision: projection_revision,
        target_kind: command.target_kind.clone(),
        target_stable_ref: command.target_stable_ref.clone(),
        destination: command.destination.clone(),
        protected_surface_labels: command.protected_surface_labels.clone(),
        content_origin_refs: command.content_origin_refs.clone(),
        task_proposal_hash: command.task_proposal_hash.clone(),
        proposed_plan_version: command.proposed_plan_version,
        proposed_plan_hash: command.proposed_plan_hash.clone(),
    };
    let projection = project_material_authorization(facts).map_err(|error| {
        StoreError::new(
            StoreErrorCode::ContractInvalid,
            format!("material fingerprint projection failed: {error}"),
        )
    })?;
    Ok(projection.sha256)
}

fn project_observation_fingerprint(
    observation: &ObservationEvidenceFactsV1,
) -> Result<String, StoreError> {
    let projection = project_observation_evidence(observation.clone()).map_err(|error| {
        StoreError::new(
            StoreErrorCode::ContractInvalid,
            format!("observation fingerprint projection failed: {error}"),
        )
    })?;
    Ok(projection.sha256)
}

fn validate_command(command: &EvaluateActionPermissionCommand) -> Result<(), StoreError> {
    Uuid::parse_str(&command.action_id).map_err(|_| contract_error())?;
    Uuid::parse_str(&command.permission_decision_id).map_err(|_| contract_error())?;
    Uuid::parse_str(&command.audit_record_id).map_err(|_| contract_error())?;
    if command.expected_action_revision < 1 || command.target_kind.trim().is_empty() {
        return Err(contract_error());
    }
    Ok(())
}

fn draft_reason(pd: &PermissionDecisionV2) -> String {
    pd.reason_codes
        .first()
        .cloned()
        .unwrap_or_else(|| pd.decision.as_str().to_owned())
}

fn map_draft_decision(
    decision: PermissionDecisionDecision,
) -> Result<PermissionDecisionV2Decision, StoreError> {
    Ok(match decision {
        PermissionDecisionDecision::Allow => PermissionDecisionV2Decision::Allow,
        PermissionDecisionDecision::Deny => PermissionDecisionV2Decision::Deny,
        PermissionDecisionDecision::RequireConfirmation => {
            PermissionDecisionV2Decision::RequireConfirmation
        }
        PermissionDecisionDecision::RequireLocalConfirmation => {
            PermissionDecisionV2Decision::RequireLocalConfirmation
        }
        PermissionDecisionDecision::RequireSystemAuthentication => {
            PermissionDecisionV2Decision::RequireSystemAuthentication
        }
        PermissionDecisionDecision::RequirePlanRevision => {
            PermissionDecisionV2Decision::RequirePlanRevision
        }
    })
}

fn confirmation_mode_from_v1(
    mode: PolicyRuleConfirmationMode,
) -> Result<ConfirmationModeV1, StoreError> {
    Ok(match mode {
        PolicyRuleConfirmationMode::Generic => ConfirmationModeV1::Generic,
        PolicyRuleConfirmationMode::Local => ConfirmationModeV1::Local,
        PolicyRuleConfirmationMode::SystemAuthentication => {
            ConfirmationModeV1::SystemAuthentication
        }
        PolicyRuleConfirmationMode::PlanRevision => ConfirmationModeV1::PlanRevision,
    })
}

fn decision_from_confirmation_mode(mode: ConfirmationModeV1) -> PermissionDecisionV2Decision {
    match mode {
        ConfirmationModeV1::Generic => PermissionDecisionV2Decision::RequireConfirmation,
        ConfirmationModeV1::Local => PermissionDecisionV2Decision::RequireLocalConfirmation,
        ConfirmationModeV1::SystemAuthentication => {
            PermissionDecisionV2Decision::RequireSystemAuthentication
        }
        ConfirmationModeV1::RemoteSignature => PermissionDecisionV2Decision::RequireRemoteSignature,
        ConfirmationModeV1::PlanRevision => PermissionDecisionV2Decision::RequirePlanRevision,
    }
}

fn policy_rule_v2_to_matcher(rule: &PolicyRuleV2) -> Result<PolicyRule, StoreError> {
    if matches!(
        rule.confirmation_mode,
        Some(ConfirmationModeV1::RemoteSignature)
    ) {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "remote_signature policy rules must not enter v1 matcher conversion",
        ));
    }

    let confirmation_mode = match rule.confirmation_mode {
        None => None,
        Some(ConfirmationModeV1::Generic) => Some(PolicyRuleConfirmationMode::Generic),
        Some(ConfirmationModeV1::Local) => Some(PolicyRuleConfirmationMode::Local),
        Some(ConfirmationModeV1::SystemAuthentication) => {
            Some(PolicyRuleConfirmationMode::SystemAuthentication)
        }
        Some(ConfirmationModeV1::PlanRevision) => Some(PolicyRuleConfirmationMode::PlanRevision),
        Some(ConfirmationModeV1::RemoteSignature) => unreachable!("filtered above"),
    };

    let effect = match rule.effect {
        PolicyRuleV2Effect::Allow => PolicyRuleEffect::Allow,
        PolicyRuleV2Effect::Confirm => PolicyRuleEffect::Confirm,
        PolicyRuleV2Effect::Deny => PolicyRuleEffect::Deny,
    };

    Ok(PolicyRule {
        action_match: PolicyRuleActionMatch {
            capability_ids: rule.action_match.capability_ids.clone(),
            operation_patterns: rule.action_match.operation_patterns.clone(),
            side_effect_max: rule.action_match.side_effect_max,
        },
        actor_match: PolicyRuleActorMatch {
            kind: rule.actor_match.kind.map(|kind| match kind {
                PolicyRuleV2ActorMatchKind::Owner => PolicyRuleActorMatchKind::Owner,
                PolicyRuleV2ActorMatchKind::KnownUser => PolicyRuleActorMatchKind::KnownUser,
                PolicyRuleV2ActorMatchKind::Guest => PolicyRuleActorMatchKind::Guest,
                PolicyRuleV2ActorMatchKind::Companion => PolicyRuleActorMatchKind::Companion,
                PolicyRuleV2ActorMatchKind::System => PolicyRuleActorMatchKind::System,
                PolicyRuleV2ActorMatchKind::Extension => PolicyRuleActorMatchKind::Extension,
            }),
            source_patterns: rule.actor_match.source_patterns.clone(),
            entry_point: rule.actor_match.entry_point,
            auth_level_min: rule.actor_match.auth_level_min.map(|level| match level {
                PolicyRuleV2ActorMatchAuthLevelMin::Unauthenticated => {
                    PolicyRuleActorMatchAuthLevelMin::Unauthenticated
                }
                PolicyRuleV2ActorMatchAuthLevelMin::Asserted => {
                    PolicyRuleActorMatchAuthLevelMin::Asserted
                }
                PolicyRuleV2ActorMatchAuthLevelMin::PlatformVerified => {
                    PolicyRuleActorMatchAuthLevelMin::PlatformVerified
                }
                PolicyRuleV2ActorMatchAuthLevelMin::SystemAuthenticated => {
                    PolicyRuleActorMatchAuthLevelMin::SystemAuthenticated
                }
            }),
        },
        content_origin_match: PolicyRuleContentOriginMatch {
            kinds: rule.content_origin_match.kinds.as_ref().map(|kinds| {
                kinds
                    .iter()
                    .map(|kind| match kind {
                        PolicyRuleV2ContentOriginMatchKindsItem::UserInput => {
                            PolicyRuleContentOriginMatchKindsItem::UserInput
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::CompanionGenerated => {
                            PolicyRuleContentOriginMatchKindsItem::CompanionGenerated
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::SystemGenerated => {
                            PolicyRuleContentOriginMatchKindsItem::SystemGenerated
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::RemoteMessage => {
                            PolicyRuleContentOriginMatchKindsItem::RemoteMessage
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::WebContent => {
                            PolicyRuleContentOriginMatchKindsItem::WebContent
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::DocumentContent => {
                            PolicyRuleContentOriginMatchKindsItem::DocumentContent
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::ModelOutput => {
                            PolicyRuleContentOriginMatchKindsItem::ModelOutput
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::ExtensionOutput => {
                            PolicyRuleContentOriginMatchKindsItem::ExtensionOutput
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::ProviderOutput => {
                            PolicyRuleContentOriginMatchKindsItem::ProviderOutput
                        }
                        PolicyRuleV2ContentOriginMatchKindsItem::ImportedData => {
                            PolicyRuleContentOriginMatchKindsItem::ImportedData
                        }
                    })
                    .collect()
            }),
            source_patterns: rule.content_origin_match.source_patterns.clone(),
        },
        resource_match: PolicyRuleResourceMatch {
            scope_patterns: rule.resource_match.scope_patterns.clone(),
            exclude_patterns: rule.resource_match.exclude_patterns.clone(),
        },
        condition: PolicyRuleCondition {
            time_window: rule.condition.time_window.as_ref().map(|window| {
                PolicyRuleConditionTimeWindow {
                    timezone: window.timezone.clone(),
                    weekdays: window
                        .weekdays
                        .iter()
                        .map(|day| match day {
                            PolicyRuleV2ConditionTimeWindowWeekdaysItem::Monday => {
                                PolicyRuleConditionTimeWindowWeekdaysItem::Monday
                            }
                            PolicyRuleV2ConditionTimeWindowWeekdaysItem::Tuesday => {
                                PolicyRuleConditionTimeWindowWeekdaysItem::Tuesday
                            }
                            PolicyRuleV2ConditionTimeWindowWeekdaysItem::Wednesday => {
                                PolicyRuleConditionTimeWindowWeekdaysItem::Wednesday
                            }
                            PolicyRuleV2ConditionTimeWindowWeekdaysItem::Thursday => {
                                PolicyRuleConditionTimeWindowWeekdaysItem::Thursday
                            }
                            PolicyRuleV2ConditionTimeWindowWeekdaysItem::Friday => {
                                PolicyRuleConditionTimeWindowWeekdaysItem::Friday
                            }
                            PolicyRuleV2ConditionTimeWindowWeekdaysItem::Saturday => {
                                PolicyRuleConditionTimeWindowWeekdaysItem::Saturday
                            }
                            PolicyRuleV2ConditionTimeWindowWeekdaysItem::Sunday => {
                                PolicyRuleConditionTimeWindowWeekdaysItem::Sunday
                            }
                        })
                        .collect(),
                    local_start: window.local_start.clone(),
                    local_end: window.local_end.clone(),
                }
            }),
            rate_limit: rule.condition.rate_limit.as_ref().map(|limit| {
                PolicyRuleConditionRateLimit {
                    count: limit.count,
                    window_seconds: limit.window_seconds,
                    key_scope: match limit.key_scope {
                        PolicyRuleV2ConditionRateLimitKeyScope::Rule => {
                            PolicyRuleConditionRateLimitKeyScope::Rule
                        }
                        PolicyRuleV2ConditionRateLimitKeyScope::Actor => {
                            PolicyRuleConditionRateLimitKeyScope::Actor
                        }
                        PolicyRuleV2ConditionRateLimitKeyScope::Task => {
                            PolicyRuleConditionRateLimitKeyScope::Task
                        }
                        PolicyRuleV2ConditionRateLimitKeyScope::Action => {
                            PolicyRuleConditionRateLimitKeyScope::Action
                        }
                        PolicyRuleV2ConditionRateLimitKeyScope::Resource => {
                            PolicyRuleConditionRateLimitKeyScope::Resource
                        }
                    },
                }
            }),
            delegation_required: rule.condition.delegation_required,
            local_presence_required: rule.condition.local_presence_required,
        },
        effect,
        confirmation_mode,
        expires_at: rule.expires_at.clone(),
        created_by: PolicyRuleCreatedBy {
            actor: rule.created_by.actor.clone(),
            entry_point: rule.created_by.entry_point,
        },
        updated_by: PolicyRuleUpdatedBy {
            actor: rule.updated_by.actor.clone(),
            entry_point: rule.updated_by.entry_point,
        },
        created_at: rule.created_at.clone(),
        updated_at: rule.updated_at.clone(),
        id: rule.id.clone(),
        name: rule.name.clone(),
        description: rule.description.clone(),
        priority: rule.priority,
        enabled: rule.enabled,
        revision: rule.revision,
        schema_version: PolicyRuleSchemaVersion,
        source: match rule.source {
            PolicyRuleV2Source::UserDefined => PolicyRuleSource::UserDefined,
            PolicyRuleV2Source::CompanionGenerated => PolicyRuleSource::CompanionGenerated,
            PolicyRuleV2Source::System => PolicyRuleSource::System,
        },
    })
}

fn contract_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::ContractInvalid,
        "permission evaluation facts violate a generated JSON contract",
    )
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored data failed integrity validation during permission evaluation",
    )
}
