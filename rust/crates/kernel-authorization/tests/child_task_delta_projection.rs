//! Integration tests for `project_child_task_delta`.

mod support;

use kernel_authorization::{
    project_child_task_delta, AuthorizationProjectionError, ChildTaskDeltaFactsV1,
    VerifiedDelegationAuthorityV1,
};
use kernel_contracts::{
    ChildTaskDeltaProjectionV1AuthorityStatus, ChildTaskDeltaProjectionV1DelegationChange,
};
use support::assertions::{
    assert_canonical_projection, assert_error_variant_shape, assert_invalid_fact,
    assert_invalid_fact_reason, assert_projection_sha256,
};
use support::fixtures::{
    delta_facts, delta_facts_with_verified_delegation, hash, uuid, FIXED_DELTA_SHA256,
};

#[test]
fn positive_projects_canonical_preimage_and_anchored_sha256() {
    let projection = project_child_task_delta(delta_facts()).expect("baseline delta");
    assert_canonical_projection(&projection);
    assert_projection_sha256(&projection, FIXED_DELTA_SHA256);

    assert_eq!(
        projection.value.added_resource_patterns,
        vec!["https://example.com/b/**"]
    );
    assert_eq!(
        projection.value.removed_resource_patterns,
        vec!["https://example.com/a/**", "https://example.com/c/**"]
    );
    assert_eq!(projection.value.added_exclusions, Vec::<String>::new());
    assert_eq!(
        projection.value.removed_exclusions,
        vec!["https://example.com/a/tmp/**"]
    );
    assert_eq!(
        projection.value.parent_allowed_capability_hints,
        vec!["read"]
    );
    assert_eq!(
        projection.value.child_allowed_capability_hints,
        vec!["read", "write"]
    );
    assert_eq!(projection.value.added_capabilities, vec!["write"]);
    assert!(projection.value.removed_capabilities.is_empty());
    assert_eq!(
        projection.value.authority_status,
        ChildTaskDeltaProjectionV1AuthorityStatus::NotApplicable
    );
    assert_eq!(
        projection.value.delegation_change,
        ChildTaskDeltaProjectionV1DelegationChange::Unchanged
    );
    assert_eq!(
        projection.value.parent_task_id,
        "00000000-0000-4000-8000-000000000001"
    );
}

#[test]
fn positive_verified_delegation_sets_authority_status_and_change() {
    let projection =
        project_child_task_delta(delta_facts_with_verified_delegation()).expect("delegated delta");
    assert_canonical_projection(&projection);
    assert_eq!(
        projection.value.authority_status,
        ChildTaskDeltaProjectionV1AuthorityStatus::Verified
    );
    assert_eq!(
        projection.value.delegation_change,
        ChildTaskDeltaProjectionV1DelegationChange::Replaced
    );
    assert_eq!(
        projection.value.delegation_authority_ref.as_deref(),
        Some("delegation-authority://1")
    );
    assert_eq!(projection.value.delegation_revision, Some(3));
    assert_eq!(
        projection.value.delegation_scope_hash.as_deref(),
        Some(hash('a').as_str())
    );
    assert_eq!(
        projection.value.child_delegation_ref.as_deref(),
        Some("00000000-0000-4000-8000-000000000009")
    );
}

#[test]
fn positive_delegation_change_matrix() {
    // Unchanged / both null covered by baseline.
    let mut added = delta_facts();
    added.child_delegation_ref = Some(uuid(9));
    added.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        "auth://added".into(),
        1,
        hash('b'),
    ));
    let added = project_child_task_delta(added).expect("added");
    assert_eq!(
        added.value.delegation_change,
        ChildTaskDeltaProjectionV1DelegationChange::Added
    );

    let mut removed = delta_facts();
    removed.parent_delegation_ref = Some(uuid(8));
    let removed = project_child_task_delta(removed).expect("removed");
    assert_eq!(
        removed.value.delegation_change,
        ChildTaskDeltaProjectionV1DelegationChange::Removed
    );
    assert_eq!(
        removed.value.authority_status,
        ChildTaskDeltaProjectionV1AuthorityStatus::NotApplicable
    );

    let mut unchanged_same = delta_facts();
    unchanged_same.parent_delegation_ref = Some(uuid(9));
    unchanged_same.child_delegation_ref = Some(uuid(9));
    unchanged_same.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        "auth://same".into(),
        2,
        hash('c'),
    ));
    let unchanged_same = project_child_task_delta(unchanged_same).expect("unchanged same ref");
    assert_eq!(
        unchanged_same.value.delegation_change,
        ChildTaskDeltaProjectionV1DelegationChange::Unchanged
    );
}

