//! Integration tests for `project_material_authorization`.

mod support;

use kernel_authorization::{
    project_material_authorization, AuthorizationProjectionError, DestinationFactsV1,
    MaterialAuthorizationFactsV1, ProtectedSurfaceLabelFactsV1,
};
use kernel_contracts::{canonical_json_bytes, sha256_hex};
use support::assertions::{
    assert_canonical_projection, assert_error_variant_shape, assert_invalid_fact,
    assert_invalid_fact_reason, assert_projection_sha256,
};
use support::fixtures::{hash, material_facts, uuid, FIXED_MATERIAL_SHA256};

#[test]
fn positive_projects_canonical_preimage_and_anchored_sha256() {
    let projection = project_material_authorization(material_facts()).expect("baseline material");
    assert_canonical_projection(&projection);
    assert_projection_sha256(&projection, FIXED_MATERIAL_SHA256);

    assert_eq!(
        projection.value.resource_refs,
        vec!["https://example.com/a", "https://example.com/b"]
    );
    assert_eq!(projection.value.content_origin_refs.len(), 2);
    assert_eq!(
        projection.value.content_origin_refs,
        vec![
            "00000000-0000-4000-8000-000000000007".to_string(),
            "00000000-0000-4000-8000-000000000008".to_string()
        ]
    );
    assert_eq!(projection.value.protected_surface_labels.len(), 2);
    assert_eq!(
        projection.value.protected_surface_labels[0].label,
        "authentication"
    );
    assert_eq!(
        projection.value.protected_surface_labels[1].label,
        "payment"
    );
    assert_eq!(projection.value.key_params_hash.len(), 64);
    assert_eq!(projection.value.resource_refs_hash.len(), 64);
    assert_eq!(projection.value.capability_id, "computer.input");
    assert_eq!(projection.value.operation, "click");
}

#[test]
fn positive_derives_key_params_and_resource_refs_hashes_independently() {
    let projection = project_material_authorization(material_facts()).expect("material");
    let key_params = serde_json::json!({"a": 1, "z": 2});
    let expected_key_hash = sha256_hex(&canonical_json_bytes(&key_params).expect("key params jcs"));
    assert_eq!(projection.value.key_params_hash, expected_key_hash);

    let resource_refs = serde_json::json!(["https://example.com/a", "https://example.com/b"]);
    let expected_resource_hash =
        sha256_hex(&canonical_json_bytes(&resource_refs).expect("resource refs jcs"));
    assert_eq!(projection.value.resource_refs_hash, expected_resource_hash);

    // Insertion order of key params must not affect derived hash (JCS sorts object keys).
    let mut reordered = material_facts();
    reordered.normalized_key_params.clear();
    reordered
        .normalized_key_params
        .insert("a".into(), serde_json::json!(1));
    reordered
        .normalized_key_params
        .insert("z".into(), serde_json::json!(2));
    let reordered = project_material_authorization(reordered).expect("reordered keys");
    assert_eq!(projection.sha256, reordered.sha256);
    assert_eq!(
        projection.value.key_params_hash,
        reordered.value.key_params_hash
    );
}

#[test]
fn preimage_is_stable_for_fixed_input() {
    let first = project_material_authorization(material_facts()).expect("first");
    let second = project_material_authorization(material_facts()).expect("second");
    assert_eq!(first.jcs_utf8, second.jcs_utf8);
    assert_eq!(first.sha256, second.sha256);
    assert_projection_sha256(&first, FIXED_MATERIAL_SHA256);
}

#[test]
fn resource_refs_normalize_sort_and_dedup_so_input_order_is_not_hash_sensitive_after_normalize() {
    let base = project_material_authorization(material_facts()).expect("base");
    let mut shuffled = material_facts();
    shuffled.resource_refs = vec![
        "https://example.com/a".into(),
        "HTTPS://Example.COM:443/b".into(),
    ];
    let shuffled = project_material_authorization(shuffled).expect("shuffled resources");
    assert_eq!(base.sha256, shuffled.sha256);
    assert_eq!(
        shuffled.value.resource_refs,
        vec!["https://example.com/a", "https://example.com/b"]
    );
}

