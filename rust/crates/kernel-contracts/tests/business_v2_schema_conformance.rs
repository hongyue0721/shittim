//! First-batch task-creation business-v2 Schema/typed conformance.
//!
//! Scope: the historical first task-creation batch of 12 component-native roots
//! remains covered here, while production now also contains the Event v2 eight
//! schema batch (61 total = 41 retained + 20 component-native). Official
//! JCS/hash fixtures are a later independent commit and are intentionally not
//! committed here.

use kernel_contracts::{
    decode_validated, validate_json, ChildTaskMaterializationAllocationV1, ChildTaskProposalV1,
    InputContentOriginV1, InputTaskScopeV1, KcpCommandEnvelopeV2, KcpCommandEnvelopeV2Payload,
    KcpQueryEnvelopeV2, KcpQueryEnvelopeV2Payload, MethodVersionBinding,
    NormalizedChildTaskProposalV1, NormalizedRootTaskCreatePayloadV2, RootTaskCreateAllocationV2,
    RootTaskCreateIdempotencyProjectionV1, SchemaCatalog, TaskCreateRequestV2,
    TaskCreateResponseV2, TypedKcpCommandEnvelope, TypedKcpQueryEnvelope, EVENT_ACTIVE_BINDINGS,
    EVENT_ACTIVE_TYPES, EVENT_LEGACY_V1_BINDINGS, EVENT_LEGACY_V1_TYPES,
    KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS, KCP_ENVELOPE_AUTHORITY_METHODS,
    KCP_ENVELOPE_AUTHORITY_QUERY_METHODS, KCP_LEGACY_V1_METHODS, KCP_PROTOCOL_VERSION,
    METHOD_VERSION_BINDINGS,
};
use kernel_task_creation::{
    validate_child_task_materialization_allocation, validate_root_task_create_allocation,
    ChildTaskMaterializationExternalUuidRefsV1, RootTaskCreateExternalUuidRefsV1,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const INPUT_CONTENT_ORIGIN: &str = "https://schemas.shittim.local/common/input_content_origin/v1";
const INPUT_TASK_SCOPE: &str = "https://schemas.shittim.local/task/input_task_scope/v1";
const TASK_CREATE_REQUEST_V2: &str = "https://schemas.shittim.local/kcp/task_create_request/v2";
const NORMALIZED_ROOT: &str =
    "https://schemas.shittim.local/task/normalized_root_task_create_payload/v2";
const IDEMPOTENCY_PROJECTION: &str =
    "https://schemas.shittim.local/task/root_task_create_idempotency_projection/v1";
const TASK_CREATE_RESPONSE_V2: &str = "https://schemas.shittim.local/kcp/task_create_response/v2";
const ROOT_ALLOCATION: &str = "https://schemas.shittim.local/task/root_task_create_allocation/v2";
const CHILD_PROPOSAL: &str = "https://schemas.shittim.local/task/child_task_proposal/v1";
const NORMALIZED_CHILD: &str =
    "https://schemas.shittim.local/task/normalized_child_task_proposal/v1";
const CHILD_ALLOCATION: &str =
    "https://schemas.shittim.local/task/child_task_materialization_allocation/v1";
const COMMAND_ENVELOPE_V2: &str = "https://schemas.shittim.local/kcp/command_envelope/v2";
const QUERY_ENVELOPE_V2: &str = "https://schemas.shittim.local/kcp/query_envelope/v2";
const TASK_SPEC: &str = "https://schemas.shittim.local/v1/task/task_spec.json";
const ACTOR: &str = "https://schemas.shittim.local/v1/common/actor.json";
const ENTRY_POINT: &str = "https://schemas.shittim.local/v1/common/entry_point.json";

const NINE_CALLER_FIELDS: &[&str] = &[
    "proposer",
    "goal",
    "constraints",
    "success_criteria",
    "risk_hint",
    "capability_hints",
    "task_scope",
    "delegation_ref",
    "origin",
];

const BUSINESS_V2_ROOTS: &[(&str, &str, u32)] = &[
    (INPUT_CONTENT_ORIGIN, "InputContentOriginV1", 1),
    (INPUT_TASK_SCOPE, "InputTaskScopeV1", 1),
    (TASK_CREATE_REQUEST_V2, "TaskCreateRequestV2", 2),
    (NORMALIZED_ROOT, "NormalizedRootTaskCreatePayloadV2", 2),
    (
        IDEMPOTENCY_PROJECTION,
        "RootTaskCreateIdempotencyProjectionV1",
        1,
    ),
    (TASK_CREATE_RESPONSE_V2, "TaskCreateResponseV2", 2),
    (ROOT_ALLOCATION, "RootTaskCreateAllocationV2", 2),
    (CHILD_PROPOSAL, "ChildTaskProposalV1", 1),
    (NORMALIZED_CHILD, "NormalizedChildTaskProposalV1", 1),
    (CHILD_ALLOCATION, "ChildTaskMaterializationAllocationV1", 1),
    (COMMAND_ENVELOPE_V2, "KcpCommandEnvelopeV2", 2),
    (QUERY_ENVELOPE_V2, "KcpQueryEnvelopeV2", 2),
];

fn catalog() -> SchemaCatalog {
    SchemaCatalog::load_embedded().expect("embedded catalog")
}

fn sample_actor() -> Value {
    json!({
        "schema_version": 1,
        "revision": 1,
        "id": "actor-local-user-1",
        "kind": "known_user",
        "source": "actor-source://local/desktop",
        "authentication_level": "unauthenticated",
        "confidence": null
    })
}

fn sample_origin_minimal() -> Value {
    json!({
        "schema_version": 1,
        "kind": "user_input",
        "source_uri": null,
        "upstream_stable_id": null,
        "producer_ref": {"kind": "actor", "id": "actor-local-user-1"},
        "parent_origin_refs": []
    })
}

fn sample_origin_full() -> Value {
    json!({
        "schema_version": 1,
        "kind": "document_content",
        "source_uri": "https://example.com/doc",
        "upstream_stable_id": "upstream-stable-1",
        "producer_ref": {"kind": "extension", "id": "ext-reader"},
        "parent_origin_refs": [
            "11111111-1111-4111-8111-111111111111",
            "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222"
        ]
    })
}

fn sample_scope_minimal() -> Value {
    json!({
        "schema_version": 1,
        "resource_patterns": [],
        "exclusions": [],
        "allowed_capability_hints": [],
        "expires_at": null
    })
}

fn sample_scope_full() -> Value {
    json!({
        "schema_version": 1,
        "resource_patterns": ["file://workspace/**", "file://workspace/**"],
        "exclusions": ["file://workspace/secrets/**"],
        "allowed_capability_hints": ["kernel.task", "fs.read"],
        "expires_at": "2030-01-01T00:00:00Z"
    })
}

fn nine_field_payload(schema_version: u32, full: bool) -> Value {
    let mut payload = json!({
        "schema_version": schema_version,
        "proposer": "user",
        "goal": "create a root task",
        "constraints": if full { json!(["c1", "c1", "c2"]) } else { json!([]) },
        "success_criteria": if full { json!(["done", "done"]) } else { json!([]) },
        "risk_hint": if full { json!("low") } else { Value::Null },
        "capability_hints": if full { json!(["kernel.task", "kernel.task"]) } else { json!([]) },
        "task_scope": if full { sample_scope_full() } else { sample_scope_minimal() },
        "delegation_ref": if full {
            json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
        } else {
            Value::Null
        },
        "origin": if full { sample_origin_full() } else { sample_origin_minimal() }
    });
    // Keep field order deterministic for readability; JSON Schema ignores order.
    let _ = payload.as_object_mut();
    payload
}

fn sample_task_spec() -> Value {
    json!({
        "id": "11111111-1111-4111-8111-111111111111",
        "origin_ref": "22222222-2222-4222-8222-222222222222",
        "actor": sample_actor(),
        "proposer": "user",
        "goal": "created",
        "constraints": [],
        "success_criteria": ["done"],
        "risk_hint": null,
        "capability_hints": ["kernel.task"],
        "delegation_ref": null,
        "task_scope_ref": "33333333-3333-4333-8333-333333333333",
        "parent_task_id": null,
        "status": "planned",
        "plan_version": 0,
        "schema_version": 1,
        "revision": 1,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "failed_recovery_meta": null
    })
}

fn sample_root_allocation_distinct() -> Value {
    json!({
        "schema_version": 2,
        "task_id": "11111111-1111-4111-8111-111111111111",
        "task_scope_id": "22222222-2222-4222-8222-222222222222",
        "content_origin_id": "33333333-3333-4333-8333-333333333333",
        "kernel_receipt_id": "44444444-4444-4444-8444-444444444444",
        "creation_provenance_id": "55555555-5555-4555-8555-555555555555",
        "audit_record_id": "66666666-6666-4666-8666-666666666666",
        "task_created_event_id": "77777777-7777-4777-8777-777777777777",
        "correlation_id": "corr-root-1",
        "task_created_dedup_key": "dedup-root-1"
    })
}

fn sample_child_allocation_distinct() -> Value {
    json!({
        "schema_version": 1,
        "child_task_id": "11111111-1111-4111-8111-111111111111",
        "task_scope_id": "22222222-2222-4222-8222-222222222222",
        "content_origin_id": "33333333-3333-4333-8333-333333333333",
        "kernel_receipt_id": "44444444-4444-4444-8444-444444444444",
        "creation_provenance_id": "55555555-5555-4555-8555-555555555555",
        "verification_result_id": "66666666-6666-4666-8666-666666666666",
        "audit_record_id": "77777777-7777-4777-8777-777777777777",
        "task_created_event_id": "88888888-8888-4888-8888-888888888888",
        "action_state_changed_event_id": "99999999-9999-4999-8999-999999999999",
        "action_transition_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "correlation_id": "corr-child-1",
        "task_created_dedup_key": "dedup-child-task",
        "action_state_changed_dedup_key": "dedup-child-action"
    })
}

fn sample_command_envelope_v2() -> Value {
    json!({
        "protocol_version": "1.0",
        "message_kind": "command",
        "request_id": "11111111-1111-4111-8111-111111111111",
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "auth": null,
        "task_id": null,
        "context": {"trace": "business-v2"},
        "deadline": "2030-01-01T00:00:00Z",
        "idempotency_key": "idem-1",
        "expected_revision": null,
        "command_type": "task.create",
        "payload": nine_field_payload(2, false)
    })
}

fn sample_query_envelope_v2() -> Value {
    json!({
        "protocol_version": "1.0",
        "message_kind": "query",
        "request_id": "11111111-1111-4111-8111-111111111111",
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "auth": null,
        "task_id": null,
        "deadline": "2030-01-01T00:00:00Z",
        "query_type": "system.ping",
        "payload": {"schema_version": 1, "echo": "hi"}
    })
}

fn assert_valid(schema_id: &str, value: &Value) {
    validate_json(schema_id, value)
        .unwrap_or_else(|error| panic!("expected valid for {schema_id}: {error}; value={value}"));
}

fn assert_invalid(schema_id: &str, value: &Value, reason: &str) {
    assert!(
        validate_json(schema_id, value).is_err(),
        "expected invalid ({reason}) for {schema_id}; value={value}"
    );
}

fn document(schema_id: &str) -> Value {
    catalog()
        .document(schema_id)
        .cloned()
        .unwrap_or_else(|| panic!("missing document {schema_id}"))
}

fn assert_validated_round_trip<T>(schema_id: &str, value: &Value)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let typed: T = decode_validated(schema_id, value).unwrap_or_else(|error| {
        panic!("validated decode failed for {schema_id}: {error}; value={value}")
    });
    let encoded = serde_json::to_value(typed).expect("serialize validated type");
    assert_valid(schema_id, &encoded);
}

