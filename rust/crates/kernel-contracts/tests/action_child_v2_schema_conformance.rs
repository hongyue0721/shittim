//! V2InitialBuildActive slice 1b Action/child Schema conformance.

use kernel_contracts::{
    canonical_json_bytes, decode_validated, validate_json, ActionRequestV2,
    ActionTransitionIntentV1, ChildTaskDeltaProjectionV1, MaterialAuthorizationProjectionV1,
    ObservationEvidenceProjectionV1, SchemaCatalog, VerificationResult,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const ACTION_INTENT: &str = "https://schemas.shittim.local/task/action_transition_intent/v1";
const ACTION_REQUEST: &str = "https://schemas.shittim.local/task/action_request/v2";
const CHILD_DELTA: &str = "https://schemas.shittim.local/task/child_task_delta_projection/v1";
const MATERIAL: &str = "https://schemas.shittim.local/policy/material_authorization_projection/v1";
const OBSERVATION: &str = "https://schemas.shittim.local/policy/observation_evidence_projection/v1";
const VERIFICATION: &str = "https://schemas.shittim.local/v1/task/verification_result.json";
const ACTION_STATUS: &str = "https://schemas.shittim.local/v1/common/action_status.json";
const ACTOR: &str = "https://schemas.shittim.local/v1/common/actor.json";
const ENTRY_POINT: &str = "https://schemas.shittim.local/v1/common/entry_point.json";
const SIDE_EFFECT: &str = "https://schemas.shittim.local/v1/common/side_effect_class.json";

fn uuid(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02}")
}

fn hash(character: char) -> String {
    std::iter::repeat_n(character, 64).collect()
}

fn actor() -> Value {
    json!({
        "schema_version": 1,
        "revision": 2,
        "id": "actor-local-1",
        "kind": "known_user",
        "source": "local-desktop",
        "authentication_level": "platform_verified",
        "confidence": 0.9
    })
}

fn action_intent() -> Value {
    json!({
        "schema_version": 1,
        "transition_id": uuid(1),
        "action_id": uuid(2),
        "expected_action_revision": 3,
        "execution_generation": 1,
        "from_status": "in_flight",
        "to_status": "completed",
        "reason_code": "child_materialized",
        "correlation_id": "corr-child-1",
        "created_at": "2026-07-20T08:00:00Z"
    })
}

fn action_request() -> Value {
    json!({
        "action_id": uuid(2),
        "task_id": uuid(3),
        "step_id": null,
        "parent_action_id": null,
        "capability_id": "kernel.task",
        "operation": "task.child.create",
        "structured_arguments": {"schema_version": 1, "goal": "child"},
        "resource_refs": ["task://parent/3"],
        "task_scope_ref": uuid(4),
        "side_effect_class": "S1",
        "idempotency_key": "child-action-1",
        "execution_generation": 1,
        "permission_decision_ref": uuid(5),
        "approval_chain_id": null,
        "verification_policy": {
            "strategy": "kernel_local_materialization",
            "expected_outcome": {"child_created": true},
            "timeout": "PT30S"
        },
        "rollback_policy": null,
        "result": {
            "materialized_child_task_ref": uuid(6),
            "verification_result_refs": [uuid(7)]
        },
        "status": "completed",
        "recovery_meta": null,
        "lease": null,
        "schema_version": 2,
        "revision": 4,
        "created_at": "2026-07-20T07:59:00Z",
        "updated_at": "2026-07-20T08:00:00Z"
    })
}