#[test]
fn material_families_change_hash() {
    let base = project_material_authorization(material_facts()).unwrap();
    let mut variants: Vec<MaterialAuthorizationFactsV1> = Vec::new();

    let mut facts = material_facts();
    facts.action_revision += 1;
    variants.push(facts);

    let mut facts = material_facts();
    facts.operation = "double_click".into();
    variants.push(facts);

    let mut facts = material_facts();
    facts.resource_refs.push("https://example.com/c".into());
    variants.push(facts);

    let mut facts = material_facts();
    facts.policy_set_revision += 1;
    variants.push(facts);

    let mut facts = material_facts();
    facts.target_stable_ref = Some("element://button/8".into());
    variants.push(facts);

    let mut facts = material_facts();
    facts.content_origin_refs.push(uuid(9));
    variants.push(facts);

    let mut facts = material_facts();
    facts.capability_id = "computer.keyboard".into();
    variants.push(facts);

    let mut facts = material_facts();
    facts.task_revision += 1;
    variants.push(facts);

    for facts in variants {
        let changed = project_material_authorization(facts).unwrap();
        assert_ne!(base.sha256, changed.sha256);
    }
}

#[test]
fn rejects_zero_positive_counters() {
    let mut facts = material_facts();
    facts.task_revision = 0;
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "task_revision",
        "must be positive",
    );

    let mut facts = material_facts();
    facts.action_revision = 0;
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "action_revision",
        "must be positive",
    );

    let mut facts = material_facts();
    facts.policy_set_revision = 0;
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "policy_set_revision",
        "must be positive",
    );
}

#[test]
fn rejects_empty_required_strings() {
    let mut facts = material_facts();
    facts.capability_id.clear();
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "capability_id",
        "must be non-empty",
    );

    let mut facts = material_facts();
    facts.operation.clear();
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "operation",
        "must be non-empty",
    );

    let mut facts = material_facts();
    facts.target_kind.clear();
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "target_kind",
        "must be non-empty",
    );

    let mut facts = material_facts();
    facts.target_stable_ref = Some(String::new());
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "target_stable_ref",
        "must be non-empty",
    );
}

#[test]
fn rejects_invalid_optional_hashes() {
    let mut facts = material_facts();
    facts.child_task_delta_hash = Some("ZZ".into());
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "child_task_delta_hash",
        "must be lowercase 64-hex",
    );

    let mut facts = material_facts();
    facts.task_proposal_hash = Some("A".repeat(64));
    assert_invalid_fact(project_material_authorization(facts), "task_proposal_hash");

    let mut facts = material_facts();
    facts.proposed_plan_version = Some(2);
    facts.proposed_plan_hash = Some("not-hex".into());
    assert_invalid_fact(project_material_authorization(facts), "proposed_plan_hash");

    let mut facts = material_facts();
    facts.child_task_delta_hash = Some(hash('c'));
    let ok = project_material_authorization(facts).expect("valid optional hash");
    assert_eq!(
        ok.value.child_task_delta_hash.as_deref(),
        Some(hash('c').as_str())
    );
}

