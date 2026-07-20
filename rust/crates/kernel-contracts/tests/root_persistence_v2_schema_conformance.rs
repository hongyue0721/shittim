//! V2InitialBuildActive slice 1a: root-persistence Schema and typed conformance.

use kernel_contracts::{
    canonical_json_bytes, decode_validated, validate_json, AuditAllocationV2, AuditRecordV2,
    ContentOriginV2, SchemaCatalog, TaskCreationProvenanceV1,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const CONTENT_ORIGIN_V2: &str = "https://schemas.shittim.local/common/content_origin/v2";
const AUDIT_RECORD_V2: &str = "https://schemas.shittim.local/audit/audit_record/v2";
const AUDIT_ALLOCATION_V2: &str = "https://schemas.shittim.local/audit/audit_allocation/v2";
const TASK_CREATION_PROVENANCE_V1: &str =
    "https://schemas.shittim.local/task/task_creation_provenance/v1";
const CONTENT_ORIGIN_V1: &str = "https://schemas.shittim.local/v1/common/content_origin.json";
const AUDIT_RECORD_V1: &str = "https://schemas.shittim.local/v1/audit/audit_record.json";
const ACTOR: &str = "https://schemas.shittim.local/v1/common/actor.json";
const ENTRY_POINT: &str = "https://schemas.shittim.local/v1/common/entry_point.json";
const CAUSATION_REF_V2: &str = "https://schemas.shittim.local/common/causation_ref/v2";

#[derive(Debug)]
struct ExpectedRoot {
    id: &'static str,
    title: &'static str,
    component: &'static str,
    kind: &'static str,
    source: &'static str,
    version: u64,
    compatibility: &'static str,
    schema_version_field: Option<&'static str>,
    direct_refs: &'static [&'static str],
}

const SLICE_ROOTS: &[ExpectedRoot] = &[
    ExpectedRoot {
        id: AUDIT_ALLOCATION_V2,
        title: "AuditAllocationV2",
        component: "audit",
        kind: "object",
        source: "schemas/source/audit/audit_allocation.v2.json",
        version: 2,
        compatibility: "new-contract",
        schema_version_field: None,
        direct_refs: &[CAUSATION_REF_V2],
    },
    ExpectedRoot {
        id: AUDIT_RECORD_V2,
        title: "AuditRecordV2",
        component: "audit",
        kind: "domain_object",
        source: "schemas/source/audit/audit_record.v2.json",
        version: 2,
        compatibility: "breaking-replacement",
        schema_version_field: Some("schema_version"),
        direct_refs: &[ACTOR, CAUSATION_REF_V2, ENTRY_POINT],
    },
    ExpectedRoot {
        id: CONTENT_ORIGIN_V2,
        title: "ContentOriginV2",
        component: "common",
        kind: "domain_object",
        source: "schemas/source/common/content_origin.v2.json",
        version: 2,
        compatibility: "breaking-replacement",
        schema_version_field: Some("schema_version"),
        direct_refs: &[ENTRY_POINT],
    },
    ExpectedRoot {
        id: TASK_CREATION_PROVENANCE_V1,
        title: "TaskCreationProvenanceV1",
        component: "task",
        kind: "domain_object",
        source: "schemas/source/task/task_creation_provenance.v1.json",
        version: 1,
        compatibility: "new-contract",
        schema_version_field: Some("schema_version"),
        direct_refs: &[ACTOR, ENTRY_POINT],
    },
];

fn catalog() -> SchemaCatalog {
    SchemaCatalog::load_embedded().expect("embedded catalog")
}

fn document(schema_id: &str) -> Value {
    catalog()
        .document(schema_id)
        .cloned()
        .unwrap_or_else(|| panic!("missing document {schema_id}"))
}

fn sample_actor() -> Value {
    json!({
        "schema_version": 1,
        "revision": 3,
        "id": "actor-local-user-1",
        "kind": "known_user",
        "source": "actor-source://local/desktop",
        "authentication_level": "platform_verified",
        "confidence": 0.9
    })
}