fn child_delta() -> Value {
    json!({
        "schema_version": 1,
        "parent_task_id": uuid(3),
        "parent_task_revision": 2,
        "parent_task_scope_ref": uuid(4),
        "parent_resource_patterns": ["https://example.com/a/**", "https://example.com/a/**"],
        "parent_exclusions": ["https://example.com/a/tmp/**"],
        "child_resource_patterns": ["https://example.com/a/**", "https://example.com/b/**"],
        "child_exclusions": [],
        "parent_allowed_capability_hints": ["read"],
        "child_allowed_capability_hints": ["read", "write"],
        "added_resource_patterns": ["https://example.com/b/**"],
        "removed_resource_patterns": ["https://example.com/a/**"],
        "added_exclusions": [],
        "removed_exclusions": ["https://example.com/a/tmp/**"],
        "added_capabilities": ["write"],
        "removed_capabilities": [],
        "parent_delegation_ref": null,
        "child_delegation_ref": null,
        "delegation_change": "unchanged",
        "delegation_authority_ref": null,
        "delegation_revision": null,
        "delegation_scope_hash": null,
        "authority_status": "not_applicable"
    })
}

fn material() -> Value {
    json!({
        "schema_version": 1,
        "actor": actor(),
        "entry_point": "local_desktop",
        "task_id": uuid(3),
        "task_revision": 2,
        "task_plan_version": 1,
        "action_id": uuid(2),
        "action_revision": 3,
        "capability_id": "kernel.task",
        "operation": "task.child.create",
        "side_effect_class": "S1",
        "normalized_key_params": {"goal": "child", "schema_version": 1},
        "key_params_hash": hash('a'),
        "task_scope_ref": uuid(4),
        "resource_refs": ["https://example.com/a", "https://example.com/b"],
        "resource_refs_hash": hash('b'),
        "child_task_delta_hash": hash('c'),
        "delegation_ref": null,
        "delegation_authority_ref": null,
        "delegation_revision": null,
        "policy_set_revision": 5,
        "target_kind": "task_proposal",
        "target_stable_ref": uuid(3),
        "destination": null,
        "protected_surface_labels": [],
        "content_origin_refs": [uuid(8)],
        "task_proposal_hash": hash('d'),
        "proposed_plan_version": null,
        "proposed_plan_hash": null
    })
}

fn observed() -> Value {
    json!({
        "schema_version": 1,
        "observation_kind": "observed",
        "provider_ref": "provider://desktop/1",
        "provider_revision": 2,
        "snapshot_ref": "snapshot://desktop/4",
        "snapshot_generation": 4,
        "target_observation_ref": "observation://target/1",
        "coordinate_transform_hash": hash('e'),
        "observed_at": "2026-07-20T08:00:00Z",
        "valid_until": "2026-07-20T08:01:00Z",
        "evidence_refs": ["evidence://1", "evidence://2"],
        "protected_surface_observations": [{"label": "authentication"}],
        "destination_observation_ref": null
    })
}

fn assert_round_trip<T>(schema_id: &str, value: &Value)
where
    T: DeserializeOwned + Serialize,
{
    let typed: T = decode_validated(schema_id, value).expect("typed decode");
    let encoded = serde_json::to_value(typed).expect("serialize");
    validate_json(schema_id, &encoded).expect("revalidate");
    assert_eq!(
        canonical_json_bytes(value).expect("input JCS"),
        canonical_json_bytes(&encoded).expect("encoded JCS")
    );
}

