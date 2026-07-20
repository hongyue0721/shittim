//! Assertion helpers shared by authorization projection integration tests.

use kernel_authorization::{AuthorizationProjectionError, CanonicalProjection};
use kernel_contracts::{canonical_json_bytes, sha256_hex};
use serde::Serialize;
use std::fmt::Debug;

/// Recomputes JCS + SHA-256 from the typed value and asserts they match the projection.
pub fn assert_canonical_projection<T>(projection: &CanonicalProjection<T>)
where
    T: Serialize + Debug,
{
    let value_json = serde_json::to_value(&projection.value).expect("serialize projection value");
    let independent_jcs =
        canonical_json_bytes(&value_json).expect("independent canonical_json_bytes");
    let independent_sha = sha256_hex(&independent_jcs);

    assert_eq!(
        projection.jcs_utf8, independent_jcs,
        "projection jcs_utf8 must match independent JCS of value"
    );
    assert_eq!(
        projection.sha256, independent_sha,
        "projection sha256 must match independent SHA-256 of JCS"
    );
    assert_eq!(
        projection.sha256.len(),
        64,
        "sha256 must be lowercase 64-hex"
    );
    assert!(
        projection
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "sha256 must be lowercase hex"
    );
    assert!(
        !projection.jcs_utf8.starts_with(&[0xef, 0xbb, 0xbf]),
        "JCS bytes must not have BOM"
    );
    assert_ne!(
        projection.jcs_utf8.last(),
        Some(&b'\n'),
        "JCS bytes must not have trailing newline"
    );
    assert_eq!(
        projection.jcs_utf8.last(),
        Some(&b'}'),
        "projection JCS should end with object close"
    );
}

/// Asserts the projection's sha256 equals an anchored snapshot.
pub fn assert_projection_sha256<T>(projection: &CanonicalProjection<T>, expected: &str) {
    assert_eq!(
        projection.sha256, expected,
        "anchored preimage sha256 snapshot mismatch"
    );
}

/// Asserts `InvalidFact` for a specific field path.
pub fn assert_invalid_fact<T: Debug>(
    result: Result<T, AuthorizationProjectionError>,
    expected_field: &'static str,
) {
    match result.expect_err("expected InvalidFact") {
        AuthorizationProjectionError::InvalidFact { field, reason } => {
            assert_eq!(field, expected_field, "unexpected InvalidFact field");
            assert!(!reason.is_empty(), "InvalidFact reason must be non-empty");
        }
        error => panic!("expected InvalidFact at {expected_field}, got {error:?}"),
    }
}

/// Asserts `InvalidFact` for a specific field path and reason.
pub fn assert_invalid_fact_reason<T: Debug>(
    result: Result<T, AuthorizationProjectionError>,
    expected_field: &'static str,
    expected_reason: &'static str,
) {
    match result.expect_err("expected InvalidFact") {
        AuthorizationProjectionError::InvalidFact { field, reason } => {
            assert_eq!(field, expected_field, "unexpected InvalidFact field");
            assert_eq!(reason, expected_reason, "unexpected InvalidFact reason");
        }
        error => panic!(
            "expected InvalidFact {{ field: {expected_field}, reason: {expected_reason} }}, got {error:?}"
        ),
    }
}

/// Documents the stable error taxonomy without conflating caller-invalid and internal variants.
pub fn assert_error_variant_shape(error: &AuthorizationProjectionError) {
    match error {
        AuthorizationProjectionError::InvalidFact { field, reason } => {
            assert!(!field.is_empty());
            assert!(!reason.is_empty());
        }
        AuthorizationProjectionError::Contract(_) | AuthorizationProjectionError::Json(_) => {
            // Internal / contract-layer failures stay distinct from InvalidFact.
        }
    }
}
