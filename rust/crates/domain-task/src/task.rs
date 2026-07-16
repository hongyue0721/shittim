//! Task status transition legality and domain command application.
//!
//! Implements CORE_ARCHITECTURE §10. Does not persist or emit real envelopes.

use std::collections::BTreeMap;

use kernel_contracts::{TaskStatus, VerificationResultOutcome};
use serde::{Deserialize, Serialize};

use crate::error::DomainTaskError;
use crate::types::{SideEffectRef, TaskTransitionOutcome, VerificationEvidenceSummary};

/// Evidence that one occurrence of a success criterion content was verified.
///
/// `criterion` is the full success-criterion string from TaskSpec (not an ID).
/// Duplicate contents are distinct multiset members and each needs its own evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessCriterionEvidence {
    /// Full success criterion content string (mirrors TaskSpec.success_criteria entry).
    pub criterion: String,
    /// Whether this criterion occurrence is satisfied.
    pub satisfied: bool,
    /// Optional verification evidence summary backing the claim.
    pub verification: Option<VerificationEvidenceSummary>,
}

/// Domain command to transition a Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTransitionCommand {
    /// Current Task status.
    pub current_status: TaskStatus,
    /// Current Task revision.
    pub current_revision: u64,
    /// Current plan version.
    ///
    /// `task.create` facts use `plan_version = 0`. Candidate-era transitions keep
    /// `0` until a replan edge (`failed|rolled_back -> planned`) increments it.
    pub current_plan_version: u64,
    /// Optional optimistic concurrency check.
    pub expected_revision: Option<u64>,
    /// Desired target status.
    pub target_status: TaskStatus,
    /// Structured reason (required, non-empty).
    pub reason: String,
    /// Whether this transition is an explicit replan request (`plan_version++`).
    ///
    /// Only legal for `failed -> planned` and `rolled_back -> planned`.
    pub replan: bool,
    /// Full success criteria content list from TaskSpec.success_criteria.
    ///
    /// Compared as a **multiset** (content string -> occurrence count) against
    /// evidence when targeting `succeeded`. Duplicates are allowed and each
    /// occurrence requires its own evidence entry. No criterion IDs.
    pub required_success_criteria: Vec<String>,
    /// Success criterion evidence (required multiset cover when targeting `succeeded`).
    pub success_criteria_evidence: Vec<SuccessCriterionEvidence>,
    /// Side effects already produced (required non-empty for `partially_completed`).
    pub produced_side_effect_refs: Vec<SideEffectRef>,
    /// Side effects that require external compensation when entering `rolling_back`.
    ///
    /// Must be non-empty for every transition whose target is `rolling_back`.
    /// Current `partially_completed` status alone is not proof; callers must pass
    /// the concrete refs on this command.
    pub rollback_required_side_effect_refs: Vec<SideEffectRef>,
}

/// Return whether `(from, to)` is a legal edge under CORE §10.2 (graph only).
///
/// Invariants (evidence, plan_version, revision) are checked by
/// [`apply_task_transition`]. Self-loops are never legal graph edges.
pub fn is_task_transition_allowed(from: TaskStatus, to: TaskStatus) -> bool {
    use TaskStatus::*;
    if from == to {
        return false;
    }
    match from {
        Candidate => matches!(to, AwaitingApproval | Planned | Rejected),
        AwaitingApproval => matches!(to, Planned | Rejected),
        Planned => matches!(to, Running | Cancelled | Rejected),
        Running => matches!(
            to,
            WaitingUser
                | Paused
                | PartiallyCompleted
                | Succeeded
                | Failed
                | Cancelled
                | RollingBack
        ),
        WaitingUser => matches!(to, Running | Cancelled | RollingBack),
        Paused => matches!(to, Running | Cancelled | RollingBack),
        PartiallyCompleted => {
            matches!(to, Running | Succeeded | Failed | Cancelled | RollingBack)
        }
        Succeeded => matches!(to, Archived),
        Failed => matches!(to, Archived | Planned | RollingBack),
        Cancelled => matches!(to, Archived | RollingBack),
        RollingBack => matches!(to, RolledBack | Failed),
        RolledBack => matches!(to, Archived | Planned),
        Rejected | Archived => false,
    }
}

