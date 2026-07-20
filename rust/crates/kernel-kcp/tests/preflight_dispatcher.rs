use chrono::{DateTime, TimeZone, Utc};
use kernel_contracts::{
    validate_json, KcpResponseEnvelopeStatus, KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS,
    KCP_ENVELOPE_AUTHORITY_QUERY_METHODS,
};
use kernel_kcp::{
    narrow_to_registered, preflight_value, BackendError, ClockError, HandlerResult,
    IdGenerationError, KernelClock, KernelIdGenerator, KnownCatalogMethodNotImplemented,
    OpaqueIdPurpose, PostCommitNotificationIntent, PreflightLocalRejectionKind, PreflightResult,
    RegisteredMethod, RegistrationResult, TaskApplicationBackend, TaskCreateBackendResult,
    TaskCreateOperation, TypedCatalogRequestFamily, TypedDispatcher, UuidPurpose,
};
use serde::Serialize;
use serde_json::{json, Value};
use static_assertions::assert_not_impl_any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use uuid::Uuid;

assert_not_impl_any!(kernel_kcp::PreflightLocalRejection: Serialize);
assert_not_impl_any!(KnownCatalogMethodNotImplemented: Serialize);
assert_not_impl_any!(kernel_kcp::RegisteredRequest: Serialize);
assert_not_impl_any!(kernel_kcp::TypedCatalogRequest: Serialize);

// V2 Envelope Schema requires lowercase UUID text.
const REQUEST_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const RESPONSE_SCHEMA: &str = "https://schemas.shittim.local/v1/kcp/response_envelope.json";

fn actor() -> Value {
    json!({
        "schema_version": 1,
        "revision": 1,
        "id": "actor",
        "kind": "known_user",
        "source": "actor-source://local/desktop",
        "authentication_level": "platform_verified",
        "confidence": 0.9
    })
}

fn query(method: &str, payload: Value) -> Value {
    json!({
        "protocol_version": "1.0",
        "message_kind": "query",
        "request_id": REQUEST_ID,
        "actor": actor(),
        "entry_point": "local_desktop",
        "auth": null,
        "task_id": null,
        "deadline": "2026-07-18T12:00:10Z",
        "query_type": method,
        "payload": payload
    })
}

fn command(method: &str, payload: Value) -> Value {
    json!({
        "protocol_version": "1.0",
        "message_kind": "command",
        "request_id": REQUEST_ID,
        "actor": actor(),
        "entry_point": "local_desktop",
        "auth": null,
        "task_id": null,
        "context": null,
        "deadline": "2026-07-18T12:00:10Z",
        "idempotency_key": "key",
        "expected_revision": null,
        "command_type": method,
        "payload": payload
    })
}

fn valid_values() -> Vec<(&'static str, TypedCatalogRequestFamily, Value)> {
    vec![
        (
            "system.ping",
            TypedCatalogRequestFamily::Query,
            query("system.ping", json!({"schema_version":1,"echo":"hello"})),
        ),
        (
            "task.create",
            TypedCatalogRequestFamily::Command,
            command("task.create", task_create_payload_v2()),
        ),
        (
            "task.get",
            TypedCatalogRequestFamily::Query,
            query(
                "task.get",
                json!({"schema_version":1,"task_id":"00000000-0000-4000-8000-000000000001"}),
            ),
        ),
        (
            "task.list",
            TypedCatalogRequestFamily::Query,
            query(
                "task.list",
                json!({"schema_version":1,"statuses":[],"parent_filter":{"mode":"any","task_id":null},"proposer":null,"created_after":null,"cursor":null,"limit":20}),
            ),
        ),
        (
            "event.subscribe",
            TypedCatalogRequestFamily::Query,
            query(
                "event.subscribe",
                json!({"schema_version":1,"event_types":[],"aggregate_types":[],"after_outbox_position":null}),
            ),
        ),
        (
            "event.poll",
            TypedCatalogRequestFamily::Query,
            query(
                "event.poll",
                json!({"schema_version":1,"subscription_id":"00000000-0000-4000-8000-000000000002","after_outbox_position":"0","limit":10}),
            ),
        ),
        (
            "stop.activate",
            TypedCatalogRequestFamily::Command,
            command(
                "stop.activate",
                json!({"schema_version":1,"reason":"stop now","origin_ref":null}),
            ),
        ),
        (
            "stop.status",
            TypedCatalogRequestFamily::Query,
            query("stop.status", json!({"schema_version":1})),
        ),
    ]
}