#[test]
fn embedded_catalog_contains_production_or_probe_and_historical_twelve_task_creation_roots() {
    let catalog = catalog();
    let ids = catalog.schema_ids();
    // Production is exactly 61 = 41 retained + 20 component-native. Synthetic probe
    // repos used by schema-tool tests may temporarily append extra component-native
    // entries; those must not weaken the 12-root identity assertions below.
    assert!(
        ids.len() >= 61,
        "embedded catalog must contain at least production 61 schemas, got {}",
        ids.len()
    );
    let retained_prefix = "https://schemas.shittim.local/v1/";
    let retained = ids
        .iter()
        .filter(|id| id.starts_with(retained_prefix))
        .count();
    assert_eq!(retained, 41, "retained ledger set must remain exactly 41");
    for (id, title, version) in BUSINESS_V2_ROOTS {
        let doc = document(id);
        assert_eq!(doc.get("title").and_then(Value::as_str), Some(*title));
        assert_eq!(doc.get("$id").and_then(Value::as_str), Some(*id));
        if *title == "KcpCommandEnvelopeV2" || *title == "KcpQueryEnvelopeV2" {
            // Envelope roots intentionally have no schema_version field.
            assert!(doc.pointer("/properties/schema_version").is_none());
        } else {
            assert_eq!(
                doc.pointer("/properties/schema_version/const")
                    .and_then(Value::as_u64),
                Some(u64::from(*version))
            );
        }
    }
    assert_eq!(
        BUSINESS_V2_ROOTS.len(),
        12,
        "historical task-creation batch remains 12 roots"
    );
    // Pure production catalogs must still be exact 61. Probe-only extras are allowed
    // only when additional component-native ids are present beyond the production set.
    let production_native_ids: BTreeSet<&str> = [
        "https://schemas.shittim.local/common/action_transition_ref/v1",
        "https://schemas.shittim.local/common/causation_ref/v2",
        "https://schemas.shittim.local/common/confirmation_mode/v1",
        "https://schemas.shittim.local/common/input_content_origin/v1",
        "https://schemas.shittim.local/event/action_state_changed_payload/v1",
        "https://schemas.shittim.local/event/approval_state_changed_payload/v1",
        "https://schemas.shittim.local/event/event_envelope/v2",
        "https://schemas.shittim.local/kcp/command_envelope/v2",
        "https://schemas.shittim.local/kcp/query_envelope/v2",
        "https://schemas.shittim.local/kcp/task_create_request/v2",
        "https://schemas.shittim.local/kcp/task_create_response/v2",
        "https://schemas.shittim.local/policy/approval_record_kind/v2",
        "https://schemas.shittim.local/policy/approval_subject_kind/v2",
        "https://schemas.shittim.local/task/child_task_materialization_allocation/v1",
        "https://schemas.shittim.local/task/child_task_proposal/v1",
        "https://schemas.shittim.local/task/input_task_scope/v1",
        "https://schemas.shittim.local/task/normalized_child_task_proposal/v1",
        "https://schemas.shittim.local/task/normalized_root_task_create_payload/v2",
        "https://schemas.shittim.local/task/root_task_create_allocation/v2",
        "https://schemas.shittim.local/task/root_task_create_idempotency_projection/v1",
    ]
    .into_iter()
    .collect();
    let extra_native = ids
        .iter()
        .filter(|id| {
            !id.starts_with(retained_prefix) && !production_native_ids.contains(id.as_str())
        })
        .count();
    if extra_native == 0 {
        assert_eq!(ids.len(), 61, "pure production catalog must be exactly 61");
    }
}

