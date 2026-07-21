use crate::canonical::{finalize_projection, CanonicalProjection};
use crate::child_delta::{require_hash, require_nonempty, require_positive, to_i64, uuid_text};
use crate::AuthorizationProjectionError;
use kernel_contracts::{
    canonical_json_bytes, sha256_hex, Actor, EntryPoint, MaterialAuthorizationProjectionV1,
    MaterialAuthorizationProjectionV1Destination,
    MaterialAuthorizationProjectionV1ProtectedSurfaceLabelsItem,
    MaterialAuthorizationProjectionV1SchemaVersion, SideEffectClass,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

const SCHEMA_ID: &str = "https://schemas.shittim.local/policy/material_authorization_projection/v1";

/// Caller-injected material authorization facts.
///
/// Official fixture raw inputs deserialize into this type. JSON is only a test-artifact
/// encoding of authoritative facts; there is no business Schema for Facts themselves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialAuthorizationFactsV1 {
    /// Complete Actor revision snapshot.
    pub actor: Actor,
    /// Invocation entry point.
    pub entry_point: EntryPoint,
    /// Current Task UUID.
    pub task_id: Uuid,
    /// Current Task revision.
    pub task_revision: u64,
    /// Current Task plan version.
    pub task_plan_version: u64,
    /// Current Action UUID.
    pub action_id: Uuid,
    /// Current Action revision.
    pub action_revision: u64,
    /// Capability ID.
    pub capability_id: String,
    /// Operation name.
    pub operation: String,
    /// Side-effect class.
    pub side_effect_class: SideEffectClass,
    /// Complete normalized operation material parameter object.
    pub normalized_key_params: serde_json::Map<String, Value>,
    /// Current TaskScope UUID.
    pub task_scope_ref: Uuid,
    /// Concrete resource URI facts; this API normalizes, sorts, and deduplicates them.
    pub resource_refs: Vec<String>,
    /// Child delta fingerprint when this material concerns a child proposal.
    pub child_task_delta_hash: Option<String>,
    /// Selected Delegation UUID, if any.
    pub delegation_ref: Option<Uuid>,
    /// Stable authority reference for a non-null Delegation.
    pub delegation_authority_ref: Option<String>,
    /// Current Delegation revision for a non-null Delegation.
    pub delegation_revision: Option<u64>,
    /// Authoritative Policy set revision snapshot (0 = bootstrap empty PolicySet).
    pub policy_set_revision: u64,
    /// Target semantic kind.
    pub target_kind: String,
    /// Stable target reference, if available.
    pub target_stable_ref: Option<String>,
    /// Destination semantics, if present.
    pub destination: Option<DestinationFactsV1>,
    /// Material protected-surface labels; this API sorts and deduplicates exact tuples.
    pub protected_surface_labels: Vec<ProtectedSurfaceLabelFactsV1>,
    /// ContentOrigin UUIDs; this API emits lowercase sorted unique text.
    pub content_origin_refs: Vec<Uuid>,
    /// Child proposal fingerprint, if applicable.
    pub task_proposal_hash: Option<String>,
    /// Proposed plan version, if applicable.
    pub proposed_plan_version: Option<u64>,
    /// Proposed plan hash, if applicable.
    pub proposed_plan_hash: Option<String>,
}

/// Caller-injected destination semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationFactsV1 {
    /// Destination kind.
    pub kind: String,
    /// Stable destination reference.
    pub stable_ref: String,
    /// Stable account reference, if any.
    pub account_ref: Option<String>,
    /// Stable channel reference, if any.
    pub channel_ref: Option<String>,
}

/// Caller-injected material protected-surface label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedSurfaceLabelFactsV1 {
    /// Label.
    pub label: String,
    /// Material classification.
    pub classification: String,
    /// Stable source reference.
    pub source_ref: String,
}