#[test]
fn manifest_batch_identity_refs_and_retained_boundary() {
    let manifest: Value =
        serde_json::from_str(include_str!("../../../../schemas/manifest.json")).expect("manifest");
    let entries = manifest["schemas"].as_array().expect("schemas");
    assert!(
        entries.len() >= 70,
        "production baseline plus synthetic probe roots"
    );
    assert!(manifest["method_version_bindings"]
        .as_array()
        .expect("bindings")
        .is_empty());

    let expected = [
        (
            ACTION_INTENT,
            "ActionTransitionIntentV1",
            "task",
            "domain_object",
            "new-contract",
        ),
        (
            ACTION_REQUEST,
            "ActionRequestV2",
            "task",
            "domain_object",
            "breaking-replacement",
        ),
        (
            CHILD_DELTA,
            "ChildTaskDeltaProjectionV1",
            "task",
            "object",
            "new-contract",
        ),
        (
            MATERIAL,
            "MaterialAuthorizationProjectionV1",
            "policy",
            "object",
            "new-contract",
        ),
        (
            OBSERVATION,
            "ObservationEvidenceProjectionV1",
            "policy",
            "object",
            "new-contract",
        ),
    ];
    for (id, title, component, kind, compatibility) in expected {
        let entry = entries
            .iter()
            .find(|entry| entry["id"] == id)
            .expect("entry");
        assert_eq!(entry["title"], title);
        assert_eq!(entry["component"], component);
        assert_eq!(entry["kind"], kind);
        assert_eq!(entry["compatibility"], compatibility);
    }

    let catalog = SchemaCatalog::load_embedded().expect("catalog");
    assert!(catalog.document(VERIFICATION).is_some());
    assert!(entries.iter().any(|entry| entry["id"] == VERIFICATION));
    assert!(!entries.iter().any(|entry| {
        entry["id"] == "https://schemas.shittim.local/task/verification_result/v2"
    }));
}

#[test]
fn typed_round_trips_cover_all_new_roots_and_both_observation_branches() {
    assert_round_trip::<ActionTransitionIntentV1>(ACTION_INTENT, &action_intent());
    assert_round_trip::<ActionRequestV2>(ACTION_REQUEST, &action_request());
    assert_round_trip::<ChildTaskDeltaProjectionV1>(CHILD_DELTA, &child_delta());
    assert_round_trip::<MaterialAuthorizationProjectionV1>(MATERIAL, &material());
    assert_round_trip::<ObservationEvidenceProjectionV1>(
        OBSERVATION,
        &json!({"schema_version": 1, "observation_kind": "not_applicable"}),
    );
    assert_round_trip::<ObservationEvidenceProjectionV1>(OBSERVATION, &observed());
}

#[test]
fn required_fields_unknown_fields_and_scalar_bounds_fail_closed() {
    for (schema, mut value, field) in [
        (ACTION_INTENT, action_intent(), "correlation_id"),
        (ACTION_REQUEST, action_request(), "approval_chain_id"),
        (CHILD_DELTA, child_delta(), "authority_status"),
        (MATERIAL, material(), "child_task_delta_hash"),
        (OBSERVATION, observed(), "snapshot_generation"),
    ] {
        value.as_object_mut().expect("object").remove(field);
        assert!(
            validate_json(schema, &value).is_err(),
            "{schema} missing {field}"
        );
    }

    for (schema, mut value) in [
        (ACTION_INTENT, action_intent()),
        (ACTION_REQUEST, action_request()),
        (CHILD_DELTA, child_delta()),
        (MATERIAL, material()),
        (OBSERVATION, observed()),
    ] {
        value["unexpected"] = json!(true);
        assert!(validate_json(schema, &value).is_err(), "{schema} unknown");
    }

    let mut intent = action_intent();
    intent["expected_action_revision"] = json!(-1);
    assert!(validate_json(ACTION_INTENT, &intent).is_err());
    let mut illegal_edge = action_intent();
    illegal_edge["from_status"] = json!("completed");
    illegal_edge["to_status"] = json!("approved");
    assert!(validate_json(ACTION_INTENT, &illegal_edge).is_err());
    let mut action = action_request();
    action["revision"] = json!(0);
    assert!(validate_json(ACTION_REQUEST, &action).is_err());
    let mut delta = child_delta();
    delta["parent_task_revision"] = json!(0);
    assert!(validate_json(CHILD_DELTA, &delta).is_err());
    let mut material = material();
    material["key_params_hash"] = json!("A");
    assert!(validate_json(MATERIAL, &material).is_err());
}

