use crate::{normalize_uri, PolicyError, PolicyErrorCode};
use chrono::{DateTime, Utc};
use kernel_contracts::{
    sha256_canonical, Actor, ContentOrigin, EntryPoint, PermissionDecisionDecision, PolicyRule,
    PolicyRuleConfirmationMode, SideEffectClass,
};
use serde::Serialize;
use serde_json::Value;

/// Kernel invariant state evaluated before ordinary policy rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum KernelInvariantState {
    /// No invariant blocks ordinary policy evaluation.
    Clear,
    /// The persistent Emergency Stop fence is active.
    StopFence {
        /// Persistent Stop Fence generation.
        generation: u64,
    },
    /// A recovery invariant blocks this action independently of PolicyRule.
    Recovery {
        /// Stable Kernel recovery reason code.
        reason_code: String,
    },
}

/// Evidence that an active Delegation covers the current Action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DelegationCoverageEvidence {
    /// Stable Delegation reference selected by the Kernel.
    pub delegation_ref: String,
}

/// Current local-presence evidence observed by the Kernel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalPresenceEvidence {
    /// Stable evidence reference; the matcher does not create or validate it.
    pub evidence_ref: String,
}

/// Pure policy evaluation facts. No field creates authorization, time, IDs, or persistence facts.
#[derive(Debug, Clone)]
pub struct PolicyEvaluationContext {
    /// Generated Actor snapshot.
    pub actor: Actor,
    /// Current invocation entry point, separate from Actor and ContentOrigin entry points.
    pub entry_point: EntryPoint,
    /// All content origins relevant to this evaluation.
    pub content_origins: Vec<ContentOrigin>,
    /// Current Task ID when one exists.
    pub task_id: Option<String>,
    /// Current Action ID when one exists.
    pub action_id: Option<String>,
    /// Plan version used by PermissionDecision binding.
    pub plan_version: i64,
    /// Action resource URI facts. They are normalized by the matcher.
    pub resource_refs: Vec<String>,
    /// Action capability ID.
    pub capability_id: String,
    /// Action operation.
    pub operation: String,
    /// Generated side-effect class label.
    pub side_effect_class: SideEffectClass,
    /// Exact structured arguments used for RFC 8785 key-parameter binding.
    pub structured_arguments: Value,
    /// Existing covering Delegation evidence, if any.
    pub delegation: Option<DelegationCoverageEvidence>,
    /// Existing local-presence evidence, if any.
    pub local_presence: Option<LocalPresenceEvidence>,
    /// Kernel-supplied evaluation instant.
    pub evaluation_instant: DateTime<Utc>,
    /// Current security-mode name; it is context only and has no hidden behavior.
    pub security_mode: String,
    /// Stop Fence / recovery invariant state, evaluated before rules.
    pub kernel_invariant: KernelInvariantState,
}

/// Binding material that can be persisted later as part of a PermissionDecision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionBindingDraft {
    /// Existing Action ID; absent when the caller is preflighting before Action creation.
    pub action_id: Option<String>,
    /// Existing plan version.
    pub plan_version: i64,
    /// Sorted, deduplicated, normalized resource URI list.
    pub resource_refs: Vec<String>,
    /// SHA-256 of RFC 8785 canonical structured arguments.
    pub key_params_hash: String,
}

/// Canonicalizable policy-evaluation material returned to agentd.
///
/// This crate intentionally does not call it `evaluation_context_hash`: no persisted complete
/// context schema/policy-set revision envelope exists yet. agentd must add the persistence-owned
/// fields and hash the final schema-defined object.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CanonicalEvaluationInput {
    /// Actor snapshot.
    pub actor: Actor,
    /// Current entry point.
    pub entry_point: EntryPoint,
    /// Content origins as supplied by the Kernel.
    pub content_origins: Vec<ContentOrigin>,
    /// Task reference.
    pub task_id: Option<String>,
    /// Action reference.
    pub action_id: Option<String>,
    /// Normalized resource references.
    pub resource_refs: Vec<String>,
    /// Capability ID.
    pub capability_id: String,
    /// Operation.
    pub operation: String,
    /// Side-effect class.
    pub side_effect_class: SideEffectClass,
    /// Structured arguments.
    pub structured_arguments: Value,
    /// Covering Delegation reference.
    pub delegation_ref: Option<String>,
    /// Local-presence evidence reference.
    pub local_presence_evidence_ref: Option<String>,
    /// Evaluation instant, RFC 3339 UTC.
    pub evaluation_instant: String,
    /// Security-mode context string.
    pub security_mode: String,
    /// Kernel invariant state.
    pub kernel_invariant: KernelInvariantState,
}

