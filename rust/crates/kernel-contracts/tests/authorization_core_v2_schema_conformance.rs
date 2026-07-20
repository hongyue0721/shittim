//! V2InitialBuildActive slice 1c-i authorization core Schema conformance.

use kernel_contracts::{
    canonical_json_bytes, decode_validated, validate_json, ApprovalEventAllocationV1,
    ApprovalRecordV2, PermissionDecisionV2, PolicyRuleV2, SchemaCatalog, SubjectProjectionV1,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const PD: &str = "https://schemas.shittim.local/policy/permission_decision/v2";
const RULE: &str = "https://schemas.shittim.local/policy/policy_rule/v2";
const APPROVAL: &str = "https://schemas.shittim.local/policy/approval_record/v2";
const SUBJECT: &str = "https://schemas.shittim.local/policy/subject_projection/v1";
const ALLOCATION: &str = "https://schemas.shittim.local/policy/approval_event_allocation/v1";
const ACTOR: &str = "https://schemas.shittim.local/v1/common/actor.json";
const ENTRY: &str = "https://schemas.shittim.local/v1/common/entry_point.json";
const SIDE_EFFECT: &str = "https://schemas.shittim.local/v1/common/side_effect_class.json";
const CONFIRMATION: &str = "https://schemas.shittim.local/common/confirmation_mode/v1";
const CAUSATION: &str = "https://schemas.shittim.local/common/causation_ref/v2";

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

fn operation_subject() -> Value {
    json!({
        "subject_kind": "operation",
        "task_id": uuid(1),
        "task_revision": 2,
        "task_plan_version": 1,
        "action_id": uuid(2),
        "action_revision": 3,
        "permission_decision_ref": uuid(3),
        "permission_decision_revision": 4,
        "policy_set_revision": 5,
        "material_authorization_fingerprint": hash('a'),
        "capability_id": "computer.input",
        "operation": "click",
        "side_effect_class": "S2",
        "resource_refs_hash": hash('b'),
        "key_params_hash": hash('c')
    })
}

fn task_proposal_subject() -> Value {
    json!({
        "subject_kind": "task_proposal",
        "candidate_task_id": uuid(4),
        "candidate_revision": 1,
        "proposal_hash": hash('d'),
        "proposer_actor_ref": "actor-local-1",
        "task_scope_hash": hash('e'),
        "delegation_ref": null,
        "policy_set_revision": 5
    })
}

fn plan_subject() -> Value {
    json!({
        "subject_kind": "plan_revision",
        "task_id": uuid(1),
        "task_revision": 2,
        "base_plan_version": 1,
        "proposed_plan_version": 2,
        "proposed_plan_hash": hash('f'),
        "policy_set_revision": 5
    })
}

fn permission_decision() -> Value {
    json!({
        "id": uuid(3),
        "schema_version": 2,
        "action_id": uuid(2),
        "decision": "require_remote_signature",
        "reason_codes": ["protected_surface"],
        "matched_rule_ref": "policy-rule://remote-sensitive",
        "decision_revision": 4,
        "evaluated_at": "2026-07-20T08:00:00Z",
        "policy_set_revision": 5,
        "material_authorization_fingerprint": hash('a'),
        "observation_evidence_fingerprint": hash('9'),
        "binding": {
            "action_id": uuid(2),
            "action_revision": 3,
            "task_id": uuid(1),
            "plan_version": 1,
            "capability_id": "computer.input",
            "operation": "click",
            "side_effect_class": "S2",
            "resource_refs": ["https://example.com/account"],
            "key_params_hash": hash('c'),
            "delegation_authority_ref": null
        },
        "approval_requirement": {
            "confirmation_mode": "remote_signature",
            "approval_chain_id": uuid(5),
            "reusable_resolution_ref": null
        },
        "expires_at": "2026-07-20T08:05:00Z",
        "lease_ref": null
    })
}

fn policy_rule() -> Value {
    json!({
        "id": "policy-rule://remote-sensitive",
        "schema_version": 2,
        "revision": 3,
        "name": "Remote signature for protected input",
        "description": "Requires a remote signature.",
        "priority": 100,
        "enabled": true,
        "actor_match": {"kind": "known_user", "source_patterns": [], "entry_point": "local_desktop", "auth_level_min": "asserted"},
        "content_origin_match": {"kinds": [], "source_patterns": []},
        "resource_match": {"scope_patterns": ["https://example.com/**"], "exclude_patterns": []},
        "action_match": {"capability_ids": ["computer.input"], "operation_patterns": ["click"], "side_effect_max": "S3"},
        "condition": {"delegation_required": false, "local_presence_required": false},
        "effect": "confirm",
        "confirmation_mode": "remote_signature",
        "expires_at": null,
        "created_by": {"actor": actor(), "entry_point": "local_desktop"},
        "updated_by": {"actor": actor(), "entry_point": "local_desktop"},
        "created_at": "2026-07-20T07:00:00Z",
        "updated_at": "2026-07-20T07:30:00Z",
        "source": "user_defined"
    })
}

fn approval_request() -> Value {
    json!({
        "id": uuid(6),
        "schema_version": 2,
        "approval_chain_id": uuid(5),
        "predecessor_ref": null,
        "record_kind": "request",
        "subject": operation_subject(),
        "created_at": "2026-07-20T08:00:00Z",
        "expires_at": "2026-07-20T08:05:00Z",
        "record": {
            "request_id": uuid(6),
            "confirmation_mode": "remote_signature",
            "requested_by_actor": actor(),
            "requested_from_entry_point": "local_desktop",
            "reason_codes": ["protected_surface"],
            "challenge_ref": uuid(7),
            "request_expires_at": "2026-07-20T08:05:00Z"
        }
    })
}

fn approval_resolution() -> Value {
    json!({
        "id": uuid(8),
        "schema_version": 2,
        "approval_chain_id": uuid(5),
        "predecessor_ref": uuid(6),
        "record_kind": "resolution",
        "subject": operation_subject(),
        "created_at": "2026-07-20T08:01:00Z",
        "expires_at": "2026-07-20T08:10:00Z",
        "record": {
            "request_ref": uuid(6),
            "decision": "approved",
            "resolved_by_actor": actor(),
            "resolved_from_entry_point": "personal_remote_channel",
            "resolved_at": "2026-07-20T08:01:00Z",
            "evidence_refs": [uuid(9)],
            "remote_response_ref": uuid(9),
            "local_presence_evidence_ref": null,
            "system_auth_evidence_ref": null
        }
    })
}

fn approval_invalidation() -> Value {
    json!({
        "id": uuid(10),
        "schema_version": 2,
        "approval_chain_id": uuid(5),
        "predecessor_ref": uuid(8),
        "record_kind": "invalidation",
        "subject": operation_subject(),
        "created_at": "2026-07-20T08:02:00Z",
        "expires_at": null,
        "record": {
            "invalidated_record_ref": uuid(8),
            "reason_code": "material_changed",
            "invalidated_at": "2026-07-20T08:02:00Z",
            "invalidated_by_actor": null,
            "invalidated_from_entry_point": "system_internal",
            "replacement_request_ref": null
        }
    })
}

fn allocation() -> Value {
    json!({
        "event_id": uuid(11),
        "correlation_id": "corr-approval-1",
        "dedup_key": "approval-head-5-request-6",
        "changed_at": "2026-07-20T08:00:00Z",
        "causation_ref": {"kind": "command_request", "id": uuid(12)}
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
fn manifest_batch_is_exactly_75_and_bindings_remain_empty() {
    let manifest: Value =
        serde_json::from_str(include_str!("../../../../schemas/manifest.json")).expect("manifest");
    let entries = manifest["schemas"].as_array().expect("schemas");
    assert!(
        entries.len() >= 75,
        "production baseline plus synthetic probe roots"
    );
    assert!(manifest["method_version_bindings"]
        .as_array()
        .expect("bindings")
        .is_empty());
    for id in [PD, RULE, APPROVAL, SUBJECT, ALLOCATION] {
        assert!(entries.iter().any(|entry| entry["id"] == id), "{id}");
    }
}

#[test]
fn typed_round_trips_cover_five_roots_and_all_union_branches() {
    assert_round_trip::<PermissionDecisionV2>(PD, &permission_decision());
    assert_round_trip::<PolicyRuleV2>(RULE, &policy_rule());
    assert_round_trip::<ApprovalRecordV2>(APPROVAL, &approval_request());
    assert_round_trip::<ApprovalRecordV2>(APPROVAL, &approval_resolution());
    assert_round_trip::<ApprovalRecordV2>(APPROVAL, &approval_invalidation());
    for subject in [operation_subject(), task_proposal_subject(), plan_subject()] {
        let mut projection = subject;
        projection["schema_version"] = json!(1);
        assert_round_trip::<SubjectProjectionV1>(SUBJECT, &projection);
    }
    assert_round_trip::<ApprovalEventAllocationV1>(ALLOCATION, &allocation());
}

#[test]
fn required_and_unknown_fields_fail_closed_for_every_root() {
    for (schema, mut value, field) in [
        (
            PD,
            permission_decision(),
            "observation_evidence_fingerprint",
        ),
        (RULE, policy_rule(), "effect"),
        (APPROVAL, approval_request(), "predecessor_ref"),
        (
            SUBJECT,
            {
                let mut v = operation_subject();
                v["schema_version"] = json!(1);
                v
            },
            "task_plan_version",
        ),
        (ALLOCATION, allocation(), "changed_at"),
    ] {
        value.as_object_mut().expect("object").remove(field);
        assert!(
            validate_json(schema, &value).is_err(),
            "{schema} missing {field}"
        );
    }
    for (schema, mut value) in [
        (PD, permission_decision()),
        (RULE, policy_rule()),
        (APPROVAL, approval_request()),
        (SUBJECT, {
            let mut v = operation_subject();
            v["schema_version"] = json!(1);
            v
        }),
        (ALLOCATION, allocation()),
    ] {
        value["unexpected"] = json!(true);
        assert!(validate_json(schema, &value).is_err(), "{schema} unknown");
    }
}

#[test]
fn closed_enums_and_permission_decision_mode_mapping_are_enforced() {
    let mut decision = permission_decision();
    decision["decision"] = json!("require_owner_confirmation");
    assert!(validate_json(PD, &decision).is_err());

    let mut mismatch = permission_decision();
    mismatch["approval_requirement"]["confirmation_mode"] = json!("local");
    assert!(validate_json(PD, &mismatch).is_err());

    let mut allow = permission_decision();
    allow["decision"] = json!("allow");
    assert!(validate_json(PD, &allow).is_err());
    allow["approval_requirement"] = Value::Null;
    assert!(validate_json(PD, &allow).is_ok());

    let mut rule = policy_rule();
    rule["confirmation_mode"] = json!("system_auth");
    assert!(validate_json(RULE, &rule).is_err());
    rule["confirmation_mode"] = json!("remote_signature");
    rule["effect"] = json!("allow");
    assert!(validate_json(RULE, &rule).is_err());
}

#[test]
fn approval_record_and_subject_are_true_closed_tagged_unions() {
    let mut mixed = approval_request();
    mixed["record"]["decision"] = json!("approved");
    assert!(validate_json(APPROVAL, &mixed).is_err());

    let mut wrong_subject = approval_request();
    wrong_subject["subject"]["proposal_hash"] = json!(hash('d'));
    assert!(validate_json(APPROVAL, &wrong_subject).is_err());

    let mut request_without_challenge = approval_request();
    request_without_challenge["record"]["challenge_ref"] = Value::Null;
    assert!(validate_json(APPROVAL, &request_without_challenge).is_err());

    let mut local_request = approval_request();
    local_request["record"]["confirmation_mode"] = json!("local");
    local_request["record"]["challenge_ref"] = Value::Null;
    assert!(validate_json(APPROVAL, &local_request).is_ok());

    let mut invalidation_expiry = approval_invalidation();
    invalidation_expiry["expires_at"] = json!("2026-07-20T08:03:00Z");
    assert!(validate_json(APPROVAL, &invalidation_expiry).is_err());

    let mut generic_resolution = approval_resolution();
    generic_resolution["record"]["evidence_refs"] = json!([]);
    generic_resolution["record"]["remote_response_ref"] = Value::Null;
    assert!(validate_json(APPROVAL, &generic_resolution).is_ok());

    let mut evidence_without_specialized_ref = approval_resolution();
    evidence_without_specialized_ref["record"]["remote_response_ref"] = Value::Null;
    assert!(validate_json(APPROVAL, &evidence_without_specialized_ref).is_err());

    let mut specialized_ref_with_extra_evidence = approval_resolution();
    specialized_ref_with_extra_evidence["record"]["evidence_refs"] = json!([uuid(9), uuid(10)]);
    assert!(validate_json(APPROVAL, &specialized_ref_with_extra_evidence).is_err());

    let mut multiple_specialized_refs = approval_resolution();
    multiple_specialized_refs["record"]["local_presence_evidence_ref"] = json!(uuid(10));
    assert!(validate_json(APPROVAL, &multiple_specialized_refs).is_err());
}

#[test]
fn subject_projection_rejects_noncanonical_identifiers_hashes_and_branch_leakage() {
    let mut projection = operation_subject();
    projection["schema_version"] = json!(1);
    projection["task_id"] = json!("00000000-0000-4000-8000-0000000000AA");
    assert!(validate_json(SUBJECT, &projection).is_err());

    let mut projection = task_proposal_subject();
    projection["schema_version"] = json!(1);
    projection["proposal_hash"] = json!(hash('A'));
    assert!(validate_json(SUBJECT, &projection).is_err());

    let mut projection = plan_subject();
    projection["schema_version"] = json!(1);
    projection["permission_decision_ref"] = json!(uuid(3));
    assert!(validate_json(SUBJECT, &projection).is_err());
}

#[test]
fn allocation_enforces_utc_second_shape_and_closed_causation_union() {
    let mut value = allocation();
    value["changed_at"] = json!("2026-07-20T10:00:00+02:00");
    assert!(validate_json(ALLOCATION, &value).is_err());
    let mut value = allocation();
    value["changed_at"] = json!("2026-07-20T08:00:00.000Z");
    assert!(validate_json(ALLOCATION, &value).is_err());
    let mut value = allocation();
    value["causation_ref"]["kind"] = json!("approval");
    assert!(validate_json(ALLOCATION, &value).is_err());
}

#[test]
fn legacy_v1_field_names_do_not_leak_into_new_roots() {
    for forbidden in [
        "approval_record_ref",
        "approval_type",
        "evaluation_context_hash",
        "granted_scopes",
        "supersedes_ref",
        "resolved_at",
        "current_head_ref",
    ] {
        for (schema, mut value) in [
            (PD, permission_decision()),
            (RULE, policy_rule()),
            (APPROVAL, approval_request()),
            (SUBJECT, {
                let mut v = operation_subject();
                v["schema_version"] = json!(1);
                v
            }),
            (ALLOCATION, allocation()),
        ] {
            value[forbidden] = json!(uuid(13));
            assert!(
                validate_json(schema, &value).is_err(),
                "{schema} leaked {forbidden}"
            );
        }
    }
}

#[test]
fn direct_refs_match_the_component_native_dag() {
    let catalog = SchemaCatalog::load_embedded().expect("catalog");
    let expected = [
        (PD, BTreeSet::from([CONFIRMATION, SIDE_EFFECT])),
        (
            RULE,
            BTreeSet::from([ACTOR, CONFIRMATION, ENTRY, SIDE_EFFECT]),
        ),
        (
            APPROVAL,
            BTreeSet::from([ACTOR, CONFIRMATION, ENTRY, SIDE_EFFECT]),
        ),
        (SUBJECT, BTreeSet::from([SIDE_EFFECT])),
        (ALLOCATION, BTreeSet::from([CAUSATION])),
    ];
    for (schema, expected_refs) in expected {
        let mut refs = BTreeSet::new();
        collect_whole_root_refs(catalog.document(schema).expect("schema"), &mut refs);
        assert_eq!(refs, expected_refs, "{schema}");
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