#[test]
fn retained_public_types_and_typed_v1_wrappers_remain_usable() {
    // Compile-time/use-site proof that retained typed wrappers still exist.
    let _ = std::any::type_name::<TypedKcpCommandEnvelope>();
    let _ = std::any::type_name::<TypedKcpQueryEnvelope>();
    let _ = std::any::type_name::<MethodVersionBinding>();
    assert!(std::any::type_name::<TypedKcpCommandEnvelope>().contains("TypedKcpCommandEnvelope"));
    assert!(!std::any::type_name::<TypedKcpCommandEnvelope>().contains("V2"));
}

#[test]
fn generated_v2_envelope_payload_schema_version_is_exactly_u32() {
    fn assert_u32(_: u32) {}

    let command = KcpCommandEnvelopeV2Payload {
        schema_version: u32::MAX,
        additional_properties: serde_json::Map::new(),
    };
    let query = KcpQueryEnvelopeV2Payload {
        schema_version: u32::MAX,
        additional_properties: serde_json::Map::new(),
    };
    assert_u32(command.schema_version);
    assert_u32(query.schema_version);
    assert_eq!(
        serde_json::to_value(command).expect("serialize command payload")["schema_version"],
        json!(u32::MAX)
    );
}

#[test]
fn generated_root_types_exist_and_typed_v2_wrappers_do_not() {
    let _ = std::any::type_name::<InputContentOriginV1>();
    let _ = std::any::type_name::<InputTaskScopeV1>();
    let _ = std::any::type_name::<TaskCreateRequestV2>();
    let _ = std::any::type_name::<NormalizedRootTaskCreatePayloadV2>();
    let _ = std::any::type_name::<RootTaskCreateIdempotencyProjectionV1>();
    let _ = std::any::type_name::<TaskCreateResponseV2>();
    let _ = std::any::type_name::<RootTaskCreateAllocationV2>();
    let _ = std::any::type_name::<ChildTaskProposalV1>();
    let _ = std::any::type_name::<NormalizedChildTaskProposalV1>();
    let _ = std::any::type_name::<ChildTaskMaterializationAllocationV1>();
    let _ = std::any::type_name::<KcpCommandEnvelopeV2>();
    let _ = std::any::type_name::<KcpQueryEnvelopeV2>();

    // No generic Typed*EnvelopeV2 wrappers: V2 envelopes have zero method payload refs.
    let types_source = include_str!("../src/generated/types.rs");
    let typed_source = include_str!("../src/generated/typed.rs");
    assert!(!typed_source.contains("TypedKcpCommandEnvelopeV2"));
    assert!(!typed_source.contains("TypedKcpQueryEnvelopeV2"));
    assert!(types_source.contains("pub struct KcpCommandEnvelopeV2"));
    assert!(types_source.contains("pub struct KcpQueryEnvelopeV2"));
}