#[test]
fn preimage_is_stable_for_fixed_input_and_object_key_order_is_jcs() {
    let first = project_child_task_delta(delta_facts()).expect("first");
    let second = project_child_task_delta(delta_facts()).expect("second");
    assert_eq!(first.jcs_utf8, second.jcs_utf8);
    assert_eq!(first.sha256, second.sha256);
    assert_projection_sha256(&first, FIXED_DELTA_SHA256);

    // JCS requires lexicographic key order; first key must be added_capabilities.
    let jcs = String::from_utf8(first.jcs_utf8.clone()).expect("utf8");
    assert!(
        jcs.starts_with(r#"{"added_capabilities":"#),
        "JCS object keys must start with added_capabilities, got prefix {}",
        &jcs[..jcs.len().min(40)]
    );
}

#[test]
fn array_order_of_resource_patterns_is_preserved_and_hash_sensitive() {
    let base = project_child_task_delta(delta_facts()).expect("base");

    let mut reordered = delta_facts();
    reordered.child_resource_patterns = vec![
        "https://example.com/b/**".into(),
        "https://example.com/a/**".into(),
    ];
    let reordered = project_child_task_delta(reordered).expect("reordered patterns");
    assert_eq!(
        reordered.value.child_resource_patterns,
        vec![
            "https://example.com/b/**".to_string(),
            "https://example.com/a/**".to_string()
        ]
    );
    assert_ne!(
        base.sha256, reordered.sha256,
        "resource pattern array order is hash-sensitive"
    );

    // Multiset difference sorts its output, but raw child/parent arrays preserve order.
    assert_eq!(
        base.value.child_resource_patterns,
        vec![
            "https://example.com/a/**".to_string(),
            "https://example.com/b/**".to_string()
        ]
    );
}

#[test]
fn capability_hints_are_set_projected_so_duplicate_order_does_not_change_hash() {
    let base = project_child_task_delta(delta_facts()).expect("base");
    let mut shuffled = delta_facts();
    shuffled.child_allowed_capability_hints = vec!["read".into(), "write".into()];
    let shuffled = project_child_task_delta(shuffled).expect("shuffled capabilities");
    assert_eq!(base.sha256, shuffled.sha256);

    let mut with_dup = delta_facts();
    with_dup.child_allowed_capability_hints = vec!["write".into(), "read".into(), "write".into()];
    let with_dup = project_child_task_delta(with_dup).expect("dup capabilities");
    assert_eq!(base.sha256, with_dup.sha256);
    assert_eq!(
        with_dup.value.child_allowed_capability_hints,
        vec!["read", "write"]
    );
}

#[test]
fn rejects_zero_parent_task_revision() {
    let mut facts = delta_facts();
    facts.parent_task_revision = 0;
    assert_invalid_fact_reason(
        project_child_task_delta(facts),
        "parent_task_revision",
        "must be positive",
    );
}

#[test]
fn rejects_empty_and_noncanonical_and_invalid_resource_patterns() {
    let mut empty = delta_facts();
    empty.parent_resource_patterns = vec!["".into()];
    assert_invalid_fact_reason(
        project_child_task_delta(empty),
        "parent_resource_patterns",
        "must be non-empty",
    );

    let mut noncanonical = delta_facts();
    noncanonical.parent_resource_patterns = vec!["HTTPS://Example.COM:443/a/**".into()];
    assert_invalid_fact_reason(
        project_child_task_delta(noncanonical),
        "parent_resource_patterns",
        "URI pattern is not canonical",
    );

    let mut invalid = delta_facts();
    invalid.child_resource_patterns = vec!["not a uri".into()];
    assert_invalid_fact_reason(
        project_child_task_delta(invalid),
        "child_resource_patterns",
        "invalid URI pattern",
    );

    let mut empty_exclusion = delta_facts();
    empty_exclusion.parent_exclusions = vec!["".into()];
    assert_invalid_fact(
        project_child_task_delta(empty_exclusion),
        "parent_exclusions",
    );

    let mut empty_child_exclusion = delta_facts();
    empty_child_exclusion.child_exclusions = vec!["".into()];
    assert_invalid_fact(
        project_child_task_delta(empty_child_exclusion),
        "child_exclusions",
    );
}

#[test]
fn rejects_empty_capability_hints() {
    let mut parent = delta_facts();
    parent.parent_allowed_capability_hints = vec!["".into()];
    assert_invalid_fact_reason(
        project_child_task_delta(parent),
        "parent_allowed_capability_hints",
        "must be non-empty",
    );

    let mut child = delta_facts();
    child.child_allowed_capability_hints = vec!["read".into(), "".into()];
    assert_invalid_fact(
        project_child_task_delta(child),
        "child_allowed_capability_hints",
    );
}

#[test]
fn rejects_delegation_authority_when_child_ref_null() {
    let mut facts = delta_facts();
    facts.child_delegation_ref = None;
    facts.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        "auth://orphan".into(),
        1,
        hash('d'),
    ));
    assert_invalid_fact_reason(
        project_child_task_delta(facts),
        "child_delegation_authority",
        "must be absent when child_delegation_ref is null",
    );
}

