//! RFC 8785 JSON Canonicalization Scheme (JCS).

use crate::error::ContractError;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, ContractError> {
    serde_json_canonicalizer::to_vec(value)
        .map_err(|error| ContractError::Canonicalize(error.to_string()))
}

pub fn canonical_json_string(value: &Value) -> Result<String, ContractError> {
    serde_json_canonicalizer::to_string(value)
        .map_err(|error| ContractError::Canonicalize(error.to_string()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn sha256_canonical(value: &Value) -> Result<String, ContractError> {
    Ok(sha256_hex(&canonical_json_bytes(value)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Number};

    #[test]
    fn official_rfc8785_serialization_example() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../../schemas/examples/jcs/rfc8785-example.input.json"
        ))
        .expect("fixture JSON");
        let expected =
            include_str!("../../../../schemas/examples/jcs/rfc8785-example.canonical.txt")
                .trim_end();
        assert_eq!(canonical_json_string(&value).expect("canonical"), expected);
    }

    #[test]
    fn utf16_property_sorting_uses_code_units() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../../schemas/examples/jcs/rfc8785-utf16-sort.input.json"
        ))
        .expect("fixture JSON");
        let expected =
            include_str!("../../../../schemas/examples/jcs/rfc8785-utf16-sort.canonical.txt")
                .trim_end();
        assert_eq!(canonical_json_string(&value).expect("canonical"), expected);
    }

    #[test]
    fn negative_zero_serializes_as_zero() {
        let value = Value::Number(Number::from_f64(-0.0).expect("finite"));
        assert_eq!(canonical_json_string(&value).expect("canonical"), "0");
    }

    #[test]
    fn exponent_boundaries_follow_ecmascript_number_serialization() {
        assert_eq!(
            canonical_json_string(&json!(1e20)).expect("canonical"),
            "100000000000000000000"
        );
        assert_eq!(
            canonical_json_string(&json!(1e21)).expect("canonical"),
            "1e+21"
        );
        assert_eq!(
            canonical_json_string(&json!(1e-6)).expect("canonical"),
            "0.000001"
        );
        assert_eq!(
            canonical_json_string(&json!(1e-7)).expect("canonical"),
            "1e-7"
        );
    }

    #[test]
    fn sha256_known_empty_object() {
        assert_eq!(
            sha256_canonical(&json!({})).expect("hash"),
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
    }
}