fn task_create_payload_v2() -> Value {
    json!({
        "schema_version":2,
        "proposer":"user",
        "goal":"goal",
        "constraints":[],
        "success_criteria":["done"],
        "risk_hint":null,
        "capability_hints":[],
        "task_scope":{
            "schema_version":1,
            "resource_patterns":[],
            "exclusions":[],
            "allowed_capability_hints":[],
            "expires_at":null
        },
        "delegation_ref":null,
        "origin":{
            "schema_version":1,
            "kind":"user_input",
            "source_uri":null,
            "upstream_stable_id":null,
            "producer_ref":{"kind":"actor","id":"actor"},
            "parent_origin_refs":[]
        }
    })
}

fn task_create_payload_v1() -> Value {
    json!({
        "schema_version":1,
        "proposer":"user",
        "goal":"goal",
        "constraints":[],
        "success_criteria":["done"],
        "risk_hint":null,
        "capability_hints":[],
        "task_scope":{
            "schema_version":1,
            "resource_patterns":[],
            "exclusions":[],
            "allowed_capability_hints":[],
            "expires_at":null
        },
        "delegation_ref":null,
        "parent_task_id":null,
        "origin":{
            "schema_version":1,
            "kind":"user_input",
            "source_uri":null,
            "upstream_stable_id":null,
            "producer_ref":{"kind":"actor","id":"actor"},
            "parent_origin_refs":[]
        }
    })
}

