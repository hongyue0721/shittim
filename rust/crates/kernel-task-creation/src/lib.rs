//! Pure task-creation contract logic.
//!
//! This crate owns root/child proposal normalization and canonical hashes plus allocation
//! validation. It does not allocate IDs, read repositories, open transactions, or depend on KCP
//! and SQLite implementations. External relationship UUID snapshots are strongly typed so callers
//! must parse wire text and explicitly construct every relationship slot.

#![deny(missing_docs)]

mod allocation;
mod error;
mod normalization;
mod projection;

pub use allocation::{
    validate_child_task_materialization_allocation, validate_root_task_create_allocation,
    ChildTaskMaterializationExternalUuidRefsV1, RootTaskCreateExternalUuidRefsV1,
};
pub use error::{
    AllocationConflictKind, AllocationPurpose, NormalizationInputKind, TaskCreationError,
    TaskCreationPublicError,
};
pub use normalization::{
    normalize_child_task_proposal, normalize_root_task_create, ChildTaskCreationProjection,
    RootTaskCreateProjection, RootTaskProjectionInput,
};
pub use projection::CanonicalProjection;