#[test]
fn rejects_delegation_relationship_inconsistencies() {
    let mut orphan_authority = material_facts();
    orphan_authority.delegation_authority_ref = Some("authority://orphan".into());
    assert_invalid_fact_reason(
        project_material_authorization(orphan_authority),
        "delegation_ref",
        "authority and revision must be null when delegation_ref is null",
    );

    let mut orphan_revision = material_facts();
    orphan_revision.delegation_revision = Some(1);
    assert_invalid_fact(
        project_material_authorization(orphan_revision),
        "delegation_ref",
    );

    let mut missing_authority = material_facts();
    missing_authority.delegation_ref = Some(uuid(11));
    missing_authority.delegation_revision = Some(2);
    assert_invalid_fact_reason(
        project_material_authorization(missing_authority),
        "delegation_authority_ref",
        "required when delegation_ref is non-null",
    );

    let mut missing_revision = material_facts();
    missing_revision.delegation_ref = Some(uuid(11));
    missing_revision.delegation_authority_ref = Some("authority://1".into());
    assert_invalid_fact_reason(
        project_material_authorization(missing_revision),
        "delegation_revision",
        "required when delegation_ref is non-null",
    );

    let mut empty_authority = material_facts();
    empty_authority.delegation_ref = Some(uuid(11));
    empty_authority.delegation_authority_ref = Some(String::new());
    empty_authority.delegation_revision = Some(2);
    assert_invalid_fact_reason(
        project_material_authorization(empty_authority),
        "delegation_authority_ref",
        "must be non-empty",
    );

    let mut zero_revision = material_facts();
    zero_revision.delegation_ref = Some(uuid(11));
    zero_revision.delegation_authority_ref = Some("authority://1".into());
    zero_revision.delegation_revision = Some(0);
    assert_invalid_fact_reason(
        project_material_authorization(zero_revision),
        "delegation_revision",
        "must be positive",
    );

    let mut complete = material_facts();
    complete.delegation_ref = Some(uuid(11));
    complete.delegation_authority_ref = Some("authority://1".into());
    complete.delegation_revision = Some(2);
    let complete = project_material_authorization(complete).expect("complete delegation");
    assert_eq!(
        complete.value.delegation_ref.as_deref(),
        Some("00000000-0000-4000-8000-00000000000b")
    );
    assert_eq!(complete.value.delegation_revision, Some(2));
}

#[test]
fn rejects_proposed_plan_pair_mismatch() {
    let mut version_only = material_facts();
    version_only.proposed_plan_version = Some(2);
    assert_invalid_fact_reason(
        project_material_authorization(version_only),
        "proposed_plan_version",
        "proposed plan version and hash must be jointly null or non-null",
    );

    // Use a valid lowercase 64-hex so hash-format validation does not fire first;
    // the relationship check must surface as proposed_plan_version.
    let mut hash_only = material_facts();
    hash_only.proposed_plan_hash = Some(hash('a'));
    assert_invalid_fact_reason(
        project_material_authorization(hash_only),
        "proposed_plan_version",
        "proposed plan version and hash must be jointly null or non-null",
    );

    let mut both = material_facts();
    both.proposed_plan_version = Some(2);
    both.proposed_plan_hash = Some(hash('a'));
    let both = project_material_authorization(both).expect("joint plan pair");
    assert_eq!(both.value.proposed_plan_version, Some(2));
    assert_eq!(
        both.value.proposed_plan_hash.as_deref(),
        Some(hash('a').as_str())
    );
}

#[test]
fn rejects_invalid_resource_uri() {
    let mut facts = material_facts();
    facts.resource_refs = vec!["not a uri".into()];
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "resource_refs",
        "invalid resource URI",
    );

    let mut empty = material_facts();
    empty.resource_refs = vec!["".into()];
    assert_invalid_fact(project_material_authorization(empty), "resource_refs");
}

#[test]
fn rejects_empty_destination_fields() {
    let mut facts = material_facts();
    facts.destination = Some(DestinationFactsV1 {
        kind: String::new(),
        stable_ref: "channel://x".into(),
        account_ref: None,
        channel_ref: None,
    });
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "destination.kind",
        "must be non-empty",
    );

    let mut facts = material_facts();
    facts.destination = Some(DestinationFactsV1 {
        kind: "channel".into(),
        stable_ref: String::new(),
        account_ref: None,
        channel_ref: None,
    });
    assert_invalid_fact(
        project_material_authorization(facts),
        "destination.stable_ref",
    );

    let mut facts = material_facts();
    facts.destination = Some(DestinationFactsV1 {
        kind: "channel".into(),
        stable_ref: "channel://x".into(),
        account_ref: Some(String::new()),
        channel_ref: None,
    });
    assert_invalid_fact(
        project_material_authorization(facts),
        "destination.account_ref",
    );

    let mut facts = material_facts();
    facts.destination = Some(DestinationFactsV1 {
        kind: "channel".into(),
        stable_ref: "channel://x".into(),
        account_ref: None,
        channel_ref: Some(String::new()),
    });
    assert_invalid_fact(
        project_material_authorization(facts),
        "destination.channel_ref",
    );
}

