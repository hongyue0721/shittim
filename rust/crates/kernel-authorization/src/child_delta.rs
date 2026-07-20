use crate::canonical::{finalize_projection, CanonicalProjection};
use crate::AuthorizationProjectionError;
use kernel_contracts::{
    ChildTaskDeltaProjectionV1, ChildTaskDeltaProjectionV1AuthorityStatus,
    ChildTaskDeltaProjectionV1DelegationChange, ChildTaskDeltaProjectionV1SchemaVersion,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const SCHEMA_ID: &str = "https://schemas.shittim.local/task/child_task_delta_projection/v1";

/// Caller-injected parent/child scope and delegation facts.
///
/// Official fixture raw inputs deserialize into this type. Fields mirror the production
/// typed API; JSON is only a test-artifact encoding of authoritative facts, not a Schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildTaskDeltaFactsV1 {
    /// Current parent Task UUID.
    pub parent_task_id: Uuid,
    /// Current parent Task revision.
    pub parent_task_revision: u64,
    /// Current parent TaskScope UUID.
    pub parent_task_scope_ref: Uuid,
    /// Stored, already-normalized parent resource patterns, preserving order and duplicates.
    pub parent_resource_patterns: Vec<String>,
    /// Stored, already-normalized parent exclusions, preserving order and duplicates.
    pub parent_exclusions: Vec<String>,
    /// Normalized proposal resource patterns, preserving order and duplicates.
    pub child_resource_patterns: Vec<String>,
    /// Normalized proposal exclusions, preserving order and duplicates.
    pub child_exclusions: Vec<String>,
    /// Parent TaskScope capability hints; this API derives the set projection.
    pub parent_allowed_capability_hints: Vec<String>,
    /// Child TaskScope capability hints; this API derives the set projection.
    pub child_allowed_capability_hints: Vec<String>,
    /// Current parent Delegation UUID, if any.
    pub parent_delegation_ref: Option<Uuid>,
    /// Proposed child Delegation UUID, if any.
    pub child_delegation_ref: Option<Uuid>,
    /// Verified authority facts for a non-null child Delegation.
    pub child_delegation_authority: Option<VerifiedDelegationAuthorityV1>,
}

/// Caller-injected verified Delegation authority facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedDelegationAuthorityV1 {
    /// Stable authority reference.
    authority_ref: String,
    /// Current positive Delegation revision.
    revision: u64,
    /// Lowercase SHA-256 of the authority canonical scope projection.
    scope_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedDelegationAuthorityWire {
    authority_ref: String,
    revision: u64,
    scope_hash: String,
}

impl<'de> Deserialize<'de> for VerifiedDelegationAuthorityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Funnel through `new` so fixture JSON cannot bypass the documented verification contract.
        let wire = VerifiedDelegationAuthorityWire::deserialize(deserializer)?;
        Ok(Self::new(
            wire.authority_ref,
            wire.revision,
            wire.scope_hash,
        ))
    }
}

impl Serialize for VerifiedDelegationAuthorityV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("VerifiedDelegationAuthorityV1", 3)?;
        state.serialize_field("authority_ref", &self.authority_ref)?;
        state.serialize_field("revision", &self.revision)?;
        state.serialize_field("scope_hash", &self.scope_hash)?;
        state.end()
    }
}

impl VerifiedDelegationAuthorityV1 {
    /// Constructs a verified Delegation authority snapshot.
    ///
    /// # 合同
    ///
    /// 调用方必须已经按 IC §5.3.1 验证该 Delegation authority 为 current/active/applicable。
    /// 本 crate 是纯计算 owner，不读 repository，无法自行验证；构造点即唯一责任入口，
    /// 禁止未经验证的调用方以此伪造“已验证”事实。字段密封是为了让误用必须经过这个
    /// 有明文档前置条件的构造点，而不是四处字面量拼造。
    pub fn new(authority_ref: String, revision: u64, scope_hash: String) -> Self {
        Self {
            authority_ref,
            revision,
            scope_hash,
        }
    }

