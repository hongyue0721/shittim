use crate::canonical::{finalize_projection, CanonicalProjection};
use crate::child_delta::{require_hash, require_nonempty, require_positive, to_i64};
use crate::AuthorizationProjectionError;
use kernel_contracts::{
    ObservationEvidenceProjectionV1, ObservationEvidenceProjectionV1NotApplicableSchemaVersion,
    ObservationEvidenceProjectionV1ObservedSchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

const SCHEMA_ID: &str = "https://schemas.shittim.local/policy/observation_evidence_projection/v1";
const RESERVED_PROVIDER_REFS: &[&str] = &["core", "none", "system"];

/// Caller-injected instantaneous observation facts.
///
/// Official fixture raw inputs deserialize into this tagged union. JSON is only a
/// test-artifact encoding; the production API remains a typed enum.
///
/// Custom (de)serialization is required because serde's internally-tagged unit
/// variant does not honor `deny_unknown_fields` for `not_applicable`.
#[derive(Debug, Clone, PartialEq)]
pub enum ObservationEvidenceFactsV1 {
    /// The authorization does not depend on a Profile observation.
    NotApplicable,
    /// A real Provider produced current observation evidence.
    Observed(Box<ObservedEvidenceFactsV1>),
}

impl Serialize for ObservationEvidenceFactsV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::NotApplicable => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("observation_kind", "not_applicable")?;
                map.end()
            }
            Self::Observed(facts) => {
                // Flatten observed fields with the discriminator for fixture JSON shape.
                let mut map = serde_json::Map::new();
                map.insert("observation_kind".into(), Value::String("observed".into()));
                let body =
                    serde_json::to_value(facts.as_ref()).map_err(serde::ser::Error::custom)?;
                let Value::Object(fields) = body else {
                    return Err(serde::ser::Error::custom(
                        "observed facts must serialize as object",
                    ));
                };
                for (key, value) in fields {
                    map.insert(key, value);
                }
                map.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for ObservationEvidenceFactsV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("observation facts must be a JSON object"))?;
        let kind = object
            .get("observation_kind")
            .and_then(Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("missing observation_kind"))?;
        match kind {
            "not_applicable" => {
                if object.len() != 1 {
                    return Err(serde::de::Error::custom(
                        "not_applicable forbids fields other than observation_kind",
                    ));
                }
                Ok(Self::NotApplicable)
            }
            "observed" => {
                let mut body = object.clone();
                body.remove("observation_kind");
                let facts: ObservedEvidenceFactsV1 = serde_json::from_value(Value::Object(body))
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::Observed(Box::new(facts)))
            }
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["not_applicable", "observed"],
            )),
        }
    }
}

/// Caller-injected observed branch facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEvidenceFactsV1 {
    /// Real negotiated Provider stable reference.
    pub provider_ref: String,
    /// Current positive Provider revision.
    pub provider_revision: u64,
    /// Stable Snapshot reference, jointly optional with `snapshot_generation`.
    pub snapshot_ref: Option<String>,
    /// Snapshot generation, jointly optional with `snapshot_ref`.
    pub snapshot_generation: Option<u64>,
    /// Stable target observation reference, if any.
    pub target_observation_ref: Option<String>,
    /// Coordinate-transform fingerprint, if any.
    pub coordinate_transform_hash: Option<String>,
    /// RFC3339 observation time.
    pub observed_at: String,
    /// RFC3339 expiry strictly later than `observed_at`.
    pub valid_until: String,
    /// Evidence stable refs; this API sorts and deduplicates them.
    pub evidence_refs: Vec<String>,
    /// Profile-defined protected-surface observation facts; order and duplicates are preserved.
    pub protected_surface_observations: Vec<Value>,
    /// Stable destination observation reference, if any.
    pub destination_observation_ref: Option<String>,
}

