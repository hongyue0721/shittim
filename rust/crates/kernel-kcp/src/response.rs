//! Handler response and local contract-failure values.

use crate::ports::ResponseContractValidator;
use kernel_contracts::{
    KcpError, KcpErrorSchemaVersion, KcpResponseEnvelope, KcpResponseEnvelopeMessageKind,
    KcpResponseEnvelopeProtocolVersion, KcpResponseEnvelopeStatus,
};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SafeWireErrorKind {
    InvalidRequest,
    UnsupportedProtocolVersion,
    UnsupportedSchemaVersion,
    UnsupportedMethod,
    UnsupportedAuthSchema,
}

pub(crate) fn validated_error_response_with_validator(
    request_id: &str,
    kind: SafeWireErrorKind,
    validator: &impl ResponseContractValidator,
) -> Result<KcpResponseEnvelope, HandlerContractFailure> {
    let (code, message) = match kind {
        SafeWireErrorKind::InvalidRequest => ("invalid_request", "request is invalid"),
        SafeWireErrorKind::UnsupportedProtocolVersion => (
            "unsupported_protocol_version",
            "protocol version is not supported",
        ),
        SafeWireErrorKind::UnsupportedSchemaVersion => (
            "unsupported_schema_version",
            "payload schema version is not supported",
        ),
        SafeWireErrorKind::UnsupportedMethod => ("unsupported_method", "method is not supported"),
        SafeWireErrorKind::UnsupportedAuthSchema => (
            "unsupported_auth_schema",
            "authentication schema is not supported",
        ),
    };
    let response = KcpResponseEnvelope {
        error: Some(KcpError {
            code: code.to_owned(),
            details: None,
            message: message.to_owned(),
            retryable: false,
            schema_version: KcpErrorSchemaVersion,
        }),
        message_kind: KcpResponseEnvelopeMessageKind::Value,
        payload: None,
        protocol_version: KcpResponseEnvelopeProtocolVersion::Value,
        request_id: request_id.to_owned(),
        status: KcpResponseEnvelopeStatus::Error,
    };
    let valid = serde_json::to_value(&response)
        .ok()
        .filter(|value| validator.validate_response_envelope(value).is_ok())
        .is_some();
    if valid {
        Ok(response)
    } else {
        Err(HandlerContractFailure {
            kind: HandlerContractFailureKind::FinalResponseInvalid,
            message: "final response contract validation failed",
        })
    }
}