/// Apply a Task transition command, returning an immutable outcome.
///
/// Does not persist. On success, `new_revision = current_revision + 1`.
pub fn apply_task_transition(
    cmd: &TaskTransitionCommand,
) -> Result<TaskTransitionOutcome, DomainTaskError> {
    validate_base_inputs(cmd)?;

    if let Some(expected) = cmd.expected_revision {
        if expected != cmd.current_revision {
            return Err(DomainTaskError::revision_conflict(
                expected,
                cmd.current_revision,
            ));
        }
    }

    if !is_task_transition_allowed(cmd.current_status, cmd.target_status) {
        return Err(DomainTaskError::illegal_task_transition(
            cmd.current_status,
            cmd.target_status,
        ));
    }

    enforce_task_invariants(cmd)?;

    let plan_version_incremented = requires_plan_version_increment(cmd);
    if cmd.replan && !plan_version_incremented {
        return Err(DomainTaskError::invariant(
            "replan=true is only legal for failed|rolled_back -> planned",
        ));
    }

    let new_plan_version = if plan_version_incremented {
        cmd.current_plan_version
            .checked_add(1)
            .ok_or_else(|| DomainTaskError::invalid_input("plan_version overflow"))?
    } else {
        // Non-replan transitions must not silently bump plan_version, including
        // candidate-era plan_version = 0 from task.create.
        cmd.current_plan_version
    };

    let new_revision = cmd
        .current_revision
        .checked_add(1)
        .ok_or_else(|| DomainTaskError::invalid_input("revision overflow"))?;

    Ok(TaskTransitionOutcome::with_event(
        cmd.current_status,
        cmd.target_status,
        new_revision,
        new_plan_version,
        plan_version_incremented,
        &cmd.reason,
    ))
}

fn validate_base_inputs(cmd: &TaskTransitionCommand) -> Result<(), DomainTaskError> {
    if cmd.current_revision < 1 {
        return Err(DomainTaskError::invalid_input(
            "current_revision must be >= 1",
        ));
    }
    // plan_version may be 0 (task.create fact). No lower-bound of 1.
    if cmd.reason.trim().is_empty() {
        return Err(DomainTaskError::invalid_input(
            "reason must be non-empty structured text",
        ));
    }
    Ok(())
}

fn requires_plan_version_increment(cmd: &TaskTransitionCommand) -> bool {
    matches!(
        (cmd.current_status, cmd.target_status),
        (TaskStatus::Failed, TaskStatus::Planned) | (TaskStatus::RolledBack, TaskStatus::Planned)
    )
}

