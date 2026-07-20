use chrono::{DateTime, TimeZone, Utc};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, EntryPoint,
    InputContentOriginV1, InputContentOriginV1Kind, InputContentOriginV1ProducerRef,
    InputContentOriginV1ProducerRefKind, InputContentOriginV1SchemaVersion, InputTaskScopeV1,
    InputTaskScopeV1SchemaVersion, KcpError, KcpQueryPayload, KcpResponseEnvelopeStatus,
    NormalizedRootTaskCreatePayloadV2Proposer, NullOnly, SystemPingRequest,
    SystemPingRequestSchemaVersion, TaskCreateRequestV2, TaskCreateRequestV2SchemaVersion,
    TaskGetRequest, TaskGetRequestSchemaVersion, TaskSpec, TypedKcpQueryEnvelope,
};
use kernel_kcp::{
    handle_system_ping, handle_task_create, handle_task_get, BackendError, ClockError,
    HandlerContractFailureKind, HandlerResult, IdGenerationError, KernelClock, KernelIdGenerator,
    OpaqueIdPurpose, PostCommitNotificationIntent, TaskApplicationBackend, TaskCreateBackendResult,
    TaskCreateCommandRequestV2, TaskCreateOperation, UuidPurpose,
};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use uuid::Uuid;

type ClockScript = Rc<RefCell<VecDeque<Result<DateTime<Utc>, ClockError>>>>;

#[derive(Clone)]
struct ScriptClock {
    values: ClockScript,
    log: Rc<RefCell<Vec<String>>>,
}

impl KernelClock for ScriptClock {
    fn now_utc(&self) -> Result<DateTime<Utc>, ClockError> {
        self.log.borrow_mut().push("clock".into());
        self.values
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(ClockError))
    }
}

struct FakeIds {
    uuids: RefCell<VecDeque<Result<String, IdGenerationError>>>,
    opaque: RefCell<VecDeque<Result<String, IdGenerationError>>>,
    purposes: RefCell<Vec<String>>,
    log: Rc<RefCell<Vec<String>>>,
}

impl KernelIdGenerator for FakeIds {
    fn next_uuid(&self, purpose: UuidPurpose) -> Result<String, IdGenerationError> {
        self.log.borrow_mut().push("id".into());
        self.purposes.borrow_mut().push(format!("{purpose:?}"));
        self.uuids
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(IdGenerationError))
    }

    fn next_opaque_id(&self, purpose: OpaqueIdPurpose) -> Result<String, IdGenerationError> {
        self.log.borrow_mut().push("opaque".into());
        self.purposes.borrow_mut().push(format!("{purpose:?}"));
        self.opaque
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(IdGenerationError))
    }
}

struct FakeBackend {
    create: RefCell<Option<Result<TaskCreateBackendResult, BackendError>>>,
    get: RefCell<Option<Result<Option<TaskSpec>, BackendError>>>,
    create_calls: Cell<usize>,
    get_calls: Cell<usize>,
    operation: RefCell<Option<TaskCreateOperation>>,
    log: Rc<RefCell<Vec<String>>>,
}

impl TaskApplicationBackend for FakeBackend {
    fn create_task(
        &self,
        operation: TaskCreateOperation,
    ) -> Result<TaskCreateBackendResult, BackendError> {
        self.log.borrow_mut().push("backend.create".into());
        self.create_calls.set(self.create_calls.get() + 1);
        *self.operation.borrow_mut() = Some(operation);
        self.create
            .borrow_mut()
            .take()
            .unwrap_or(Err(BackendError::Internal))
    }

    fn get_task(&self, _task_id: Uuid) -> Result<Option<TaskSpec>, BackendError> {
        self.log.borrow_mut().push("backend.get".into());
        self.get_calls.set(self.get_calls.get() + 1);
        self.get
            .borrow_mut()
            .take()
            .unwrap_or(Err(BackendError::Internal))
    }
}

