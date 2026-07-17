//! Action status transition legality and domain command application.
//!
//! Implements CORE_ARCHITECTURE §11. Does not persist, lease-store, or call Policy.

use kernel_contracts::{ActionStatus, VerificationResultOutcome};
use serde::{Deserialize, Serialize};

use crate::error::DomainTaskError;
use crate::types::{ActionEffects, ActionTransitionOutcome, VerificationEvidenceSummary};

/// Certainty about whether dispatch to Extension/Provider has started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchCertainty {
    /// Kernel can prove dispatch has not started.
    NotStarted,
    /// Dispatch may have started; outcome unknown.
    Uncertain,
    /// Dispatch has started (in_flight path).
    Started,
}

/// Structured reason that an in-flight / leased outcome is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UncertainOutcomeReason {
    /// Extension / holder process crashed.
    Crash,
    /// Deadline / timeout without definitive result.
    Timeout,
    /// Provider returned an ambiguous result.
    Ambiguous,
}

/// Atomic lease + lock release effect required by leased exits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseReleaseEffect {
    /// Action whose lease must be invalidated (always bound from the command).
    pub action_id: String,
    /// Structured reason (`lease_expired` or cancel).
    pub reason: String,
    /// Persistence must release all resource locks held by this Action.
    pub release_all_resource_locks: bool,
    /// Persistence must invalidate the Lease record in the same transaction.
    pub invalidate_lease: bool,
}

/// Evidence bag for Action transitions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActionEvidence {
    /// PermissionDecision id (required to leave pending via allow/deny/confirm).
    pub permission_decision_ref: Option<String>,
    /// ApprovalRecord id (required for confirm stay / approval resolution).
    pub approval_record_ref: Option<String>,
    /// Verification summary (required for completed and failed targets).
    pub verification: Option<VerificationEvidenceSummary>,
    /// Provider-reported success alone is insufficient for completed.
    pub provider_reported_success: bool,
    /// Structured transition reason code (e.g. `lease_expired`).
    pub reason_code: Option<String>,
    /// Dispatch certainty for leased cancel / unknown paths.
    pub dispatch_certainty: Option<DispatchCertainty>,
    /// Structured uncertain-outcome reason (required for in_flight -> unknown).
    pub uncertain_outcome_reason: Option<UncertainOutcomeReason>,
}

/// Domain command to transition an Action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionTransitionCommand {
    /// Non-empty Action id (bound into lease release effects).
    pub action_id: String,
    /// Mirrors ActionRequest.parent_action_id.
    ///
    /// - `None`: original Action (may enter rolling_back / rolled_back / rollback_failed)
    /// - `Some(non-empty)`: compensation Action (ordinary chain only)
    pub parent_action_id: Option<String>,
    /// Current Action status.
    pub current_status: ActionStatus,
    /// Current Action revision.
    pub current_revision: u64,
    /// Optional optimistic concurrency check.
    pub expected_revision: Option<u64>,
    /// Desired target status.
    pub target_status: ActionStatus,
    /// Structured reason (required, non-empty).
    pub reason: String,
    /// Supporting evidence / reason codes.
    pub evidence: ActionEvidence,
}

/// Draft fields for a compensation Action (IDs supplied by caller; not generated here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationActionDraft {
    /// New compensation Action id (must differ from original).
    pub action_id: String,
    /// Original Action id being compensated (`parent_action_id`).
    pub parent_action_id: String,
    /// New idempotency key (must differ from original).
    pub idempotency_key: String,
    /// Original Action id (for comparison).
    pub original_action_id: String,
    /// Original idempotency key (for comparison).
    pub original_idempotency_key: String,
    /// Initial status of the compensation draft (must be pending).
    pub status: ActionStatus,
    /// permission_decision_ref must be null / absent until re-evaluated.
    pub permission_decision_ref: Option<String>,
}