fn enforce_task_invariants(cmd: &TaskTransitionCommand) -> Result<(), DomainTaskError> {
    use TaskStatus::*;

    // Terminal / archived constraints
    if matches!(cmd.current_status, Rejected | Archived) {
        return Err(DomainTaskError::illegal_task_transition(
            cmd.current_status,
            cmd.target_status,
        ));
    }
    if cmd.target_status == Archived
        && !matches!(
            cmd.current_status,
            Succeeded | Failed | Cancelled | RolledBack
        )
    {
        return Err(DomainTaskError::illegal_task_transition(
            cmd.current_status,
            cmd.target_status,
        ));
    }

    // succeeded: required_success_criteria and evidence must be exact multiset covers
    if cmd.target_status == Succeeded {
        enforce_succeeded_coverage(cmd)?;
    }

    // partially_completed: must list at least one produced side effect ref
    if cmd.target_status == PartiallyCompleted {
        validate_non_empty_side_effect_refs(
            &cmd.produced_side_effect_refs,
            "partially_completed must list at least one produced side_effect ref",
        )?;
    }

    // rolling_back: must prove at least one side effect requiring compensation.
    // Current status alone (including partially_completed) is not sufficient.
    if cmd.target_status == RollingBack {
        validate_non_empty_side_effect_refs(
            &cmd.rollback_required_side_effect_refs,
            "rolling_back requires at least one rollback_required_side_effect_ref proving external side effects need compensation",
        )?;
    }

    // replan flag must not be set on non-replan edges
    if cmd.replan
        && !matches!(
            (cmd.current_status, cmd.target_status),
            (Failed, Planned) | (RolledBack, Planned)
        )
    {
        return Err(DomainTaskError::invariant(
            "replan flag is only valid for failed|rolled_back -> planned; other transitions must not bump plan_version",
        ));
    }

    // failed|rolled_back -> planned always increments plan_version via graph edge.
    // rolling_back is external compensation, never SQLite rollback.
    // failed_recovery_meta is metadata, not a status node.

    Ok(())
}

fn enforce_succeeded_coverage(cmd: &TaskTransitionCommand) -> Result<(), DomainTaskError> {
    if cmd.required_success_criteria.is_empty() {
        return Err(DomainTaskError::missing_evidence(
            "succeeded requires non-empty required_success_criteria from TaskSpec.success_criteria",
        ));
    }

    let mut required_counts: BTreeMap<String, usize> = BTreeMap::new();
    for content in &cmd.required_success_criteria {
        let content = content.trim();
        if content.is_empty() {
            return Err(DomainTaskError::invalid_input(
                "required_success_criteria criterion content must be non-empty",
            ));
        }
        *required_counts.entry(content.to_string()).or_insert(0) += 1;
    }

    if cmd.success_criteria_evidence.is_empty() {
        return Err(DomainTaskError::missing_evidence(
            "succeeded requires success_criteria_evidence covering every required criterion content occurrence",
        ));
    }

    let mut evidence_counts: BTreeMap<String, usize> = BTreeMap::new();
    for item in &cmd.success_criteria_evidence {
        let content = item.criterion.trim();
        if content.is_empty() {
            return Err(DomainTaskError::invalid_input(
                "success criterion content must be non-empty",
            ));
        }
        if !item.satisfied {
            return Err(DomainTaskError::invariant(format!(
                "succeeded requires all success criteria satisfied; criterion content '{content}' is not"
            )));
        }
        let Some(verification) = item.verification.as_ref() else {
            return Err(DomainTaskError::missing_evidence(format!(
                "succeeded requires verification evidence for criterion content '{content}'"
            )));
        };
        if verification.outcome != VerificationResultOutcome::VerifiedOk {
            return Err(DomainTaskError::invariant(format!(
                "succeeded requires verification outcome verified_ok for criterion content '{content}', got {}",
                verification.outcome.as_str()
            )));
        }
        *evidence_counts.entry(content.to_string()).or_insert(0) += 1;
    }

    if required_counts != evidence_counts {
        let mut missing = Vec::new();
        let mut extra = Vec::new();
        for (content, req_n) in &required_counts {
            let ev_n = evidence_counts.get(content).copied().unwrap_or(0);
            if ev_n < *req_n {
                missing.push(format!("{content} (need {req_n}, have {ev_n})"));
            }
        }
        for (content, ev_n) in &evidence_counts {
            let req_n = required_counts.get(content).copied().unwrap_or(0);
            if *ev_n > req_n {
                extra.push(format!("{content} (need {req_n}, have {ev_n})"));
            }
        }
        return Err(DomainTaskError::invariant(format!(
            "succeeded requires exact multiset coverage of required_success_criteria criterion content; missing={missing:?} extra={extra:?}"
        )));
    }

    Ok(())
}