fn response_error(value: Value) -> kernel_contracts::KcpError {
    match preflight_value(value) {
        PreflightResult::Response(response) => {
            assert_eq!(response.request_id, REQUEST_ID);
            assert_eq!(response.status, KcpResponseEnvelopeStatus::Error);
            assert!(response.payload.is_none());
            let response_value = serde_json::to_value(&response).expect("response value");
            validate_json(RESPONSE_SCHEMA, &response_value).expect("valid response envelope");
            response.error.expect("error")
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

#[test]
fn all_eight_valid_values_are_accepted_with_exact_method_and_family() {
    for (method, family, value) in valid_values() {
        let PreflightResult::Accepted(request) = preflight_value(value) else {
            panic!("{method} not accepted")
        };
        assert_eq!(request.method(), method);
        assert_eq!(request.family(), family);
    }
    assert_eq!(
        KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS.len() + KCP_ENVELOPE_AUTHORITY_QUERY_METHODS.len(),
        valid_values().len()
    );
}

#[test]
fn method_aware_version_matrix_for_create_and_retained_v1() {
    // Active create v2 → Accepted.
    let PreflightResult::Accepted(create_v2) =
        preflight_value(command("task.create", task_create_payload_v2()))
    else {
        panic!("create v2 accepted")
    };
    assert_eq!(create_v2.method(), "task.create");

    // Known legacy create v1 → unsupported_schema_version (never Accepted).
    assert_eq!(
        response_error(command("task.create", task_create_payload_v1())).code,
        "unsupported_schema_version"
    );

    // Unknown create version → unsupported_schema_version.
    let mut unknown = command("task.create", task_create_payload_v2());
    unknown["payload"]["schema_version"] = json!(3);
    assert_eq!(response_error(unknown).code, "unsupported_schema_version");

    // Remaining seven methods stay on active v1.
    for (method, _, value) in valid_values() {
        if method == "task.create" {
            continue;
        }
        let PreflightResult::Accepted(request) = preflight_value(value) else {
            panic!("{method} active v1 accepted")
        };
        assert_eq!(request.method(), method);
    }

    // system.ping v2 is unsupported.
    assert_eq!(
        response_error(query(
            "system.ping",
            json!({"schema_version":2,"echo":null})
        ))
        .code,
        "unsupported_schema_version"
    );
}

#[test]
fn request_id_eligibility_is_local_and_valid_text_is_preserved_verbatim() {
    for value in [
        Value::Null,
        json!({}),
        json!({"request_id":7}),
        json!({"request_id":"not-a-uuid"}),
    ] {
        let PreflightResult::LocalRejection(rejection) = preflight_value(value) else {
            panic!("local rejection")
        };
        assert_eq!(
            rejection.kind,
            PreflightLocalRejectionKind::UncorrelatableRequest
        );
        assert_eq!(rejection.message, "request cannot be correlated");
    }
    let response = match preflight_value(json!({
        "request_id": REQUEST_ID,
        "message_kind":"response"
    })) {
        PreflightResult::Response(response) => response,
        other => panic!("response: {other:?}"),
    };
    assert_eq!(response.request_id, REQUEST_ID);
}

#[test]
fn priority_matrix_short_circuits_in_contract_order() {
    let mut value = query("unknown.method", json!({"schema_version":2}));
    value["protocol_version"] = json!("2.0");
    value["auth"] = json!({"token":"x"});
    assert_eq!(
        response_error(value.clone()).code,
        "unsupported_protocol_version"
    );

    value["protocol_version"] = json!("1.0");
    assert_eq!(
        response_error(value.clone()).code,
        "unsupported_auth_schema"
    );
    value["auth"] = Value::Null;
    assert_eq!(response_error(value.clone()).code, "unsupported_method");
    value["query_type"] = json!("system.ping");
    assert_eq!(
        response_error(value.clone()).code,
        "unsupported_schema_version"
    );
    value["payload"]["schema_version"] = json!(1);
    assert_eq!(response_error(value).code, "invalid_request");

    let mut family_first = query("unknown.method", json!({"schema_version":2}));
    family_first["message_kind"] = json!("response");
    family_first["protocol_version"] = json!("2.0");
    assert_eq!(response_error(family_first).code, "invalid_request");
}

#[test]
fn every_preflight_field_type_and_value_has_stable_classification() {
    let cases = [
        ("message_kind", Value::Null, "invalid_request"),
        ("message_kind", json!("response"), "invalid_request"),
        ("protocol_version", Value::Null, "invalid_request"),
        (
            "protocol_version",
            json!("2.0"),
            "unsupported_protocol_version",
        ),
        ("auth", json!({}), "unsupported_auth_schema"),
        ("query_type", Value::Null, "invalid_request"),
        ("query_type", json!("missing"), "unsupported_method"),
        ("payload", Value::Null, "invalid_request"),
    ];
    for (field, replacement, expected) in cases {
        let mut value = query("system.ping", json!({"schema_version":1,"echo":null}));
        value[field] = replacement;
        assert_eq!(response_error(value).code, expected, "field {field}");
    }
    for missing in [
        "message_kind",
        "protocol_version",
        "auth",
        "query_type",
        "payload",
    ] {
        let mut value = query("system.ping", json!({"schema_version":1,"echo":null}));
        value.as_object_mut().expect("object").remove(missing);
        assert_eq!(response_error(value).code, "invalid_request", "{missing}");
    }
}

#[test]
fn cross_family_methods_are_unsupported_method_from_generated_catalogs() {
    assert_eq!(
        response_error(query("task.create", task_create_payload_v2())).code,
        "unsupported_method"
    );
    assert_eq!(
        response_error(command(
            "task.get",
            json!({"schema_version":1,"task_id":"00000000-0000-4000-8000-000000000001"})
        ))
        .code,
        "unsupported_method"
    );
}

#[test]
fn opposite_family_discriminator_is_a_full_schema_invalid_request() {
    let mut query_value = query("system.ping", json!({"schema_version":1,"echo":null}));
    query_value["command_type"] = json!("task.create");
    assert_eq!(response_error(query_value).code, "invalid_request");

    let mut command_value = command("task.create", task_create_payload_v2());
    command_value["query_type"] = json!("task.get");
    assert_eq!(response_error(command_value).code, "invalid_request");
}

#[test]
fn root_schema_version_number_rules_are_exact() {
    let versions = [
        (json!(1.0), "invalid_request"),
        (json!(-1), "invalid_request"),
        (json!(0), "invalid_request"),
        (json!(2), "unsupported_schema_version"),
    ];
    for (version, expected) in versions {
        let mut value = query("system.ping", json!({"schema_version":1,"echo":null}));
        value["payload"]["schema_version"] = version;
        assert_eq!(response_error(value).code, expected);
    }
    let large: Value = serde_json::from_str(
        r#"{"protocol_version":"1.0","message_kind":"query","request_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","actor":{"schema_version":1,"revision":1,"id":"actor","kind":"known_user","source":"actor-source://local/desktop","authentication_level":"platform_verified","confidence":0.9},"entry_point":"local_desktop","auth":null,"task_id":null,"deadline":"2026-07-18T12:00:10Z","query_type":"system.ping","payload":{"schema_version":18446744073709551616,"echo":null}}"#,
    )
    .expect("JSON number retained as Value");
    let PreflightResult::Response(response) = preflight_value(large) else {
        panic!("large number invalid response")
    };
    assert_eq!(response.error.expect("error").code, "invalid_request");
}

#[test]
fn nested_versions_and_business_failures_are_invalid_request() {
    let mut create = command("task.create", task_create_payload_v2());
    create["payload"]["task_scope"]["schema_version"] = json!(2);
    assert_eq!(response_error(create).code, "invalid_request");

    // parent_task_id is forbidden on v2 create (unknown field).
    let mut with_parent = command("task.create", task_create_payload_v2());
    with_parent["payload"]["parent_task_id"] = json!(null);
    assert_eq!(response_error(with_parent).code, "invalid_request");

    let mut list = query(
        "task.list",
        json!({"schema_version":1,"statuses":[],"parent_filter":{"mode":"exact","task_id":null},"proposer":null,"created_after":null,"cursor":null,"limit":20}),
    );
    assert_eq!(response_error(list.clone()).code, "invalid_request");
    list["payload"]["limit"] = json!(201);
    assert_eq!(response_error(list).code, "invalid_request");
}

#[test]
fn malformed_known_methods_fail_before_registration_and_valid_set_is_exact() {
    for (method, family, mut value) in valid_values() {
        if matches!(
            method,
            "task.list" | "event.subscribe" | "event.poll" | "stop.activate" | "stop.status"
        ) {
            value["payload"] = json!({"schema_version":1});
            if method == "stop.status" {
                value["payload"]["extra"] = json!(true);
            }
            assert_eq!(response_error(value).code, "invalid_request", "{method}");
            let _ = family;
        }
    }

    let expected = [
        ("system.ping", Some(RegisteredMethod::SystemPing), None),
        ("task.create", Some(RegisteredMethod::TaskCreate), None),
        ("task.get", Some(RegisteredMethod::TaskGet), None),
        (
            "task.list",
            None,
            Some(KnownCatalogMethodNotImplemented::TaskList),
        ),
        (
            "event.subscribe",
            None,
            Some(KnownCatalogMethodNotImplemented::EventSubscribe),
        ),
        (
            "event.poll",
            None,
            Some(KnownCatalogMethodNotImplemented::EventPoll),
        ),
        (
            "stop.activate",
            None,
            Some(KnownCatalogMethodNotImplemented::StopActivate),
        ),
        (
            "stop.status",
            None,
            Some(KnownCatalogMethodNotImplemented::StopStatus),
        ),
    ];
    for ((method, _, value), (expected_method, registered, known)) in
        valid_values().into_iter().zip(expected)
    {
        assert_eq!(method, expected_method);
        let PreflightResult::Accepted(request) = preflight_value(value) else {
            panic!("accepted")
        };
        match narrow_to_registered(request) {
            RegistrationResult::Registered(request) => {
                assert_eq!(Some(request.method()), registered);
                assert!(known.is_none());
            }
            RegistrationResult::KnownCatalogMethodNotImplemented(actual) => {
                assert_eq!(Some(actual), known);
                assert!(registered.is_none());
            }
            RegistrationResult::InternalContractViolation(violation) => {
                panic!("unexpected internal contract violation {violation:?}")
            }
        }
    }
}

#[test]
fn fixed_wire_error_table_and_response_schema_are_exact() {
    let cases = [
        (
            query("system.ping", json!({"schema_version":1})),
            "invalid_request",
            "request is invalid",
        ),
        (
            {
                let mut value = query("system.ping", json!({"schema_version":1,"echo":null}));
                value["protocol_version"] = json!("2.0");
                value
            },
            "unsupported_protocol_version",
            "protocol version is not supported",
        ),
        (
            query("system.ping", json!({"schema_version":2,"echo":null})),
            "unsupported_schema_version",
            "payload schema version is not supported",
        ),
        (
            command("task.create", task_create_payload_v1()),
            "unsupported_schema_version",
            "payload schema version is not supported",
        ),
        (
            query("missing", json!({"schema_version":1})),
            "unsupported_method",
            "method is not supported",
        ),
        (
            {
                let mut value = query("system.ping", json!({"schema_version":1,"echo":null}));
                value["auth"] = json!({});
                value
            },
            "unsupported_auth_schema",
            "authentication schema is not supported",
        ),
    ];
    for (value, code, message) in cases {
        let error = response_error(value);
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(
            error.schema_version,
            kernel_contracts::KcpErrorSchemaVersion
        );
        assert!(error.details.is_none());
        assert!(!error.retryable);
    }
}

#[derive(Clone)]
struct Clock {
    values: RefCell<VecDeque<DateTime<Utc>>>,
    calls: Cell<usize>,
}

impl KernelClock for Clock {
    fn now_utc(&self) -> Result<DateTime<Utc>, ClockError> {
        self.calls.set(self.calls.get() + 1);
        self.values.borrow_mut().pop_front().ok_or(ClockError)
    }
}

struct Ids;
impl KernelIdGenerator for Ids {
    fn next_uuid(&self, _purpose: UuidPurpose) -> Result<String, IdGenerationError> {
        Err(IdGenerationError)
    }
    fn next_opaque_id(&self, _purpose: OpaqueIdPurpose) -> Result<String, IdGenerationError> {
        Err(IdGenerationError)
    }
}

struct Backend {
    gets: Cell<usize>,
}
impl TaskApplicationBackend for Backend {
    fn create_task(
        &self,
        _operation: TaskCreateOperation,
    ) -> Result<TaskCreateBackendResult, BackendError> {
        Err(BackendError::Internal)
    }
    fn get_task(&self, _task_id: Uuid) -> Result<Option<kernel_contracts::TaskSpec>, BackendError> {
        self.gets.set(self.gets.get() + 1);
        Ok(None)
    }
}

struct CreateIds(Cell<u32>);
impl KernelIdGenerator for CreateIds {
    fn next_uuid(&self, _purpose: UuidPurpose) -> Result<String, IdGenerationError> {
        let next = self.0.replace(self.0.get() + 1);
        Ok(format!("00000000-0000-4000-8000-{next:012}"))
    }
    fn next_opaque_id(&self, purpose: OpaqueIdPurpose) -> Result<String, IdGenerationError> {
        Ok(match purpose {
            OpaqueIdPurpose::Correlation => "dispatcher-correlation".into(),
            OpaqueIdPurpose::EventDedup => "dispatcher-dedup".into(),
        })
    }
}

struct CreatedBackend;
impl TaskApplicationBackend for CreatedBackend {
    fn create_task(
        &self,
        operation: TaskCreateOperation,
    ) -> Result<TaskCreateBackendResult, BackendError> {
        Ok(TaskCreateBackendResult::Created {
            current_task: task(&operation.task_id.to_string()),
            creation_provenance_ref: operation.creation_provenance_id.to_string(),
            committed_event_id: operation.event_id,
        })
    }
    fn get_task(&self, _task_id: Uuid) -> Result<Option<kernel_contracts::TaskSpec>, BackendError> {
        Err(BackendError::Internal)
    }
}

fn task(id: &str) -> kernel_contracts::TaskSpec {
    serde_json::from_value(json!({
        "id":id,
        "origin_ref":"30000000-0000-4000-8000-000000000001",
        "actor":actor(),
        "proposer":"user",
        "goal":"goal",
        "constraints":[],
        "success_criteria":["done"],
        "risk_hint":null,
        "capability_hints":[],
        "delegation_ref":null,
        "task_scope_ref":"20000000-0000-4000-8000-000000000001",
        "parent_task_id":null,
        "status":"candidate",
        "plan_version":0,
        "schema_version":1,
        "revision":1,
        "created_at":"2026-07-18T12:00:01Z",
        "updated_at":"2026-07-18T12:00:01Z",
        "failed_recovery_meta":null
    }))
    .expect("valid task")
}

fn instant(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, second)
        .single()
        .expect("time")
}

#[test]
fn dispatcher_preserves_created_intent_exactly() {
    let clock = Clock {
        values: RefCell::new([instant(1), instant(2)].into()),
        calls: Cell::new(0),
    };
    let ids = CreateIds(Cell::new(1));
    let backend = CreatedBackend;
    let dispatcher = TypedDispatcher::new(&clock, &ids, &backend);
    let request = match preflight_value(command("task.create", task_create_payload_v2())) {
        PreflightResult::Accepted(request) => match narrow_to_registered(request) {
            RegistrationResult::Registered(request) => request,
            _ => panic!("registered"),
        },
        _ => panic!("accepted"),
    };
    let HandlerResult::Response(response) = dispatcher.dispatch(request) else {
        panic!("response")
    };
    assert_eq!(clock.calls.get(), 2);
    assert_eq!(
        response.post_commit_notification_intents,
        vec![PostCommitNotificationIntent::TaskCreatedCommitted {
            task_id: "00000000-0000-4000-8000-000000000001".into(),
            // Seven UUID purposes: Task, Scope, Origin, Receipt, Provenance, Audit, Event.
            event_id: Uuid::parse_str("00000000-0000-4000-8000-000000000007").expect("event uuid"),
        }]
    );
    assert_eq!(
        response.response.payload.as_ref().unwrap()["creation_provenance_ref"],
        "00000000-0000-4000-8000-000000000005"
    );
}

#[test]
fn dispatcher_routes_registered_requests_without_extra_clock_or_rewriting() {
    let clock = Clock {
        values: RefCell::new([instant(1), instant(2), instant(3), instant(4)].into()),
        calls: Cell::new(0),
    };
    let backend = Backend { gets: Cell::new(0) };
    let ids = Ids;
    let dispatcher = TypedDispatcher::new(&clock, &ids, &backend);

    let ping = match preflight_value(query(
        "system.ping",
        json!({"schema_version":1,"echo":"dispatcher"}),
    )) {
        PreflightResult::Accepted(request) => match narrow_to_registered(request) {
            RegistrationResult::Registered(request) => request,
            _ => panic!("registered"),
        },
        _ => panic!("accepted"),
    };
    let HandlerResult::Response(ping_response) = dispatcher.dispatch(ping) else {
        panic!("ping response")
    };
    assert_eq!(ping_response.response.status, KcpResponseEnvelopeStatus::Ok);
    assert_eq!(clock.calls.get(), 2);

    let get = match preflight_value(query(
        "task.get",
        json!({"schema_version":1,"task_id":"00000000-0000-4000-8000-000000000001"}),
    )) {
        PreflightResult::Accepted(request) => match narrow_to_registered(request) {
            RegistrationResult::Registered(request) => request,
            _ => panic!("registered"),
        },
        _ => panic!("accepted"),
    };
    let HandlerResult::Response(get_response) = dispatcher.dispatch(get) else {
        panic!("get response")
    };
    assert_eq!(
        get_response.response.error.expect("not found").code,
        "task_not_found"
    );
    assert_eq!(clock.calls.get(), 4);
    assert_eq!(backend.gets.get(), 1);
}