fn sample_content_origin_v2() -> Value {
    json!({
        "schema_version": 2,
        "id": uuid(1),
        "kind": "user_input",
        "entry_point": "local_desktop",
        "source_uri": "https://example.com/input",
        "upstream_stable_id": null,
        "producer_ref": {
            "kind": "actor",
            "id": "actor-local-user-1"
        },
        "received_at": "2026-07-20T08:00:00Z",
        "carrier_ref": {
            "kind": "command_request",
            "id": uuid(2)
        },
        "parent_origin_refs": [uuid(3), uuid(3)],
        "kernel_receipt": {
            "receipt_id": uuid(4),
            "content_hash": hash('a'),
            "recorded_at": "2026-07-20T08:00:00Z"
        }
    })
}

fn sample_task_creation_context() -> Value {
    json!({
        "task_revision": 1,
        "goal": "create a root task",
        "origin_ref": uuid(1),
        "proposer": "user",
        "creation_provenance_ref": uuid(5),
        "creation_kind": "root_command_v2",
        "accepted_at": "2026-07-20T08:00:00Z",
        "materialized_at": null
    })
}

fn sample_policy_context() -> Value {
    json!({
        "matched_rule_ref": "policy-rule-1",
        "policy_set_revision": 4,
        "permission_decision_revision": 2,
        "material_authorization_fingerprint": hash('b'),
        "observation_evidence_fingerprint": hash('c'),
        "reused_approval_resolution_ref": null,
        "child_task_delta_hash": null,
        "authentication_evidence_refs": []
    })
}

fn sample_audit_record_v2() -> Value {
    json!({
        "id": uuid(6),
        "schema_version": 2,
        "audit_type": "task.creation_recorded",
        "level": "user_activity",
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "occurred_at": "2026-07-20T08:00:00Z",
        "task_id": uuid(7),
        "task_creation_context": sample_task_creation_context(),
        "action_id": null,
        "permission_decision_ref": null,
        "approval_resolution_ref": null,
        "recovery_attempt_ref": null,
        "delegation_ref": null,
        "model_call_refs": [],
        "payload_manifest_refs": [],
        "external_content_status": "not_sent",
        "verification_result_refs": [],
        "content_origin_refs": [uuid(1)],
        "artifact_refs": [],
        "resource_refs": [],
        "extension_id": null,
        "provider_id": null,
        "causation_ref": {
            "kind": "command_request",
            "id": uuid(2)
        },
        "correlation_id": "corr-root-1",
        "rollback_capability": "unknown",
        "stop_fence_generation": null,
        "policy_context": null,
        "outcome": "succeeded",
        "reason_codes": ["task_created_root_v2"],
        "summary": null,
        "details": {}
    })
}

fn sample_audit_allocation_v2() -> Value {
    json!({
        "audit_record_id": uuid(6),
        "correlation_id": "corr-audit-1",
        "occurred_at": "2026-07-20T08:00:00Z",
        "causation_ref": {
            "kind": "command_request",
            "id": uuid(2)
        }
    })
}

fn sample_root_provenance() -> Value {
    json!({
        "id": uuid(5),
        "schema_version": 1,
        "kind": "root_command_v2",
        "command_request_id": uuid(2),
        "entry_point": "local_desktop",
        "actor": sample_actor(),
        "receipt_ref": uuid(4),
        "parent_task_id": null,
        "action_id": null,
        "accepted_at": "2026-07-20T08:00:00Z",
        "materialized_at": null
    })
}

fn sample_child_provenance() -> Value {
    json!({
        "id": uuid(8),
        "schema_version": 1,
        "kind": "child_action_v2",
        "command_request_id": null,
        "entry_point": null,
        "actor": null,
        "receipt_ref": null,
        "parent_task_id": uuid(9),
        "action_id": uuid(10),
        "permission_decision_ref": uuid(11),
        "approval_resolution_ref": null,
        "verification_result_ref": uuid(12),
        "proposal_hash": hash('d'),
        "child_task_delta_hash": hash('e'),
        "accepted_at": "2026-07-20T07:59:00Z",
        "materialized_at": "2026-07-20T08:00:00Z"
    })
}

