use kernel_contracts::ContractError;
use thiserror::Error;

/// Pure authorization projection failure.
#[derive(Debug, Error)]
pub enum AuthorizationProjectionError {
    /// A caller-injected authoritative fact is malformed or not canonical.
    #[error("invalid authoritative fact at {field}: {reason}")]
    InvalidFact {
        /// Stable field path without rejected sensitive content.
        field: &'static str,
        /// Stable reason.
        reason: &'static str,
    },
    /// Constructed projection failed its source Schema or typed decode.
    #[error("authorization projection contract failed: {0}")]
    Contract(#[source] ContractError),
    /// Trusted typed data could not be serialized.
    #[error("authorization projection serialization failed: {0}")]
    Json(#[source] serde_json::Error),
}

impl AuthorizationProjectionError {
    pub(crate) fn invalid(field: &'static str, reason: &'static str) -> Self {
        Self::InvalidFact { field, reason }
    }
}