#[test]
fn rejects_missing_delegation_authority_when_child_ref_present() {
    let mut facts = delta_facts();
    facts.child_delegation_ref = Some(uuid(9));
    facts.child_delegation_authority = None;
    assert_invalid_fact_reason(
        project_child_task_delta(facts),
        "child_delegation_authority",
        "is required when child_delegation_ref is non-null",
    );
}

#[test]
fn rejects_invalid_verified_authority_fields() {
    let mut empty_ref = delta_facts_with_verified_delegation();
    empty_ref.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        String::new(),
        3,
        hash('a'),
    ));
    assert_invalid_fact_reason(
        project_child_task_delta(empty_ref),
        "delegation_authority_ref",
        "must be non-empty",
    );

    let mut zero_revision = delta_facts_with_verified_delegation();
    zero_revision.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        "delegation-authority://1".into(),
        0,
        hash('a'),
    ));
    assert_invalid_fact_reason(
        project_child_task_delta(zero_revision),
        "delegation_revision",
        "must be positive",
    );

    let mut bad_hash = delta_facts_with_verified_delegation();
    bad_hash.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        "delegation-authority://1".into(),
        3,
        "not-a-hash".into(),
    ));
    assert_invalid_fact_reason(
        project_child_task_delta(bad_hash),
        "delegation_scope_hash",
        "must be lowercase 64-hex",
    );

    let mut upper_hash = delta_facts_with_verified_delegation();
    upper_hash.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        "delegation-authority://1".into(),
        3,
        "A".repeat(64),
    ));
    assert_invalid_fact_reason(
        project_child_task_delta(upper_hash),
        "delegation_scope_hash",
        "must be lowercase 64-hex",
    );

    let mut short_hash = delta_facts_with_verified_delegation();
    short_hash.child_delegation_authority = Some(VerifiedDelegationAuthorityV1::new(
        "delegation-authority://1".into(),
        3,
        "ab".repeat(16),
    ));
    assert_invalid_fact(
        project_child_task_delta(short_hash),
        "delegation_scope_hash",
    );
}

#[test]
fn rejects_parent_task_revision_exceeding_i64() {
    let mut facts = delta_facts();
    facts.parent_task_revision = u64::MAX;
    assert_invalid_fact_reason(
        project_child_task_delta(facts),
        "parent_task_revision",
        "exceeds signed 64-bit range",
    );
}

#[test]
fn typed_facts_do_not_accept_v1_or_legacy_field_names_at_api_boundary() {
    // Compile-time boundary: callers must use ChildTaskDeltaFactsV1 fields, not free JSON.
    // Runtime check: projection value is Schema-validated and has only v1 projection fields.
    let projection = project_child_task_delta(delta_facts()).expect("baseline");
    let json = serde_json::to_value(&projection.value).expect("json");
    let obj = json.as_object().expect("object");
    for legacy in [
        "parentTaskId",
        "parent_task",
        "delta_v0",
        "resourcePatterns",
        "authority",
    ] {
        assert!(
            !obj.contains_key(legacy),
            "projection must not emit legacy field {legacy}"
        );
    }
    assert!(obj.contains_key("parent_task_id"));
    assert!(obj.contains_key("schema_version"));
    assert_eq!(obj.get("schema_version"), Some(&serde_json::json!(1)));
}

#[test]
fn verified_delegation_authority_construction_is_funnelled_through_documented_contract() {
    // The type is sealed: fields are private and construction funnels through
    // `VerifiedDelegationAuthorityV1::new`, whose documented contract requires the caller
    // to have verified the Delegation authority as current/active/applicable (IC §5.3.1).
    // The pure crate cannot verify it itself, so the constructor is the single documented
    // responsibility point; struct-literal forgery no longer compiles.
    let authority = VerifiedDelegationAuthorityV1::new("forged://authority".into(), 99, hash('f'));
    let mut facts = delta_facts();
    facts.child_delegation_ref = Some(uuid(9));
    facts.child_delegation_authority = Some(authority);
    let projection = project_child_task_delta(facts).expect("constructor-produced authority");
    assert_eq!(
        projection.value.authority_status,
        ChildTaskDeltaProjectionV1AuthorityStatus::Verified
    );
    assert_eq!(
        projection.value.delegation_authority_ref.as_deref(),
        Some("forged://authority")
    );
}

#[test]
fn invalid_fact_errors_are_not_contract_or_json_variants() {
    let mut facts = delta_facts();
    facts.parent_task_revision = 0;
    let err = project_child_task_delta(facts).expect_err("must fail");
    assert_error_variant_shape(&err);
    match err {
        AuthorizationProjectionError::InvalidFact { .. } => {}
        AuthorizationProjectionError::Contract(_) | AuthorizationProjectionError::Json(_) => {
            panic!("caller-invalid input must not surface as Contract/Json")
        }
    }
}

#[test]
fn function_signature_is_typed_facts_not_json_bag() {
    let _: fn(ChildTaskDeltaFactsV1) -> Result<_, AuthorizationProjectionError> =
        project_child_task_delta;
}