/// Graph-only legality (CORE §11.3). Invariants enforced in apply.
///
/// Self-loops are **never** legal graph edges. Policy `confirm` is not a status
/// edge; it is a metadata update via [`crate::apply_policy_evaluation_outcome`].
pub fn is_action_transition_allowed(from: ActionStatus, to: ActionStatus) -> bool {
    use ActionStatus::*;
    if from == to {
        return false;
    }
    match from {
        Pending => matches!(to, Approved | Cancelled),
        Approved => matches!(to, Leased | Cancelled),
        Leased => matches!(to, InFlight | Approved | Cancelled | UnknownSideEffect),
        InFlight => matches!(to, Completed | Failed | UnknownSideEffect),
        UnknownSideEffect => matches!(to, Completed | Failed | RollingBack),
        Failed => matches!(to, RollingBack),
        RollingBack => matches!(to, RolledBack | RollbackFailed),
        Completed | RolledBack | RollbackFailed | Cancelled => false,
    }
}

/// Apply an Action transition command.
///
/// Does **not** handle Policy confirm. Confirm is only applied through
/// [`crate::apply_policy_evaluation_outcome`] (metadata update, no status edge).
pub fn apply_action_transition(
    cmd: &ActionTransitionCommand,
) -> Result<ActionTransitionOutcome, DomainTaskError> {
    validate_base_inputs(cmd)?;

    if let Some(expected) = cmd.expected_revision {
        if expected != cmd.current_revision {
            return Err(DomainTaskError::revision_conflict(
                expected,
                cmd.current_revision,
            ));
        }
    }

    if !is_action_transition_allowed(cmd.current_status, cmd.target_status) {
        return Err(DomainTaskError::illegal_action_transition(
            cmd.current_status,
            cmd.target_status,
        ));
    }

    let mut effects = ActionEffects::default();
    enforce_action_invariants(cmd, &mut effects)?;

    let new_revision = cmd
        .current_revision
        .checked_add(1)
        .ok_or_else(|| DomainTaskError::invalid_input("revision overflow"))?;

    Ok(ActionTransitionOutcome::with_event(
        cmd.current_status,
        cmd.target_status,
        new_revision,
        true,
        effects,
        &cmd.reason,
    ))
}

/// Apply a Policy-confirm metadata update while staying `pending`.
///
/// This is **not** a graph status edge. Revision advances so persistence can
/// bind PermissionDecision + ApprovalRecord facts; no status-change event intent
/// is emitted.
pub(crate) fn apply_confirm_metadata_update(
    cmd: &ActionTransitionCommand,
) -> Result<ActionTransitionOutcome, DomainTaskError> {
    validate_base_inputs(cmd)?;

    if let Some(expected) = cmd.expected_revision {
        if expected != cmd.current_revision {
            return Err(DomainTaskError::revision_conflict(
                expected,
                cmd.current_revision,
            ));
        }
    }

    if cmd.current_status != ActionStatus::Pending || cmd.target_status != ActionStatus::Pending {
        return Err(DomainTaskError::invariant(
            "confirm metadata update only applies when current and target are pending",
        ));
    }

    let mut effects = ActionEffects::default();
    enforce_confirm_metadata(cmd, &mut effects)?;

    let new_revision = cmd
        .current_revision
        .checked_add(1)
        .ok_or_else(|| DomainTaskError::invalid_input("revision overflow"))?;

    Ok(ActionTransitionOutcome::with_event(
        ActionStatus::Pending,
        ActionStatus::Pending,
        new_revision,
        false, // status unchanged; no status event intent
        effects,
        &cmd.reason,
    ))
}

/// Validate a compensation Action draft against CORE §11.4 / §13.4.
///
/// Does not generate ids. Compensation is a **new** Action: different id,
/// different idempotency_key, `parent_action_id` points at original, status
/// starts at `pending`, permission must be re-evaluated (ref absent/null).
pub fn validate_compensation_action_draft(
    draft: &CompensationActionDraft,
) -> Result<(), DomainTaskError> {
    if draft.action_id.trim().is_empty() || draft.parent_action_id.trim().is_empty() {
        return Err(DomainTaskError::illegal_compensation(
            "compensation action_id and parent_action_id must be non-empty",
        ));
    }
    if draft.action_id == draft.original_action_id {
        return Err(DomainTaskError::illegal_compensation(
            "compensation Action must have a different action_id from the original",
        ));
    }
    if draft.parent_action_id != draft.original_action_id {
        return Err(DomainTaskError::illegal_compensation(
            "compensation parent_action_id must point at the original Action",
        ));
    }
    if draft.idempotency_key.trim().is_empty() {
        return Err(DomainTaskError::illegal_compensation(
            "compensation idempotency_key must be non-empty",
        ));
    }
    if draft.idempotency_key == draft.original_idempotency_key {
        return Err(DomainTaskError::illegal_compensation(
            "compensation Action must use a different idempotency_key",
        ));
    }
    if draft.status != ActionStatus::Pending {
        return Err(DomainTaskError::illegal_compensation(
            "compensation Action must start as pending for re-evaluation",
        ));
    }
    if draft
        .permission_decision_ref
        .as_ref()
        .is_some_and(|r| !r.trim().is_empty())
    {
        return Err(DomainTaskError::illegal_compensation(
            "compensation Action must re-evaluate Policy; permission_decision_ref must be null at draft time",
        ));
    }
    Ok(())
}