#[test]
fn joint_nullability_and_tagged_union_fail_closed() {
    let mut delta = child_delta();
    delta["child_delegation_ref"] = json!(uuid(9));
    assert!(validate_json(CHILD_DELTA, &delta).is_err());
    delta["delegation_authority_ref"] = json!("delegation-authority://1");
    delta["delegation_revision"] = json!(2);
    delta["delegation_scope_hash"] = json!(hash('f'));
    delta["delegation_change"] = json!("added");
    delta["authority_status"] = json!("verified");
    assert!(validate_json(CHILD_DELTA, &delta).is_ok());

    let mut material = material();
    material["delegation_authority_ref"] = json!("authority://1");
    assert!(validate_json(MATERIAL, &material).is_err());

    let mut observation = observed();
    observation["snapshot_generation"] = Value::Null;
    assert!(validate_json(OBSERVATION, &observation).is_err());
    observation["snapshot_ref"] = Value::Null;
    assert!(validate_json(OBSERVATION, &observation).is_ok());

    let mut not_applicable = json!({"schema_version": 1, "observation_kind": "not_applicable"});
    not_applicable["provider_ref"] = json!("provider://illegal");
    assert!(validate_json(OBSERVATION, &not_applicable).is_err());
}

#[test]
fn verification_result_v1_has_every_child_materialization_fact() {
    let document = SchemaCatalog::load_embedded()
        .expect("catalog")
        .document(VERIFICATION)
        .expect("verification")
        .clone();
    let required = document["required"]
        .as_array()
        .expect("required")
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    for field in [
        "id",
        "schema_version",
        "action_id",
        "strategy_used",
        "outcome",
        "verifier_kind",
        "observed_resource_refs",
        "evidence_refs",
        "verified_at",
        "observations",
        "side_effect_confirmed",
        "recommendation",
        "created_at",
    ] {
        assert!(required.contains(field), "missing retained field {field}");
    }
    let sample = json!({
        "id": uuid(7),
        "schema_version": 1,
        "action_id": uuid(2),
        "strategy_used": "kernel_local_materialization",
        "outcome": "verified_ok",
        "verifier_kind": "kernel",
        "observed_resource_refs": [uuid(6)],
        "before_version": null,
        "after_version": uuid(6),
        "evidence_refs": [uuid(6)],
        "confidence": 1.0,
        "verified_at": "2026-07-20T08:00:00Z",
        "observations": [{
            "check_type": "resource_state",
            "expected": {"child_absent": true},
            "actual": {"child_task_id": uuid(6)},
            "passed": true,
            "evidence_ref": uuid(6)
        }],
        "side_effect_confirmed": true,
        "recommendation": "complete",
        "created_at": "2026-07-20T08:00:00Z"
    });
    assert_round_trip::<VerificationResult>(VERIFICATION, &sample);
}

#[test]
fn direct_refs_are_exact_for_new_roots() {
    let catalog = SchemaCatalog::load_embedded().expect("catalog");
    let expected = [
        (ACTION_INTENT, BTreeSet::from([ACTION_STATUS])),
        (ACTION_REQUEST, BTreeSet::from([ACTION_STATUS, SIDE_EFFECT])),
        (CHILD_DELTA, BTreeSet::new()),
        (MATERIAL, BTreeSet::from([ACTOR, ENTRY_POINT, SIDE_EFFECT])),
        (OBSERVATION, BTreeSet::new()),
    ];
    for (schema_id, expected_refs) in expected {
        let mut refs = BTreeSet::new();
        collect_whole_root_refs(catalog.document(schema_id).expect("document"), &mut refs);
        assert_eq!(refs, expected_refs, "{schema_id}");
    }
}

fn collect_whole_root_refs<'a>(value: &'a Value, output: &mut BTreeSet<&'a str>) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                if !reference.contains('#') {
                    output.insert(reference);
                }
            }
            for child in object.values() {
                collect_whole_root_refs(child, output);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_whole_root_refs(child, output);
            }
        }
        _ => {}
    }
}
