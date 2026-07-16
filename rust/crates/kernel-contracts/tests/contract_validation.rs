//! Contract-level validation tests for first-batch schemas.

use kernel_contracts::{
    sha256_canonical, validate_json, EventPayload, KcpCommandEnvelopeProtocolVersion,
    KcpCommandPayload, NullOnly, SchemaCatalog, TypedEventEnvelope, TypedKcpCommandEnvelope,
    TypedKcpQueryEnvelope, EVENT_V1_TYPES, KCP_PROTOCOL_VERSION, KCP_V1_METHODS,
};
use serde_json::{json, Value};

const AUDIT_RECORD_ID: &str = "https://schemas.shittim.local/v1/audit/audit_record.json";
const ACTOR_ID: &str = "https://schemas.shittim.local/v1/common/actor.json";
const ENTRY_POINT_ID: &str = "https://schemas.shittim.local/v1/common/entry_point.json";
const TASK_STATUS_ID: &str = "https://schemas.shittim.local/v1/common/task_status.json";
const POLICY_RULE_ID: &str = "https://schemas.shittim.local/v1/policy/policy_rule.json";
const COMMAND_ID: &str = "https://schemas.shittim.local/v1/kcp/command_envelope.json";
const QUERY_ID: &str = "https://schemas.shittim.local/v1/kcp/query_envelope.json";
const RESPONSE_ID: &str = "https://schemas.shittim.local/v1/kcp/response_envelope.json";
const TASK_LIST_ID: &str = "https://schemas.shittim.local/v1/kcp/task_list_request.json";
const EVENT_SUBSCRIBE_ID: &str =
    "https://schemas.shittim.local/v1/kcp/event_subscribe_request.json";
const EVENT_POLL_ID: &str = "https://schemas.shittim.local/v1/kcp/event_poll_request.json";
const RECOVERY_ID: &str = "https://schemas.shittim.local/v1/task/recovery_decision_candidate.json";

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

#[test]
fn embedded_catalog_loads_all_first_batch_schemas() {
    let catalog = SchemaCatalog::load_embedded().expect("catalog");
    assert!(catalog.schema_ids().len() >= 41);
}

#[test]
fn audit_record_valid_user_activity_is_accepted() {
    validate_json(AUDIT_RECORD_ID, &sample_audit_record()).expect("valid audit record");
}

#[test]
fn audit_record_rejects_unknown_fields() {
    let mut record = sample_audit_record();
    record
        .as_object_mut()
        .expect("object")
        .insert("trace_id".into(), json!("hidden-attribution"));
    assert!(validate_json(AUDIT_RECORD_ID, &record).is_err());
}

#[test]
fn audit_record_rejects_unknown_audit_type() {
    let mut record = sample_audit_record();
    record["audit_type"] = json!("task.succeeded");
    assert!(validate_json(AUDIT_RECORD_ID, &record).is_err());
}

#[test]
fn audit_record_only_allows_null_actor_for_system_internal() {
    let mut non_system = sample_audit_record();
    non_system["actor"] = Value::Null;
    assert!(validate_json(AUDIT_RECORD_ID, &non_system).is_err());

    let mut system = sample_audit_record();
    system["actor"] = Value::Null;
    system["entry_point"] = json!("system_internal");
    system["audit_type"] = json!("kernel.invariant_blocked");
    system["task_creation_context"] = Value::Null;
    system["level"] = json!("security");
    system["outcome"] = json!("blocked");
    validate_json(AUDIT_RECORD_ID, &system).expect("system internal can lack actor");
}

#[test]
fn audit_record_actor_requires_complete_revision_snapshot() {
    let mut record = sample_audit_record();
    record["actor"]
        .as_object_mut()
        .expect("actor")
        .remove("revision");
    assert!(validate_json(AUDIT_RECORD_ID, &record).is_err());
}

