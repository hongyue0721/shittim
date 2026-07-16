//! Pure Task/Action domain state machines for Shittim Kernel.
//!
//! This crate owns transition legality, revision/plan_version arithmetic, and
//! domain invariants only. It does **not** persist facts, allocate real event
//! envelope positions, talk to SQLite, Policy matcher, KCP, UI, or Extensions.
//!
//! Status enums come exclusively from `kernel-contracts` generated types.

#![deny(missing_docs)]

mod action;
mod catalog;
mod error;
mod event_intent;
mod policy_outcome;
mod recovery;
mod task;
mod types;

pub use action::{
    apply_action_transition, evaluate_policy_on_pending, is_action_transition_allowed,
    validate_compensation_action_draft, ActionEvidence, ActionTransitionCommand,
    CompensationActionDraft, DispatchCertainty, LeaseReleaseEffect, UncertainOutcomeReason,
};
pub use catalog::{
    assert_action_catalog_exhaustive, assert_task_catalog_exhaustive, ACTION_STATUS_CATALOG,
    TASK_STATUS_CATALOG,
};
pub use error::{DomainTaskError, DomainTaskErrorCode};
pub use event_intent::{ActionEventIntent, EventIntent, TaskEventIntent};
pub use policy_outcome::{
    apply_policy_evaluation_outcome, PolicyEvaluationEffect, PolicyEvaluationOutcome,
};
pub use recovery::{
    validate_recovery_candidate_kind, validate_retry_original_candidate, RetryOriginalFacts,
};
pub use task::{
    apply_task_transition, is_task_transition_allowed, SuccessCriterionEvidence,
    TaskTransitionCommand,
};
pub use types::{
    ActionEffects, ActionTransitionOutcome, SideEffectRef, TaskTransitionOutcome,
    TransitionOutcome, VerificationEvidenceSummary,
};