#[test]
fn envelope_authority_and_legacy_catalogs_are_each_eight_and_bindings_empty() {
    assert_eq!(KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS.len(), 2);
    assert_eq!(KCP_ENVELOPE_AUTHORITY_QUERY_METHODS.len(), 6);
    assert_eq!(KCP_ENVELOPE_AUTHORITY_METHODS.len(), 8);
    assert_eq!(KCP_LEGACY_V1_METHODS.len(), 8);
    assert_eq!(KCP_PROTOCOL_VERSION, "1.0");
    assert_eq!(EVENT_ACTIVE_BINDINGS.len(), 5);
    assert_eq!(EVENT_ACTIVE_TYPES.len(), 5);
    assert_eq!(EVENT_LEGACY_V1_BINDINGS.len(), 3);
    assert_eq!(EVENT_LEGACY_V1_TYPES.len(), 3);
    assert!(METHOD_VERSION_BINDINGS.is_empty());
    for method in [
        "task.create",
        "stop.activate",
        "system.ping",
        "task.get",
        "task.list",
        "event.subscribe",
        "event.poll",
        "stop.status",
    ] {
        assert!(
            KCP_ENVELOPE_AUTHORITY_METHODS.contains(&method),
            "missing V2 envelope authority method {method}"
        );
        assert!(
            KCP_LEGACY_V1_METHODS.contains(&method),
            "missing legacy {method}"
        );
    }
}

#[test]
fn validated_decode_matrix_for_all_twelve_roots() {
    assert_validated_round_trip::<InputContentOriginV1>(
        INPUT_CONTENT_ORIGIN,
        &sample_origin_full(),
    );
    assert_validated_round_trip::<InputTaskScopeV1>(INPUT_TASK_SCOPE, &sample_scope_full());
    assert_validated_round_trip::<TaskCreateRequestV2>(
        TASK_CREATE_REQUEST_V2,
        &nine_field_payload(2, true),
    );
    assert_validated_round_trip::<NormalizedRootTaskCreatePayloadV2>(
        NORMALIZED_ROOT,
        &nine_field_payload(2, true),
    );
    let projection = json!({
        "schema_version": 1,
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "command_type": "task.create",
        "task_id": null,
        "context": {"x": 1},
        "expected_revision": null,
        "payload": nine_field_payload(2, true)
    });
    assert_validated_round_trip::<RootTaskCreateIdempotencyProjectionV1>(
        IDEMPOTENCY_PROJECTION,
        &projection,
    );
    let response = json!({
        "schema_version": 2,
        "task": sample_task_spec(),
        "creation_provenance_ref": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
    });
    assert_validated_round_trip::<TaskCreateResponseV2>(TASK_CREATE_RESPONSE_V2, &response);
    assert_validated_round_trip::<RootTaskCreateAllocationV2>(
        ROOT_ALLOCATION,
        &sample_root_allocation_distinct(),
    );
    assert_validated_round_trip::<ChildTaskProposalV1>(
        CHILD_PROPOSAL,
        &nine_field_payload(1, true),
    );
    assert_validated_round_trip::<NormalizedChildTaskProposalV1>(
        NORMALIZED_CHILD,
        &nine_field_payload(1, true),
    );
    assert_validated_round_trip::<ChildTaskMaterializationAllocationV1>(
        CHILD_ALLOCATION,
        &sample_child_allocation_distinct(),
    );
    assert_validated_round_trip::<KcpCommandEnvelopeV2>(
        COMMAND_ENVELOPE_V2,
        &sample_command_envelope_v2(),
    );
    assert_validated_round_trip::<KcpQueryEnvelopeV2>(
        QUERY_ENVELOPE_V2,
        &sample_query_envelope_v2(),
    );

    let mut unknown = nine_field_payload(2, false);
    unknown["unexpected"] = json!(true);
    assert!(decode_validated::<TaskCreateRequestV2>(TASK_CREATE_REQUEST_V2, &unknown).is_err());

    let mut missing_nullable = nine_field_payload(2, false);
    missing_nullable
        .as_object_mut()
        .unwrap()
        .remove("risk_hint");
    assert!(
        decode_validated::<TaskCreateRequestV2>(TASK_CREATE_REQUEST_V2, &missing_nullable).is_err()
    );
    let mut explicit_null = nine_field_payload(2, false);
    explicit_null["risk_hint"] = Value::Null;
    assert!(
        decode_validated::<TaskCreateRequestV2>(TASK_CREATE_REQUEST_V2, &explicit_null).is_ok()
    );

    let mut empty_goal = nine_field_payload(2, false);
    empty_goal["goal"] = json!("");
    assert!(serde_json::from_value::<TaskCreateRequestV2>(empty_goal.clone()).is_ok());
    assert!(decode_validated::<TaskCreateRequestV2>(TASK_CREATE_REQUEST_V2, &empty_goal).is_err());
}