fn instant(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, second)
        .single()
        .unwrap()
}

fn clock(
    values: Vec<Result<DateTime<Utc>, ClockError>>,
    log: Rc<RefCell<Vec<String>>>,
) -> ScriptClock {
    ScriptClock {
        values: Rc::new(RefCell::new(values.into())),
        log,
    }
}

fn ids(log: Rc<RefCell<Vec<String>>>) -> FakeIds {
    FakeIds {
        uuids: RefCell::new(
            (1..=7)
                .map(|n| Ok(format!("00000000-0000-4000-8000-{n:012}")))
                .collect(),
        ),
        opaque: RefCell::new([Ok("correlation".into()), Ok("dedup".into())].into()),
        purposes: RefCell::new(vec![]),
        log,
    }
}

fn backend(log: Rc<RefCell<Vec<String>>>) -> FakeBackend {
    FakeBackend {
        create: RefCell::new(None),
        get: RefCell::new(None),
        create_calls: Cell::new(0),
        get_calls: Cell::new(0),
        operation: RefCell::new(None),
        log,
    }
}

fn actor() -> Actor {
    Actor {
        authentication_level: ActorAuthenticationLevel::PlatformVerified,
        confidence: Some(0.9),
        id: "actor".into(),
        kind: ActorKind::KnownUser,
        revision: 1,
        schema_version: ActorSchemaVersion,
        source: "actor-source://local/desktop".into(),
    }
}

fn ping(deadline: &str, echo: Option<&str>) -> TypedKcpQueryEnvelope {
    TypedKcpQueryEnvelope {
        actor: actor(),
        auth: NullOnly,
        deadline: deadline.into(),
        entry_point: EntryPoint::LocalDesktop,
        message_kind: kernel_contracts::KcpQueryEnvelopeMessageKind::Value,
        protocol_version: kernel_contracts::KcpQueryEnvelopeProtocolVersion::Value,
        request_id: "10000000-0000-4000-8000-000000000001".into(),
        task_id: None,
        query_type: "system.ping".into(),
        payload: KcpQueryPayload::SystemPing(Box::new(SystemPingRequest {
            echo: echo.map(str::to_owned),
            schema_version: SystemPingRequestSchemaVersion,
        })),
    }
}

fn get_envelope(deadline: &str) -> TypedKcpQueryEnvelope {
    TypedKcpQueryEnvelope {
        actor: actor(),
        auth: NullOnly,
        deadline: deadline.into(),
        entry_point: EntryPoint::LocalDesktop,
        message_kind: kernel_contracts::KcpQueryEnvelopeMessageKind::Value,
        protocol_version: kernel_contracts::KcpQueryEnvelopeProtocolVersion::Value,
        request_id: "10000000-0000-4000-8000-000000000002".into(),
        task_id: None,
        query_type: "task.get".into(),
        payload: KcpQueryPayload::TaskGet(Box::new(TaskGetRequest {
            schema_version: TaskGetRequestSchemaVersion,
            task_id: "00000000-0000-4000-8000-000000000001".into(),
        })),
    }
}

