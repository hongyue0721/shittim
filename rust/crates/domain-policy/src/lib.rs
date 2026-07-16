//! Pure Freedom-first PolicyRule matcher for the Shittim Kernel.
//!
//! The crate consumes generated `kernel-contracts` policy/actor/origin enums and structs. It does
//! not persist decisions, allocate UUIDs/timestamps/revisions, connect to SQLite/Tokio/KCP/UI, or
//! own Stop Fence/recovery facts. Kernel invariants are explicit input and bypass ordinary rules.

#![deny(missing_docs)]

mod error;
mod matcher;
mod rate_limit;
mod types;
mod uri;

pub use error::{PolicyError, PolicyErrorCode};
pub use matcher::{evaluate_policy, Specificity};
pub use rate_limit::{
    RateLimitConsume, RateLimitKey, RateLimitPort, RateLimitPreview, RateLimitRequest,
    RejectRateLimits,
};
pub use types::{
    parse_policy_rule_json, CanonicalEvaluationInput, DelegationCoverageEvidence,
    KernelInvariantBlock, KernelInvariantState, LocalPresenceEvidence, PermissionBindingDraft,
    PermissionDecisionDraft, PolicyEvaluationContext, PolicyEvaluationResult,
};
pub use uri::{normalize_uri, normalize_uri_pattern};