    /// Stable authority reference.
    pub fn authority_ref(&self) -> &str {
        &self.authority_ref
    }

    /// Current positive Delegation revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Lowercase SHA-256 of the authority canonical scope projection.
    pub fn scope_hash(&self) -> &str {
        &self.scope_hash
    }
}

struct DelegationProjection {
    change: ChildTaskDeltaProjectionV1DelegationChange,
    authority_ref: Option<String>,
    revision: Option<i64>,
    scope_hash: Option<String>,
    status: ChildTaskDeltaProjectionV1AuthorityStatus,
}

/// Constructs, validates, and hashes `ChildTaskDeltaProjectionV1`.
pub fn project_child_task_delta(
    facts: ChildTaskDeltaFactsV1,
) -> Result<CanonicalProjection<ChildTaskDeltaProjectionV1>, AuthorizationProjectionError> {
    require_positive(facts.parent_task_revision, "parent_task_revision")?;
    validate_patterns(&facts.parent_resource_patterns, "parent_resource_patterns")?;
    validate_patterns(&facts.parent_exclusions, "parent_exclusions")?;
    validate_patterns(&facts.child_resource_patterns, "child_resource_patterns")?;
    validate_patterns(&facts.child_exclusions, "child_exclusions")?;
    validate_nonempty_strings(
        &facts.parent_allowed_capability_hints,
        "parent_allowed_capability_hints",
    )?;
    validate_nonempty_strings(
        &facts.child_allowed_capability_hints,
        "child_allowed_capability_hints",
    )?;

    let delegation = delegation_projection(&facts)?;
    let parent_capabilities = set_projection(facts.parent_allowed_capability_hints);
    let child_capabilities = set_projection(facts.child_allowed_capability_hints);

    let value = ChildTaskDeltaProjectionV1 {
        added_capabilities: set_difference(&child_capabilities, &parent_capabilities),
        added_exclusions: multiset_difference(&facts.child_exclusions, &facts.parent_exclusions),
        added_resource_patterns: multiset_difference(
            &facts.child_resource_patterns,
            &facts.parent_resource_patterns,
        ),
        authority_status: delegation.status,
        child_allowed_capability_hints: child_capabilities.clone(),
        child_delegation_ref: facts.child_delegation_ref.map(uuid_text),
        child_exclusions: facts.child_exclusions.clone(),
        child_resource_patterns: facts.child_resource_patterns.clone(),
        delegation_authority_ref: delegation.authority_ref,
        delegation_change: delegation.change,
        delegation_revision: delegation.revision,
        delegation_scope_hash: delegation.scope_hash,
        parent_allowed_capability_hints: parent_capabilities.clone(),
        parent_delegation_ref: facts.parent_delegation_ref.map(uuid_text),
        parent_exclusions: facts.parent_exclusions.clone(),
        parent_resource_patterns: facts.parent_resource_patterns.clone(),
        parent_task_id: uuid_text(facts.parent_task_id),
        parent_task_revision: to_i64(facts.parent_task_revision, "parent_task_revision")?,
        parent_task_scope_ref: uuid_text(facts.parent_task_scope_ref),
        removed_capabilities: set_difference(&parent_capabilities, &child_capabilities),
        removed_exclusions: multiset_difference(&facts.parent_exclusions, &facts.child_exclusions),
        removed_resource_patterns: multiset_difference(
            &facts.parent_resource_patterns,
            &facts.child_resource_patterns,
        ),
        schema_version: ChildTaskDeltaProjectionV1SchemaVersion,
    };
    finalize_projection(SCHEMA_ID, value)
}