#[test]
fn input_content_origin_positive_and_boundary() {
    assert_valid(INPUT_CONTENT_ORIGIN, &sample_origin_minimal());
    assert_valid(INPUT_CONTENT_ORIGIN, &sample_origin_full());

    let mut missing = sample_origin_minimal();
    missing.as_object_mut().unwrap().remove("kind");
    assert_invalid(INPUT_CONTENT_ORIGIN, &missing, "missing required");

    let mut unknown = sample_origin_minimal();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("id".into(), json!("11111111-1111-4111-8111-111111111111"));
    assert_invalid(INPUT_CONTENT_ORIGIN, &unknown, "kernel-owned id forbidden");

    for forbidden in [
        "received_at",
        "carrier_ref",
        "kernel_receipt",
        "entry_point",
    ] {
        let mut bad = sample_origin_minimal();
        bad.as_object_mut()
            .unwrap()
            .insert(forbidden.into(), json!("x"));
        assert_invalid(INPUT_CONTENT_ORIGIN, &bad, forbidden);
    }

    let mut empty_uri = sample_origin_minimal();
    empty_uri["source_uri"] = json!("");
    assert_invalid(INPUT_CONTENT_ORIGIN, &empty_uri, "empty source_uri");

    let mut empty_upstream = sample_origin_minimal();
    empty_upstream["upstream_stable_id"] = json!("");
    assert_invalid(
        INPUT_CONTENT_ORIGIN,
        &empty_upstream,
        "empty upstream_stable_id",
    );

    let mut bad_parent = sample_origin_minimal();
    bad_parent["parent_origin_refs"] = json!([1]);
    assert_invalid(
        INPUT_CONTENT_ORIGIN,
        &bad_parent,
        "non-string parent origin item",
    );

    let mut invalid_uri = sample_origin_minimal();
    invalid_uri["source_uri"] = json!("not a uri");
    assert_invalid(INPUT_CONTENT_ORIGIN, &invalid_uri, "source_uri format uri");

    let mut invalid_uuid = sample_origin_minimal();
    invalid_uuid["parent_origin_refs"] = json!(["not-a-uuid"]);
    assert_invalid(INPUT_CONTENT_ORIGIN, &invalid_uuid, "uuid format assertion");

    let mut uppercase_uuid = sample_origin_minimal();
    uppercase_uuid["parent_origin_refs"] = json!(["AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"]);
    assert_invalid(
        INPUT_CONTENT_ORIGIN,
        &uppercase_uuid,
        "canonical lowercase uuid pattern",
    );

    // Duplicates are legal; uniqueItems must not be set.
    let mut dups = sample_origin_full();
    dups["parent_origin_refs"] = json!([
        "11111111-1111-4111-8111-111111111111",
        "11111111-1111-4111-8111-111111111111"
    ]);
    assert_valid(INPUT_CONTENT_ORIGIN, &dups);
}

#[test]
fn input_task_scope_positive_and_boundary() {
    assert_valid(INPUT_TASK_SCOPE, &sample_scope_minimal());
    assert_valid(INPUT_TASK_SCOPE, &sample_scope_full());

    for stored in [
        "id",
        "task_id",
        "revision",
        "source_refs",
        "created_by",
        "created_at",
        "updated_at",
    ] {
        let mut bad = sample_scope_minimal();
        bad.as_object_mut()
            .unwrap()
            .insert(stored.into(), json!("x"));
        assert_invalid(INPUT_TASK_SCOPE, &bad, stored);
    }

    let mut empty_item = sample_scope_minimal();
    empty_item["resource_patterns"] = json!([""]);
    assert_invalid(INPUT_TASK_SCOPE, &empty_item, "empty array item");

    let mut invalid_time = sample_scope_minimal();
    invalid_time["expires_at"] = json!("2030-99-99T99:99:99Z");
    assert_invalid(INPUT_TASK_SCOPE, &invalid_time, "date-time assertion");

    let mut glob_not_uri = sample_scope_minimal();
    glob_not_uri["resource_patterns"] = json!(["**/[abc]"]);
    assert_valid(INPUT_TASK_SCOPE, &glob_not_uri);

    let mut dups = sample_scope_minimal();
    dups["exclusions"] = json!(["a", "a"]);
    assert_valid(INPUT_TASK_SCOPE, &dups);
}

fn assert_nine_field_object_rules(schema_id: &str, schema_version: u32) {
    assert_valid(schema_id, &nine_field_payload(schema_version, false));
    assert_valid(schema_id, &nine_field_payload(schema_version, true));

    let mut missing = nine_field_payload(schema_version, false);
    missing.as_object_mut().unwrap().remove("goal");
    assert_invalid(schema_id, &missing, "missing goal");

    let mut parent = nine_field_payload(schema_version, false);
    parent
        .as_object_mut()
        .unwrap()
        .insert("parent_task_id".into(), json!(null));
    assert_invalid(schema_id, &parent, "parent_task_id unknown field");

    let mut empty_goal = nine_field_payload(schema_version, false);
    empty_goal["goal"] = json!("");
    assert_invalid(schema_id, &empty_goal, "empty goal");

    let mut empty_risk = nine_field_payload(schema_version, false);
    empty_risk["risk_hint"] = json!("");
    assert_invalid(schema_id, &empty_risk, "empty risk_hint");

    let mut empty_item = nine_field_payload(schema_version, false);
    empty_item["constraints"] = json!([""]);
    assert_invalid(schema_id, &empty_item, "empty constraint item");

    let mut dups = nine_field_payload(schema_version, false);
    dups["capability_hints"] = json!(["a", "a"]);
    assert_valid(schema_id, &dups);

    let mut wrong_version = nine_field_payload(schema_version, false);
    wrong_version["schema_version"] = json!(if schema_version == 1 { 2 } else { 1 });
    assert_invalid(schema_id, &wrong_version, "schema_version const");
}

#[test]
fn four_shared_field_roots_accept_same_caller_owned_shape() {
    assert_nine_field_object_rules(TASK_CREATE_REQUEST_V2, 2);
    assert_nine_field_object_rules(NORMALIZED_ROOT, 2);
    assert_nine_field_object_rules(CHILD_PROPOSAL, 1);
    assert_nine_field_object_rules(NORMALIZED_CHILD, 1);
}