fn validate_non_empty_side_effect_refs(
    refs: &[SideEffectRef],
    missing_message: &str,
) -> Result<(), DomainTaskError> {
    if refs.is_empty() {
        return Err(DomainTaskError::missing_evidence(missing_message));
    }
    for side_effect in refs {
        if side_effect.ref_id.trim().is_empty() {
            return Err(DomainTaskError::invalid_input(
                "side_effect ref_id must be non-empty",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod graph_tests {
    use super::*;
    use crate::catalog::TASK_STATUS_CATALOG;

    #[test]
    fn legal_edges_match_core_section_10() {
        let legal = [
            (TaskStatus::Candidate, TaskStatus::AwaitingApproval),
            (TaskStatus::Candidate, TaskStatus::Planned),
            (TaskStatus::Candidate, TaskStatus::Rejected),
            (TaskStatus::AwaitingApproval, TaskStatus::Planned),
            (TaskStatus::AwaitingApproval, TaskStatus::Rejected),
            (TaskStatus::Planned, TaskStatus::Running),
            (TaskStatus::Planned, TaskStatus::Cancelled),
            (TaskStatus::Planned, TaskStatus::Rejected),
            (TaskStatus::Running, TaskStatus::WaitingUser),
            (TaskStatus::Running, TaskStatus::Paused),
            (TaskStatus::Running, TaskStatus::PartiallyCompleted),
            (TaskStatus::Running, TaskStatus::Succeeded),
            (TaskStatus::Running, TaskStatus::Failed),
            (TaskStatus::Running, TaskStatus::Cancelled),
            (TaskStatus::Running, TaskStatus::RollingBack),
            (TaskStatus::WaitingUser, TaskStatus::Running),
            (TaskStatus::WaitingUser, TaskStatus::Cancelled),
            (TaskStatus::WaitingUser, TaskStatus::RollingBack),
            (TaskStatus::Paused, TaskStatus::Running),
            (TaskStatus::Paused, TaskStatus::Cancelled),
            (TaskStatus::Paused, TaskStatus::RollingBack),
            (TaskStatus::PartiallyCompleted, TaskStatus::Running),
            (TaskStatus::PartiallyCompleted, TaskStatus::Succeeded),
            (TaskStatus::PartiallyCompleted, TaskStatus::Failed),
            (TaskStatus::PartiallyCompleted, TaskStatus::Cancelled),
            (TaskStatus::PartiallyCompleted, TaskStatus::RollingBack),
            (TaskStatus::Succeeded, TaskStatus::Archived),
            (TaskStatus::Failed, TaskStatus::Archived),
            (TaskStatus::Failed, TaskStatus::Planned),
            (TaskStatus::Failed, TaskStatus::RollingBack),
            (TaskStatus::Cancelled, TaskStatus::Archived),
            (TaskStatus::Cancelled, TaskStatus::RollingBack),
            (TaskStatus::RollingBack, TaskStatus::RolledBack),
            (TaskStatus::RollingBack, TaskStatus::Failed),
            (TaskStatus::RolledBack, TaskStatus::Archived),
            (TaskStatus::RolledBack, TaskStatus::Planned),
        ];
        for (from, to) in legal {
            assert!(
                is_task_transition_allowed(from, to),
                "expected legal: {} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }

    #[test]
    fn nxn_illegal_count_is_stable() {
        let mut legal = 0usize;
        let mut illegal = 0usize;
        for &from in TASK_STATUS_CATALOG {
            for &to in TASK_STATUS_CATALOG {
                if is_task_transition_allowed(from, to) {
                    legal += 1;
                } else {
                    illegal += 1;
                }
            }
        }
        // 14x14 = 196; 36 legal edges from CORE §10.2; self-loops are illegal
        assert_eq!(legal, 36);
        assert_eq!(illegal, 196 - 36);
    }

    #[test]
    fn self_loops_are_not_graph_edges() {
        for &status in TASK_STATUS_CATALOG {
            assert!(!is_task_transition_allowed(status, status));
        }
    }
}