fn delegation_projection(
    facts: &ChildTaskDeltaFactsV1,
) -> Result<DelegationProjection, AuthorizationProjectionError> {
    let change = match (facts.parent_delegation_ref, facts.child_delegation_ref) {
        (None, None) => ChildTaskDeltaProjectionV1DelegationChange::Unchanged,
        (None, Some(_)) => ChildTaskDeltaProjectionV1DelegationChange::Added,
        (Some(_), None) => ChildTaskDeltaProjectionV1DelegationChange::Removed,
        (Some(parent), Some(child)) if parent == child => {
            ChildTaskDeltaProjectionV1DelegationChange::Unchanged
        }
        (Some(_), Some(_)) => ChildTaskDeltaProjectionV1DelegationChange::Replaced,
    };
    match facts.child_delegation_ref {
        None => {
            if facts.child_delegation_authority.is_some() {
                return Err(AuthorizationProjectionError::invalid(
                    "child_delegation_authority",
                    "must be absent when child_delegation_ref is null",
                ));
            }
            Ok(DelegationProjection {
                change,
                authority_ref: None,
                revision: None,
                scope_hash: None,
                status: ChildTaskDeltaProjectionV1AuthorityStatus::NotApplicable,
            })
        }
        Some(_) => {
            let authority = facts.child_delegation_authority.as_ref().ok_or_else(|| {
                AuthorizationProjectionError::invalid(
                    "child_delegation_authority",
                    "is required when child_delegation_ref is non-null",
                )
            })?;
            require_nonempty(&authority.authority_ref, "delegation_authority_ref")?;
            require_positive(authority.revision, "delegation_revision")?;
            require_hash(&authority.scope_hash, "delegation_scope_hash")?;
            Ok(DelegationProjection {
                change,
                authority_ref: Some(authority.authority_ref.clone()),
                revision: Some(to_i64(authority.revision, "delegation_revision")?),
                scope_hash: Some(authority.scope_hash.clone()),
                status: ChildTaskDeltaProjectionV1AuthorityStatus::Verified,
            })
        }
    }
}

fn multiset_difference(left: &[String], right: &[String]) -> Vec<String> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for value in right {
        *counts.entry(value.as_str()).or_default() += 1;
    }
    let mut output = Vec::new();
    for value in left {
        match counts.get_mut(value.as_str()) {
            Some(count) if *count > 0 => *count -= 1,
            _ => output.push(value.clone()),
        }
    }
    output.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    output
}

fn set_projection(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn set_difference(left: &[String], right: &[String]) -> Vec<String> {
    let right = right.iter().collect::<BTreeSet<_>>();
    left.iter()
        .filter(|value| !right.contains(value))
        .cloned()
        .collect()
}

fn validate_patterns(
    values: &[String],
    field: &'static str,
) -> Result<(), AuthorizationProjectionError> {
    for value in values {
        require_nonempty(value, field)?;
        let normalized = domain_policy::normalize_uri_pattern(value)
            .map_err(|_| AuthorizationProjectionError::invalid(field, "invalid URI pattern"))?;
        if normalized != *value {
            return Err(AuthorizationProjectionError::invalid(
                field,
                "URI pattern is not canonical",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_nonempty_strings(
    values: &[String],
    field: &'static str,
) -> Result<(), AuthorizationProjectionError> {
    for value in values {
        require_nonempty(value, field)?;
    }
    Ok(())
}

pub(crate) fn require_nonempty(
    value: &str,
    field: &'static str,
) -> Result<(), AuthorizationProjectionError> {
    if value.is_empty() {
        Err(AuthorizationProjectionError::invalid(
            field,
            "must be non-empty",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn require_positive(
    value: u64,
    field: &'static str,
) -> Result<(), AuthorizationProjectionError> {
    if value == 0 {
        Err(AuthorizationProjectionError::invalid(
            field,
            "must be positive",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn require_hash(
    value: &str,
    field: &'static str,
) -> Result<(), AuthorizationProjectionError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(AuthorizationProjectionError::invalid(
            field,
            "must be lowercase 64-hex",
        ))
    }
}

pub(crate) fn uuid_text(value: Uuid) -> String {
    value.hyphenated().to_string()
}

pub(crate) fn to_i64(value: u64, field: &'static str) -> Result<i64, AuthorizationProjectionError> {
    i64::try_from(value)
        .map_err(|_| AuthorizationProjectionError::invalid(field, "exceeds signed 64-bit range"))
}
