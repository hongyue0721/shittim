//! Stable fixture builders. Values are intentionally fixed so preimage snapshots stay anchored.

use kernel_authorization::{
    ChildTaskDeltaFactsV1, DestinationFactsV1, MaterialAuthorizationFactsV1,
    ObservationEvidenceFactsV1, ObservedEvidenceFactsV1, ProtectedSurfaceLabelFactsV1,
    VerifiedDelegationAuthorityV1,
};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, EntryPoint, SideEffectClass,
};
use serde_json::{json, Map};
use uuid::Uuid;

/// Anchored SHA-256 of the baseline child-delta fixture projection.
pub const FIXED_DELTA_SHA256: &str =
    "7b9dd8a89c45a39ecd8be5c83182bd8b7de134fba5ddec70bdc39ccf191515f1";
/// Anchored SHA-256 of the baseline material authorization fixture projection.
pub const FIXED_MATERIAL_SHA256: &str =
    "3e4287d10fc0aadc9a41722b41f524117835b3b67b814cd757a268833338da1d";
/// Anchored SHA-256 of the baseline observed evidence fixture projection.
pub const FIXED_OBSERVATION_SHA256: &str =
    "27a53f3df37c24ffa9b991143253a9dc17dd5a64b7ed542c88e28a22a6275df4";
/// Anchored SHA-256 of `ObservationEvidenceFactsV1::NotApplicable`.
pub const FIXED_OBSERVATION_NA_SHA256: &str =
    "6a92f747124fb00970a909708946dc170bb03bfee1bbbf4bf7f152289b2666c0";

/// Deterministic UUID for fixture fields (`00000000-0000-4000-8000-…`).
pub fn uuid(n: u128) -> Uuid {
    Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0000u128 + n)
}

/// Lowercase 64-hex filled with a single character (valid SHA-256 shape).
pub fn hash(character: char) -> String {
    std::iter::repeat_n(character, 64).collect()
}

/// Shared Actor snapshot used by material fixtures.
pub fn actor() -> Actor {
    Actor {
        authentication_level: ActorAuthenticationLevel::PlatformVerified,
        confidence: Some(0.9),
        id: "actor-local-1".into(),
        kind: ActorKind::KnownUser,
        revision: 2,
        schema_version: ActorSchemaVersion,
        source: "local-desktop".into(),
    }
}

/// Baseline child-delta facts (no Delegation).
pub fn delta_facts() -> ChildTaskDeltaFactsV1 {
    ChildTaskDeltaFactsV1 {
        parent_task_id: uuid(1),
        parent_task_revision: 2,
        parent_task_scope_ref: uuid(2),
        parent_resource_patterns: vec![
            "https://example.com/a/**".into(),
            "https://example.com/a/**".into(),
            "https://example.com/c/**".into(),
        ],
        parent_exclusions: vec!["https://example.com/a/tmp/**".into()],
        child_resource_patterns: vec![
            "https://example.com/a/**".into(),
            "https://example.com/b/**".into(),
        ],
        child_exclusions: Vec::new(),
        parent_allowed_capability_hints: vec!["read".into(), "read".into()],
        child_allowed_capability_hints: vec!["write".into(), "read".into()],
        parent_delegation_ref: None,
        child_delegation_ref: None,
        child_delegation_authority: None,
    }
}

/// Baseline child-delta facts with verified child Delegation authority.
pub fn delta_facts_with_verified_delegation() -> ChildTaskDeltaFactsV1 {
    let mut facts = delta_facts();
    facts.parent_delegation_ref = Some(uuid(8));
    facts.child_delegation_ref = Some(uuid(9));
    facts.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        "delegation-authority://1".into(),
        3,
        hash('a'),
    ));
    facts
}

/// Baseline material authorization facts.
pub fn material_facts() -> MaterialAuthorizationFactsV1 {
    let mut key_params = Map::new();
    // Insertion order deliberately non-alphabetical; JCS must sort keys.
    key_params.insert("z".into(), json!(2));
    key_params.insert("a".into(), json!(1));
    MaterialAuthorizationFactsV1 {
        actor: actor(),
        entry_point: EntryPoint::LocalDesktop,
        task_id: uuid(1),
        task_revision: 2,
        task_plan_version: 1,
        action_id: uuid(3),
        action_revision: 4,
        capability_id: "computer.input".into(),
        operation: "click".into(),
        side_effect_class: SideEffectClass::S2,
        normalized_key_params: key_params,
        task_scope_ref: uuid(2),
        resource_refs: vec![
            "HTTPS://Example.COM:443/b".into(),
            "https://example.com/a".into(),
            "https://example.com/a".into(),
        ],
        child_task_delta_hash: None,
        delegation_ref: None,
        delegation_authority_ref: None,
        delegation_revision: None,
        policy_set_revision: 5,
        target_kind: "semantic_element".into(),
        target_stable_ref: Some("element://button/7".into()),
        destination: Some(DestinationFactsV1 {
            kind: "channel".into(),
            stable_ref: "channel://personal/1".into(),
            account_ref: Some("account://owner".into()),
            channel_ref: Some("channel://personal/1".into()),
        }),
        protected_surface_labels: vec![
            ProtectedSurfaceLabelFactsV1 {
                label: "authentication".into(),
                classification: "sensitive".into(),
                source_ref: "surface://2".into(),
            },
            ProtectedSurfaceLabelFactsV1 {
                label: "authentication".into(),
                classification: "sensitive".into(),
                source_ref: "surface://2".into(),
            },
            ProtectedSurfaceLabelFactsV1 {
                label: "payment".into(),
                classification: "critical".into(),
                source_ref: "surface://1".into(),
            },
        ],
        content_origin_refs: vec![uuid(8), uuid(7), uuid(8)],
        task_proposal_hash: None,
        proposed_plan_version: None,
        proposed_plan_hash: None,
    }
}

/// Baseline observed-branch observation facts.
pub fn observed_facts() -> ObservedEvidenceFactsV1 {
    ObservedEvidenceFactsV1 {
        provider_ref: "provider://desktop/1".into(),
        provider_revision: 2,
        snapshot_ref: Some("snapshot://desktop/4".into()),
        snapshot_generation: Some(4),
        target_observation_ref: Some("observation://target/1".into()),
        coordinate_transform_hash: Some(hash('e')),
        observed_at: "2026-07-20T10:00:00+02:00".into(),
        valid_until: "2026-07-20T08:01:00Z".into(),
        evidence_refs: vec![
            "evidence://2".into(),
            "evidence://1".into(),
            "evidence://1".into(),
        ],
        protected_surface_observations: vec![json!({"label": "first"}), json!({"label": "first"})],
        destination_observation_ref: None,
    }
}

/// Baseline observed observation facts enum wrapper.
pub fn observation_facts() -> ObservationEvidenceFactsV1 {
    ObservationEvidenceFactsV1::Observed(Box::new(observed_facts()))
}
