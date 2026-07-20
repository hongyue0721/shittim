use crate::AuthorizationProjectionError;
use kernel_contracts::{canonical_json_bytes, decode_validated, sha256_hex};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// A Schema-validated typed projection with its exact RFC 8785 bytes and SHA-256 digest.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalProjection<T> {
    /// Typed projection value.
    pub value: T,
    /// Exact JCS UTF-8 preimage, without BOM or trailing newline.
    pub jcs_utf8: Vec<u8>,
    /// Lowercase SHA-256 digest of `jcs_utf8`.
    pub sha256: String,
}

pub(crate) fn finalize_projection<T>(
    schema_id: &str,
    value: T,
) -> Result<CanonicalProjection<T>, AuthorizationProjectionError>
where
    T: DeserializeOwned + Serialize,
{
    let json = serde_json::to_value(&value).map_err(AuthorizationProjectionError::Json)?;
    let decoded: T =
        decode_validated(schema_id, &json).map_err(AuthorizationProjectionError::Contract)?;
    let decoded_json =
        serde_json::to_value(&decoded).map_err(AuthorizationProjectionError::Json)?;
    if decoded_json != json {
        return Err(AuthorizationProjectionError::Contract(
            kernel_contracts::ContractError::InvalidJson(format!(
                "typed roundtrip changed projection for {schema_id}"
            )),
        ));
    }
    let jcs_utf8 = canonical_json_bytes(&json).map_err(AuthorizationProjectionError::Contract)?;
    let sha256 = sha256_hex(&jcs_utf8);
    Ok(CanonicalProjection {
        value: decoded,
        jcs_utf8,
        sha256,
    })
}
