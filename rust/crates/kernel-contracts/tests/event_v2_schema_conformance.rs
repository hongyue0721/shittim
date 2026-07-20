//! Event v2 eight-schema, catalog, and typed envelope conformance.

use kernel_contracts::{
    decode_validated, validate_json, ActionStateChangedPayloadV1, ActionTransitionRefV1,
    ApprovalRecordKindV2, ApprovalStateChangedPayloadV1, ApprovalSubjectKindV2, CausationRefV2,
    ConfirmationModeV1, EventEnvelopeV2Payload, EventTypeBinding, SchemaCatalog,
    TypedEventEnvelope, TypedEventEnvelopeV2, EVENT_ACTIVE_BINDINGS, EVENT_ACTIVE_TYPES,
    EVENT_LEGACY_V1_BINDINGS, EVENT_LEGACY_V1_TYPES, METHOD_VERSION_BINDINGS,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug)]
struct ExpectedEventV2Root {
    id: &'static str,
    title: &'static str,
    component: &'static str,
    kind: &'static str,
    source: &'static str,
    version: u32,
    compatibility: &'static str,
    schema_version_field: Option<&'static str>,
    direct_refs: &'static [&'static str],
}

const ACTION_TRANSITION_REF: &str = "https://schemas.shittim.local/common/action_transition_ref/v1";
const CONFIRMATION_MODE: &str = "https://schemas.shittim.local/common/confirmation_mode/v1";
const CAUSATION_REF_V2: &str = "https://schemas.shittim.local/common/causation_ref/v2";
const APPROVAL_RECORD_KIND: &str = "https://schemas.shittim.local/policy/approval_record_kind/v2";
const APPROVAL_SUBJECT_KIND: &str = "https://schemas.shittim.local/policy/approval_subject_kind/v2";
const ACTION_STATE_CHANGED: &str =
    "https://schemas.shittim.local/event/action_state_changed_payload/v1";
const APPROVAL_STATE_CHANGED: &str =
    "https://schemas.shittim.local/event/approval_state_changed_payload/v1";
const EVENT_ENVELOPE_V2: &str = "https://schemas.shittim.local/event/event_envelope/v2";
const EVENT_ENVELOPE_V1: &str = "https://schemas.shittim.local/v1/event/event_envelope.json";