fn create_request(deadline: &str) -> TaskCreateCommandRequestV2 {
    TaskCreateCommandRequestV2 {
        actor: actor(),
        auth: NullOnly,
        context: Some(json!({"conversation":1})),
        deadline: deadline.into(),
        entry_point: EntryPoint::LocalDesktop,
        expected_revision: None,
        idempotency_key: "key".into(),
        request_id: "10000000-0000-4000-8000-000000000003".into(),
        task_id: None,
        command_type: "task.create".into(),
        payload: TaskCreateRequestV2 {
            capability_hints: vec!["filesystem.read".into()],
            constraints: vec!["keep".into()],
            delegation_ref: None,
            goal: "goal".into(),
            origin: InputContentOriginV1 {
                kind: InputContentOriginV1Kind::UserInput,
                parent_origin_refs: vec![],
                producer_ref: InputContentOriginV1ProducerRef {
                    id: "actor".into(),
                    kind: InputContentOriginV1ProducerRefKind::Actor,
                },
                schema_version: InputContentOriginV1SchemaVersion,
                source_uri: Some("https://example.com/inbox".into()),
                upstream_stable_id: None,
            },
            proposer: NormalizedRootTaskCreatePayloadV2Proposer::User,
            risk_hint: None,
            schema_version: TaskCreateRequestV2SchemaVersion,
            success_criteria: vec!["done".into()],
            task_scope: InputTaskScopeV1 {
                allowed_capability_hints: vec!["filesystem.read".into()],
                exclusions: vec![],
                expires_at: None,
                resource_patterns: vec!["https://example.com/a/**".into()],
                schema_version: InputTaskScopeV1SchemaVersion,
            },
        },
    }
}

fn task(id: &str) -> TaskSpec {
    serde_json::from_value(json!({"id":id,"origin_ref":"30000000-0000-4000-8000-000000000001","actor":actor(),"proposer":"user","goal":"goal","constraints":["keep"],"success_criteria":["done"],"risk_hint":null,"capability_hints":["filesystem.read"],"delegation_ref":null,"task_scope_ref":"20000000-0000-4000-8000-000000000001","parent_task_id":null,"status":"candidate","plan_version":0,"schema_version":1,"revision":1,"created_at":"2026-07-18T12:00:00Z","updated_at":"2026-07-18T12:00:00Z","failed_recovery_meta":null})).unwrap()
}

fn response_error(result: HandlerResult) -> KcpError {
    match result {
        HandlerResult::Response(value) => value.response.error.unwrap(),
        _ => panic!("expected response"),
    }
}

#[test]
fn ping_success_uses_first_clock_and_validated_envelope() {
    let log = Rc::new(RefCell::new(vec![]));
    let result = handle_system_ping(
        &ping("2026-07-18T12:00:10Z", Some(" hi ")),
        &clock(vec![Ok(instant(1)), Ok(instant(2))], log.clone()),
    );
    let HandlerResult::Response(value) = result else {
        panic!("response")
    };
    assert_eq!(
        value.response.request_id,
        "10000000-0000-4000-8000-000000000001"
    );
    assert_eq!(value.response.status, KcpResponseEnvelopeStatus::Ok);
    assert_eq!(
        value.response.payload.as_ref().unwrap()["kernel_time"],
        "2026-07-18T12:00:01Z"
    );
    let envelope_json = serde_json::to_value(&value.response).unwrap();
    assert!(envelope_json.get("query_type").is_none());
    assert!(envelope_json.get("command_type").is_none());
    assert_eq!(&*log.borrow(), &["clock", "clock"]);
}

#[test]
fn ping_entry_deadline_equality_reads_clock_once() {
    let log = Rc::new(RefCell::new(vec![]));
    assert_eq!(
        response_error(handle_system_ping(
            &ping("2026-07-18T12:00:01Z", None),
            &clock(vec![Ok(instant(1))], log.clone()),
        ))
        .code,
        "deadline_exceeded"
    );
    assert_eq!(&*log.borrow(), &["clock"]);
}

#[test]
fn ping_second_clock_failure_is_internal() {
    let log = Rc::new(RefCell::new(vec![]));
    assert_eq!(
        response_error(handle_system_ping(
            &ping("2026-07-18T12:00:10Z", None),
            &clock(vec![Ok(instant(1)), Err(ClockError)], log.clone()),
        ))
        .code,
        "internal_error"
    );
    assert_eq!(&*log.borrow(), &["clock", "clock"]);
}

#[test]
fn ping_invalid_deadline_is_internal_after_one_clock() {
    let log = Rc::new(RefCell::new(vec![]));
    assert_eq!(
        response_error(handle_system_ping(
            &ping("not-a-deadline", None),
            &clock(vec![Ok(instant(1))], log.clone()),
        ))
        .code,
        "internal_error"
    );
    assert_eq!(&*log.borrow(), &["clock"]);
}