/// Evaluate Policy outcome on pending Action (wrapper used by policy_outcome).
pub use crate::policy_outcome::apply_policy_evaluation_outcome as evaluate_policy_on_pending;

fn validate_base_inputs(cmd: &ActionTransitionCommand) -> Result<(), DomainTaskError> {
    if cmd.action_id.trim().is_empty() {
        return Err(DomainTaskError::invalid_input(
            "action_id must be non-empty",
        ));
    }
    if let Some(parent) = &cmd.parent_action_id {
        if parent.trim().is_empty() {
            return Err(DomainTaskError::invalid_input(
                "parent_action_id when present must be non-empty (use None for original Action)",
            ));
        }
        if parent == &cmd.action_id {
            return Err(DomainTaskError::invalid_input(
                "parent_action_id must differ from action_id",
            ));
        }
    }
    if cmd.current_revision < 1 {
        return Err(DomainTaskError::invalid_input(
            "current_revision must be >= 1",
        ));
    }
    if cmd.reason.trim().is_empty() {
        return Err(DomainTaskError::invalid_input(
            "reason must be non-empty structured text",
        ));
    }
    Ok(())
}

fn enforce_confirm_metadata(
    cmd: &ActionTransitionCommand,
    effects: &mut ActionEffects,
) -> Result<(), DomainTaskError> {
    let approval = cmd.evidence.approval_record_ref.as_ref().ok_or_else(|| {
        DomainTaskError::missing_evidence(
            "confirm requires approval_record_ref (deferred ApprovalRecord)",
        )
    })?;
    if approval.trim().is_empty() {
        return Err(DomainTaskError::missing_evidence(
            "confirm approval_record_ref must be non-empty",
        ));
    }
    let permission = cmd
        .evidence
        .permission_decision_ref
        .as_ref()
        .ok_or_else(|| {
            DomainTaskError::missing_evidence(
                "confirm requires permission_decision_ref after evaluation",
            )
        })?;
    if permission.trim().is_empty() {
        return Err(DomainTaskError::missing_evidence(
            "confirm permission_decision_ref must be non-empty",
        ));
    }
    effects.requires_approval_record_ref = true;
    Ok(())
}

fn lease_release(cmd: &ActionTransitionCommand, reason: &str) -> LeaseReleaseEffect {
    LeaseReleaseEffect {
        action_id: cmd.action_id.clone(),
        reason: reason.to_string(),
        release_all_resource_locks: true,
        invalidate_lease: true,
    }
}