/// Constructs, validates, and hashes `ObservationEvidenceProjectionV1`.
pub fn project_observation_evidence(
    facts: ObservationEvidenceFactsV1,
) -> Result<CanonicalProjection<ObservationEvidenceProjectionV1>, AuthorizationProjectionError> {
    let value = match facts {
        ObservationEvidenceFactsV1::NotApplicable => {
            ObservationEvidenceProjectionV1::NotApplicable {
                schema_version: ObservationEvidenceProjectionV1NotApplicableSchemaVersion,
            }
        }
        ObservationEvidenceFactsV1::Observed(facts) => project_observed(*facts)?,
    };
    finalize_projection(SCHEMA_ID, value)
}

fn project_observed(
    facts: ObservedEvidenceFactsV1,
) -> Result<ObservationEvidenceProjectionV1, AuthorizationProjectionError> {
    require_nonempty(&facts.provider_ref, "provider_ref")?;
    if RESERVED_PROVIDER_REFS.contains(&facts.provider_ref.as_str()) {
        return Err(AuthorizationProjectionError::invalid(
            "provider_ref",
            "reserved pseudo-provider is forbidden",
        ));
    }
    require_positive(facts.provider_revision, "provider_revision")?;
    validate_snapshot_pair(&facts)?;
    validate_optional_nonempty(&facts.target_observation_ref, "target_observation_ref")?;
    validate_optional_nonempty(
        &facts.destination_observation_ref,
        "destination_observation_ref",
    )?;
    if let Some(value) = &facts.coordinate_transform_hash {
        require_hash(value, "coordinate_transform_hash")?;
    }
    let observed_at = kernel_contracts::canonicalize_rfc3339_seconds(&facts.observed_at)
        .map_err(|_| AuthorizationProjectionError::invalid("observed_at", "invalid timestamp"))?;
    let valid_until = kernel_contracts::canonicalize_rfc3339_seconds(&facts.valid_until)
        .map_err(|_| AuthorizationProjectionError::invalid("valid_until", "invalid timestamp"))?;
    let observed_instant = chrono::DateTime::parse_from_rfc3339(&observed_at)
        .map_err(|_| AuthorizationProjectionError::invalid("observed_at", "invalid timestamp"))?;
    let valid_instant = chrono::DateTime::parse_from_rfc3339(&valid_until)
        .map_err(|_| AuthorizationProjectionError::invalid("valid_until", "invalid timestamp"))?;
    if valid_instant <= observed_instant {
        return Err(AuthorizationProjectionError::invalid(
            "valid_until",
            "must be later than observed_at",
        ));
    }
    for evidence in &facts.evidence_refs {
        require_nonempty(evidence, "evidence_refs")?;
    }
    let evidence_refs = facts
        .evidence_refs
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(ObservationEvidenceProjectionV1::Observed {
        coordinate_transform_hash: facts.coordinate_transform_hash,
        destination_observation_ref: facts.destination_observation_ref,
        evidence_refs,
        observed_at,
        protected_surface_observations: facts.protected_surface_observations,
        provider_ref: facts.provider_ref,
        provider_revision: to_i64(facts.provider_revision, "provider_revision")?,
        schema_version: ObservationEvidenceProjectionV1ObservedSchemaVersion,
        snapshot_generation: facts
            .snapshot_generation
            .map(|value| to_i64(value, "snapshot_generation"))
            .transpose()?,
        snapshot_ref: facts.snapshot_ref,
        target_observation_ref: facts.target_observation_ref,
        valid_until,
    })
}

fn validate_snapshot_pair(
    facts: &ObservedEvidenceFactsV1,
) -> Result<(), AuthorizationProjectionError> {
    match (&facts.snapshot_ref, facts.snapshot_generation) {
        (None, None) => Ok(()),
        (Some(reference), Some(_)) => require_nonempty(reference, "snapshot_ref"),
        _ => Err(AuthorizationProjectionError::invalid(
            "snapshot_ref",
            "snapshot_ref and snapshot_generation must be jointly null or non-null",
        )),
    }
}

fn validate_optional_nonempty(
    value: &Option<String>,
    field: &'static str,
) -> Result<(), AuthorizationProjectionError> {
    match value {
        Some(value) => require_nonempty(value, field),
        None => Ok(()),
    }
}