#[test]
fn task_get_entry_deadline_equality_skips_backend() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    assert_eq!(
        response_error(handle_task_get(
            &get_envelope("2026-07-18T12:00:01Z"),
            &clock(vec![Ok(instant(1))], log.clone()),
            &fake_backend,
        ))
        .code,
        "deadline_exceeded"
    );
    assert_eq!(fake_backend.get_calls.get(), 0);
    assert_eq!(&*log.borrow(), &["clock"]);
}

#[test]
fn task_get_invalid_deadline_is_internal_without_backend() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    assert_eq!(
        response_error(handle_task_get(
            &get_envelope("not-a-deadline"),
            &clock(vec![Ok(instant(1))], log.clone()),
            &fake_backend,
        ))
        .code,
        "internal_error"
    );
    assert_eq!(fake_backend.get_calls.get(), 0);
    assert_eq!(&*log.borrow(), &["clock"]);
}

#[test]
fn first_observable_operation_is_clock_and_entry_equality_expires() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    let result = handle_task_create(
        &create_request("2026-07-18T12:00:01Z"),
        &clock(vec![Ok(instant(1))], log.clone()),
        &ids(log.clone()),
        &fake_backend,
    );
    assert_eq!(response_error(result).code, "deadline_exceeded");
    assert_eq!(&*log.borrow(), &["clock"]);
    assert_eq!(fake_backend.create_calls.get(), 0);
}

#[test]
fn invalid_deadline_and_clock_failure_prevent_ids_and_backend() {
    for values in [vec![Ok(instant(1))], vec![Err(ClockError)]] {
        let log = Rc::new(RefCell::new(vec![]));
        let fake_backend = backend(log.clone());
        let request = create_request(if values[0].is_ok() {
            "bad"
        } else {
            "2026-07-18T12:00:10Z"
        });
        assert_eq!(
            response_error(handle_task_create(
                &request,
                &clock(values, log.clone()),
                &ids(log.clone()),
                &fake_backend
            ))
            .code,
            "internal_error"
        );
        assert_eq!(fake_backend.create_calls.get(), 0);
        assert_eq!(&*log.borrow(), &["clock"]);
    }
}

#[test]
fn create_root_only_rejects_task_id_and_expected_revision() {
    for mutate in [
        |request: &mut TaskCreateCommandRequestV2| {
            request.task_id = Some("00000000-0000-4000-8000-000000000099".into());
        },
        |request: &mut TaskCreateCommandRequestV2| {
            request.expected_revision = Some(1);
        },
    ] {
        let log = Rc::new(RefCell::new(vec![]));
        let fake_backend = backend(log.clone());
        let mut request = create_request("2026-07-18T12:00:10Z");
        mutate(&mut request);
        let error = response_error(handle_task_create(
            &request,
            &clock(vec![Ok(instant(1))], log.clone()),
            &ids(log),
            &fake_backend,
        ));
        assert_eq!(error.code, "invalid_request");
        assert_eq!(fake_backend.create_calls.get(), 0);
    }
}

