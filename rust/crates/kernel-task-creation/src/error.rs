use kernel_contracts::ContractError;
use serde::Serialize;
use serde_json::{json, Value};
use thiserror::Error;

/// Caller-visible normalization input category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizationInputKind {
    /// `origin.source_uri`.
    OriginSourceUri,
    /// One `task_scope.resource_patterns` item.
    ResourcePattern,
    /// One `task_scope.exclusions` item.
    Exclusion,
}

/// Allocation field purpose, without exposing the rejected value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationPurpose {
    /// Root allocation.
    RootTaskCreate,
    /// Child materialization allocation.
    ChildTaskMaterialization,
}

/// Cross-field allocation conflict category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationConflictKind {
    /// Two internal UUID slots contain the same UUID.
    DuplicateInternalUuid,
    /// An internal UUID collides with a caller-injected external fact.
    ExternalUuidCollision,
    /// Two opaque fields contain the same value.
    DuplicateOpaque,
}

/// Stable public error mapping for a task-creation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCreationPublicError {
    /// Error Catalog code.
    pub code: &'static str,
    /// Safe structured details.
    pub details: Option<Value>,
}

/// Pure task-creation failure.
#[derive(Debug, Error)]
pub enum TaskCreationError {
    /// Raw caller JSON failed its selected Schema or typed decode.
    #[error("raw task-creation contract rejected caller input: {0}")]
    RawContract(#[source] ContractError),
    /// Origin source URI normalization failed.
    #[error("invalid origin source URI")]
    InvalidOriginSourceUri,
    /// Resource pattern normalization failed.
    #[error("invalid resource pattern at index {index}")]
    InvalidResourcePattern {
        /// Zero-based array index.
        index: usize,
    },
    /// Exclusion normalization failed.
    #[error("invalid exclusion at index {index}")]
    InvalidExclusion {
        /// Zero-based array index.
        index: usize,
    },
    /// A normalized object, projection, or canonicalization step violated an internal contract.
    #[error("post-normalization internal contract failure: {0}")]
    InternalContract(#[source] ContractError),
    /// Trusted typed data could not be represented as JSON.
    #[error("internal JSON serialization failure: {0}")]
    InternalJson(#[source] serde_json::Error),
    /// A typed allocation failed its selected Schema before repository entry.
    #[error("invalid allocation contract for {purpose:?}: {source}")]
    InvalidAllocationContract {
        /// Allocation path.
        purpose: AllocationPurpose,
        /// Schema validation failure.
        #[source]
        source: ContractError,
    },
    /// An allocation or external snapshot UUID was not parseable.
    #[error("invalid UUID in {purpose:?} allocation input at {field}")]
    InvalidUuid {
        /// Allocation path.
        purpose: AllocationPurpose,
        /// Safe field name.
        field: &'static str,
    },
    /// Allocation cross-field semantics failed.
    #[error("allocation conflict for {purpose:?}: {kind:?}")]
    AllocationConflict {
        /// Allocation path.
        purpose: AllocationPurpose,
        /// Conflict category.
        kind: AllocationConflictKind,
    },
}

impl TaskCreationError {
    /// Maps caller-controlled normalization failures to the frozen Error Catalog semantics.
    /// Internal and allocation failures intentionally have no caller-visible mapping here.
    pub fn public_error(&self) -> Option<TaskCreationPublicError> {
        match self {
            Self::RawContract(_) => Some(TaskCreationPublicError {
                code: "invalid_request",
                details: None,
            }),
            Self::InvalidOriginSourceUri => {
                Some(scope_error(NormalizationInputKind::OriginSourceUri, None))
            }
            Self::InvalidResourcePattern { index } => Some(scope_error(
                NormalizationInputKind::ResourcePattern,
                Some(*index),
            )),
            Self::InvalidExclusion { index } => {
                Some(scope_error(NormalizationInputKind::Exclusion, Some(*index)))
            }
            Self::InternalContract(_)
            | Self::InternalJson(_)
            | Self::InvalidAllocationContract { .. }
            | Self::InvalidUuid { .. }
            | Self::AllocationConflict { .. } => None,
        }
    }
}

fn scope_error(kind: NormalizationInputKind, index: Option<usize>) -> TaskCreationPublicError {
    TaskCreationPublicError {
        code: "invalid_scope_pattern",
        details: Some(json!({"input_kind": kind, "index": index})),
    }
}
