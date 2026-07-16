use crate::PolicyError;
use chrono::{DateTime, Utc};

/// Stable key passed to the authoritative rate-limit implementation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RateLimitKey(pub String);

/// Complete atomic rate-limit request for one candidate rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitRequest<'a> {
    /// Rule ID.
    pub rule_id: &'a str,
    /// Rule revision.
    pub rule_revision: i64,
    /// Stable key derived from the configured key scope.
    pub key: &'a RateLimitKey,
    /// Rolling window in seconds.
    pub window_seconds: i64,
    /// Maximum consumed decisions in the rolling window.
    pub count: i64,
    /// Evaluation instant supplied by the Kernel.
    pub instant: DateTime<Utc>,
}

/// Result of a non-consuming rate-limit preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitPreview {
    /// The rule may currently win and proceed to atomic consumption.
    Available,
    /// The rule is already exhausted and is not a candidate.
    Exceeded,
}

/// Result of the winner-only atomic consumption operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitConsume {
    /// One decision was atomically consumed.
    Consumed,
    /// A concurrent winner consumed the final slot; this rule must be removed and selection retried.
    Exceeded,
}

/// Port owned by the Kernel persistence layer, not by this pure domain crate.
///
/// Evaluation previews all otherwise-matching rate-limited rules without mutation. It then sorts
/// candidates, calls `check_and_consume` only for the current winner, and on a concurrent
/// `Exceeded` removes that rule and selects again. Losing candidates are never consumed.
pub trait RateLimitPort {
    /// Checks current availability without consuming a decision.
    fn preview(&self, request: &RateLimitRequest<'_>) -> Result<RateLimitPreview, PolicyError>;

    /// Atomically checks and consumes one decision for the selected winner.
    fn check_and_consume(
        &self,
        request: &RateLimitRequest<'_>,
    ) -> Result<RateLimitConsume, PolicyError>;
}

/// A port for evaluations that contain no rate-limit condition.
#[derive(Debug, Default, Clone, Copy)]
pub struct RejectRateLimits;

impl RateLimitPort for RejectRateLimits {
    fn preview(&self, _request: &RateLimitRequest<'_>) -> Result<RateLimitPreview, PolicyError> {
        Err(PolicyError::new(
            crate::PolicyErrorCode::RateLimitFailed,
            "rate_limit condition requires an authoritative RateLimitPort",
        ))
    }

    fn check_and_consume(
        &self,
        _request: &RateLimitRequest<'_>,
    ) -> Result<RateLimitConsume, PolicyError> {
        Err(PolicyError::new(
            crate::PolicyErrorCode::RateLimitFailed,
            "rate_limit condition requires an authoritative RateLimitPort",
        ))
    }
}