#[test]
fn create_generates_seven_uuid_purposes_maps_operation_and_created_intent() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    *fake_backend.create.borrow_mut() = Some(Ok(TaskCreateBackendResult::Created {
        current_task: task("00000000-0000-4000-8000-000000000001"),
        creation_provenance_ref: "00000000-0000-4000-8000-000000000005".into(),
        committed_event_id: Uuid::parse_str("00000000-0000-4000-8000-000000000007").unwrap(),
    }));
    let fake_ids = ids(log.clone());
    let result = handle_task_create(
        &create_request("2026-07-18T12:00:10Z"),
        &clock(vec![Ok(instant(1)), Ok(instant(2))], log.clone()),
        &fake_ids,
        &fake_backend,
    );
    let HandlerResult::Response(value) = result else {
        panic!("response")
    };
    assert_eq!(value.response.status, KcpResponseEnvelopeStatus::Ok);
    assert_eq!(
        value.response.payload.as_ref().unwrap()["schema_version"],
        2
    );
    assert_eq!(
        value.response.payload.as_ref().unwrap()["creation_provenance_ref"],
        "00000000-0000-4000-8000-000000000005"
    );
    assert_eq!(
        value.post_commit_notification_intents,
        vec![PostCommitNotificationIntent::TaskCreatedCommitted {
            task_id: "00000000-0000-4000-8000-000000000001".into(),
            event_id: Uuid::parse_str("00000000-0000-4000-8000-000000000007").unwrap()
        }]
    );
    let operation = fake_backend.operation.borrow();
    let operation = operation.as_ref().unwrap();
    assert_eq!(operation.accepted_at, instant(1));
    assert_eq!(operation.actor, actor());
    assert_eq!(operation.entry_point, EntryPoint::LocalDesktop);
    assert_eq!(operation.request_id, "10000000-0000-4000-8000-000000000003");
    assert_eq!(operation.context, Some(json!({"conversation":1})));
    assert_eq!(operation.idempotency_key, "key");
    assert_eq!(
        operation.request.schema_version,
        TaskCreateRequestV2SchemaVersion
    );
    for (actual, expected) in [
        (operation.task_id, "00000000-0000-4000-8000-000000000001"),
        (
            operation.task_scope_id,
            "00000000-0000-4000-8000-000000000002",
        ),
        (
            operation.content_origin_id,
            "00000000-0000-4000-8000-000000000003",
        ),
        (operation.receipt_id, "00000000-0000-4000-8000-000000000004"),
        (
            operation.creation_provenance_id,
            "00000000-0000-4000-8000-000000000005",
        ),
        (operation.audit_id, "00000000-0000-4000-8000-000000000006"),
        (operation.event_id, "00000000-0000-4000-8000-000000000007"),
    ] {
        assert_eq!(actual, Uuid::parse_str(expected).unwrap());
    }
    assert_eq!(operation.correlation_id, "correlation");
    assert_eq!(operation.dedup_key, "dedup");
    assert_eq!(
        *fake_ids.purposes.borrow(),
        vec![
            "Task".to_owned(),
            "TaskScope".to_owned(),
            "ContentOrigin".to_owned(),
            "KernelReceipt".to_owned(),
            "CreationProvenance".to_owned(),
            "AuditRecord".to_owned(),
            "Event".to_owned(),
            "Correlation".to_owned(),
            "EventDedup".to_owned(),
        ]
    );
    assert_eq!(
        &*log.borrow(),
        &[
            "clock",
            "id",
            "id",
            "id",
            "id",
            "id",
            "id",
            "id",
            "opaque",
            "opaque",
            "backend.create",
            "clock"
        ]
    );
}

#[test]
fn replay_has_no_intent_and_get_found_not_found_are_exactly_once() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    *fake_backend.create.borrow_mut() = Some(Ok(TaskCreateBackendResult::Replayed {
        current_task: task("00000000-0000-4000-8000-000000000001"),
        creation_provenance_ref: "00000000-0000-4000-8000-000000000005".into(),
    }));
    let HandlerResult::Response(value) = handle_task_create(
        &create_request("2026-07-18T12:00:10Z"),
        &clock(vec![Ok(instant(1)), Ok(instant(2))], log.clone()),
        &ids(log.clone()),
        &fake_backend,
    ) else {
        panic!("response")
    };
    assert!(value.post_commit_notification_intents.is_empty());
    assert_eq!(
        value.response.payload.as_ref().unwrap()["creation_provenance_ref"],
        "00000000-0000-4000-8000-000000000005"
    );
    for found in [Some(task("00000000-0000-4000-8000-000000000001")), None] {
        let expected_found = found.is_some();
        let log = Rc::new(RefCell::new(vec![]));
        let fake_backend = backend(log.clone());
        *fake_backend.get.borrow_mut() = Some(Ok(found));
        let result = handle_task_get(
            &get_envelope("2026-07-18T12:00:10Z"),
            &clock(vec![Ok(instant(1)), Ok(instant(2))], log),
            &fake_backend,
        );
        assert_eq!(fake_backend.get_calls.get(), 1);
        if let HandlerResult::Response(value) = result {
            assert_eq!(
                value.response.status == KcpResponseEnvelopeStatus::Ok,
                expected_found
            );
        }
    }
}