#[test]
fn shared_defs_host_is_single_and_absolute_refs_are_exact() {
    let host = document(NORMALIZED_ROOT);
    let host_defs = host
        .get("$defs")
        .and_then(Value::as_object)
        .expect("host $defs");
    let def_names: BTreeSet<_> = host_defs.keys().cloned().collect();
    let expected: BTreeSet<_> = NINE_CALLER_FIELDS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        def_names, expected,
        "host $defs must be exactly nine fields"
    );

    for name in NINE_CALLER_FIELDS {
        let local = host
            .pointer(&format!("/properties/{name}/$ref"))
            .and_then(Value::as_str);
        assert_eq!(local, Some(format!("#/$defs/{name}").as_str()));
    }
    assert_eq!(
        host.pointer("/$defs/task_scope/$ref")
            .and_then(Value::as_str),
        Some(INPUT_TASK_SCOPE)
    );
    assert_eq!(
        host.pointer("/$defs/origin/$ref").and_then(Value::as_str),
        Some(INPUT_CONTENT_ORIGIN)
    );

    for schema_id in [TASK_CREATE_REQUEST_V2, CHILD_PROPOSAL, NORMALIZED_CHILD] {
        let doc = document(schema_id);
        assert!(
            doc.get("$defs").is_none(),
            "{schema_id} must not copy host $defs"
        );
        for name in NINE_CALLER_FIELDS {
            let reference = doc
                .pointer(&format!("/properties/{name}/$ref"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{schema_id} missing ref for {name}"));
            assert_eq!(
                reference,
                format!("{NORMALIZED_ROOT}#/$defs/{name}"),
                "{schema_id}.{name}"
            );
        }
        // Must not bypass host with whole-schema refs to Input* roots.
        let raw = serde_json::to_string(&doc).unwrap();
        assert!(
            !raw.contains(&format!("\"$ref\":\"{INPUT_TASK_SCOPE}\"")),
            "{schema_id} must not whole-schema ref InputTaskScopeV1"
        );
        assert!(
            !raw.contains(&format!("\"$ref\":\"{INPUT_CONTENT_ORIGIN}\"")),
            "{schema_id} must not whole-schema ref InputContentOriginV1"
        );
    }

    // No thirteenth parallel schema for the nine-field contract.
    let thirteenth_candidates = [
        "https://schemas.shittim.local/task/task_create_proposal_fields/v1",
        "https://schemas.shittim.local/common/task_create_proposal_fields/v1",
        "https://schemas.shittim.local/kcp/task_create_proposal_fields/v1",
    ];
    for id in thirteenth_candidates {
        assert!(
            catalog().document(id).is_none(),
            "must not invent thirteenth schema {id}"
        );
    }
}

#[test]
fn shared_defs_mutant_style_proves_single_constraint_site() {
    // If goal minLength lived in four places, a single host change would not
    // simultaneously affect all four roots. We prove the only goal constraint
    // site is host $defs/goal by inspecting documents, not by rewriting production.
    let host_goal = document(NORMALIZED_ROOT)
        .pointer("/$defs/goal")
        .cloned()
        .expect("host goal def");
    assert_eq!(host_goal.get("minLength").and_then(Value::as_u64), Some(1));
    for schema_id in [
        TASK_CREATE_REQUEST_V2,
        NORMALIZED_ROOT,
        CHILD_PROPOSAL,
        NORMALIZED_CHILD,
    ] {
        let doc = document(schema_id);
        if schema_id == NORMALIZED_ROOT {
            assert!(doc.pointer("/properties/goal/minLength").is_none());
            assert_eq!(
                doc.pointer("/properties/goal/$ref").and_then(Value::as_str),
                Some("#/$defs/goal")
            );
        } else {
            assert!(doc.pointer("/properties/goal/minLength").is_none());
            assert_eq!(
                doc.pointer("/properties/goal/$ref").and_then(Value::as_str),
                Some(format!("{NORMALIZED_ROOT}#/$defs/goal").as_str())
            );
        }
        // Runtime proof: empty goal fails all four roots through the shared constraint.
        let mut bad = nine_field_payload(if schema_id.contains("/v2") { 2 } else { 1 }, false);
        bad["goal"] = json!("");
        assert_invalid(schema_id, &bad, "shared empty goal");
    }
}

#[test]
fn idempotency_projection_shape_and_retained_refs() {
    let payload = nine_field_payload(2, true);
    let projection = json!({
        "schema_version": 1,
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "command_type": "task.create",
        "task_id": null,
        "context": {"k": "v"},
        "expected_revision": null,
        "payload": payload
    });
    assert_valid(IDEMPOTENCY_PROJECTION, &projection);
    assert_valid(ACTOR, &sample_actor());
    assert_valid(ENTRY_POINT, &json!("local_desktop"));

    let doc = document(IDEMPOTENCY_PROJECTION);
    assert_eq!(
        doc.pointer("/properties/actor/$ref")
            .and_then(Value::as_str),
        Some(ACTOR)
    );
    assert_eq!(
        doc.pointer("/properties/entry_point/$ref")
            .and_then(Value::as_str),
        Some(ENTRY_POINT)
    );
    assert_eq!(
        doc.pointer("/properties/payload/$ref")
            .and_then(Value::as_str),
        Some(NORMALIZED_ROOT)
    );
    assert_eq!(
        doc.pointer("/properties/command_type/const")
            .and_then(Value::as_str),
        Some("task.create")
    );
    assert_eq!(
        doc.pointer("/properties/task_id/type")
            .and_then(Value::as_str),
        Some("null")
    );
    assert_eq!(
        doc.pointer("/properties/expected_revision/type")
            .and_then(Value::as_str),
        Some("null")
    );

    let mut bad_task = projection.clone();
    bad_task["task_id"] = json!("11111111-1111-4111-8111-111111111111");
    assert_invalid(IDEMPOTENCY_PROJECTION, &bad_task, "task_id must be null");

    let mut missing = projection.clone();
    missing.as_object_mut().unwrap().remove("context");
    assert_invalid(
        IDEMPOTENCY_PROJECTION,
        &missing,
        "context required-nullable",
    );

    let mut open_context = projection;
    open_context["context"] = json!({"any": true, "nested": {"x": 1}});
    assert_valid(IDEMPOTENCY_PROJECTION, &open_context);
}

#[test]
fn task_create_response_v2_refs_retained_task_spec() {
    let response = json!({
        "schema_version": 2,
        "task": sample_task_spec(),
        "creation_provenance_ref": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
    });
    assert_valid(TASK_CREATE_RESPONSE_V2, &response);
    assert_valid(TASK_SPEC, &sample_task_spec());

    let doc = document(TASK_CREATE_RESPONSE_V2);
    assert_eq!(
        doc.pointer("/properties/task/$ref").and_then(Value::as_str),
        Some(TASK_SPEC)
    );
    assert!(catalog()
        .document("https://schemas.shittim.local/task/task_spec/v2")
        .is_none());

    let mut missing_prov = response.clone();
    missing_prov
        .as_object_mut()
        .unwrap()
        .remove("creation_provenance_ref");
    assert_invalid(TASK_CREATE_RESPONSE_V2, &missing_prov, "missing provenance");

    // Typed serialization path.
    let typed: TaskCreateResponseV2 =
        serde_json::from_value(response.clone()).expect("typed response");
    let roundtrip = serde_json::to_value(&typed).expect("serialize");
    assert_eq!(roundtrip["schema_version"], 2);
    assert_eq!(
        roundtrip["creation_provenance_ref"],
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
    );
}

#[test]
fn root_and_child_allocation_shapes_and_production_domain_validation() {
    let root_json = sample_root_allocation_distinct();
    let child_json = sample_child_allocation_distinct();
    assert_valid(ROOT_ALLOCATION, &root_json);
    assert_valid(CHILD_ALLOCATION, &child_json);
    let root: RootTaskCreateAllocationV2 =
        decode_validated(ROOT_ALLOCATION, &root_json).expect("typed root allocation");
    let child: ChildTaskMaterializationAllocationV1 =
        decode_validated(CHILD_ALLOCATION, &child_json).expect("typed child allocation");

    let root_external = RootTaskCreateExternalUuidRefsV1 {
        command_request_id: uuid("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1"),
        delegation_ref: Some(uuid("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2")),
        parent_origin_refs: vec![uuid("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3")],
    };
    let child_external = ChildTaskMaterializationExternalUuidRefsV1 {
        parent_task_id: uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb1"),
        action_id: uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb2"),
        permission_decision_id: uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb3"),
        approval_resolution_ref: Some(uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb4")),
        credential_refs: vec![uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb5")],
        challenge_refs: vec![uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb6")],
        delegation_ref: Some(uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb7")),
        parent_origin_refs: vec![uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb8")],
    };
    validate_root_task_create_allocation(&root, &root_external).expect("valid root allocation");
    validate_child_task_materialization_allocation(&child, &child_external)
        .expect("valid child allocation");

    // Schema does NOT reject cross-field domain conflicts; the production helper does.
    let mut root_dup = root.clone();
    root_dup.task_scope_id = root_dup.task_id.clone();
    assert_valid(
        ROOT_ALLOCATION,
        &serde_json::to_value(&root_dup).expect("root duplicate JSON"),
    );
    assert!(validate_root_task_create_allocation(&root_dup, &root_external).is_err());

    let mut child_dup = child.clone();
    child_dup.verification_result_id = child_dup.child_task_id.clone();
    assert_valid(
        CHILD_ALLOCATION,
        &serde_json::to_value(&child_dup).expect("child duplicate JSON"),
    );
    assert!(validate_child_task_materialization_allocation(&child_dup, &child_external).is_err());

    let mut root_external_collision = root_external.clone();
    root_external_collision.command_request_id = uuid(&root.task_id);
    assert!(validate_root_task_create_allocation(&root, &root_external_collision).is_err());

    let mut duplicate_opaque = child.clone();
    duplicate_opaque.action_state_changed_dedup_key =
        duplicate_opaque.task_created_dedup_key.clone();
    assert_valid(
        CHILD_ALLOCATION,
        &serde_json::to_value(&duplicate_opaque).expect("duplicate opaque JSON"),
    );
    assert!(
        validate_child_task_materialization_allocation(&duplicate_opaque, &child_external).is_err()
    );

    let mut empty_opaque = root_json;
    empty_opaque["correlation_id"] = json!("");
    assert_invalid(ROOT_ALLOCATION, &empty_opaque, "empty opaque");

    let root_doc = document(ROOT_ALLOCATION);
    let child_doc = document(CHILD_ALLOCATION);
    assert_eq!(uuid_formatted_property_count(&root_doc), 7);
    assert_eq!(uuid_formatted_property_count(&child_doc), 10);
}

fn uuid(value: &str) -> uuid::Uuid {
    uuid::Uuid::parse_str(value).expect("canonical UUID fixture")
}

fn uuid_formatted_property_count(document: &Value) -> usize {
    document["properties"]
        .as_object()
        .expect("properties")
        .values()
        .filter(|property| property.get("format") == Some(&json!("uuid")))
        .count()
}

#[test]
fn expires_at_pattern_and_format_are_both_hard_gates() {
    for valid in [
        "2030-01-01T00:00:00Z",
        "2030-01-01T00:00:00.000Z",
        "2030-01-01T08:00:00+08:00",
        "2030-01-01T08:00:00.000+08:00",
    ] {
        let mut scope = sample_scope_minimal();
        scope["expires_at"] = json!(valid);
        assert_valid(INPUT_TASK_SCOPE, &scope);
    }

    for invalid in [
        "2030-01-01T00:00:00.001Z",
        "2030-01-01t00:00:00Z",
        "2030-01-01T00:00:00z",
        "2030-01-01T00:00:60Z",
        "2030-01-01T00:00:00+0800",
        "2030-02-30T00:00:00Z",
    ] {
        let mut scope = sample_scope_minimal();
        scope["expires_at"] = json!(invalid);
        assert_invalid(INPUT_TASK_SCOPE, &scope, invalid);
    }

    let document = document(INPUT_TASK_SCOPE);
    assert_eq!(
        document
            .pointer("/properties/expires_at/format")
            .and_then(Value::as_str),
        Some("date-time")
    );
    assert_eq!(
        document
            .pointer("/properties/expires_at/pattern")
            .and_then(Value::as_str),
        Some("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-5][0-9](?:\\.0+)?(?:Z|[+-][0-9]{2}:[0-9]{2})$")
    );
}

#[test]
fn envelope_v2_structure_methods_and_open_payload() {
    assert_valid(COMMAND_ENVELOPE_V2, &sample_command_envelope_v2());
    assert_valid(QUERY_ENVELOPE_V2, &sample_query_envelope_v2());

    let command_doc = document(COMMAND_ENVELOPE_V2);
    let query_doc = document(QUERY_ENVELOPE_V2);
    assert!(
        command_doc.get("allOf").is_none(),
        "no conditional mappings"
    );
    assert!(query_doc.get("allOf").is_none(), "no conditional mappings");
    assert_eq!(
        command_doc
            .pointer("/properties/protocol_version/const")
            .and_then(Value::as_str),
        Some("1.0")
    );
    assert_eq!(
        query_doc
            .pointer("/properties/protocol_version/const")
            .and_then(Value::as_str),
        Some("1.0")
    );

    let command_methods: BTreeSet<_> = command_doc
        .pointer("/properties/command_type/enum")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        command_methods,
        BTreeSet::from(["task.create", "stop.activate"])
    );
    let query_methods: BTreeSet<_> = query_doc
        .pointer("/properties/query_type/enum")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        query_methods,
        BTreeSet::from([
            "system.ping",
            "task.get",
            "task.list",
            "event.subscribe",
            "event.poll",
            "stop.status"
        ])
    );

    // Query V2 must not grow command-only fields.
    for forbidden in ["context", "idempotency_key", "expected_revision"] {
        assert!(
            query_doc
                .pointer(&format!("/properties/{forbidden}"))
                .is_none(),
            "query envelope must not declare {forbidden}"
        );
    }

    // payload.schema_version must be positive integer; business payload is open.
    let mut zero = sample_command_envelope_v2();
    zero["payload"] = json!({"schema_version": 0});
    assert_invalid(COMMAND_ENVELOPE_V2, &zero, "schema_version >= 1");

    let mut open = sample_command_envelope_v2();
    open["payload"] = json!({"schema_version": 2, "goal": "x", "extra": true});
    assert_valid(COMMAND_ENVELOPE_V2, &open);

    let mut missing_payload_version = sample_query_envelope_v2();
    missing_payload_version["payload"] = json!({"echo": "hi"});
    assert_invalid(
        QUERY_ENVELOPE_V2,
        &missing_payload_version,
        "payload.schema_version required",
    );

    let mut maximum = sample_command_envelope_v2();
    maximum["payload"] = json!({"schema_version": 4294967295u64, "extra": {"kept": true}});
    assert_validated_round_trip::<KcpCommandEnvelopeV2>(COMMAND_ENVELOPE_V2, &maximum);
    let typed: KcpCommandEnvelopeV2 =
        decode_validated(COMMAND_ENVELOPE_V2, &maximum).expect("decode open payload");
    let roundtrip = serde_json::to_value(typed).expect("serialize open payload");
    assert_eq!(roundtrip["payload"]["extra"]["kept"], true);

    let mut overflow = sample_command_envelope_v2();
    overflow["payload"] = json!({"schema_version": 4294967296u64});
    assert_invalid(
        COMMAND_ENVELOPE_V2,
        &overflow,
        "schema_version u32 overflow",
    );

    let mut invalid_request_uuid = sample_command_envelope_v2();
    invalid_request_uuid["request_id"] = json!("not-a-uuid");
    assert_invalid(COMMAND_ENVELOPE_V2, &invalid_request_uuid, "request uuid");

    let mut uppercase_request_uuid = sample_command_envelope_v2();
    uppercase_request_uuid["request_id"] = json!("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA");
    assert_invalid(
        COMMAND_ENVELOPE_V2,
        &uppercase_request_uuid,
        "canonical request uuid",
    );

    let mut invalid_deadline = sample_command_envelope_v2();
    invalid_deadline["deadline"] = json!("tomorrow");
    assert_invalid(COMMAND_ENVELOPE_V2, &invalid_deadline, "deadline date-time");

    // Root still closed.
    let mut unknown = sample_query_envelope_v2();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("context".into(), json!({}));
    assert_invalid(QUERY_ENVELOPE_V2, &unknown, "unknown root field");

    // Auth remains null-only.
    let mut auth = sample_command_envelope_v2();
    auth["auth"] = json!({"token": "x"});
    assert_invalid(COMMAND_ENVELOPE_V2, &auth, "auth must be null");
}

#[test]
fn retained_v1_envelopes_still_validate_with_conditional_payloads() {
    // Legacy typed path remains for preflight; V2 does not replace retained bytes.
    let command_v1 = json!({
        "protocol_version": "1.0",
        "message_kind": "command",
        "request_id": "11111111-1111-4111-8111-111111111111",
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "auth": null,
        "task_id": null,
        "context": null,
        "deadline": "2030-01-01T00:00:00Z",
        "idempotency_key": "idem",
        "expected_revision": null,
        "command_type": "stop.activate",
        "payload": {
            "schema_version": 1,
            "reason": "test",
            "origin_ref": null
        }
    });
    assert_valid(
        "https://schemas.shittim.local/v1/kcp/command_envelope.json",
        &command_v1,
    );
}

#[test]
fn four_roots_required_and_property_sets_match_except_schema_version_const() {
    for (schema_id, version) in [
        (TASK_CREATE_REQUEST_V2, 2u64),
        (NORMALIZED_ROOT, 2),
        (CHILD_PROPOSAL, 1),
        (NORMALIZED_CHILD, 1),
    ] {
        let doc = document(schema_id);
        let required: BTreeSet<_> = doc
            .get("required")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let properties: BTreeSet<_> = doc
            .get("properties")
            .and_then(Value::as_object)
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut expected: BTreeSet<_> = NINE_CALLER_FIELDS.iter().copied().collect();
        expected.insert("schema_version");
        assert_eq!(required, expected, "{schema_id} required");
        assert_eq!(properties, expected, "{schema_id} properties");
        assert_eq!(
            doc.pointer("/properties/schema_version/const")
                .and_then(Value::as_u64),
            Some(version)
        );
        assert_eq!(
            doc.get("additionalProperties").and_then(Value::as_bool),
            Some(false)
        );
    }
}