fn uuid(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02}")
}

fn hash(character: char) -> String {
    std::iter::repeat_n(character, 64).collect()
}

fn assert_valid(schema_id: &str, value: &Value) {
    validate_json(schema_id, value)
        .unwrap_or_else(|error| panic!("expected valid {schema_id}: {error}; value={value}"));
}

fn assert_invalid(schema_id: &str, value: &Value, reason: &str) {
    assert!(
        validate_json(schema_id, value).is_err(),
        "expected invalid ({reason}) for {schema_id}: {value}"
    );
}

fn assert_round_trip<T>(schema_id: &str, value: &Value)
where
    T: DeserializeOwned + Serialize,
{
    let typed: T = decode_validated(schema_id, value)
        .unwrap_or_else(|error| panic!("typed decode {schema_id}: {error}"));
    let encoded = serde_json::to_value(typed).expect("serialize generated type");
    assert_valid(schema_id, &encoded);
    assert_eq!(
        canonical_json_bytes(value).expect("canonical input"),
        canonical_json_bytes(&encoded).expect("canonical encoded"),
        "typed round-trip must preserve the exact JSON fact"
    );
}

fn collect_direct_root_refs(value: &Value, refs: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                if !reference.contains('#') {
                    refs.insert(reference.to_owned());
                }
            }
            for child in object.values() {
                collect_direct_root_refs(child, refs);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_direct_root_refs(item, refs);
            }
        }
        _ => {}
    }
}

#[test]
fn slice_roots_match_manifest_identity_refs_and_component_dag() {
    let manifest: Value = serde_json::from_str(include_str!("../../../../schemas/manifest.json"))
        .expect("manifest json");
    let entries = manifest["schemas"].as_array().expect("manifest schemas");
    assert!(
        entries.len() >= 70,
        "production baseline plus optional synthetic probe roots after slice 1b"
    );
    assert_eq!(
        manifest["method_version_bindings"]
            .as_array()
            .expect("bindings")
            .len(),
        8,
        "slice 3a production MethodVersionBindings equal IC §13.5 eight-method set"
    );

    let components = manifest["components"].as_array().expect("components");
    let allowed = |name: &str| {
        components
            .iter()
            .find(|component| component["name"] == name)
            .expect("component")["allowed_refs"]
            .clone()
    };
    assert_eq!(allowed("common"), json!([]));
    assert_eq!(allowed("policy"), json!(["common"]));
    assert_eq!(allowed("audit"), json!(["common", "policy"]));
    assert_eq!(allowed("task"), json!(["common", "policy"]));

    for expected in SLICE_ROOTS {
        let entry = entries
            .iter()
            .find(|entry| entry["id"] == expected.id)
            .unwrap_or_else(|| panic!("missing manifest entry {}", expected.id));
        assert_eq!(entry["title"], expected.title);
        assert_eq!(entry["component"], expected.component);
        assert_eq!(entry["kind"], expected.kind);
        assert_eq!(entry["source"], expected.source);
        assert_eq!(entry["version"], expected.version);
        assert_eq!(entry["compatibility"], expected.compatibility);
        assert_eq!(
            entry["schema_version_field"].as_str(),
            expected.schema_version_field
        );
        assert_eq!(entry["generation_targets"], json!(["rust"]));

        let doc = document(expected.id);
        assert_eq!(doc["$id"], expected.id);
        assert_eq!(doc["title"], expected.title);
        let mut refs = BTreeSet::new();
        collect_direct_root_refs(&doc, &mut refs);
        assert_eq!(
            refs,
            expected
                .direct_refs
                .iter()
                .map(|reference| (*reference).to_owned())
                .collect(),
            "direct refs for {}",
            expected.id
        );
    }
}

