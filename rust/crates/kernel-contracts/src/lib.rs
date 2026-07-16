//! Kernel contract types, JSON Schema validation, and RFC 8785 canonical hashing.
//!
//! Generated Rust types live in [`generated`]. They are produced by `schema-tool`
//! from `schemas/source` and must not be hand-edited.

pub mod canonical;
pub mod error;
pub mod generated;
pub mod validator;

pub use canonical::{canonical_json_bytes, canonical_json_string, sha256_canonical, sha256_hex};
pub use error::ContractError;
pub use generated::*;
pub use validator::{validate_json, SchemaCatalog};