/// Shared re-projection helper: PolicySet revision used in material preimage must equal the
/// value stored on PermissionDecision / PolicySet metadata (including bootstrap `0`).
///
/// Callers that re-verify a PD must pass `pd.policy_set_revision` through this helper rather
/// than inventing a parallel rewrite (e.g. empty-set `0 → 1`).
pub fn material_policy_set_revision_for_projection(
    policy_set_revision: i64,
) -> Result<u64, AuthorizationProjectionError> {
    if policy_set_revision < 0 {
        return Err(AuthorizationProjectionError::invalid(
            "policy_set_revision",
            "must be >= 0",
        ));
    }
    Ok(policy_set_revision as u64)
}

/// Constructs, validates, and hashes `MaterialAuthorizationProjectionV1`.
pub fn project_material_authorization(
    facts: MaterialAuthorizationFactsV1,
) -> Result<CanonicalProjection<MaterialAuthorizationProjectionV1>, AuthorizationProjectionError> {
    require_positive(facts.task_revision, "task_revision")?;
    require_positive(facts.action_revision, "action_revision")?;
    // policy_set_revision is authoritative PolicySet metadata: 0 is the bootstrap empty set
    // (IC §6.6 / §6.7 / migration 0007). It must match PermissionDecision.policy_set_revision
    // for re-projection; do not rewrite 0→1 in callers.
    require_nonempty(&facts.capability_id, "capability_id")?;
    require_nonempty(&facts.operation, "operation")?;
    require_nonempty(&facts.target_kind, "target_kind")?;
    if let Some(value) = &facts.target_stable_ref {
        require_nonempty(value, "target_stable_ref")?;
    }
    validate_optional_hash(&facts.child_task_delta_hash, "child_task_delta_hash")?;
    validate_optional_hash(&facts.task_proposal_hash, "task_proposal_hash")?;
    validate_optional_hash(&facts.proposed_plan_hash, "proposed_plan_hash")?;
    validate_delegation(&facts)?;
    validate_plan_pair(&facts)?;

    let normalized_key_params = Value::Object(facts.normalized_key_params.clone());
    let key_params_hash = sha256_hex(
        &canonical_json_bytes(&normalized_key_params)
            .map_err(AuthorizationProjectionError::Contract)?,
    );
    let resource_refs = normalize_resource_set(&facts.resource_refs)?;
    let resource_refs_hash = sha256_hex(
        &canonical_json_bytes(
            &serde_json::to_value(&resource_refs).map_err(AuthorizationProjectionError::Json)?,
        )
        .map_err(AuthorizationProjectionError::Contract)?,
    );
    let protected_surface_labels = project_labels(facts.protected_surface_labels)?;
    let content_origin_refs = facts
        .content_origin_refs
        .into_iter()
        .map(uuid_text)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let value = MaterialAuthorizationProjectionV1 {
        action_id: uuid_text(facts.action_id),
        action_revision: to_i64(facts.action_revision, "action_revision")?,
        actor: facts.actor,
        capability_id: facts.capability_id,
        child_task_delta_hash: facts.child_task_delta_hash,
        content_origin_refs,
        delegation_authority_ref: facts.delegation_authority_ref,
        delegation_ref: facts.delegation_ref.map(uuid_text),
        delegation_revision: facts
            .delegation_revision
            .map(|value| to_i64(value, "delegation_revision"))
            .transpose()?,
        destination: facts.destination.map(project_destination).transpose()?,
        entry_point: facts.entry_point,
        key_params_hash,
        normalized_key_params,
        operation: facts.operation,
        policy_set_revision: to_i64(facts.policy_set_revision, "policy_set_revision")?,
        proposed_plan_hash: facts.proposed_plan_hash,
        proposed_plan_version: facts
            .proposed_plan_version
            .map(|value| to_i64(value, "proposed_plan_version"))
            .transpose()?,
        protected_surface_labels,
        resource_refs,
        resource_refs_hash,
        schema_version: MaterialAuthorizationProjectionV1SchemaVersion,
        side_effect_class: facts.side_effect_class,
        target_kind: facts.target_kind,
        target_stable_ref: facts.target_stable_ref,
        task_id: uuid_text(facts.task_id),
        task_plan_version: to_i64(facts.task_plan_version, "task_plan_version")?,
        task_proposal_hash: facts.task_proposal_hash,
        task_revision: to_i64(facts.task_revision, "task_revision")?,
        task_scope_ref: uuid_text(facts.task_scope_ref),
    };
    finalize_projection(SCHEMA_ID, value)
}