const EVENT_V2_ROOTS: &[ExpectedEventV2Root] = &[
    ExpectedEventV2Root {
        id: ACTION_TRANSITION_REF,
        title: "ActionTransitionRefV1",
        component: "common",
        kind: "object",
        source: "schemas/source/common/action_transition_ref.v1.json",
        version: 1,
        compatibility: "new-contract",
        schema_version_field: None,
        direct_refs: &[],
    },
    ExpectedEventV2Root {
        id: CONFIRMATION_MODE,
        title: "ConfirmationModeV1",
        component: "common",
        kind: "enum",
        source: "schemas/source/common/confirmation_mode.v1.json",
        version: 1,
        compatibility: "new-contract",
        schema_version_field: None,
        direct_refs: &[],
    },
    ExpectedEventV2Root {
        id: CAUSATION_REF_V2,
        title: "CausationRefV2",
        component: "common",
        kind: "object",
        source: "schemas/source/common/causation_ref.v2.json",
        version: 2,
        compatibility: "breaking-replacement",
        schema_version_field: None,
        direct_refs: &[ACTION_TRANSITION_REF],
    },
    ExpectedEventV2Root {
        id: APPROVAL_RECORD_KIND,
        title: "ApprovalRecordKindV2",
        component: "policy",
        kind: "enum",
        source: "schemas/source/policy/approval_record_kind.v2.json",
        version: 2,
        compatibility: "new-contract",
        schema_version_field: None,
        direct_refs: &[],
    },
    ExpectedEventV2Root {
        id: APPROVAL_SUBJECT_KIND,
        title: "ApprovalSubjectKindV2",
        component: "policy",
        kind: "enum",
        source: "schemas/source/policy/approval_subject_kind.v2.json",
        version: 2,
        compatibility: "new-contract",
        schema_version_field: None,
        direct_refs: &[],
    },
    ExpectedEventV2Root {
        id: ACTION_STATE_CHANGED,
        title: "ActionStateChangedPayloadV1",
        component: "event",
        kind: "event_payload",
        source: "schemas/source/event/action_state_changed_payload.v1.json",
        version: 1,
        compatibility: "new-contract",
        schema_version_field: Some("schema_version"),
        direct_refs: &["https://schemas.shittim.local/v1/common/action_status.json"],
    },
    ExpectedEventV2Root {
        id: APPROVAL_STATE_CHANGED,
        title: "ApprovalStateChangedPayloadV1",
        component: "event",
        kind: "event_payload",
        source: "schemas/source/event/approval_state_changed_payload.v1.json",
        version: 1,
        compatibility: "new-contract",
        schema_version_field: Some("schema_version"),
        direct_refs: &[
            CONFIRMATION_MODE,
            APPROVAL_RECORD_KIND,
            APPROVAL_SUBJECT_KIND,
        ],
    },
    ExpectedEventV2Root {
        id: EVENT_ENVELOPE_V2,
        title: "EventEnvelopeV2",
        component: "event",
        kind: "envelope",
        source: "schemas/source/event/event_envelope.v2.json",
        version: 2,
        compatibility: "breaking-replacement",
        schema_version_field: Some("schema_version"),
        direct_refs: &[
            CAUSATION_REF_V2,
            ACTION_STATE_CHANGED,
            APPROVAL_STATE_CHANGED,
            "https://schemas.shittim.local/v1/event/stop_fence_activated_payload.json",
            "https://schemas.shittim.local/v1/event/task_created_payload.json",
            "https://schemas.shittim.local/v1/event/task_state_changed_payload.json",
        ],
    },
];

fn catalog() -> SchemaCatalog {
    SchemaCatalog::load_embedded().expect("embedded catalog")
}

fn document(id: &str) -> Value {
    catalog().document(id).expect("document").clone()
}

fn assert_valid(schema_id: &str, value: &Value) {
    validate_json(schema_id, value).unwrap_or_else(|error| {
        panic!("expected valid {schema_id}: {error}; value={value}");
    });
}

fn assert_invalid(schema_id: &str, value: &Value) {
    assert!(
        validate_json(schema_id, value).is_err(),
        "expected invalid {schema_id}: {value}"
    );
}

fn uuid(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02}")
}

fn sample_action_payload() -> Value {
    json!({
        "schema_version": 1,
        "action_id": uuid(1),
        "task_id": uuid(2),
        "from_status": "approved",
        "to_status": "leased",
        "action_revision": 2,
        "execution_generation": 0,
        "permission_decision_ref": uuid(3),
        "approval_resolution_ref": null,
        "materialized_child_task_ref": null,
        "verification_result_refs": [],
        "reason_code": "lease_granted",
        "changed_at": "2026-07-19T12:00:00Z"
    })
}

