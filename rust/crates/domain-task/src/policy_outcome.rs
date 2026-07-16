//! Policy evaluation outcome application without depending on `domain-policy`.
//!
//! Policy matcher lives elsewhere. This module only maps an already-evaluated
//! effect onto Action pending-state commands.

use kernel_contracts::ActionStatus;
use serde::{Deserialize, Serialize};

use crate::action::{
    apply_action_transition, apply_confirm_metadata_update, ActionEvidence, ActionTransitionCommand,
};
use crate::error::DomainTaskError;
use crate::types::ActionTransitionOutcome;

/// High-level Policy effect relevant to Action pending transitions.
///
/// Mirrors PolicyRule effects / confirm family. Does not implement matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEvaluationEffect {
    /// Policy allow (including default allow).
    Allow,
    /// Policy deny.
    Deny,
    /// Policy requires confirmation (Action must stay pending).
    Confirm,
}

/// Already-evaluated Policy result applied to a pending Action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyEvaluationOutcome {
    /// Effect of the evaluation.
    pub effect: PolicyEvaluationEffect,
    /// PermissionDecision id produced by the evaluator (required for allow/deny/confirm).
    pub permission_decision_ref: String,
    /// ApprovalRecord id for confirm (deferred) or for a resolved approval.
    pub approval_record_ref: Option<String>,
    /// Human / machine structured reason.
    pub reason: String,
}

/// Apply a Policy evaluation outcome to a pending Action.
///
/// - `allow` → `pending -> approved` (requires permission_decision_ref)
/// - `deny` → `pending -> cancelled`
/// - `confirm` → **metadata update**, stay `pending`, requires approval_record_ref
///
/// Confirm is **not** an approved status edge and is **not** reported by
/// [`crate::is_action_transition_allowed`]. Revision still advances so persistence
/// can record PermissionDecision / ApprovalRecord facts; no status-change event
/// intent is emitted.
///
/// `parent_action_id` mirrors ActionRequest: `None` original, `Some` compensation.
pub fn apply_policy_evaluation_outcome(
    action_id: impl Into<String>,
    parent_action_id: Option<String>,
    current_status: ActionStatus,
    current_revision: u64,
    expected_revision: Option<u64>,
    outcome: &PolicyEvaluationOutcome,
) -> Result<ActionTransitionOutcome, DomainTaskError> {
    if current_status != ActionStatus::Pending {
        return Err(DomainTaskError::invariant(format!(
            "PolicyEvaluationOutcome can only be applied to pending Action, got {}",
            current_status.as_str()
        )));
    }
    if outcome.permission_decision_ref.trim().is_empty() {
        return Err(DomainTaskError::missing_evidence(
            "permission_decision_ref is required after Policy evaluation",
        ));
    }

    let action_id = action_id.into();
    match outcome.effect {
        PolicyEvaluationEffect::Allow => {
            let cmd = ActionTransitionCommand {
                action_id,
                parent_action_id,
                current_status,
                current_revision,
                expected_revision,
                target_status: ActionStatus::Approved,
                reason: outcome.reason.clone(),
                evidence: ActionEvidence {
                    permission_decision_ref: Some(outcome.permission_decision_ref.clone()),
                    approval_record_ref: outcome.approval_record_ref.clone(),
                    ..ActionEvidence::default()
                },
            };
            apply_action_transition(&cmd)
        }
        PolicyEvaluationEffect::Deny => {
            let cmd = ActionTransitionCommand {
                action_id,
                parent_action_id,
                current_status,
                current_revision,
                expected_revision,
                target_status: ActionStatus::Cancelled,
                reason: outcome.reason.clone(),
                evidence: ActionEvidence {
                    permission_decision_ref: Some(outcome.permission_decision_ref.clone()),
                    approval_record_ref: outcome.approval_record_ref.clone(),
                    ..ActionEvidence::default()
                },
            };
            apply_action_transition(&cmd)
        }
        PolicyEvaluationEffect::Confirm => {
            let approval = outcome.approval_record_ref.as_ref().ok_or_else(|| {
                DomainTaskError::missing_evidence(
                    "Policy confirm requires approval_record_ref (deferred ApprovalRecord)",
                )
            })?;
            if approval.trim().is_empty() {
                return Err(DomainTaskError::missing_evidence(
                    "Policy confirm approval_record_ref must be non-empty",
                ));
            }
            let cmd = ActionTransitionCommand {
                action_id,
                parent_action_id,
                current_status,
                current_revision,
                expected_revision,
                target_status: ActionStatus::Pending,
                reason: outcome.reason.clone(),
                evidence: ActionEvidence {
                    permission_decision_ref: Some(outcome.permission_decision_ref.clone()),
                    approval_record_ref: Some(approval.clone()),
                    ..ActionEvidence::default()
                },
            };
            apply_confirm_metadata_update(&cmd)
        }
    }
}
