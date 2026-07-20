//! Integration tests for `project_observation_evidence`.

mod support;

use kernel_authorization::{
    project_observation_evidence, AuthorizationProjectionError, ObservationEvidenceFactsV1,
    ObservedEvidenceFactsV1,
};
use kernel_contracts::ObservationEvidenceProjectionV1;
use support::assertions::{
    assert_canonical_projection, assert_error_variant_shape, assert_invalid_fact,
    assert_invalid_fact_reason, assert_projection_sha256,
};
use support::fixtures::{
    hash, observation_facts, observed_facts, FIXED_OBSERVATION_NA_SHA256, FIXED_OBSERVATION_SHA256,
};

#[test]
fn positive_not_applicable_projects_canonical_preimage_and_anchored_sha256() {
    let projection = project_observation_evidence(ObservationEvidenceFactsV1::NotApplicable)
        .expect("not applicable");
    assert_canonical_projection(&projection);
    assert_projection_sha256(&projection, FIXED_OBSERVATION_NA_SHA256);
    assert_eq!(
        projection.jcs_utf8,
        br#"{"observation_kind":"not_applicable","schema_version":1}"#
    );
    assert!(matches!(
        projection.value,
        ObservationEvidenceProjectionV1::NotApplicable { .. }
    ));
}

#[test]
fn positive_observed_projects_canonical_preimage_and_anchored_sha256() {
    let projection = project_observation_evidence(observation_facts()).expect("observed");
    assert_canonical_projection(&projection);
    assert_projection_sha256(&projection, FIXED_OBSERVATION_SHA256);

    match &projection.value {
        ObservationEvidenceProjectionV1::Observed {
            observed_at,
            valid_until,
            evidence_refs,
            protected_surface_observations,
            provider_ref,
            provider_revision,
            snapshot_ref,
            snapshot_generation,
            ..
        } => {
            assert_eq!(observed_at, "2026-07-20T08:00:00Z");
            assert_eq!(valid_until, "2026-07-20T08:01:00Z");
            assert_eq!(
                evidence_refs,
                &vec!["evidence://1".to_string(), "evidence://2".to_string()]
            );
            assert_eq!(protected_surface_observations.len(), 2);
            assert_eq!(
                protected_surface_observations[0],
                protected_surface_observations[1]
            );
            assert_eq!(provider_ref, "provider://desktop/1");
            assert_eq!(*provider_revision, 2);
            assert_eq!(snapshot_ref.as_deref(), Some("snapshot://desktop/4"));
            assert_eq!(*snapshot_generation, Some(4));
        }
        ObservationEvidenceProjectionV1::NotApplicable { .. } => {
            panic!("expected observed branch")
        }
    }
}

#[test]
fn preimage_is_stable_for_fixed_input() {
    let first = project_observation_evidence(observation_facts()).expect("first");
    let second = project_observation_evidence(observation_facts()).expect("second");
    assert_eq!(first.jcs_utf8, second.jcs_utf8);
    assert_eq!(first.sha256, second.sha256);
    assert_projection_sha256(&first, FIXED_OBSERVATION_SHA256);
}

#[test]
fn evidence_refs_are_sorted_and_deduped_while_protected_surface_order_is_preserved() {
    let projection = project_observation_evidence(observation_facts()).expect("observed");
    match projection.value {
        ObservationEvidenceProjectionV1::Observed {
            evidence_refs,
            protected_surface_observations,
            ..
        } => {
            assert_eq!(
                evidence_refs,
                vec!["evidence://1".to_string(), "evidence://2".to_string()]
            );
            // Duplicates preserved in protected_surface_observations (order-sensitive array).
            assert_eq!(protected_surface_observations.len(), 2);
        }
        _ => panic!("expected observed"),
    }

    let mut reordered_protected = observed_facts();
    reordered_protected.protected_surface_observations = vec![
        serde_json::json!({"label": "second"}),
        serde_json::json!({"label": "first"}),
    ];
    let reordered = project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(
        reordered_protected,
    )))
    .expect("reordered protected");
    let base = project_observation_evidence(observation_facts()).expect("base");
    assert_ne!(
        base.sha256, reordered.sha256,
        "protected_surface_observations order is hash-sensitive"
    );
}

