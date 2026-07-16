//! Structured domain errors for Task/Action transitions.
//!
//! Machine-readable codes are stable; messages are human-readable and language
//! agnostic in intent (currently English for code comments/tests; Chinese
//! explanations may be layered by callers).

use std::fmt;

use kernel_contracts::{ActionStatus, TaskStatus};
use thiserror::Error;

/// Stable machine code for domain-task failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DomainTaskErrorCode {
    /// The requested status edge is not in CORE §10 / §11.
    IllegalTransition,
    /// Caller expected a different concurrent revision.
    ExpectedRevisionConflict,
    /// Transition violated a domain invariant (evidence, plan_version, etc.).
    InvariantViolation,
    /// Required evidence or reason was missing / malformed.
    MissingEvidence,
    /// Recovery candidate is not legal under defined facts.
    IllegalRecoveryCandidate,
    /// Compensation draft is not a valid new Action relative to the original.
    IllegalCompensationDraft,
    /// Input values were inconsistent (e.g. plan_version < 1).
    InvalidInput,
}

impl DomainTaskErrorCode {
    /// Stable snake_case machine code string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IllegalTransition => "illegal_transition",
            Self::ExpectedRevisionConflict => "expected_revision_conflict",
            Self::InvariantViolation => "invariant_violation",
            Self::MissingEvidence => "missing_evidence",
            Self::IllegalRecoveryCandidate => "illegal_recovery_candidate",
            Self::IllegalCompensationDraft => "illegal_compensation_draft",
            Self::InvalidInput => "invalid_input",
        }
    }
}

impl fmt::Display for DomainTaskErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Domain error with machine code and explanatory message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct DomainTaskError {
    /// Stable machine code.
    pub code: DomainTaskErrorCode,
    /// Human-readable explanation (not a panic message).
    pub message: String,
}

impl DomainTaskError {
    /// Construct a domain error.
    pub fn new(code: DomainTaskErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Machine code string.
    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub(crate) fn illegal_task_transition(from: TaskStatus, to: TaskStatus) -> Self {
        Self::new(
            DomainTaskErrorCode::IllegalTransition,
            format!(
                "illegal Task transition: {} -> {} (CORE_ARCHITECTURE §10.2)",
                from.as_str(),
                to.as_str()
            ),
        )
    }

    pub(crate) fn illegal_action_transition(from: ActionStatus, to: ActionStatus) -> Self {
        Self::new(
            DomainTaskErrorCode::IllegalTransition,
            format!(
                "illegal Action transition: {} -> {} (CORE_ARCHITECTURE §11.3)",
                from.as_str(),
                to.as_str()
            ),
        )
    }

    pub(crate) fn revision_conflict(expected: u64, actual: u64) -> Self {
        Self::new(
            DomainTaskErrorCode::ExpectedRevisionConflict,
            format!(
                "expected_revision conflict: caller expected {expected}, current revision is {actual}"
            ),
        )
    }

    pub(crate) fn invariant(message: impl Into<String>) -> Self {
        Self::new(DomainTaskErrorCode::InvariantViolation, message)
    }

    pub(crate) fn missing_evidence(message: impl Into<String>) -> Self {
        Self::new(DomainTaskErrorCode::MissingEvidence, message)
    }

    pub(crate) fn illegal_recovery(message: impl Into<String>) -> Self {
        Self::new(DomainTaskErrorCode::IllegalRecoveryCandidate, message)
    }

    pub(crate) fn illegal_compensation(message: impl Into<String>) -> Self {
        Self::new(DomainTaskErrorCode::IllegalCompensationDraft, message)
    }

    pub(crate) fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(DomainTaskErrorCode::InvalidInput, message)
    }
}