#[test]
fn completion_clock_priority_beats_backend_result() {
    for second in [Err(ClockError), Ok(instant(10))] {
        let expected = if second.is_err() {
            "internal_error"
        } else {
            "deadline_exceeded"
        };
        let log = Rc::new(RefCell::new(vec![]));
        let fake_backend = backend(log.clone());
        *fake_backend.get.borrow_mut() = Some(Err(BackendError::SqliteBusy));
        let error = response_error(handle_task_get(
            &get_envelope("2026-07-18T12:00:10Z"),
            &clock(vec![Ok(instant(1)), second], log),
            &fake_backend,
        ));
        assert_eq!(error.code, expected);
    }
}

#[test]
fn generation_failures_duplicates_bad_uuid_and_empty_opaque_stop_before_backend() {
    let cases = [
        (vec![Ok("bad".into())], vec![Ok("c".into()), Ok("d".into())]),
        (
            vec![Ok("00000000-0000-4000-8000-000000000001".into()); 7],
            vec![Ok("c".into()), Ok("d".into())],
        ),
        (
            (1..=7)
                .map(|n| Ok(format!("00000000-0000-4000-8000-{n:012}")))
                .collect(),
            vec![Ok("".into()), Ok("d".into())],
        ),
    ];
    for (uuids, opaque) in cases {
        let log = Rc::new(RefCell::new(vec![]));
        let fake_backend = backend(log.clone());
        let fake_ids = FakeIds {
            uuids: RefCell::new(uuids.into()),
            opaque: RefCell::new(opaque.into()),
            purposes: RefCell::new(vec![]),
            log: log.clone(),
        };
        assert_eq!(
            response_error(handle_task_create(
                &create_request("2026-07-18T12:00:10Z"),
                &clock(vec![Ok(instant(1))], log),
                &fake_ids,
                &fake_backend
            ))
            .code,
            "internal_error"
        );
        assert_eq!(fake_backend.create_calls.get(), 0);
    }
}

#[test]
fn correlation_and_dedup_may_have_the_same_non_empty_value() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    *fake_backend.create.borrow_mut() = Some(Ok(TaskCreateBackendResult::Replayed {
        current_task: task("00000000-0000-4000-8000-000000000001"),
        creation_provenance_ref: "00000000-0000-4000-8000-000000000005".into(),
    }));
    let fake_ids = FakeIds {
        uuids: RefCell::new(
            (1..=7)
                .map(|n| Ok(format!("00000000-0000-4000-8000-{n:012}")))
                .collect(),
        ),
        opaque: RefCell::new([Ok("same".into()), Ok("same".into())].into()),
        purposes: RefCell::new(vec![]),
        log: log.clone(),
    };
    let HandlerResult::Response(response) = handle_task_create(
        &create_request("2026-07-18T12:00:10Z"),
        &clock(vec![Ok(instant(1)), Ok(instant(2))], log),
        &fake_ids,
        &fake_backend,
    ) else {
        panic!("response")
    };
    assert_eq!(response.response.status, KcpResponseEnvelopeStatus::Ok);
    let operation = fake_backend.operation.borrow();
    let operation = operation.as_ref().unwrap();
    assert_eq!(operation.correlation_id, "same");
    assert_eq!(operation.dedup_key, "same");
}