#[test]
fn evidence_refs_input_order_is_not_hash_sensitive_after_set_projection() {
    let base = project_observation_evidence(observation_facts()).expect("base");
    let mut shuffled = observed_facts();
    shuffled.evidence_refs = vec![
        "evidence://1".into(),
        "evidence://2".into(),
        "evidence://1".into(),
    ];
    let shuffled =
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(shuffled)))
            .expect("shuffled evidence");
    assert_eq!(base.sha256, shuffled.sha256);
}

#[test]
fn rejects_empty_and_reserved_provider_refs() {
    let mut empty = observed_facts();
    empty.provider_ref.clear();
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(empty))),
        "provider_ref",
        "must be non-empty",
    );

    for reserved in ["core", "none", "system"] {
        let mut facts = observed_facts();
        facts.provider_ref = reserved.into();
        assert_invalid_fact_reason(
            project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(facts))),
            "provider_ref",
            "reserved pseudo-provider is forbidden",
        );
    }
}

#[test]
fn rejects_zero_provider_revision() {
    let mut facts = observed_facts();
    facts.provider_revision = 0;
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(facts))),
        "provider_revision",
        "must be positive",
    );
}

#[test]
fn rejects_snapshot_pair_mismatch() {
    let mut ref_only = observed_facts();
    ref_only.snapshot_generation = None;
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(ref_only))),
        "snapshot_ref",
        "snapshot_ref and snapshot_generation must be jointly null or non-null",
    );

    let mut gen_only = observed_facts();
    gen_only.snapshot_ref = None;
    assert_invalid_fact(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(gen_only))),
        "snapshot_ref",
    );

    let mut empty_ref = observed_facts();
    empty_ref.snapshot_ref = Some(String::new());
    empty_ref.snapshot_generation = Some(1);
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(empty_ref))),
        "snapshot_ref",
        "must be non-empty",
    );

    let mut both_null = observed_facts();
    both_null.snapshot_ref = None;
    both_null.snapshot_generation = None;
    project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(both_null)))
        .expect("jointly null snapshot pair is allowed");
}

#[test]
fn rejects_empty_optional_observation_refs() {
    let mut target = observed_facts();
    target.target_observation_ref = Some(String::new());
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(target))),
        "target_observation_ref",
        "must be non-empty",
    );

    let mut destination = observed_facts();
    destination.destination_observation_ref = Some(String::new());
    assert_invalid_fact(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(destination))),
        "destination_observation_ref",
    );
}

#[test]
fn rejects_invalid_coordinate_transform_hash() {
    let mut bad = observed_facts();
    bad.coordinate_transform_hash = Some("nope".into());
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(bad))),
        "coordinate_transform_hash",
        "must be lowercase 64-hex",
    );

    let mut upper = observed_facts();
    upper.coordinate_transform_hash = Some("E".repeat(64));
    assert_invalid_fact(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(upper))),
        "coordinate_transform_hash",
    );
}

#[test]
fn rejects_invalid_and_unordered_timestamps() {
    let mut bad_observed = observed_facts();
    bad_observed.observed_at = "not-a-timestamp".into();
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(bad_observed))),
        "observed_at",
        "invalid timestamp",
    );

    let mut bad_until = observed_facts();
    bad_until.valid_until = "also-not".into();
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(bad_until))),
        "valid_until",
        "invalid timestamp",
    );

    let mut equal = observed_facts();
    equal.valid_until = "2026-07-20T08:00:00Z".into();
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(equal))),
        "valid_until",
        "must be later than observed_at",
    );

    let mut earlier = observed_facts();
    earlier.valid_until = "2026-07-20T07:59:00Z".into();
    assert_invalid_fact(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(earlier))),
        "valid_until",
    );
}

