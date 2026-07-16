//! Handler response and local contract-failure values.

use kernel_contracts::KcpResponseEnvelope;
use uuid::Uuid;

/// One validated KCP response and durable post-commit notification hints.
#[derive(Debug, Clone, PartialEq)]
pub struct HandledResponse {
    /// Final response envelope validated against the generic response Schema.
    pub response: KcpResponseEnvelope,
    /// Best-effort notification hints whose referenced Outbox facts are already durable.
    pub post_commit_notification_intents: Vec<PostCommitNotificationIntent>,
}

/// A local handler result; normal KCP errors remain response envelopes.
#[derive(Debug, Clone, PartialEq)]
pub enum HandlerResult {
    /// A validated success or error response.
    Response(HandledResponse),
    /// No response may be sent because the final response contract failed.
    ContractFailure {
        /// Stable local failure classification.
        failure: HandlerContractFailure,
        /// Durable post-commit hints that must not be lost.
        post_commit_notification_intents: Vec<PostCommitNotificationIntent>,
    },
}

/// Stable local contract-failure classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerContractFailureKind {
    /// The typed method discriminator or payload variant did not match the invoked handler.
    InputMethodMismatch,
    /// The final KCP response envelope failed its generated contract.
    FinalResponseInvalid,
}

/// Safe local failure without SQL, payload, path, secret, or internal identifier data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerContractFailure {
    /// Stable machine-comparable kind.
    pub kind: HandlerContractFailureKind,
    /// Stable safe summary.
    pub message: &'static str,
}

/// Best-effort post-commit notification hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostCommitNotificationIntent {
    /// A Task and its task.created Outbox Event have committed.
    TaskCreatedCommitted {
        /// Committed Task UUID text from the backend's current Task.
        task_id: String,
        /// Committed Event UUID.
        event_id: Uuid,
    },
}