#[test]
fn generated_root_types_decode_round_trip_and_provenance_is_tagged_union() {
    assert_round_trip::<ContentOriginV2>(CONTENT_ORIGIN_V2, &sample_content_origin_v2());
    assert_round_trip::<AuditRecordV2>(AUDIT_RECORD_V2, &sample_audit_record_v2());
    assert_round_trip::<AuditAllocationV2>(AUDIT_ALLOCATION_V2, &sample_audit_allocation_v2());
    assert_round_trip::<TaskCreationProvenanceV1>(
        TASK_CREATION_PROVENANCE_V1,
        &sample_root_provenance(),
    );
    assert_round_trip::<TaskCreationProvenanceV1>(
        TASK_CREATION_PROVENANCE_V1,
        &sample_child_provenance(),
    );

    let generated = include_str!("../src/generated/types.rs");
    assert!(generated.contains("pub struct ContentOriginV2"));
    assert!(generated.contains("pub struct AuditRecordV2"));
    assert!(generated.contains("pub struct AuditAllocationV2"));
    assert!(generated.contains("pub enum TaskCreationProvenanceV1"));
    assert!(generated.contains("#[serde(tag = \"kind\", deny_unknown_fields)]"));
    assert!(!generated.contains("pub struct TaskCreationProvenanceV1 {"));
}

#[test]
fn content_origin_v2_exact_wire_and_v1_boundary() {
    let valid = sample_content_origin_v2();
    assert_valid(CONTENT_ORIGIN_V2, &valid);

    assert_required_field_matrix(CONTENT_ORIGIN_V2, &valid);
    assert_all_present_fields_reject_wrong_type(CONTENT_ORIGIN_V2, &valid);
    assert_extra_and_type_fail(CONTENT_ORIGIN_V2, &valid, "received_at");

    for kind in ["command_request", "task", "artifact", "event", "action"] {
        let mut branch = valid.clone();
        branch["carrier_ref"]["kind"] = json!(kind);
        assert_valid(CONTENT_ORIGIN_V2, &branch);
    }
    let mut transition = valid.clone();
    transition["carrier_ref"] = json!({
        "kind": "action_transition",
        "action_id": uuid(9),
        "transition_id": uuid(10)
    });
    assert_invalid(
        CONTENT_ORIGIN_V2,
        &transition,
        "transition anchor is not a content carrier",
    );

    let mut empty_carrier_id = valid.clone();
    empty_carrier_id["carrier_ref"]["id"] = json!("");
    assert_invalid(CONTENT_ORIGIN_V2, &empty_carrier_id, "empty carrier id");

    let mut uppercase_parent = valid.clone();
    uppercase_parent["parent_origin_refs"] = json!(["AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"]);
    assert_invalid(CONTENT_ORIGIN_V2, &uppercase_parent, "canonical UUID");

    let mut v1_shape = valid.clone();
    v1_shape["schema_version"] = json!(1);
    v1_shape["carrier_ref"]["kind"] = json!("event");
    assert_valid(CONTENT_ORIGIN_V1, &v1_shape);
    assert_invalid(CONTENT_ORIGIN_V2, &v1_shape, "v1 version cannot pass v2");

    let mut action_carrier = valid;
    action_carrier["carrier_ref"]["kind"] = json!("action");
    assert_invalid(
        CONTENT_ORIGIN_V1,
        &action_carrier,
        "v2 action carrier must not leak into retained v1",
    );
}