#[test]
fn rejects_empty_protected_surface_label_fields() {
    let mut facts = material_facts();
    facts.protected_surface_labels = vec![ProtectedSurfaceLabelFactsV1 {
        label: String::new(),
        classification: "sensitive".into(),
        source_ref: "surface://1".into(),
    }];
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "protected_surface_labels.label",
        "must be non-empty",
    );

    let mut facts = material_facts();
    facts.protected_surface_labels = vec![ProtectedSurfaceLabelFactsV1 {
        label: "auth".into(),
        classification: String::new(),
        source_ref: "surface://1".into(),
    }];
    assert_invalid_fact(
        project_material_authorization(facts),
        "protected_surface_labels.classification",
    );

    let mut facts = material_facts();
    facts.protected_surface_labels = vec![ProtectedSurfaceLabelFactsV1 {
        label: "auth".into(),
        classification: "sensitive".into(),
        source_ref: String::new(),
    }];
    assert_invalid_fact(
        project_material_authorization(facts),
        "protected_surface_labels.source_ref",
    );
}

#[test]
fn protected_surface_labels_dedup_and_sort_by_tuple() {
    let projection = project_material_authorization(material_facts()).expect("material");
    assert_eq!(projection.value.protected_surface_labels.len(), 2);

    let mut facts = material_facts();
    facts.protected_surface_labels = vec![
        ProtectedSurfaceLabelFactsV1 {
            label: "z".into(),
            classification: "c".into(),
            source_ref: "s://2".into(),
        },
        ProtectedSurfaceLabelFactsV1 {
            label: "a".into(),
            classification: "c".into(),
            source_ref: "s://1".into(),
        },
        ProtectedSurfaceLabelFactsV1 {
            label: "a".into(),
            classification: "c".into(),
            source_ref: "s://1".into(),
        },
    ];
    let projection = project_material_authorization(facts).expect("labels");
    assert_eq!(projection.value.protected_surface_labels.len(), 2);
    assert_eq!(projection.value.protected_surface_labels[0].label, "a");
    assert_eq!(projection.value.protected_surface_labels[1].label, "z");
}

#[test]
fn rejects_counter_exceeding_i64() {
    let mut facts = material_facts();
    facts.task_revision = u64::MAX;
    assert_invalid_fact_reason(
        project_material_authorization(facts),
        "task_revision",
        "exceeds signed 64-bit range",
    );
}

#[test]
fn typed_facts_do_not_emit_legacy_field_names() {
    let projection = project_material_authorization(material_facts()).expect("material");
    let json = serde_json::to_value(&projection.value).expect("json");
    let obj = json.as_object().expect("object");
    for legacy in [
        "taskId",
        "actionId",
        "keyParams",
        "resources",
        "policySetRevisionV0",
        "authz_v1",
    ] {
        assert!(!obj.contains_key(legacy), "legacy field leaked: {legacy}");
    }
    assert!(obj.contains_key("task_id"));
    assert!(obj.contains_key("key_params_hash"));
    assert!(obj.contains_key("resource_refs_hash"));
    assert_eq!(obj.get("schema_version"), Some(&serde_json::json!(1)));
}

#[test]
fn invalid_fact_errors_are_not_contract_or_json_variants() {
    let mut facts = material_facts();
    facts.capability_id.clear();
    let err = project_material_authorization(facts).expect_err("must fail");
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
    let _: fn(MaterialAuthorizationFactsV1) -> Result<_, AuthorizationProjectionError> =
        project_material_authorization;
}