#[test]
fn rejects_empty_evidence_refs_entries() {
    let mut facts = observed_facts();
    facts.evidence_refs = vec!["evidence://1".into(), "".into()];
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(facts))),
        "evidence_refs",
        "must be non-empty",
    );
}

#[test]
fn rejects_provider_revision_exceeding_i64() {
    let mut facts = observed_facts();
    facts.provider_revision = u64::MAX;
    assert_invalid_fact_reason(
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(facts))),
        "provider_revision",
        "exceeds signed 64-bit range",
    );
}

#[test]
fn observed_and_not_applicable_are_distinct_union_branches() {
    let na = project_observation_evidence(ObservationEvidenceFactsV1::NotApplicable)
        .expect("not applicable");
    let observed = project_observation_evidence(observation_facts()).expect("observed");
    assert_ne!(na.sha256, observed.sha256);

    // Wrong branch facts are not expressible as free JSON bags; the enum forces one arm.
    let _: ObservationEvidenceFactsV1 = ObservationEvidenceFactsV1::NotApplicable;
    let _: ObservationEvidenceFactsV1 =
        ObservationEvidenceFactsV1::Observed(Box::new(observed_facts()));
}

#[test]
fn typed_facts_do_not_emit_legacy_field_names() {
    let projection = project_observation_evidence(observation_facts()).expect("observed");
    let json = serde_json::to_value(&projection.value).expect("json");
    let obj = json.as_object().expect("object");
    for legacy in [
        "observationKind",
        "providerRef",
        "observedAt",
        "evidence",
        "observation_v0",
    ] {
        assert!(!obj.contains_key(legacy), "legacy field leaked: {legacy}");
    }
    assert_eq!(
        obj.get("observation_kind"),
        Some(&serde_json::json!("observed"))
    );
    assert!(obj.contains_key("provider_ref"));
    assert_eq!(obj.get("schema_version"), Some(&serde_json::json!(1)));
}

#[test]
fn invalid_fact_errors_are_not_contract_or_json_variants() {
    let mut facts = observed_facts();
    facts.provider_ref = "core".into();
    let err = project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(facts)))
        .expect_err("must fail");
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
    let _: fn(ObservationEvidenceFactsV1) -> Result<_, AuthorizationProjectionError> =
        project_observation_evidence;
    // ObservedEvidenceFactsV1 is only reachable via the Observed arm.
    let _ = ObservedEvidenceFactsV1 {
        provider_ref: "provider://x".into(),
        provider_revision: 1,
        snapshot_ref: None,
        snapshot_generation: None,
        target_observation_ref: None,
        coordinate_transform_hash: None,
        observed_at: "2026-07-20T08:00:00Z".into(),
        valid_until: "2026-07-20T08:01:00Z".into(),
        evidence_refs: vec![],
        protected_surface_observations: vec![],
        destination_observation_ref: None,
    };
}

#[test]
fn optional_coordinate_transform_none_is_allowed() {
    let mut facts = observed_facts();
    facts.coordinate_transform_hash = None;
    let projection =
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(facts)))
            .expect("optional hash none");
    match projection.value {
        ObservationEvidenceProjectionV1::Observed {
            coordinate_transform_hash,
            ..
        } => assert!(coordinate_transform_hash.is_none()),
        _ => panic!("expected observed"),
    }
}

#[test]
fn valid_optional_hash_is_accepted() {
    let mut facts = observed_facts();
    facts.coordinate_transform_hash = Some(hash('9'));
    let projection =
        project_observation_evidence(ObservationEvidenceFactsV1::Observed(Box::new(facts)))
            .expect("valid hash");
    match projection.value {
        ObservationEvidenceProjectionV1::Observed {
            coordinate_transform_hash,
            ..
        } => assert_eq!(
            coordinate_transform_hash.as_deref(),
            Some(hash('9').as_str())
        ),
        _ => panic!("expected observed"),
    }
}