/// Non-persistent permission decision fields produced by the matcher.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PermissionDecisionDraft {
    /// Generated PermissionDecision decision enum.
    pub decision: PermissionDecisionDecision,
    /// Stable explanation codes (`default_allow` or matched rule ID).
    pub reason_codes: Vec<String>,
    /// Matched rule ID; `None` for Freedom-first Default Allow.
    pub matched_rule_ref: Option<String>,
    /// Normalized resource scopes granted by this draft.
    pub granted_scopes: Vec<String>,
    /// Decision-binding material; no persistence ID/revision/time is invented.
    pub binding: PermissionBindingDraft,
    /// Canonicalizable complete matcher input for later agentd persistence wrapping.
    pub canonical_evaluation_input: CanonicalEvaluationInput,
}

/// Why ordinary rules were bypassed by a Kernel invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelInvariantBlock {
    /// Emergency Stop is active.
    StopFence {
        /// Persistent Stop Fence generation.
        generation: u64,
    },
    /// Recovery invariant blocks the action.
    Recovery {
        /// Stable Kernel recovery reason code.
        reason_code: String,
    },
}

/// Pure matcher result. Errors are explicit and never become Default Allow.
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyEvaluationResult {
    /// Allowed by a rule or Freedom-first Default Allow.
    Allowed(PermissionDecisionDraft),
    /// A matching confirm rule selected the generated confirmation mode.
    RequiresConfirmation(PolicyRuleConfirmationMode, PermissionDecisionDraft),
    /// Denied by a matching rule.
    Denied(PermissionDecisionDraft),
    /// Blocked before ordinary matching by Stop Fence or recovery invariant.
    BlockedByKernelInvariant(KernelInvariantBlock),
    /// Invalid/unsupported policy input; fail closed.
    Error(PolicyError),
}

pub(crate) fn binding_material(
    context: &PolicyEvaluationContext,
) -> Result<(PermissionBindingDraft, CanonicalEvaluationInput), PolicyError> {
    if context.plan_version < 0 {
        return Err(PolicyError::new(
            PolicyErrorCode::InvalidRule,
            "plan_version must be non-negative",
        ));
    }
    let mut resource_refs = context
        .resource_refs
        .iter()
        .map(|resource| normalize_uri(resource))
        .collect::<Result<Vec<_>, _>>()?;
    resource_refs.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    resource_refs.dedup();
    let key_params_hash = sha256_canonical(&context.structured_arguments).map_err(|error| {
        PolicyError::new(PolicyErrorCode::CanonicalizationFailed, error.to_string())
    })?;
    let binding = PermissionBindingDraft {
        action_id: context.action_id.clone(),
        plan_version: context.plan_version,
        resource_refs: resource_refs.clone(),
        key_params_hash,
    };
    let canonical = CanonicalEvaluationInput {
        actor: context.actor.clone(),
        entry_point: context.entry_point,
        content_origins: context.content_origins.clone(),
        task_id: context.task_id.clone(),
        action_id: context.action_id.clone(),
        resource_refs,
        capability_id: context.capability_id.clone(),
        operation: context.operation.clone(),
        side_effect_class: context.side_effect_class,
        structured_arguments: context.structured_arguments.clone(),
        delegation_ref: context
            .delegation
            .as_ref()
            .map(|evidence| evidence.delegation_ref.clone()),
        local_presence_evidence_ref: context
            .local_presence
            .as_ref()
            .map(|evidence| evidence.evidence_ref.clone()),
        evaluation_instant: context.evaluation_instant.to_rfc3339(),
        security_mode: context.security_mode.clone(),
        kernel_invariant: context.kernel_invariant.clone(),
    };
    Ok((binding, canonical))
}

/// Parses a raw PolicyRule while mapping unknown Condition fields to the mandated fail-closed code.
pub fn parse_policy_rule_json(value: &Value) -> Result<PolicyRule, PolicyError> {
    const CONDITION_FIELDS: &[&str] = &[
        "time_window",
        "rate_limit",
        "delegation_required",
        "local_presence_required",
    ];
    if let Some(condition) = value.get("condition").and_then(Value::as_object) {
        if let Some(field) = condition
            .keys()
            .find(|field| !CONDITION_FIELDS.contains(&field.as_str()))
        {
            return Err(PolicyError::new(
                PolicyErrorCode::UnsupportedPolicyCondition,
                format!("unsupported Policy condition field: {field}"),
            ));
        }
    }
    serde_json::from_value(value.clone())
        .map_err(|error| PolicyError::new(PolicyErrorCode::InvalidRule, error.to_string()))
}