#[test]
fn audit_record_requires_explicit_reference_closure_fields() {
    for field in [
        "task_creation_context",
        "delegation_ref",
        "model_call_refs",
        "payload_manifest_refs",
        "external_content_status",
        "verification_result_refs",
        "resource_refs",
        "rollback_capability",
        "stop_fence_generation",
        "policy_context",
    ] {
        let mut record = sample_audit_record();
        record.as_object_mut().expect("object").remove(field);
        assert!(
            validate_json(AUDIT_RECORD_ID, &record).is_err(),
            "missing {field} must be rejected"
        );
    }
}

#[test]
fn audit_record_task_creation_context_is_conditionally_required() {
    let mut missing_context = sample_audit_record();
    missing_context["task_creation_context"] = Value::Null;
    assert!(validate_json(AUDIT_RECORD_ID, &missing_context).is_err());

    let mut missing_task = sample_audit_record();
    missing_task["task_id"] = Value::Null;
    assert!(validate_json(AUDIT_RECORD_ID, &missing_task).is_err());

    let mut wrong_revision = sample_audit_record();
    wrong_revision["task_creation_context"]["task_revision"] = json!(2);
    assert!(validate_json(AUDIT_RECORD_ID, &wrong_revision).is_err());

    let mut non_creation = sample_audit_record();
    non_creation["audit_type"] = json!("command.accepted");
    assert!(validate_json(AUDIT_RECORD_ID, &non_creation).is_err());
    non_creation["task_creation_context"] = Value::Null;
    validate_json(AUDIT_RECORD_ID, &non_creation).expect("non-creation context must be null");
}

#[test]
fn audit_record_external_content_status_rules_are_enforced() {
    let mut not_sent_with_manifest = sample_audit_record();
    not_sent_with_manifest["payload_manifest_refs"] = json!(["payload-manifest://model/request-7"]);
    assert!(validate_json(AUDIT_RECORD_ID, &not_sent_with_manifest).is_err());

    let mut sent = sample_audit_record();
    sent["external_content_status"] = json!("sent");
    sent["payload_manifest_refs"] = json!(["payload-manifest://model/request-7"]);
    validate_json(AUDIT_RECORD_ID, &sent).expect("sent record with stable manifest ref");

    let mut unknown_without_reason = sample_audit_record();
    unknown_without_reason["external_content_status"] = json!("unknown");
    unknown_without_reason["reason_codes"] = json!([]);
    assert!(validate_json(AUDIT_RECORD_ID, &unknown_without_reason).is_err());
}

#[test]
fn audit_record_permission_decision_requires_policy_context() {
    let mut record = sample_audit_record();
    record["permission_decision_ref"] = json!("44444444-4444-4444-8444-444444444444");
    record["policy_context"] = Value::Null;
    assert!(validate_json(AUDIT_RECORD_ID, &record).is_err());
}

#[test]
fn audit_record_rejects_unknown_policy_context_fields() {
    let mut record = sample_audit_record();
    record["permission_decision_ref"] = json!("44444444-4444-4444-8444-444444444444");
    record["policy_context"] = json!({
        "matched_rule_ref": null,
        "policy_set_revision": 8,
        "decision_ordering_summary": null,
        "policy_mutation_authority": null,
        "authentication_evidence_refs": []
    });
    record["policy_context"]
        .as_object_mut()
        .expect("policy context")
        .insert("copied_policy_rule".into(), json!({"effect": "allow"}));
    assert!(validate_json(AUDIT_RECORD_ID, &record).is_err());
}

#[test]
fn audit_record_reference_arrays_must_be_unique() {
    let mut record = sample_audit_record();
    record["model_call_refs"] = json!([
        "model-call://provider/request-7",
        "model-call://provider/request-7"
    ]);
    assert!(validate_json(AUDIT_RECORD_ID, &record).is_err());

    let mut record = sample_audit_record();
    record["verification_result_refs"] = json!([
        "44444444-4444-4444-8444-444444444444",
        "44444444-4444-4444-8444-444444444444"
    ]);
    assert!(validate_json(AUDIT_RECORD_ID, &record).is_err());
}

#[test]
fn unknown_task_status_enum_is_rejected() {
    let err = validate_json(TASK_STATUS_ID, &json!("not_a_status")).expect_err("must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("validation failed") || msg.contains("not_a_status") || msg.contains("enum"),
        "{msg}"
    );
}

