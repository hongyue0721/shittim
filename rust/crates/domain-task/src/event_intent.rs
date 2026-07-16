//! Event *intents* produced by pure domain transitions.
//!
//! These are not real `EventEnvelope` records: no `event_id`, no
//! `outbox_position`, no sequence assignment. Persistence layers materialize
//! envelopes later.

use kernel_contracts::{ActionStatus, TaskStatus};
use serde::{Deserialize, Serialize};

/// Aggregate-level event intent kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventIntent {
    /// Task aggregate state change intent.
    Task(TaskEventIntent),
    /// Action aggregate state change intent.
    Action(ActionEventIntent),
}

/// Intent that a Task changed status (to be recorded as task.state_changed later).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEventIntent {
    /// Previous status.
    pub from_status: TaskStatus,
    /// New status.
    pub to_status: TaskStatus,
    /// Revision after the transition.
    pub revision: u64,
    /// Plan version after the transition.
    pub plan_version: u64,
    /// Structured reason provided by the command.
    pub reason: String,
}

/// Intent that an Action changed status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionEventIntent {
    /// Previous status.
    pub from_status: ActionStatus,
    /// New status.
    pub to_status: ActionStatus,
    /// Revision after the transition.
    pub revision: u64,
    /// Structured reason provided by the command.
    pub reason: String,
}