#[test]
fn audit_record_v2_exact_wire_conditions_and_v1_boundary() {
    let valid = sample_audit_record_v2();
    assert_valid(AUDIT_RECORD_V2, &valid);
    assert_required_field_matrix(AUDIT_RECORD_V2, &valid);
    assert_all_present_fields_reject_wrong_type(AUDIT_RECORD_V2, &valid);
    assert_extra_and_type_fail(AUDIT_RECORD_V2, &valid, "occurred_at");

    let expected_types = [
        "task.creation_recorded",
        "command.accepted",
        "permission.evaluated",
        "kernel.invariant_blocked",
        "event.published",
        "recovery.recorded",
        "config.changed",
        "approval.requested",
        "approval.resolved",
        "approval.invalidated",
        "identity.challenge_expired",
        "identity.credential_registered",
        "identity.credential_rotated",
        "identity.credential_revoked",
        "identity.local_presence_recorded",
        "identity.system_authentication_recorded",
    ];
    let audit_document = document(AUDIT_RECORD_V2);
    let actual_types: Vec<_> = audit_document["properties"]["audit_type"]["enum"]
        .as_array()
        .expect("audit enum")
        .iter()
        .map(|value| value.as_str().expect("string enum"))
        .collect();
    assert_eq!(actual_types, expected_types);

    let mut unknown_type = valid.clone();
    unknown_type["audit_type"] = json!("approval.changed");
    assert_invalid(AUDIT_RECORD_V2, &unknown_type, "closed audit type");

    let mut non_creation_context = valid.clone();
    non_creation_context["audit_type"] = json!("command.accepted");
    assert_invalid(
        AUDIT_RECORD_V2,
        &non_creation_context,
        "only task creation carries creation context",
    );

    let mut missing_creation_context = valid.clone();
    missing_creation_context["task_creation_context"] = Value::Null;
    assert_invalid(
        AUDIT_RECORD_V2,
        &missing_creation_context,
        "creation requires context",
    );

    let mut permission_without_policy = valid.clone();
    permission_without_policy["permission_decision_ref"] = json!(uuid(11));
    assert_invalid(
        AUDIT_RECORD_V2,
        &permission_without_policy,
        "permission decision requires policy context",
    );
    permission_without_policy["policy_context"] = sample_policy_context();
    assert_valid(AUDIT_RECORD_V2, &permission_without_policy);

    let mut not_sent_manifest = valid.clone();
    not_sent_manifest["payload_manifest_refs"] = json!(["manifest-1"]);
    assert_invalid(
        AUDIT_RECORD_V2,
        &not_sent_manifest,
        "not_sent forbids payload manifests",
    );

    let mut unknown_without_reason = valid.clone();
    unknown_without_reason["external_content_status"] = json!("unknown");
    unknown_without_reason["reason_codes"] = json!([]);
    assert_invalid(
        AUDIT_RECORD_V2,
        &unknown_without_reason,
        "unknown requires reason",
    );

    let mut null_actor = valid.clone();
    null_actor["actor"] = Value::Null;
    assert_invalid(AUDIT_RECORD_V2, &null_actor, "non-system actor required");
    null_actor["entry_point"] = json!("system_internal");
    assert_valid(AUDIT_RECORD_V2, &null_actor);

    let mut v1_only_field = valid.clone();
    v1_only_field
        .as_object_mut()
        .expect("object")
        .insert("approval_record_ref".into(), Value::Null);
    assert_invalid(
        AUDIT_RECORD_V2,
        &v1_only_field,
        "retained v1 approval field cannot leak into v2",
    );

    let mut v2_field_in_v1 = retained_v1_audit_sample();
    v2_field_in_v1
        .as_object_mut()
        .expect("object")
        .insert("approval_resolution_ref".into(), Value::Null);
    assert_invalid(
        AUDIT_RECORD_V1,
        &v2_field_in_v1,
        "v2 approval field cannot leak into retained v1",
    );
}

