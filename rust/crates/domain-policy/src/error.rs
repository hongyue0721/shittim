use thiserror::Error;

/// Stable machine-readable policy evaluation error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyErrorCode {
    /// A URI value or URI pattern violates the policy grammar.
    InvalidUriPattern,
    /// A capability or operation pattern is not exact or a trailing `.*` prefix.
    InvalidActionPattern,
    /// A date-time fact cannot be parsed as RFC 3339.
    InvalidTimestamp,
    /// A rule's generated fields violate a runtime semantic invariant.
    InvalidRule,
    /// A supported Condition v1 field contains unsupported or invalid semantics.
    UnsupportedPolicyCondition,
    /// RFC 8785 canonicalization failed.
    CanonicalizationFailed,
    /// The injected rate-limit authority failed.
    RateLimitFailed,
}

impl PolicyErrorCode {
    /// Returns the KCP-compatible machine code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidUriPattern => "invalid_policy_uri_pattern",
            Self::InvalidActionPattern => "invalid_policy_action_pattern",
            Self::InvalidTimestamp => "invalid_policy_timestamp",
            Self::InvalidRule => "invalid_policy_rule",
            Self::UnsupportedPolicyCondition => "unsupported_policy_condition",
            Self::CanonicalizationFailed => "canonicalization_failed",
            Self::RateLimitFailed => "rate_limit_failed",
        }
    }
}

/// Structured, fail-closed policy evaluation error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}", code = .code.as_str())]
pub struct PolicyError {
    /// Stable machine code.
    pub code: PolicyErrorCode,
    /// Human-readable context without secrets.
    pub message: String,
}

impl PolicyError {
    pub(crate) fn new(code: PolicyErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