#[test]
fn unknown_entry_point_enum_is_rejected() {
    let err = validate_json(ENTRY_POINT_ID, &json!("remote_tcp")).expect_err("must reject");
    assert!(!err.to_string().is_empty());
}

#[test]
fn actor_rejects_unknown_fields() {
    let mut actor = sample_actor();
    actor
        .as_object_mut()
        .expect("obj")
        .insert("entry_point".into(), json!("local_desktop"));
    let err = validate_json(ACTOR_ID, &actor).expect_err("unknown field");
    assert!(
        err.to_string().contains("validation failed") || err.to_string().contains("additional"),
        "{}",
        err
    );
}

#[test]
fn owner_kind_is_schema_valid_but_is_reserved_label_only() {
    let mut actor = sample_actor();
    actor
        .as_object_mut()
        .expect("obj")
        .insert("kind".into(), json!("owner"));
    validate_json(ACTOR_ID, &actor).expect("owner label allowed by schema");
}

#[test]
fn policy_confirm_requires_confirmation_mode() {
    let mut rule = sample_policy_rule("confirm");
    rule.as_object_mut()
        .expect("obj")
        .remove("confirmation_mode");
    let err = validate_json(POLICY_RULE_ID, &rule).expect_err("confirm needs mode");
    assert!(!err.to_string().is_empty());
}

#[test]
fn policy_allow_forbids_confirmation_mode() {
    let mut rule = sample_policy_rule("allow");
    rule.as_object_mut()
        .expect("obj")
        .insert("confirmation_mode".into(), json!("generic"));
    let err = validate_json(POLICY_RULE_ID, &rule).expect_err("allow forbids mode");
    assert!(!err.to_string().is_empty());
}

#[test]
fn policy_deny_forbids_confirmation_mode() {
    let mut rule = sample_policy_rule("deny");
    rule.as_object_mut()
        .expect("obj")
        .insert("confirmation_mode".into(), json!("generic"));
    let err = validate_json(POLICY_RULE_ID, &rule).expect_err("deny forbids mode");
    assert!(!err.to_string().is_empty());
}

#[test]
fn policy_confirm_with_mode_is_valid() {
    let rule = sample_policy_rule("confirm");
    validate_json(POLICY_RULE_ID, &rule).expect("confirm+mode ok");
}

#[test]
fn generated_const_and_null_types_reject_other_values() {
    assert!(serde_json::from_value::<KcpCommandEnvelopeProtocolVersion>(json!("2.0")).is_err());
    assert!(serde_json::from_value::<NullOnly>(json!({})).is_err());
    let null = serde_json::from_value::<NullOnly>(Value::Null).expect("null only");
    assert_eq!(
        serde_json::to_value(null).expect("serialize null"),
        Value::Null
    );
}

#[test]
fn kcp_auth_non_null_is_rejected() {
    let mut query = sample_ping_query();
    query
        .as_object_mut()
        .expect("obj")
        .insert("auth".into(), json!({"token": "x"}));
    let err = validate_json(QUERY_ID, &query).expect_err("auth must be null");
    assert!(!err.to_string().is_empty());
}

#[test]
fn kcp_protocol_version_must_be_1_0() {
    let mut query = sample_ping_query();
    query
        .as_object_mut()
        .expect("obj")
        .insert("protocol_version".into(), json!("2.0"));
    let err = validate_json(QUERY_ID, &query).expect_err("protocol");
    assert!(!err.to_string().is_empty());
}

#[test]
fn kcp_unknown_command_method_rejected() {
    let mut cmd = sample_stop_activate_command();
    cmd.as_object_mut()
        .expect("obj")
        .insert("command_type".into(), json!("task.delete"));
    let err = validate_json(COMMAND_ID, &cmd).expect_err("unknown method");
    assert!(!err.to_string().is_empty());
}