#[test]
fn stable_backend_error_mapping_has_fixed_safe_fields() {
    let cases = [
        (
            BackendError::InvalidScopePattern,
            "invalid_scope_pattern",
            "task scope contains an invalid URI pattern",
            false,
        ),
        (
            BackendError::IdempotencyConflict,
            "idempotency_conflict",
            "idempotency key was used for different task facts",
            false,
        ),
        (
            BackendError::DelegationNotFound,
            "delegation_not_found",
            "delegation was not found",
            false,
        ),
        (
            BackendError::ParentOriginNotFound,
            "parent_origin_not_found",
            "parent content origin was not found",
            false,
        ),
        (
            BackendError::SqliteBusy,
            "sqlite_busy",
            "kernel storage is busy",
            true,
        ),
        (
            BackendError::SqliteFull,
            "sqlite_full",
            "kernel storage is full",
            false,
        ),
        (
            BackendError::SqliteCorrupt,
            "sqlite_corrupt",
            "kernel storage is corrupt or invalid",
            false,
        ),
        (
            BackendError::StoredDataInvalid,
            "stored_data_invalid",
            "stored task data failed integrity validation",
            false,
        ),
        (
            BackendError::Internal,
            "internal_error",
            "internal kernel error",
            false,
        ),
    ];
    for (backend_error, code, message, retryable) in cases {
        let log = Rc::new(RefCell::new(vec![]));
        let fake_backend = backend(log.clone());
        *fake_backend.create.borrow_mut() = Some(Err(backend_error));
        let error = response_error(handle_task_create(
            &create_request("2026-07-18T12:00:10Z"),
            &clock(vec![Ok(instant(1)), Ok(instant(2))], log.clone()),
            &ids(log),
            &fake_backend,
        ));
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.retryable),
            (code, message, retryable)
        );
        assert!(error.details.is_none());
    }
}

#[test]
fn create_backend_error_is_hidden_when_completion_deadline_expires() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    *fake_backend.create.borrow_mut() = Some(Err(BackendError::SqliteBusy));
    let error = response_error(handle_task_create(
        &create_request("2026-07-18T12:00:10Z"),
        &clock(vec![Ok(instant(1)), Ok(instant(10))], log),
        &ids(Rc::new(RefCell::new(vec![]))),
        &fake_backend,
    ));
    assert_eq!(error.code, "deadline_exceeded");
}

#[test]
fn malformed_typed_task_get_uuid_is_internal_without_backend() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    let mut envelope = get_envelope("2026-07-18T12:00:10Z");
    let KcpQueryPayload::TaskGet(request) = &mut envelope.payload else {
        panic!("task.get")
    };
    request.task_id = "not-a-uuid".into();
    let error = response_error(handle_task_get(
        &envelope,
        &clock(vec![Ok(instant(1))], log.clone()),
        &fake_backend,
    ));
    assert_eq!(error.code, "internal_error");
    assert_eq!(fake_backend.get_calls.get(), 0);
    assert_eq!(&*log.borrow(), &["clock"]);
}

#[test]
fn all_handlers_treat_completion_equality_as_expired() {
    let log = Rc::new(RefCell::new(vec![]));
    assert_eq!(
        response_error(handle_system_ping(
            &ping("2026-07-18T12:00:10Z", None),
            &clock(vec![Ok(instant(1)), Ok(instant(10))], log)
        ))
        .code,
        "deadline_exceeded"
    );

    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    *fake_backend.create.borrow_mut() = Some(Ok(TaskCreateBackendResult::Replayed {
        current_task: task("00000000-0000-4000-8000-000000000001"),
        creation_provenance_ref: "00000000-0000-4000-8000-000000000005".into(),
    }));
    assert_eq!(
        response_error(handle_task_create(
            &create_request("2026-07-18T12:00:10Z"),
            &clock(vec![Ok(instant(1)), Ok(instant(10))], log.clone()),
            &ids(log),
            &fake_backend
        ))
        .code,
        "deadline_exceeded"
    );
}