fn enforce_action_invariants(
    cmd: &ActionTransitionCommand,
    effects: &mut ActionEffects,
) -> Result<(), DomainTaskError> {
    use ActionStatus::*;

    // Terminal constraints
    if matches!(
        cmd.current_status,
        Completed | RolledBack | RollbackFailed | Cancelled
    ) {
        return Err(DomainTaskError::illegal_action_transition(
            cmd.current_status,
            cmd.target_status,
        ));
    }

    // Compensation (parent_action_id present) never enters recovery orchestration states.
    enforce_compensation_parent_constraints(cmd)?;

    // pending -> approved requires permission decision
    if cmd.current_status == Pending && cmd.target_status == Approved {
        let permission = cmd
            .evidence
            .permission_decision_ref
            .as_ref()
            .ok_or_else(|| {
                DomainTaskError::missing_evidence(
                    "pending -> approved requires permission_decision_ref",
                )
            })?;
        if permission.trim().is_empty() {
            return Err(DomainTaskError::missing_evidence(
                "permission_decision_ref must be non-empty for approved",
            ));
        }
    }

    if let Some(permission) = &cmd.evidence.permission_decision_ref {
        if permission.trim().is_empty() {
            return Err(DomainTaskError::invalid_input(
                "permission_decision_ref when present must be non-empty",
            ));
        }
    }

    // leased -> approved only with lease_expired + atomic release effects
    if cmd.current_status == Leased && cmd.target_status == Approved {
        let reason_code = cmd.evidence.reason_code.as_deref().unwrap_or("");
        if reason_code != "lease_expired" {
            return Err(DomainTaskError::invariant(
                "leased -> approved is only legal with reason_code=lease_expired",
            ));
        }
        effects.release_lease_and_locks = Some(lease_release(cmd, "lease_expired"));
    }

    // leased -> cancelled: only when dispatch not started; atomic release
    if cmd.current_status == Leased && cmd.target_status == Cancelled {
        match cmd.evidence.dispatch_certainty {
            Some(DispatchCertainty::NotStarted) => {
                effects.release_lease_and_locks =
                    Some(lease_release(cmd, "cancelled_before_dispatch"));
            }
            Some(DispatchCertainty::Uncertain) | Some(DispatchCertainty::Started) => {
                return Err(DomainTaskError::invariant(
                    "leased cancel requires dispatch_not_started evidence; if dispatch is uncertain use unknown_side_effect",
                ));
            }
            None => {
                return Err(DomainTaskError::missing_evidence(
                    "leased -> cancelled requires dispatch_certainty=not_started evidence",
                ));
            }
        }
    }

    // leased -> unknown_side_effect when dispatch uncertain
    if cmd.current_status == Leased && cmd.target_status == UnknownSideEffect {
        match cmd.evidence.dispatch_certainty {
            Some(DispatchCertainty::Uncertain) | Some(DispatchCertainty::Started) => {
                effects.forbid_automatic_replay = true;
                effects.release_lease_and_locks = Some(lease_release(cmd, "dispatch_uncertain"));
            }
            Some(DispatchCertainty::NotStarted) => {
                return Err(DomainTaskError::invariant(
                    "dispatch_certainty=not_started must use leased -> cancelled, not unknown_side_effect",
                ));
            }
            None => {
                return Err(DomainTaskError::missing_evidence(
                    "leased -> unknown_side_effect requires dispatch_certainty uncertain|started evidence",
                ));
            }
        }
    }

    // in_flight -> unknown requires structured uncertain reason
    if cmd.current_status == InFlight && cmd.target_status == UnknownSideEffect {
        if cmd.evidence.uncertain_outcome_reason.is_none() {
            return Err(DomainTaskError::missing_evidence(
                "in_flight -> unknown_side_effect requires uncertain_outcome_reason (crash|timeout|ambiguous)",
            ));
        }
        effects.forbid_automatic_replay = true;
    }

    // completed requires VerificationResult outcome verified_ok
    if cmd.target_status == Completed {
        let verification = cmd.evidence.verification.as_ref().ok_or_else(|| {
            DomainTaskError::missing_evidence(
                "completed requires VerificationResult evidence; provider success is not enough",
            )
        })?;
        if verification.outcome != VerificationResultOutcome::VerifiedOk {
            return Err(DomainTaskError::invariant(format!(
                "completed requires verification outcome verified_ok, got {}",
                verification.outcome.as_str()
            )));
        }
        let _ = cmd.evidence.provider_reported_success;
    }

    // failed requires verification evidence that proves failure / no side effect
    if cmd.target_status == Failed {
        enforce_failed_verification(cmd)?;
    }

    // unknown_side_effect: no automatic replay
    if cmd.target_status == UnknownSideEffect {
        effects.forbid_automatic_replay = true;
    }

    Ok(())
}

/// Compensation is derived solely from `parent_action_id.is_some()` (ActionRequest fact).
fn is_compensation(cmd: &ActionTransitionCommand) -> bool {
    cmd.parent_action_id.is_some()
}

