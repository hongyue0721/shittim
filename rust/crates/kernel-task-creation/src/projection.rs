use crate::TaskCreationError;
use kernel_contracts::{canonical_json_bytes, sha256_hex};
use serde::Serialize;

/// A typed projection together with its exact RFC 8785 UTF-8 bytes and SHA-256 digest.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalProjection<T> {
    /// Typed contract value.
    pub value: T,
    /// Exact JCS UTF-8 preimage bytes.
    pub jcs_utf8: Vec<u8>,
    /// Lowercase hexadecimal SHA-256 digest of `jcs_utf8`.
    pub sha256: String,
}

pub(crate) fn projection_from_typed<T: Serialize>(
    value: T,
) -> Result<CanonicalProjection<T>, TaskCreationError> {
    let typed_json = serde_json::to_value(&value).map_err(TaskCreationError::InternalJson)?;
    let jcs_utf8 =
        canonical_json_bytes(&typed_json).map_err(TaskCreationError::InternalContract)?;
    let sha256 = sha256_hex(&jcs_utf8);
    Ok(CanonicalProjection {
        value,
        jcs_utf8,
        sha256,
    })
}
