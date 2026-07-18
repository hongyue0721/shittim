//! Compatibility/lifecycle policy shared by generic loading and the production profile.
//!
//! Entry compatibility is orthogonal to method-version bindings, but it is not
//! arbitrary: each schema kind has a closed set of meaningful lifecycle labels.

use crate::manifest::ManifestEntry;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Closed compatibility labels for manifest entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaCompatibility {
    V1Stable,
    NewContract,
    BreakingReplacement,
    LegacyValidationOnly,
    LegacyReadOnly,
}

impl SchemaCompatibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1Stable => "v1-stable",
            Self::NewContract => "new-contract",
            Self::BreakingReplacement => "breaking-replacement",
            Self::LegacyValidationOnly => "legacy-validation-only",
            Self::LegacyReadOnly => "legacy-read-only",
        }
    }

    pub fn is_legacy(self) -> bool {
        matches!(self, Self::LegacyValidationOnly | Self::LegacyReadOnly)
    }
}

impl std::fmt::Display for SchemaCompatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Generic kind/lifecycle combinations. Production exact IDs are checked by the
/// versioned ledger below, not by counts.
pub fn validate_kind_compatibility(entry: &ManifestEntry) -> Result<()> {
    use SchemaCompatibility::*;
    let allowed = match entry.kind.as_str() {
        "enum" | "object" | "domain_object" | "event_payload" => {
            matches!(
                entry.compatibility,
                V1Stable | NewContract | BreakingReplacement
            )
        }
        "envelope" => matches!(
            entry.compatibility,
            V1Stable | NewContract | BreakingReplacement | LegacyValidationOnly
        ),
        "kcp_request" => matches!(
            entry.compatibility,
            V1Stable | NewContract | BreakingReplacement | LegacyValidationOnly
        ),
        "kcp_response" => matches!(
            entry.compatibility,
            V1Stable | NewContract | BreakingReplacement | LegacyReadOnly
        ),
        _ => false,
    };
    if !allowed {
        bail!(
            "manifest entry {} kind {} is incompatible with lifecycle {}",
            entry.id,
            entry.kind,
            entry.compatibility
        );
    }
    Ok(())
}

pub const PRODUCTION_LIFECYCLE_LEDGER_VERSION: u32 = 1;

const PRODUCTION_LEGACY_VALIDATION_ONLY_IDS: &[&str] = &[
    "https://schemas.shittim.local/v1/kcp/command_envelope.json",
    "https://schemas.shittim.local/v1/kcp/query_envelope.json",
    "https://schemas.shittim.local/v1/kcp/task_create_request.json",
];
const PRODUCTION_LEGACY_READ_ONLY_IDS: &[&str] =
    &["https://schemas.shittim.local/v1/kcp/task_create_response.json"];

/// Validate exact production lifecycle labels for every retained v1 ID.
pub fn validate_production_lifecycle_ledger(entries: &[ManifestEntry]) -> Result<()> {
    let expected: BTreeMap<&str, SchemaCompatibility> = entries
        .iter()
        .filter(|entry| entry.id.starts_with("https://schemas.shittim.local/v1/"))
        .map(|entry| {
            let lifecycle = if PRODUCTION_LEGACY_VALIDATION_ONLY_IDS.contains(&entry.id.as_str()) {
                SchemaCompatibility::LegacyValidationOnly
            } else if PRODUCTION_LEGACY_READ_ONLY_IDS.contains(&entry.id.as_str()) {
                SchemaCompatibility::LegacyReadOnly
            } else {
                SchemaCompatibility::V1Stable
            };
            (entry.id.as_str(), lifecycle)
        })
        .collect();
    if expected.len() != 41 {
        bail!(
            "production lifecycle ledger v{PRODUCTION_LIFECYCLE_LEDGER_VERSION} requires exactly 41 retained v1 entries, got {}",
            expected.len()
        );
    }
    for entry in entries
        .iter()
        .filter(|entry| expected.contains_key(entry.id.as_str()))
    {
        let required = expected[entry.id.as_str()];
        if entry.compatibility != required {
            bail!(
                "production lifecycle ledger v{PRODUCTION_LIFECYCLE_LEDGER_VERSION} mismatch for {}: expected {}, got {}",
                entry.id,
                required,
                entry.compatibility
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_closed_set() {
        for value in [
            SchemaCompatibility::V1Stable,
            SchemaCompatibility::NewContract,
            SchemaCompatibility::BreakingReplacement,
            SchemaCompatibility::LegacyValidationOnly,
            SchemaCompatibility::LegacyReadOnly,
        ] {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(json, format!("\"{}\"", value.as_str()));
            let back: SchemaCompatibility = serde_json::from_str(&json).unwrap();
            assert_eq!(back, value);
        }
    }

    #[test]
    fn rejects_unknown_and_test_only_values() {
        for raw in ["test-only", "future-test-only", "internal", "active", ""] {
            let err = serde_json::from_str::<SchemaCompatibility>(&format!("\"{raw}\""));
            assert!(err.is_err(), "accepted {raw}");
        }
    }
}