fn enforce_compensation_parent_constraints(
    cmd: &ActionTransitionCommand,
) -> Result<(), DomainTaskError> {
    use ActionStatus::*;

    if !is_compensation(cmd) {
        return Ok(());
    }

    // Compensation ordinary chain only; never recovery orchestration states.
    if matches!(cmd.target_status, RollingBack | RolledBack | RollbackFailed)
        || matches!(
            cmd.current_status,
            RollingBack | RolledBack | RollbackFailed
        )
    {
        return Err(DomainTaskError::invariant(
            "compensation Action (parent_action_id set) cannot enter rolling_back/rolled_back/rollback_failed; ordinary failure stays failed/unknown/completed",
        ));
    }
    // Failed -> RollingBack is only for original Action recovery orchestration.
    if cmd.current_status == Failed && cmd.target_status == RollingBack {
        return Err(DomainTaskError::invariant(
            "Failed -> RollingBack is only legal for original Action (parent_action_id=None)",
        ));
    }
    Ok(())
}

fn enforce_failed_verification(cmd: &ActionTransitionCommand) -> Result<(), DomainTaskError> {
    let verification = cmd.evidence.verification.as_ref().ok_or_else(|| {
        DomainTaskError::missing_evidence(
            "target failed requires VerificationEvidenceSummary; business fact must not be guessed",
        )
    })?;

    match verification.outcome {
        VerificationResultOutcome::VerifiedOk => Err(DomainTaskError::invariant(
            "target failed cannot use verification outcome verified_ok",
        )),
        VerificationResultOutcome::VerifiedFailed => {
            if verification.side_effect_confirmed == Some(true) {
                return Err(DomainTaskError::invariant(
                    "target failed cannot claim side_effect_confirmed=true; use recovery/compensation path",
                ));
            }
            Ok(())
        }
        VerificationResultOutcome::Inconclusive => {
            // CORE §12.2: failure is confirmable when side effect is known not to
            // have occurred. Inconclusive alone is insufficient without that fact.
            if verification.side_effect_confirmed != Some(false) {
                return Err(DomainTaskError::invariant(
                    "target failed with inconclusive verification requires side_effect_confirmed=Some(false)",
                ));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod graph_tests {
    use super::*;

    #[test]
    fn legal_edges_match_core_section_11() {
        let legal = [
            (ActionStatus::Pending, ActionStatus::Approved),
            (ActionStatus::Pending, ActionStatus::Cancelled),
            (ActionStatus::Approved, ActionStatus::Leased),
            (ActionStatus::Approved, ActionStatus::Cancelled),
            (ActionStatus::Leased, ActionStatus::InFlight),
            (ActionStatus::Leased, ActionStatus::Approved),
            (ActionStatus::Leased, ActionStatus::Cancelled),
            (ActionStatus::Leased, ActionStatus::UnknownSideEffect),
            (ActionStatus::InFlight, ActionStatus::Completed),
            (ActionStatus::InFlight, ActionStatus::Failed),
            (ActionStatus::InFlight, ActionStatus::UnknownSideEffect),
            (ActionStatus::UnknownSideEffect, ActionStatus::Completed),
            (ActionStatus::UnknownSideEffect, ActionStatus::Failed),
            (ActionStatus::UnknownSideEffect, ActionStatus::RollingBack),
            (ActionStatus::Failed, ActionStatus::RollingBack),
            (ActionStatus::RollingBack, ActionStatus::RolledBack),
            (ActionStatus::RollingBack, ActionStatus::RollbackFailed),
        ];
        for (from, to) in legal {
            assert!(
                is_action_transition_allowed(from, to),
                "expected legal: {} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
        assert!(!is_action_transition_allowed(
            ActionStatus::Pending,
            ActionStatus::Pending
        ));
    }

    #[test]
    fn nxn_counts() {
        let mut legal = 0usize;
        for &from in ActionStatus::ALL {
            for &to in ActionStatus::ALL {
                if is_action_transition_allowed(from, to) {
                    legal += 1;
                }
            }
        }
        // 17 legal status edges; confirm is not a graph edge
        assert_eq!(legal, 17);
    }
}