fn normalize_resource_set(values: &[String]) -> Result<Vec<String>, AuthorizationProjectionError> {
    values
        .iter()
        .map(|value| {
            domain_policy::normalize_uri(value).map_err(|_| {
                AuthorizationProjectionError::invalid("resource_refs", "invalid resource URI")
            })
        })
        .collect::<Result<BTreeSet<_>, _>>()
        .map(|set| set.into_iter().collect())
}

fn validate_delegation(
    facts: &MaterialAuthorizationFactsV1,
) -> Result<(), AuthorizationProjectionError> {
    match facts.delegation_ref {
        None if facts.delegation_authority_ref.is_none() && facts.delegation_revision.is_none() => {
            Ok(())
        }
        Some(_) => {
            let authority = facts.delegation_authority_ref.as_ref().ok_or_else(|| {
                AuthorizationProjectionError::invalid(
                    "delegation_authority_ref",
                    "required when delegation_ref is non-null",
                )
            })?;
            require_nonempty(authority, "delegation_authority_ref")?;
            require_positive(
                facts.delegation_revision.ok_or_else(|| {
                    AuthorizationProjectionError::invalid(
                        "delegation_revision",
                        "required when delegation_ref is non-null",
                    )
                })?,
                "delegation_revision",
            )
        }
        None => Err(AuthorizationProjectionError::invalid(
            "delegation_ref",
            "authority and revision must be null when delegation_ref is null",
        )),
    }
}

fn validate_plan_pair(
    facts: &MaterialAuthorizationFactsV1,
) -> Result<(), AuthorizationProjectionError> {
    if facts.proposed_plan_version.is_some() == facts.proposed_plan_hash.is_some() {
        Ok(())
    } else {
        Err(AuthorizationProjectionError::invalid(
            "proposed_plan_version",
            "proposed plan version and hash must be jointly null or non-null",
        ))
    }
}

fn validate_optional_hash(
    value: &Option<String>,
    field: &'static str,
) -> Result<(), AuthorizationProjectionError> {
    match value {
        Some(value) => require_hash(value, field),
        None => Ok(()),
    }
}

fn project_destination(
    value: DestinationFactsV1,
) -> Result<MaterialAuthorizationProjectionV1Destination, AuthorizationProjectionError> {
    require_nonempty(&value.kind, "destination.kind")?;
    require_nonempty(&value.stable_ref, "destination.stable_ref")?;
    if let Some(account) = &value.account_ref {
        require_nonempty(account, "destination.account_ref")?;
    }
    if let Some(channel) = &value.channel_ref {
        require_nonempty(channel, "destination.channel_ref")?;
    }
    Ok(MaterialAuthorizationProjectionV1Destination {
        account_ref: value.account_ref,
        channel_ref: value.channel_ref,
        kind: value.kind,
        stable_ref: value.stable_ref,
    })
}

fn project_labels(
    values: Vec<ProtectedSurfaceLabelFactsV1>,
) -> Result<
    Vec<MaterialAuthorizationProjectionV1ProtectedSurfaceLabelsItem>,
    AuthorizationProjectionError,
> {
    for value in &values {
        require_nonempty(&value.label, "protected_surface_labels.label")?;
        require_nonempty(
            &value.classification,
            "protected_surface_labels.classification",
        )?;
        require_nonempty(&value.source_ref, "protected_surface_labels.source_ref")?;
    }
    Ok(values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(
            |value| MaterialAuthorizationProjectionV1ProtectedSurfaceLabelsItem {
                classification: value.classification,
                label: value.label,
                source_ref: value.source_ref,
            },
        )
        .collect())
}