#[test]
fn kcp_eight_methods_closed_set_constants() {
    assert_eq!(KCP_V1_METHODS.len(), 8);
    assert_eq!(KCP_PROTOCOL_VERSION, "1.0");
    assert_eq!(EVENT_V1_TYPES.len(), 3);
    assert!(KCP_V1_METHODS.contains(&"task.create"));
    assert!(KCP_V1_METHODS.contains(&"stop.activate"));
    assert!(EVENT_V1_TYPES.contains(&"task.created"));
}

#[test]
fn jcs_known_vector_object_order_and_hash() {
    use kernel_contracts::canonical_json_string;
    let value = json!({"b": 2, "a": 1});
    let canonical = canonical_json_string(&value).expect("canon");
    assert_eq!(canonical, r#"{"a":1,"b":2}"#);
    let digest = sha256_canonical(&value).expect("hash");
    // SHA-256 of RFC8785 bytes for {"a":1,"b":2}
    assert_eq!(
        digest,
        "43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777"
    );
}

#[test]
fn envelope_rejects_schema_version_and_legacy_entry() {
    let mut query = sample_ping_query();
    query
        .as_object_mut()
        .expect("object")
        .insert("schema_version".into(), json!(1));
    assert!(validate_json(QUERY_ID, &query).is_err());

    let mut query = sample_ping_query();
    let object = query.as_object_mut().expect("object");
    object.insert("entry".into(), json!("local_desktop"));
    object.remove("entry_point");
    assert!(validate_json(QUERY_ID, &query).is_err());
}

#[test]
fn typed_command_decode_rejects_method_payload_mismatch() {
    let mut command = sample_stop_activate_command();
    command.as_object_mut().expect("object").insert(
        "payload".into(),
        json!({
            "schema_version": 1,
            "proposer": "user",
            "goal": "create task",
            "constraints": [],
            "success_criteria": ["done"],
            "risk_hint": null,
            "capability_hints": [],
            "task_scope": {
                "schema_version": 1,
                "resource_patterns": [],
                "exclusions": [],
                "allowed_capability_hints": [],
                "expires_at": null
            },
            "delegation_ref": null,
            "parent_task_id": null,
            "origin": {
                "schema_version": 1,
                "kind": "user_input",
                "source_uri": null,
                "upstream_stable_id": null,
                "producer_ref": {"kind": "actor", "id": "actor-local-user-1"},
                "parent_origin_refs": []
            }
        }),
    );
    assert!(TypedKcpCommandEnvelope::decode(command).is_err());
}

#[test]
fn typed_command_decode_exposes_tagged_payload() {
    let decoded = TypedKcpCommandEnvelope::decode(sample_stop_activate_command()).expect("decode");
    assert!(matches!(
        decoded.payload,
        KcpCommandPayload::StopActivate(_)
    ));
}

#[test]
fn typed_query_decode_rejects_method_payload_mismatch() {
    let mut query = sample_ping_query();
    query
        .as_object_mut()
        .expect("object")
        .insert("query_type".into(), json!("task.get"));
    assert!(TypedKcpQueryEnvelope::decode(query).is_err());
}

#[test]
fn task_list_parent_modes_and_limit_are_enforced() {
    let base = json!({
        "schema_version": 1,
        "statuses": [],
        "parent_filter": {"mode": "any", "task_id": null},
        "proposer": null,
        "created_after": null,
        "cursor": null,
        "limit": 200
    });
    validate_json(TASK_LIST_ID, &base).expect("valid any mode");

    let mut invalid = base.clone();
    invalid["parent_filter"] = json!({"mode": "exact", "task_id": null});
    assert!(validate_json(TASK_LIST_ID, &invalid).is_err());

    let mut invalid = base;
    invalid["limit"] = json!(201);
    assert!(validate_json(TASK_LIST_ID, &invalid).is_err());
}

#[test]
fn event_cursor_is_decimal_string_only() {
    let subscribe = json!({
        "schema_version": 1,
        "event_types": [],
        "aggregate_types": [],
        "after_outbox_position": "123"
    });
    validate_json(EVENT_SUBSCRIBE_ID, &subscribe).expect("decimal cursor");
    let mut invalid = subscribe;
    invalid["after_outbox_position"] = json!("event-123");
    assert!(validate_json(EVENT_SUBSCRIBE_ID, &invalid).is_err());

    let poll = json!({
        "schema_version": 1,
        "subscription_id": "11111111-1111-4111-8111-111111111111",
        "after_outbox_position": "42",
        "limit": 1
    });
    validate_json(EVENT_POLL_ID, &poll).expect("valid poll cursor");
}

#[test]
fn response_ok_error_are_mutually_exclusive() {
    let ok = json!({
        "protocol_version": "1.0",
        "message_kind": "response",
        "request_id": "11111111-1111-4111-8111-111111111111",
        "status": "ok",
        "payload": {"schema_version": 1},
        "error": null
    });
    validate_json(RESPONSE_ID, &ok).expect("valid ok response");
    let mut invalid = ok;
    invalid["error"] = json!({
        "schema_version": 1,
        "code": "internal_error",
        "message": "x",
        "details": null,
        "retryable": false
    });
    assert!(validate_json(RESPONSE_ID, &invalid).is_err());
}

#[test]
fn catalog_has_no_stop_clear_method() {
    assert!(!KCP_V1_METHODS.contains(&"stop.clear"));
}

#[test]
fn task_create_requires_contract_fields_and_reports_payload_path() {
    let mut command = sample_stop_activate_command();
    command
        .as_object_mut()
        .expect("object")
        .insert("command_type".into(), json!("task.create"));
    let detail = validate_json(COMMAND_ID, &command)
        .expect_err("missing task.create fields")
        .to_string();
    assert!(
        detail.contains("/payload") || detail.contains("payload"),
        "{detail}"
    );
}

#[test]
fn event_payload_type_mismatch_is_rejected() {
    let event = json!({
        "event_id": "11111111-1111-4111-8111-111111111111",
        "type": "task.created",
        "schema_version": 1,
        "aggregate_type": "task",
        "aggregate_id": "22222222-2222-4222-8222-222222222222",
        "sequence": 1,
        "outbox_position": "1",
        "occurred_at": "2026-07-16T12:00:00Z",
        "causation_ref": {"kind": "command_request", "id": "request-1"},
        "correlation_id": "correlation-1",
        "dedup_key": "dedup-1",
        "payload": {
            "schema_version": 1,
            "generation": 1,
            "reason": "stop",
            "activated_by_actor_id": "actor",
            "activated_from_entry_point": "local_desktop",
            "activated_at": "2026-07-16T12:00:00Z"
        }
    });
    assert!(TypedEventEnvelope::decode(event).is_err());
}

#[test]
fn event_typed_decode_exposes_tagged_payload() {
    let event: Value = serde_json::from_str(include_str!(
        "../../../../schemas/examples/event/task_created.valid.json"
    ))
    .expect("fixture");
    let decoded = TypedEventEnvelope::decode(event["instance"].clone()).expect("typed event");
    assert!(matches!(decoded.payload, EventPayload::TaskCreated(_)));
}

#[test]
fn first_task_created_event_sequence_zero_is_valid() {
    let event: Value = serde_json::from_str(include_str!(
        "../../../../schemas/examples/event/task_created.valid.json"
    ))
    .expect("fixture");
    assert_eq!(event["instance"]["sequence"], json!(0));
    validate_json(
        "https://schemas.shittim.local/v1/event/event_envelope.json",
        &event["instance"],
    )
    .expect("sequence zero is schema-valid for a first event");
}

#[test]
fn recovery_retry_original_requires_false_and_idempotent_facts() {
    let candidate = sample_recovery_candidate();
    validate_json(RECOVERY_ID, &candidate).expect("valid retry candidate");
    let mut invalid = candidate.clone();
    invalid["facts"]["side_effect_confirmed"] = json!(null);
    assert!(validate_json(RECOVERY_ID, &invalid).is_err());
    let mut invalid = candidate;
    invalid["facts"]["original_idempotency_guaranteed"] = json!(false);
    assert!(validate_json(RECOVERY_ID, &invalid).is_err());
}

fn sample_audit_record() -> Value {
    json!({
        "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "schema_version": 1,
        "audit_type": "task.creation_recorded",
        "level": "user_activity",
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "occurred_at": "2026-07-16T12:00:00Z",
        "task_id": "22222222-2222-4222-8222-222222222222",
        "task_creation_context": {
            "task_revision": 1,
            "goal": "Organize the Downloads directory",
            "origin_ref": "33333333-3333-4333-8333-333333333333",
            "proposer": "user"
        },
        "action_id": null,
        "permission_decision_ref": null,
        "approval_record_ref": null,
        "recovery_attempt_ref": null,
        "delegation_ref": "delegation://workspace/organize-downloads/v3",
        "model_call_refs": ["model-call://provider/request-7"],
        "payload_manifest_refs": [],
        "external_content_status": "not_sent",
        "verification_result_refs": ["44444444-4444-4444-8444-444444444444"],
        "content_origin_refs": ["33333333-3333-4333-8333-333333333333"],
        "artifact_refs": [],
        "resource_refs": [],
        "extension_id": null,
        "provider_id": null,
        "causation_ref": {"kind": "command_request", "id": "request-1"},
        "correlation_id": "correlation-1",
        "rollback_capability": "unknown",
        "stop_fence_generation": null,
        "policy_context": null,
        "outcome": "succeeded",
        "reason_codes": ["accepted"],
        "summary": "Command accepted",
        "details": {"configured_body": "allowed"}
    })
}

fn sample_recovery_candidate() -> Value {
    json!({
        "id": "11111111-1111-4111-8111-111111111111",
        "schema_version": 1,
        "revision": 1,
        "task_id": "22222222-2222-4222-8222-222222222222",
        "source_action_id": "33333333-3333-4333-8333-333333333333",
        "trigger": "failed",
        "candidate_kind": "retry_original",
        "proposed_action_request": null,
        "facts": {
            "side_effect_confirmed": false,
            "original_idempotency_guaranteed": true,
            "external_query_available": true,
            "compensatable": false
        },
        "rationale": "safe retry",
        "status": "proposed",
        "permission_decision_ref": null,
        "created_at": "2026-07-16T12:00:00Z",
        "expires_at": null
    })
}

fn sample_policy_rule(effect: &str) -> Value {
    let mut rule = json!({
        "id": "rule-1",
        "schema_version": 1,
        "revision": 1,
        "name": "rule",
        "description": "d",
        "priority": 1,
        "enabled": true,
        "actor_match": {},
        "content_origin_match": {},
        "resource_match": {"scope_patterns": [], "exclude_patterns": []},
        "action_match": {"capability_ids": [], "operation_patterns": []},
        "condition": {},
        "effect": effect,
        "expires_at": null,
        "created_by": {
            "actor": sample_actor(),
            "entry_point": "local_desktop"
        },
        "updated_by": {
            "actor": sample_actor(),
            "entry_point": "local_desktop"
        },
        "created_at": "2026-07-16T12:00:00Z",
        "updated_at": "2026-07-16T12:00:00Z",
        "source": "user_defined"
    });
    if effect == "confirm" {
        rule.as_object_mut()
            .expect("obj")
            .insert("confirmation_mode".into(), json!("generic"));
    }
    rule
}

fn sample_ping_query() -> Value {
    json!({
        "protocol_version": "1.0",
        "message_kind": "query",
        "request_id": "11111111-1111-4111-8111-111111111111",
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "auth": null,
        "task_id": null,
        "deadline": "2026-07-16T12:00:00Z",
        "query_type": "system.ping",
        "payload": {"schema_version": 1, "echo": "x"}
    })
}

fn sample_stop_activate_command() -> Value {
    json!({
        "protocol_version": "1.0",
        "message_kind": "command",
        "request_id": "11111111-1111-4111-8111-111111111111",
        "actor": sample_actor(),
        "entry_point": "local_desktop",
        "auth": null,
        "task_id": null,
        "context": null,
        "deadline": "2026-07-16T12:00:00Z",
        "idempotency_key": "stop-1",
        "expected_revision": null,
        "command_type": "stop.activate",
        "payload": {
            "schema_version": 1,
            "reason": "user pressed stop",
            "origin_ref": null
        }
    })
}
