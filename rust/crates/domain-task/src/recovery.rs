//! Recovery candidate legality helpers.
//!
//! Does **not** execute recovery. Only validates whether a candidate kind is
//! legal under the facts defined by CORE / IMPLEMENTATION_CONTRACTS.
//!
//! Boundary: only `retry_original` has hard domain fact checks here. Other
//! candidate kinds (`verify_external_state`, `compensate`, `continue_task`,
//! `stop_task`, `mark_failed`) are accepted at the enum layer only; selection
//! still requires Policy / Task Engine and is **not** authorization.

use kernel_contracts::RecoveryDecisionCandidateCandidateKind;
use serde::{Deserialize, Serialize};

use crate::error::DomainTaskError;

/// Facts required to judge `retry_original` candidate legality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryOriginalFacts {
    /// Whether the original side effect is confirmed.
    ///
    /// `retry_original` requires this to be **explicitly false** (side effect
    /// confirmed not to have occurred). `None` / `true` are illegal.
    pub side_effect_confirmed: Option<bool>,
    /// Whether the original Action has verifiable idempotency guarantees.
    pub original_idempotency_guaranteed: bool,
}

/// Validate that a recovery candidate kind is legal under the provided facts.
///
/// Currently only `retry_original` has hard domain legality constraints beyond
/// schema: `side_effect_confirmed == false` and
/// `original_idempotency_guaranteed == true`.
pub fn validate_retry_original_candidate(
    facts: &RetryOriginalFacts,
) -> Result<(), DomainTaskError> {
    match facts.side_effect_confirmed {
        Some(false) => {}
        Some(true) => {
            return Err(DomainTaskError::illegal_recovery(
                "retry_original is illegal when side_effect_confirmed=true (would re-apply side effect)",
            ));
        }
        None => {
            return Err(DomainTaskError::illegal_recovery(
                "retry_original is illegal when side_effect_confirmed is unknown/null; query external state first",
            ));
        }
    }
    if !facts.original_idempotency_guaranteed {
        return Err(DomainTaskError::illegal_recovery(
            "retry_original requires original_idempotency_guaranteed=true",
        ));
    }
    Ok(())
}

/// Validate candidate kind legality. Non-retry kinds are accepted at this layer
/// (execution still requires Policy / Task Engine).
pub fn validate_recovery_candidate_kind(
    kind: RecoveryDecisionCandidateCandidateKind,
    facts: &RetryOriginalFacts,
) -> Result<(), DomainTaskError> {
    match kind {
        RecoveryDecisionCandidateCandidateKind::RetryOriginal => {
            validate_retry_original_candidate(facts)
        }
        RecoveryDecisionCandidateCandidateKind::VerifyExternalState
        | RecoveryDecisionCandidateCandidateKind::Compensate
        | RecoveryDecisionCandidateCandidateKind::ContinueTask
        | RecoveryDecisionCandidateCandidateKind::StopTask
        | RecoveryDecisionCandidateCandidateKind::MarkFailed => Ok(()),
    }
}