fn sample_approval_payload(change_kind: &str) -> Value {
    match change_kind {
        "initial_request" => json!({
            "schema_version": 1,
            "change_kind": "initial_request",
            "approval_chain_id": uuid(1),
            "from_head_ref": null,
            "to_head_ref": uuid(2),
            "from_record_kind": null,
            "to_record_kind": "request",
            "subject_kind": "operation",
            "confirmation_mode": "generic",
            "request_ref": uuid(2),
            "resolution_ref": null,
            "invalidation_ref": null,
            "replacement_request_ref": null,
            "permission_decision_ref": uuid(3),
            "action_id": uuid(4),
            "reason_code": "policy_confirm",
            "changed_at": "2026-07-19T12:00:00Z"
        }),
        "resolution" => json!({
            "schema_version": 1,
            "change_kind": "resolution",
            "approval_chain_id": uuid(1),
            "from_head_ref": uuid(2),
            "to_head_ref": uuid(5),
            "from_record_kind": "request",
            "to_record_kind": "resolution",
            "subject_kind": "operation",
            "confirmation_mode": "local",
            "request_ref": uuid(2),
            "resolution_ref": uuid(5),
            "invalidation_ref": null,
            "replacement_request_ref": null,
            "permission_decision_ref": uuid(3),
            "action_id": uuid(4),
            "reason_code": "user_approved",
            "changed_at": "2026-07-19T12:00:01Z"
        }),
        "invalidation_without_replacement" => json!({
            "schema_version": 1,
            "change_kind": "invalidation_without_replacement",
            "approval_chain_id": uuid(1),
            "from_head_ref": uuid(5),
            "to_head_ref": uuid(6),
            "from_record_kind": "resolution",
            "to_record_kind": "invalidation",
            "subject_kind": "operation",
            "confirmation_mode": "local",
            "request_ref": uuid(2),
            "resolution_ref": uuid(5),
            "invalidation_ref": uuid(6),
            "replacement_request_ref": null,
            "permission_decision_ref": uuid(3),
            "action_id": uuid(4),
            "reason_code": "evidence_stale",
            "changed_at": "2026-07-19T12:00:02Z"
        }),
        "replacement_request" => json!({
            "schema_version": 1,
            "change_kind": "replacement_request",
            "approval_chain_id": uuid(1),
            "from_head_ref": uuid(5),
            "to_head_ref": uuid(7),
            "from_record_kind": "resolution",
            "to_record_kind": "request",
            "subject_kind": "operation",
            "confirmation_mode": "local",
            "request_ref": uuid(2),
            "resolution_ref": uuid(5),
            "invalidation_ref": uuid(6),
            "replacement_request_ref": uuid(7),
            "permission_decision_ref": uuid(3),
            "action_id": uuid(4),
            "reason_code": "reconfirm",
            "changed_at": "2026-07-19T12:00:03Z"
        }),
        other => panic!("unknown change_kind {other}"),
    }
}

fn sample_event_envelope_v2(event_type: &str, payload: Value) -> Value {
    let (aggregate_type, aggregate_id) = match event_type {
        "task.created" | "task.state_changed" => ("task", uuid(10)),
        "action.state_changed" => ("action", uuid(1)),
        "approval.state_changed" => ("approval_chain", uuid(1)),
        "stop_fence.activated" => ("stop_fence", "global".to_string()),
        other => panic!("unknown event type {other}"),
    };
    json!({
        "event_id": uuid(20),
        "type": event_type,
        "schema_version": 2,
        "aggregate_type": aggregate_type,
        "aggregate_id": aggregate_id,
        "sequence": 0,
        "outbox_position": "1",
        "occurred_at": "2026-07-19T12:00:00Z",
        "causation_ref": {
            "kind": "command_request",
            "id": uuid(21)
        },
        "correlation_id": "corr-1",
        "dedup_key": "dedup-1",
        "payload": payload
    })
}

fn sample_task_created_payload() -> Value {
    json!({
        "schema_version": 1,
        "task_id": uuid(10),
        "status": "running",
        "proposer": "user",
        "goal": "do the thing",
        "task_revision": 1,
        "created_at": "2026-07-19T12:00:00Z"
    })
}