#[test]
fn task_creation_provenance_union_branch_matrix() {
    let root = sample_root_provenance();
    let child = sample_child_provenance();
    assert_valid(TASK_CREATION_PROVENANCE_V1, &root);
    assert_valid(TASK_CREATION_PROVENANCE_V1, &child);
    assert_branch_required_field_matrix(TASK_CREATION_PROVENANCE_V1, 0, &root);
    assert_branch_required_field_matrix(TASK_CREATION_PROVENANCE_V1, 1, &child);
    assert_all_present_fields_reject_wrong_type(TASK_CREATION_PROVENANCE_V1, &root);
    assert_all_present_fields_reject_wrong_type(TASK_CREATION_PROVENANCE_V1, &child);

    let mut unknown_kind = root.clone();
    unknown_kind["kind"] = json!("legacy_direct_create_v1");
    assert_invalid(
        TASK_CREATION_PROVENANCE_V1,
        &unknown_kind,
        "legacy provenance removed",
    );

    let mut root_with_child_fact = root.clone();
    root_with_child_fact["parent_task_id"] = json!(uuid(9));
    assert_invalid(
        TASK_CREATION_PROVENANCE_V1,
        &root_with_child_fact,
        "root parent must be null",
    );

    let mut root_without_actor = root.clone();
    root_without_actor["actor"] = Value::Null;
    assert_invalid(
        TASK_CREATION_PROVENANCE_V1,
        &root_without_actor,
        "root actor required",
    );

    let mut child_with_command = child.clone();
    child_with_command["command_request_id"] = json!(uuid(2));
    assert_invalid(
        TASK_CREATION_PROVENANCE_V1,
        &child_with_command,
        "child command ref must be null",
    );

    let mut child_missing_hash = child.clone();
    child_missing_hash
        .as_object_mut()
        .expect("object")
        .remove("proposal_hash");
    assert_invalid(
        TASK_CREATION_PROVENANCE_V1,
        &child_missing_hash,
        "child proposal hash required",
    );

    let mut child_bad_hash = child.clone();
    child_bad_hash["child_task_delta_hash"] = json!("not-a-hash");
    assert_invalid(
        TASK_CREATION_PROVENANCE_V1,
        &child_bad_hash,
        "child delta hash",
    );

    let mut mixed_branch = root.clone();
    mixed_branch["permission_decision_ref"] = json!(uuid(11));
    assert_invalid(
        TASK_CREATION_PROVENANCE_V1,
        &mixed_branch,
        "branch unknown field",
    );

    let document = document(TASK_CREATION_PROVENANCE_V1);
    assert_eq!(
        document["properties"]["kind"]["enum"],
        json!(["root_command_v2", "child_action_v2"])
    );
    assert_eq!(document["oneOf"].as_array().expect("branches").len(), 2);
}

#[test]
fn audit_allocation_is_schema_validated_cross_language_boundary() {
    let valid = sample_audit_allocation_v2();
    assert_valid(AUDIT_ALLOCATION_V2, &valid);
    assert_round_trip::<AuditAllocationV2>(AUDIT_ALLOCATION_V2, &valid);
    assert_required_field_matrix(AUDIT_ALLOCATION_V2, &valid);
    assert_all_present_fields_reject_wrong_type(AUDIT_ALLOCATION_V2, &valid);
    assert_extra_and_type_fail(AUDIT_ALLOCATION_V2, &valid, "occurred_at");

    let mut empty_correlation = valid.clone();
    empty_correlation["correlation_id"] = json!("");
    assert_invalid(
        AUDIT_ALLOCATION_V2,
        &empty_correlation,
        "correlation is non-empty opaque",
    );

    let mut bad_causation = valid.clone();
    bad_causation["causation_ref"] = json!({"kind": "command_request"});
    assert_invalid(AUDIT_ALLOCATION_V2, &bad_causation, "causation branch");

    let mut action_transition = valid;
    action_transition["causation_ref"] = json!({
        "kind": "action_transition",
        "action_id": uuid(10),
        "transition_id": uuid(11)
    });
    assert_valid(AUDIT_ALLOCATION_V2, &action_transition);
}

#[test]
fn jcs_is_stable_for_all_slice_roots() {
    for (schema_id, value) in [
        (CONTENT_ORIGIN_V2, sample_content_origin_v2()),
        (AUDIT_RECORD_V2, sample_audit_record_v2()),
        (AUDIT_ALLOCATION_V2, sample_audit_allocation_v2()),
        (TASK_CREATION_PROVENANCE_V1, sample_root_provenance()),
        (TASK_CREATION_PROVENANCE_V1, sample_child_provenance()),
    ] {
        assert_valid(schema_id, &value);
        let reversed = reverse_object_keys(&value);
        assert_eq!(
            canonical_json_bytes(&value).expect("canonical original"),
            canonical_json_bytes(&reversed).expect("canonical reversed"),
            "object key insertion order must not affect JCS for {schema_id}"
        );
    }

    let origin = sample_content_origin_v2();
    let mut reversed_array = origin.clone();
    reversed_array["parent_origin_refs"] = json!([uuid(5), uuid(3)]);
    assert_ne!(
        canonical_json_bytes(&origin).expect("origin JCS"),
        canonical_json_bytes(&reversed_array).expect("array changed JCS"),
        "JCS must preserve array order"
    );
}

