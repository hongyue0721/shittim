//! Immutable transition outcomes and shared evidence types.

use kernel_contracts::{ActionStatus, TaskStatus, VerificationResultOutcome};
use serde::{Deserialize, Serialize};

use crate::action::LeaseReleaseEffect;
use crate::event_intent::{ActionEventIntent, EventIntent, TaskEventIntent};

/// Opaque reference to an already-produced side effect (resource, object, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SideEffectRef {
    /// Stable reference string (URI, object id, resource id, ...).
    pub ref_id: String,
}

impl SideEffectRef {
    /// Create a side-effect reference.
    pub fn new(ref_id: impl Into<String>) -> Self {
        Self {
            ref_id: ref_id.into(),
        }
    }
}

/// Minimal verification evidence summary required by domain invariants.
///
/// Callers may later attach full `VerificationResult` records; the pure domain
/// layer only needs the outcome and whether success criteria / side effects
/// were confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationEvidenceSummary {
    /// Verification outcome (must be `verified_ok` for Action completed).
    pub outcome: VerificationResultOutcome,
    /// Optional verification result id for audit linkage.
    pub verification_result_ref: Option<String>,
    /// Whether the external side effect was confirmed (`None` = unknown).
    pub side_effect_confirmed: Option<bool>,
}

/// Effects that persistence must apply atomically with the Action status write.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActionEffects {
    /// Lease + resource locks must be released in the same SQLite transaction.
    pub release_lease_and_locks: Option<LeaseReleaseEffect>,
    /// Whether automatic replay of the original Action is forbidden.
    pub forbid_automatic_replay: bool,
    /// Whether an ApprovalRecord association is required to remain pending.
    pub requires_approval_record_ref: bool,
}

/// Immutable Task transition result (no persistence).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTransitionOutcome {
    /// Status after the transition.
    pub new_status: TaskStatus,
    /// Revision after the transition (`current + 1`).
    pub new_revision: u64,
    /// Plan version after the transition (may be unchanged or +1 on replan).
    pub new_plan_version: u64,
    /// Whether this transition performed a replan (`plan_version` incremented).
    pub plan_version_incremented: bool,
    /// Event intents that persistence should materialize (no envelope positions).
    pub event_intents: Vec<EventIntent>,
}

/// Immutable Action transition result (no persistence).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionTransitionOutcome {
    /// Status after the transition.
    pub new_status: ActionStatus,
    /// Revision after the transition (`current + 1` on every successful domain update,
    /// including Policy confirm metadata updates that keep status pending).
    pub new_revision: u64,
    /// Whether status actually changed (confirm keeps pending).
    pub status_changed: bool,
    /// Atomic effects required alongside the status write.
    pub effects: ActionEffects,
    /// Event intents that persistence should materialize.
    pub event_intents: Vec<EventIntent>,
}

/// Shared outcome wrapper when callers want a single enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "aggregate", rename_all = "snake_case")]
pub enum TransitionOutcome {
    /// Task aggregate outcome.
    Task(TaskTransitionOutcome),
    /// Action aggregate outcome.
    Action(ActionTransitionOutcome),
}

impl TaskTransitionOutcome {
    pub(crate) fn with_event(
        from: TaskStatus,
        to: TaskStatus,
        new_revision: u64,
        new_plan_version: u64,
        plan_version_incremented: bool,
        reason: &str,
    ) -> Self {
        Self {
            new_status: to,
            new_revision,
            new_plan_version,
            plan_version_incremented,
            event_intents: vec![EventIntent::Task(TaskEventIntent {
                from_status: from,
                to_status: to,
                revision: new_revision,
                plan_version: new_plan_version,
                reason: reason.to_string(),
            })],
        }
    }
}

impl ActionTransitionOutcome {
    pub(crate) fn with_event(
        from: ActionStatus,
        to: ActionStatus,
        new_revision: u64,
        status_changed: bool,
        effects: ActionEffects,
        reason: &str,
    ) -> Self {
        let event_intents = if status_changed {
            vec![EventIntent::Action(ActionEventIntent {
                from_status: from,
                to_status: to,
                revision: new_revision,
                reason: reason.to_string(),
            })]
        } else {
            Vec::new()
        };
        Self {
            new_status: to,
            new_revision,
            status_changed,
            effects,
            event_intents,
        }
    }
}