fn sample_stop_payload() -> Value {
    json!({
        "schema_version": 1,
        "generation": 1,
        "reason": "manual",
        "activated_by_actor_id": "actor-1",
        "activated_from_entry_point": "local_desktop",
        "activated_at": "2026-07-19T12:00:00Z"
    })
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
fn event_v2_eight_roots_match_manifest_and_embedded_documents_in_production_or_synthetic_catalog() {
    let manifest: Value = serde_json::from_str(include_str!("../../../../schemas/manifest.json"))
        .expect("manifest json");
    let entries = manifest["schemas"].as_array().expect("manifest schemas");
    assert!(
        entries.len() >= 65,
        "production baseline plus optional synthetic probes must contain at least 65 entries"
    );
    assert_eq!(EVENT_V2_ROOTS.len(), 8);

    for expected in EVENT_V2_ROOTS {
        let entry = entries
            .iter()
            .find(|entry| entry["id"].as_str() == Some(expected.id))
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
        assert_eq!(
            doc.get("title").and_then(Value::as_str),
            Some(expected.title)
        );
        assert_eq!(doc.get("$id").and_then(Value::as_str), Some(expected.id));
        let mut actual_refs = BTreeSet::new();
        collect_direct_root_refs(&doc, &mut actual_refs);
        assert_eq!(
            actual_refs,
            expected
                .direct_refs
                .iter()
                .map(|reference| (*reference).to_owned())
                .collect(),
            "direct root refs for {}",
            expected.id
        );
    }
}

#[test]
fn event_catalog_bindings_are_single_source_and_projected() {
    assert_eq!(EVENT_ACTIVE_BINDINGS.len(), 5);
    assert_eq!(EVENT_LEGACY_V1_BINDINGS.len(), 3);
    assert_eq!(EVENT_ACTIVE_TYPES.len(), 5);
    assert_eq!(EVENT_LEGACY_V1_TYPES.len(), 3);
    assert!(METHOD_VERSION_BINDINGS.is_empty());
    for (index, binding) in EVENT_ACTIVE_BINDINGS.iter().enumerate() {
        assert_eq!(EVENT_ACTIVE_TYPES[index], binding.event_type);
        assert!(binding.payload_schema_version >= 1);
    }
    for (index, binding) in EVENT_LEGACY_V1_BINDINGS.iter().enumerate() {
        assert_eq!(EVENT_LEGACY_V1_TYPES[index], binding.event_type);
    }
    assert_eq!(
        EVENT_ACTIVE_BINDINGS
            .iter()
            .map(|binding| binding.event_type)
            .collect::<Vec<_>>(),
        vec![
            "task.created",
            "task.state_changed",
            "action.state_changed",
            "approval.state_changed",
            "stop_fence.activated",
        ]
    );
    assert_eq!(
        EVENT_ACTIVE_BINDINGS[2],
        EventTypeBinding {
            event_type: "action.state_changed",
            aggregate_type: "action",
            payload_schema_id: ACTION_STATE_CHANGED,
            payload_schema_version: 1,
        }
    );
    assert_eq!(EVENT_ACTIVE_BINDINGS[3].aggregate_type, "approval_chain");
    let catalog_source = include_str!("../src/generated/catalog.rs");
    assert!(catalog_source.contains("const fn project_event_types"));
    assert!(!catalog_source.contains("EVENT_V1_TYPES"));
}

#[test]
fn causation_ref_v2_and_action_transition_ref_round_trip() {
    let transition = json!({
        "kind": "action_transition",
        "action_id": uuid(1),
        "transition_id": uuid(2)
    });
    assert_valid(ACTION_TRANSITION_REF, &transition);
    let _: ActionTransitionRefV1 = decode_validated(ACTION_TRANSITION_REF, &transition).unwrap();

    for kind in ["command_request", "event", "action"] {
        let value = json!({"kind": kind, "id": uuid(3)});
        assert_valid(CAUSATION_REF_V2, &value);
        let _: CausationRefV2 = decode_validated(CAUSATION_REF_V2, &value).unwrap();
    }
    assert_valid(CAUSATION_REF_V2, &transition);
    let _: CausationRefV2 = decode_validated(CAUSATION_REF_V2, &transition).unwrap();

    assert_invalid(CAUSATION_REF_V2, &json!({"kind": "unknown", "id": uuid(3)}));
    assert_invalid(
        CAUSATION_REF_V2,
        &json!({"kind": "command_request", "id": uuid(3), "extra": true}),
    );
    assert_invalid(
        ACTION_TRANSITION_REF,
        &json!({
            "kind": "action_transition",
            "action_id": uuid(1),
            "transition_id": uuid(2),
            "action_revision": 1
        }),
    );
}

#[test]
fn confirmation_and_approval_enums_are_closed() {
    for mode in [
        "generic",
        "local",
        "system_authentication",
        "remote_signature",
        "plan_revision",
    ] {
        assert_valid(CONFIRMATION_MODE, &json!(mode));
        let _: ConfirmationModeV1 = serde_json::from_value(json!(mode)).unwrap();
    }
    assert_invalid(CONFIRMATION_MODE, &json!("owner_only"));

    for kind in ["request", "resolution", "invalidation"] {
        assert_valid(APPROVAL_RECORD_KIND, &json!(kind));
        let _: ApprovalRecordKindV2 = serde_json::from_value(json!(kind)).unwrap();
    }
    for kind in ["operation", "task_proposal", "plan_revision"] {
        assert_valid(APPROVAL_SUBJECT_KIND, &json!(kind));
        let _: ApprovalSubjectKindV2 = serde_json::from_value(json!(kind)).unwrap();
    }
}

#[test]
fn action_state_changed_payload_conditions() {
    let valid = sample_action_payload();
    assert_valid(ACTION_STATE_CHANGED, &valid);
    let _: ActionStateChangedPayloadV1 =
        decode_validated(ACTION_STATE_CHANGED, &valid).expect("typed action payload");

    let mut completed = sample_action_payload();
    completed["to_status"] = json!("completed");
    completed["verification_result_refs"] = json!([uuid(8)]);
    assert_valid(ACTION_STATE_CHANGED, &completed);

    let mut completed_empty = sample_action_payload();
    completed_empty["to_status"] = json!("completed");
    completed_empty["verification_result_refs"] = json!([]);
    assert_invalid(ACTION_STATE_CHANGED, &completed_empty);

    let mut child = sample_action_payload();
    child["to_status"] = json!("completed");
    child["materialized_child_task_ref"] = json!(uuid(9));
    child["verification_result_refs"] = json!([uuid(8)]);
    assert_valid(ACTION_STATE_CHANGED, &child);

    let mut approval_without_pd = sample_action_payload();
    approval_without_pd["approval_resolution_ref"] = json!(uuid(7));
    approval_without_pd["permission_decision_ref"] = Value::Null;
    assert_invalid(ACTION_STATE_CHANGED, &approval_without_pd);

    let mut duplicate_verification = sample_action_payload();
    duplicate_verification["to_status"] = json!("failed");
    duplicate_verification["verification_result_refs"] = json!([uuid(8), uuid(8)]);
    assert_invalid(ACTION_STATE_CHANGED, &duplicate_verification);
}

#[test]
fn approval_state_changed_change_kind_truth_table() {
    for change_kind in [
        "initial_request",
        "resolution",
        "invalidation_without_replacement",
        "replacement_request",
    ] {
        let value = sample_approval_payload(change_kind);
        assert_valid(APPROVAL_STATE_CHANGED, &value);
        let typed: ApprovalStateChangedPayloadV1 =
            decode_validated(APPROVAL_STATE_CHANGED, &value).expect("typed approval payload");
        let encoded = serde_json::to_value(typed).unwrap();
        assert_valid(APPROVAL_STATE_CHANGED, &encoded);
    }

    // initial and replacement both to_record_kind=request remain unambiguous via change_kind.
    let mut ambiguous = sample_approval_payload("initial_request");
    ambiguous["change_kind"] = json!("replacement_request");
    assert_invalid(APPROVAL_STATE_CHANGED, &ambiguous);

    let mut bad_resolution = sample_approval_payload("resolution");
    bad_resolution["from_record_kind"] = json!("resolution");
    assert_invalid(APPROVAL_STATE_CHANGED, &bad_resolution);

    let mut missing_invalidation = sample_approval_payload("invalidation_without_replacement");
    missing_invalidation["invalidation_ref"] = Value::Null;
    assert_invalid(APPROVAL_STATE_CHANGED, &missing_invalidation);
}

#[test]
fn event_envelope_v2_positive_and_negative_mappings() {
    let task_created = sample_event_envelope_v2("task.created", sample_task_created_payload());
    assert_valid(EVENT_ENVELOPE_V2, &task_created);
    let action = sample_event_envelope_v2("action.state_changed", sample_action_payload());
    assert_valid(EVENT_ENVELOPE_V2, &action);
    let approval = sample_event_envelope_v2(
        "approval.state_changed",
        sample_approval_payload("initial_request"),
    );
    assert_valid(EVENT_ENVELOPE_V2, &approval);
    let stop = sample_event_envelope_v2("stop_fence.activated", sample_stop_payload());
    assert_valid(EVENT_ENVELOPE_V2, &stop);

    let typed = TypedEventEnvelopeV2::decode(action.clone()).expect("typed v2");
    match typed.payload {
        EventEnvelopeV2Payload::ActionStateChanged(_) => {}
        other => panic!("unexpected payload variant: {other:?}"),
    }

    let mut wrong_aggregate = action.clone();
    wrong_aggregate["aggregate_type"] = json!("task");
    assert_invalid(EVENT_ENVELOPE_V2, &wrong_aggregate);

    let mut wrong_payload = action.clone();
    wrong_payload["payload"] = sample_task_created_payload();
    assert_invalid(EVENT_ENVELOPE_V2, &wrong_payload);

    let mut wrong_type = action.clone();
    wrong_type["type"] = json!("task.unknown");
    assert_invalid(EVENT_ENVELOPE_V2, &wrong_type);

    let mut stop_wrong_id = stop.clone();
    stop_wrong_id["aggregate_id"] = json!("not-global");
    assert_invalid(EVENT_ENVELOPE_V2, &stop_wrong_id);

    let mut bad_causation = action.clone();
    bad_causation["causation_ref"] = json!({"kind": "event"});
    assert_invalid(EVENT_ENVELOPE_V2, &bad_causation);

    let mut schema_version_one = action.clone();
    schema_version_one["schema_version"] = json!(1);
    assert_invalid(EVENT_ENVELOPE_V2, &schema_version_one);
}

#[test]
fn legacy_event_envelope_v1_typed_decode_still_works() {
    let event = json!({
        "event_id": uuid(20),
        "type": "task.created",
        "schema_version": 1,
        "aggregate_type": "task",
        "aggregate_id": uuid(10),
        "sequence": 0,
        "outbox_position": "1",
        "occurred_at": "2026-07-19T12:00:00Z",
        "causation_ref": {"kind": "command_request", "id": "command-1"},
        "correlation_id": "corr-1",
        "dedup_key": "dedup-1",
        "payload": sample_task_created_payload()
    });
    assert_valid(EVENT_ENVELOPE_V1, &event);
    let decoded = TypedEventEnvelope::decode(event).expect("legacy typed");
    assert_eq!(decoded.type_, "task.created");
}

#[test]
fn generated_symbols_exist_and_no_event_v1_types_alias() {
    let _ = std::any::type_name::<TypedEventEnvelopeV2>();
    let _ = std::any::type_name::<EventEnvelopeV2Payload>();
    let _ = std::any::type_name::<ActionStateChangedPayloadV1>();
    let _ = std::any::type_name::<ApprovalStateChangedPayloadV1>();
    let _ = std::any::type_name::<CausationRefV2>();
    let catalog_source = include_str!("../src/generated/catalog.rs");
    let typed_source = include_str!("../src/generated/typed.rs");
    assert!(typed_source.contains("pub struct TypedEventEnvelopeV2"));
    assert!(typed_source.contains("pub enum EventEnvelopeV2Payload"));
    assert!(catalog_source.contains("pub const EVENT_ACTIVE_BINDINGS"));
    assert!(!catalog_source.contains("EVENT_V1_TYPES"));
}
