//! Pure construction, validation, RFC 8785 canonicalization, and SHA-256 hashing for
//! authorization projections.
//!
//! This crate accepts caller-injected typed authoritative facts. It does not read SQLite or any
//! repository, allocate IDs, write storage, or replace the `domain-policy` matcher.

#![deny(missing_docs)]

mod canonical;
mod child_delta;
mod error;
mod material;
mod observation;
mod subject;

pub use canonical::CanonicalProjection;
pub use child_delta::{
    project_child_task_delta, ChildTaskDeltaFactsV1, VerifiedDelegationAuthorityV1,
};
pub use error::AuthorizationProjectionError;
pub use material::{
    project_material_authorization, DestinationFactsV1, MaterialAuthorizationFactsV1,
    ProtectedSurfaceLabelFactsV1,
};
pub use observation::{
    project_observation_evidence, ObservationEvidenceFactsV1, ObservedEvidenceFactsV1,
};
pub use subject::{project_subject_projection, SubjectProjectionFactsV1};