fn assert_required_field_matrix(schema_id: &str, valid: &Value) {
    let document = document(schema_id);
    let required = document["required"]
        .as_array()
        .expect("root required array");
    for field in required.iter().filter_map(Value::as_str) {
        let mut missing = valid.clone();
        missing.as_object_mut().expect("root object").remove(field);
        assert_invalid(schema_id, &missing, &format!("missing {field}"));
    }
}

fn assert_branch_required_field_matrix(schema_id: &str, branch_index: usize, valid: &Value) {
    let document = document(schema_id);
    let required = document["oneOf"][branch_index]["required"]
        .as_array()
        .expect("branch required array");
    for field in required.iter().filter_map(Value::as_str) {
        let mut missing = valid.clone();
        missing.as_object_mut().expect("root object").remove(field);
        assert_invalid(
            schema_id,
            &missing,
            &format!("branch {branch_index} missing {field}"),
        );
    }
}

fn assert_all_present_fields_reject_wrong_type(schema_id: &str, valid: &Value) {
    for (field, original) in valid.as_object().expect("root object") {
        let mut wrong = valid.clone();
        wrong[field] = wrong_json_type(original);
        assert_invalid(schema_id, &wrong, &format!("wrong type for {field}"));
    }
}

fn wrong_json_type(original: &Value) -> Value {
    match original {
        Value::Null => json!(true),
        Value::Bool(_) => json!("not-a-boolean"),
        Value::Number(_) => json!("not-a-number"),
        Value::String(_) => json!(42),
        Value::Array(_) => json!({}),
        Value::Object(_) => json!([]),
    }
}

fn assert_extra_and_type_fail(schema_id: &str, valid: &Value, typed_field: &str) {
    let mut extra = valid.clone();
    extra
        .as_object_mut()
        .expect("root object")
        .insert("unexpected".into(), json!(true));
    assert_invalid(schema_id, &extra, "extra field");

    let mut wrong_type = valid.clone();
    wrong_type[typed_field] = json!(42);
    assert_invalid(schema_id, &wrong_type, "wrong field type");
}

fn reverse_object_keys(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.iter().collect();
            entries.reverse();
            let mut reversed = Map::new();
            for (key, child) in entries {
                reversed.insert(key.clone(), reverse_object_keys(child));
            }
            Value::Object(reversed)
        }
        Value::Array(items) => Value::Array(items.iter().map(reverse_object_keys).collect()),
        other => other.clone(),
    }
}

fn retained_v1_audit_sample() -> Value {
    json!({
        "id": uuid(20),
        "schema_version": 1,
        "audit_type": "command.accepted",
        "level": "operational",
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "occurred_at": "2026-07-20T08:00:00Z",
        "task_id": null,
        "task_creation_context": null,
        "action_id": null,
        "permission_decision_ref": null,
        "approval_record_ref": null,
        "recovery_attempt_ref": null,
        "delegation_ref": null,
        "model_call_refs": [],
        "payload_manifest_refs": [],
        "external_content_status": "not_sent",
        "verification_result_refs": [],
        "content_origin_refs": [],
        "artifact_refs": [],
        "resource_refs": [],
        "extension_id": null,
        "provider_id": null,
        "causation_ref": null,
        "correlation_id": null,
        "rollback_capability": "unknown",
        "stop_fence_generation": null,
        "policy_context": null,
        "outcome": "succeeded",
        "reason_codes": ["accepted"],
        "summary": null,
        "details": {}
    })
}
