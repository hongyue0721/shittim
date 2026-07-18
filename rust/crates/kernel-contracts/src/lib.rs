//! Kernel contract types, JSON Schema validation, and RFC 8785 canonical hashing.
//!
//! Generated Rust types live in [`generated`]. They are produced by `schema-tool`
//! from `schemas/source` and must not be hand-edited.
//!
//! Generated string enums expose `pub const ALL: &'static [Self]` in Schema
//! declaration order (shared with variants/`as_str`). Nullable enums filter
//! `null` from `ALL`; JSON null remains `Option::None` at use sites.

pub mod canonical;
pub mod error;
pub mod generated;
pub mod validator;

pub use canonical::{canonical_json_bytes, canonical_json_string, sha256_canonical, sha256_hex};
pub use error::{
    ClassifiedContractFailure, ContractError, ContractFailureClassification, ContractFailureStage,
    DecodeStage,
};
pub use generated::*;
pub use validator::{decode_validated, validate_json, SchemaCatalog};