#[test]
fn created_intent_survives_completion_clock_failure_and_deadline() {
    for completion in [Err(ClockError), Ok(instant(10))] {
        let log = Rc::new(RefCell::new(vec![]));
        let fake_backend = backend(log.clone());
        *fake_backend.create.borrow_mut() = Some(Ok(TaskCreateBackendResult::Created {
            current_task: task("00000000-0000-4000-8000-000000000001"),
            creation_provenance_ref: "00000000-0000-4000-8000-000000000005".into(),
            committed_event_id: Uuid::parse_str("00000000-0000-4000-8000-000000000007").unwrap(),
        }));
        let HandlerResult::Response(value) = handle_task_create(
            &create_request("2026-07-18T12:00:10Z"),
            &clock(vec![Ok(instant(1)), completion], log.clone()),
            &ids(log),
            &fake_backend,
        ) else {
            panic!("response")
        };
        assert_eq!(value.post_commit_notification_intents.len(), 1);
        assert!(matches!(
            value
                .response
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("internal_error" | "deadline_exceeded")
        ));
    }
}

#[test]
fn explicit_generation_errors_stop_before_backend() {
    let log = Rc::new(RefCell::new(vec![]));
    let fake_backend = backend(log.clone());
    let fake_ids = FakeIds {
        uuids: RefCell::new([Err(IdGenerationError)].into()),
        opaque: RefCell::new(VecDeque::new()),
        purposes: RefCell::new(vec![]),
        log: log.clone(),
    };
    assert_eq!(
        response_error(handle_task_create(
            &create_request("2026-07-18T12:00:10Z"),
            &clock(vec![Ok(instant(1))], log),
            &fake_ids,
            &fake_backend
        ))
        .code,
        "internal_error"
    );
    assert_eq!(fake_backend.create_calls.get(), 0);
}

#[test]
fn get_only_exposes_storage_errors_and_folds_create_only_backend_categories() {
    for (backend_error, expected) in [
        (BackendError::SqliteBusy, "sqlite_busy"),
        (BackendError::SqliteFull, "sqlite_full"),
        (BackendError::SqliteCorrupt, "sqlite_corrupt"),
        (BackendError::StoredDataInvalid, "stored_data_invalid"),
        (BackendError::InvalidScopePattern, "internal_error"),
        (BackendError::IdempotencyConflict, "internal_error"),
        (BackendError::DelegationNotFound, "internal_error"),
        (BackendError::ParentOriginNotFound, "internal_error"),
        (BackendError::Internal, "internal_error"),
    ] {
        let log = Rc::new(RefCell::new(vec![]));
        let fake_backend = backend(log.clone());
        *fake_backend.get.borrow_mut() = Some(Err(backend_error));
        let error = response_error(handle_task_get(
            &get_envelope("2026-07-18T12:00:10Z"),
            &clock(vec![Ok(instant(1)), Ok(instant(2))], log),
            &fake_backend,
        ));
        assert_eq!(error.code, expected);
    }
}

#[test]
fn method_variant_mismatch_is_local_contract_failure_not_invalid_request() {
    let mut envelope = get_envelope("2026-07-18T12:00:10Z");
    envelope.query_type = "system.ping".into();
    let result = handle_task_get(
        &envelope,
        &clock(vec![Ok(instant(1))], Rc::new(RefCell::new(vec![]))),
        &backend(Rc::new(RefCell::new(vec![]))),
    );
    let HandlerResult::ContractFailure { failure, .. } = result else {
        panic!("contract failure")
    };
    assert_eq!(
        failure.kind,
        HandlerContractFailureKind::InputMethodMismatch
    );
}
