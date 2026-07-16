//! Typed application-handler implementations.

use crate::ports::{
    BackendError, KernelClock, KernelIdGenerator, OpaqueIdPurpose, ResponseContractValidator,
    SchemaResponseContractValidator, TaskApplicationBackend, TaskCreateBackendResult,
    TaskCreateOperation, UuidPurpose,
};
use crate::response::{
    HandledResponse, HandlerContractFailure, HandlerContractFailureKind, HandlerResult,
    PostCommitNotificationIntent,
};
use chrono::{DateTime, SecondsFormat, Utc};
use kernel_contracts::{
    KcpCommandPayload, KcpError, KcpErrorSchemaVersion, KcpQueryPayload, KcpResponseEnvelope,
    KcpResponseEnvelopeMessageKind, KcpResponseEnvelopeProtocolVersion, KcpResponseEnvelopeStatus,
    SystemPingResponse, SystemPingResponseProtocolVersion, SystemPingResponseSchemaVersion,
    TaskCreateResponse, TaskCreateResponseSchemaVersion, TaskGetResponse,
    TaskGetResponseSchemaVersion, TypedKcpCommandEnvelope, TypedKcpQueryEnvelope,
};
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

const SYSTEM_PING_RESPONSE_SCHEMA: &str =
    "https://schemas.shittim.local/v1/kcp/system_ping_response.json";
const TASK_CREATE_RESPONSE_SCHEMA: &str =
    "https://schemas.shittim.local/v1/kcp/task_create_response.json";
const TASK_GET_RESPONSE_SCHEMA: &str =
    "https://schemas.shittim.local/v1/kcp/task_get_response.json";

/// Handles a typed `system.ping` query without accepting raw JSON or transport frames.
pub fn handle_system_ping(
    envelope: &TypedKcpQueryEnvelope,
    clock: &impl KernelClock,
) -> HandlerResult {
    handle_system_ping_with_validator(envelope, clock, &SchemaResponseContractValidator)
}

pub(crate) fn handle_system_ping_with_validator(
    envelope: &TypedKcpQueryEnvelope,
    clock: &impl KernelClock,
    validator: &impl ResponseContractValidator,
) -> HandlerResult {
    let started_at = match clock.now_utc() {
        Ok(value) => value,
        Err(_) => {
            return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator)
        }
    };
    let request = match (&envelope.query_type[..], &envelope.payload) {
        ("system.ping", KcpQueryPayload::SystemPing(request)) => request,
        _ => return input_mismatch(),
    };
    let deadline = match parse_deadline(&envelope.deadline) {
        Some(value) => value,
        None => return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator),
    };
    if started_at >= deadline {
        return error_result(
            &envelope.request_id,
            ErrorKind::DeadlineExceeded,
            vec![],
            validator,
        );
    }
    let payload = SystemPingResponse {
        echo: request.echo.clone(),
        kernel_time: started_at.to_rfc3339_opts(SecondsFormat::AutoSi, true),
        protocol_version: SystemPingResponseProtocolVersion::Value,
        schema_version: SystemPingResponseSchemaVersion,
    };
    let completed_at = match clock.now_utc() {
        Ok(value) => value,
        Err(_) => {
            return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator)
        }
    };
    if completed_at >= deadline {
        return error_result(
            &envelope.request_id,
            ErrorKind::DeadlineExceeded,
            vec![],
            validator,
        );
    }
    success_result(
        &envelope.request_id,
        &payload,
        SYSTEM_PING_RESPONSE_SCHEMA,
        vec![],
        validator,
    )
}

/// Handles a typed `task.create` command through the high-level Task backend.
pub fn handle_task_create(
    envelope: &TypedKcpCommandEnvelope,
    clock: &impl KernelClock,
    ids: &impl KernelIdGenerator,
    backend: &impl TaskApplicationBackend,
) -> HandlerResult {
    handle_task_create_with_validator(
        envelope,
        clock,
        ids,
        backend,
        &SchemaResponseContractValidator,
    )
}

pub(crate) fn handle_task_create_with_validator(
    envelope: &TypedKcpCommandEnvelope,
    clock: &impl KernelClock,
    ids: &impl KernelIdGenerator,
    backend: &impl TaskApplicationBackend,
    validator: &impl ResponseContractValidator,
) -> HandlerResult {
    let accepted_at = match clock.now_utc() {
        Ok(value) => value,
        Err(_) => {
            return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator)
        }
    };
    if !matches!(
        (&envelope.command_type[..], &envelope.payload),
        ("task.create", KcpCommandPayload::TaskCreate(_))
    ) {
        return input_mismatch();
    }
    let deadline = match parse_deadline(&envelope.deadline) {
        Some(value) => value,
        None => return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator),
    };
    if accepted_at >= deadline {
        return error_result(
            &envelope.request_id,
            ErrorKind::DeadlineExceeded,
            vec![],
            validator,
        );
    }
    let allocation = match allocate_task_create(ids) {
        Some(value) => value,
        None => return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator),
    };
    let operation = TaskCreateOperation {
        envelope: envelope.clone(),
        accepted_at,
        task_id: allocation[0],
        task_scope_id: allocation[1],
        content_origin_id: allocation[2],
        receipt_id: allocation[3],
        audit_id: allocation[4],
        event_id: allocation[5],
        correlation_id: allocation.correlation_id,
        dedup_key: allocation.dedup_key,
    };
    let backend_result = backend.create_task(operation);
    let (result_kind, intents) = match backend_result {
        Ok(TaskCreateBackendResult::Created {
            current_task,
            committed_event_id,
        }) => {
            let intent = PostCommitNotificationIntent::TaskCreatedCommitted {
                task_id: current_task.id.clone(),
                event_id: committed_event_id,
            };
            (CreateCompletion::Created(current_task), vec![intent])
        }
        Ok(TaskCreateBackendResult::Replayed { current_task }) => {
            (CreateCompletion::Replayed(current_task), vec![])
        }
        Err(error) => (CreateCompletion::Failed(error), vec![]),
    };
    let completed_at = match clock.now_utc() {
        Ok(value) => value,
        Err(_) => {
            return error_result(
                &envelope.request_id,
                ErrorKind::Internal,
                intents,
                validator,
            )
        }
    };
    if completed_at >= deadline {
        return error_result(
            &envelope.request_id,
            ErrorKind::DeadlineExceeded,
            intents,
            validator,
        );
    }
    match result_kind {
        CreateCompletion::Created(task) | CreateCompletion::Replayed(task) => {
            let payload = TaskCreateResponse {
                schema_version: TaskCreateResponseSchemaVersion,
                task,
            };
            success_result(
                &envelope.request_id,
                &payload,
                TASK_CREATE_RESPONSE_SCHEMA,
                intents,
                validator,
            )
        }
        CreateCompletion::Failed(error) => error_result(
            &envelope.request_id,
            ErrorKind::CreateBackend(error),
            intents,
            validator,
        ),
    }
}

/// Handles a typed `task.get` query through exactly one backend read.
pub fn handle_task_get(
    envelope: &TypedKcpQueryEnvelope,
    clock: &impl KernelClock,
    backend: &impl TaskApplicationBackend,
) -> HandlerResult {
    handle_task_get_with_validator(envelope, clock, backend, &SchemaResponseContractValidator)
}

pub(crate) fn handle_task_get_with_validator(
    envelope: &TypedKcpQueryEnvelope,
    clock: &impl KernelClock,
    backend: &impl TaskApplicationBackend,
    validator: &impl ResponseContractValidator,
) -> HandlerResult {
    let started_at = match clock.now_utc() {
        Ok(value) => value,
        Err(_) => {
            return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator)
        }
    };
    let request = match (&envelope.query_type[..], &envelope.payload) {
        ("task.get", KcpQueryPayload::TaskGet(request)) => request,
        _ => return input_mismatch(),
    };
    let deadline = match parse_deadline(&envelope.deadline) {
        Some(value) => value,
        None => return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator),
    };
    if started_at >= deadline {
        return error_result(
            &envelope.request_id,
            ErrorKind::DeadlineExceeded,
            vec![],
            validator,
        );
    }
    let task_id = match Uuid::parse_str(&request.task_id) {
        Ok(value) => value,
        Err(_) => {
            return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator)
        }
    };
    let backend_result = backend.get_task(task_id);
    let completed_at = match clock.now_utc() {
        Ok(value) => value,
        Err(_) => {
            return error_result(&envelope.request_id, ErrorKind::Internal, vec![], validator)
        }
    };
    if completed_at >= deadline {
        return error_result(
            &envelope.request_id,
            ErrorKind::DeadlineExceeded,
            vec![],
            validator,
        );
    }
    match backend_result {
        Ok(Some(task)) => {
            let payload = TaskGetResponse {
                schema_version: TaskGetResponseSchemaVersion,
                task,
            };
            success_result(
                &envelope.request_id,
                &payload,
                TASK_GET_RESPONSE_SCHEMA,
                vec![],
                validator,
            )
        }
        Ok(None) => error_result(
            &envelope.request_id,
            ErrorKind::TaskNotFound,
            vec![],
            validator,
        ),
        Err(error) => error_result(
            &envelope.request_id,
            ErrorKind::GetBackend(error),
            vec![],
            validator,
        ),
    }
}

struct CreateAllocation {
    uuids: [Uuid; 6],
    correlation_id: String,
    dedup_key: String,
}

impl std::ops::Index<usize> for CreateAllocation {
    type Output = Uuid;

    fn index(&self, index: usize) -> &Self::Output {
        &self.uuids[index]
    }
}

fn allocate_task_create(ids: &impl KernelIdGenerator) -> Option<CreateAllocation> {
    let generated = [
        ids.next_uuid(UuidPurpose::Task).ok()?,
        ids.next_uuid(UuidPurpose::TaskScope).ok()?,
        ids.next_uuid(UuidPurpose::ContentOrigin).ok()?,
        ids.next_uuid(UuidPurpose::KernelReceipt).ok()?,
        ids.next_uuid(UuidPurpose::AuditRecord).ok()?,
        ids.next_uuid(UuidPurpose::Event).ok()?,
    ];
    let uuids = [
        Uuid::parse_str(&generated[0]).ok()?,
        Uuid::parse_str(&generated[1]).ok()?,
        Uuid::parse_str(&generated[2]).ok()?,
        Uuid::parse_str(&generated[3]).ok()?,
        Uuid::parse_str(&generated[4]).ok()?,
        Uuid::parse_str(&generated[5]).ok()?,
    ];
    if uuids.iter().copied().collect::<HashSet<_>>().len() != uuids.len() {
        return None;
    }
    let correlation_id = ids.next_opaque_id(OpaqueIdPurpose::Correlation).ok()?;
    let dedup_key = ids.next_opaque_id(OpaqueIdPurpose::EventDedup).ok()?;
    if correlation_id.is_empty() || dedup_key.is_empty() {
        return None;
    }
    Some(CreateAllocation {
        uuids,
        correlation_id,
        dedup_key,
    })
}

fn parse_deadline(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn success_result<T: Serialize>(
    request_id: &str,
    payload: &T,
    schema_id: &str,
    intents: Vec<PostCommitNotificationIntent>,
    validator: &impl ResponseContractValidator,
) -> HandlerResult {
    let payload_value = match serde_json::to_value(payload) {
        Ok(value) if validator.validate_method_payload(schema_id, &value).is_ok() => value,
        _ => return error_result(request_id, ErrorKind::Internal, intents, validator),
    };
    finalize_response(
        KcpResponseEnvelope {
            error: None,
            message_kind: KcpResponseEnvelopeMessageKind::Value,
            payload: Some(payload_value),
            protocol_version: KcpResponseEnvelopeProtocolVersion::Value,
            request_id: request_id.to_owned(),
            status: KcpResponseEnvelopeStatus::Ok,
        },
        intents,
        validator,
    )
}

fn error_result(
    request_id: &str,
    kind: ErrorKind,
    intents: Vec<PostCommitNotificationIntent>,
    validator: &impl ResponseContractValidator,
) -> HandlerResult {
    let error = kind.kcp_error();
    finalize_response(
        KcpResponseEnvelope {
            error: Some(error),
            message_kind: KcpResponseEnvelopeMessageKind::Value,
            payload: None,
            protocol_version: KcpResponseEnvelopeProtocolVersion::Value,
            request_id: request_id.to_owned(),
            status: KcpResponseEnvelopeStatus::Error,
        },
        intents,
        validator,
    )
}

fn finalize_response(
    response: KcpResponseEnvelope,
    intents: Vec<PostCommitNotificationIntent>,
    validator: &impl ResponseContractValidator,
) -> HandlerResult {
    let valid = serde_json::to_value(&response)
        .ok()
        .filter(|value| validator.validate_response_envelope(value).is_ok())
        .is_some();
    if valid {
        HandlerResult::Response(HandledResponse {
            response,
            post_commit_notification_intents: intents,
        })
    } else {
        HandlerResult::ContractFailure {
            failure: HandlerContractFailure {
                kind: HandlerContractFailureKind::FinalResponseInvalid,
                message: "final response contract validation failed",
            },
            post_commit_notification_intents: intents,
        }
    }
}

fn input_mismatch() -> HandlerResult {
    HandlerResult::ContractFailure {
        failure: HandlerContractFailure {
            kind: HandlerContractFailureKind::InputMethodMismatch,
            message: "typed envelope method does not match handler",
        },
        post_commit_notification_intents: vec![],
    }
}

enum CreateCompletion {
    Created(kernel_contracts::TaskSpec),
    Replayed(kernel_contracts::TaskSpec),
    Failed(BackendError),
}

enum ErrorKind {
    DeadlineExceeded,
    TaskNotFound,
    CreateBackend(BackendError),
    GetBackend(BackendError),
    Internal,
}

impl ErrorKind {
    fn kcp_error(self) -> KcpError {
        let (code, message, retryable) = match self {
            Self::DeadlineExceeded => ("deadline_exceeded", "request deadline exceeded", true),
            Self::TaskNotFound => ("task_not_found", "task was not found", false),
            Self::Internal
            | Self::CreateBackend(BackendError::Internal)
            | Self::GetBackend(BackendError::Internal)
            | Self::GetBackend(
                BackendError::InvalidScopePattern
                | BackendError::IdempotencyConflict
                | BackendError::DelegationNotFound
                | BackendError::ParentTaskNotFound
                | BackendError::ParentOriginNotFound,
            ) => ("internal_error", "internal kernel error", false),
            Self::CreateBackend(BackendError::InvalidScopePattern) => (
                "invalid_scope_pattern",
                "task scope contains an invalid URI pattern",
                false,
            ),
            Self::CreateBackend(BackendError::IdempotencyConflict) => (
                "idempotency_conflict",
                "idempotency key was used for different task facts",
                false,
            ),
            Self::CreateBackend(BackendError::DelegationNotFound) => {
                ("delegation_not_found", "delegation was not found", false)
            }
            Self::CreateBackend(BackendError::ParentTaskNotFound) => {
                ("parent_task_not_found", "parent task was not found", false)
            }
            Self::CreateBackend(BackendError::ParentOriginNotFound) => (
                "parent_origin_not_found",
                "parent content origin was not found",
                false,
            ),
            Self::CreateBackend(BackendError::SqliteBusy)
            | Self::GetBackend(BackendError::SqliteBusy) => {
                ("sqlite_busy", "kernel storage is busy", true)
            }
            Self::CreateBackend(BackendError::SqliteFull)
            | Self::GetBackend(BackendError::SqliteFull) => {
                ("sqlite_full", "kernel storage is full", false)
            }
            Self::CreateBackend(BackendError::SqliteCorrupt)
            | Self::GetBackend(BackendError::SqliteCorrupt) => (
                "sqlite_corrupt",
                "kernel storage is corrupt or invalid",
                false,
            ),
            Self::CreateBackend(BackendError::StoredDataInvalid)
            | Self::GetBackend(BackendError::StoredDataInvalid) => (
                "stored_data_invalid",
                "stored task data failed integrity validation",
                false,
            ),
        };
        KcpError {
            code: code.to_owned(),
            details: None,
            message: message.to_owned(),
            retryable,
            schema_version: KcpErrorSchemaVersion,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        ClockError, IdGenerationError, ResponseValidationError, TaskCreateBackendResult,
    };
    use chrono::TimeZone;
    use kernel_contracts::{
        Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, EntryPoint,
        KcpCommandEnvelopeMessageKind, KcpCommandEnvelopeProtocolVersion, NullOnly,
        TaskCreateRequest,
    };
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct FaultValidator {
        reject_payload: bool,
        reject_envelope: bool,
    }

    impl ResponseContractValidator for FaultValidator {
        fn validate_method_payload(
            &self,
            schema_id: &str,
            value: &serde_json::Value,
        ) -> Result<(), ResponseValidationError> {
            if self.reject_payload {
                Err(ResponseValidationError)
            } else {
                SchemaResponseContractValidator.validate_method_payload(schema_id, value)
            }
        }

        fn validate_response_envelope(
            &self,
            value: &serde_json::Value,
        ) -> Result<(), ResponseValidationError> {
            if self.reject_envelope {
                Err(ResponseValidationError)
            } else {
                SchemaResponseContractValidator.validate_response_envelope(value)
            }
        }
    }

    struct Clock(RefCell<VecDeque<DateTime<Utc>>>);

    impl KernelClock for Clock {
        fn now_utc(&self) -> Result<DateTime<Utc>, ClockError> {
            self.0.borrow_mut().pop_front().ok_or(ClockError)
        }
    }

    struct Ids(RefCell<u32>);

    impl KernelIdGenerator for Ids {
        fn next_uuid(&self, _purpose: UuidPurpose) -> Result<String, IdGenerationError> {
            let next = self.0.replace_with(|value| *value + 1);
            Ok(format!("00000000-0000-4000-8000-{next:012}"))
        }

        fn next_opaque_id(&self, purpose: OpaqueIdPurpose) -> Result<String, IdGenerationError> {
            Ok(match purpose {
                OpaqueIdPurpose::Correlation => "correlation".into(),
                OpaqueIdPurpose::EventDedup => "dedup".into(),
            })
        }
    }

    struct Backend(RefCell<Option<TaskCreateBackendResult>>);

    impl TaskApplicationBackend for Backend {
        fn create_task(
            &self,
            _operation: TaskCreateOperation,
        ) -> Result<TaskCreateBackendResult, BackendError> {
            self.0.borrow_mut().take().ok_or(BackendError::Internal)
        }

        fn get_task(
            &self,
            _task_id: Uuid,
        ) -> Result<Option<kernel_contracts::TaskSpec>, BackendError> {
            Err(BackendError::Internal)
        }
    }

    #[test]
    fn private_validator_seam_exercises_response_failures_and_preserves_created_intent() {
        for (reject_envelope, expected_contract_failure) in [(false, false), (true, true)] {
            let backend = Backend(RefCell::new(Some(TaskCreateBackendResult::Created {
                current_task: task(),
                committed_event_id: Uuid::parse_str("00000000-0000-4000-8000-000000000006")
                    .expect("valid event uuid"),
            })));
            let result = handle_task_create_with_validator(
                &envelope(),
                &Clock(RefCell::new([instant(1), instant(2)].into())),
                &Ids(RefCell::new(1)),
                &backend,
                &FaultValidator {
                    reject_payload: true,
                    reject_envelope,
                },
            );
            match result {
                HandlerResult::Response(response) => {
                    assert!(!expected_contract_failure);
                    assert_eq!(
                        response.response.error.expect("error response").code,
                        "internal_error"
                    );
                    assert_eq!(response.post_commit_notification_intents.len(), 1);
                }
                HandlerResult::ContractFailure {
                    failure,
                    post_commit_notification_intents,
                } => {
                    assert!(expected_contract_failure);
                    assert_eq!(
                        failure.kind,
                        HandlerContractFailureKind::FinalResponseInvalid
                    );
                    assert_eq!(post_commit_notification_intents.len(), 1);
                }
            }
        }
    }

    fn instant(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, second)
            .single()
            .expect("valid time")
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

    fn envelope() -> TypedKcpCommandEnvelope {
        let request: TaskCreateRequest = serde_json::from_value(json!({"schema_version":1,"proposer":"user","goal":"goal","constraints":[],"success_criteria":["done"],"risk_hint":null,"capability_hints":[],"task_scope":{"schema_version":1,"resource_patterns":[],"exclusions":[],"allowed_capability_hints":[],"expires_at":null},"delegation_ref":null,"parent_task_id":null,"origin":{"schema_version":1,"kind":"user_input","source_uri":null,"upstream_stable_id":null,"producer_ref":{"kind":"actor","id":"actor"},"parent_origin_refs":[]}})).expect("valid request");
        TypedKcpCommandEnvelope {
            actor: actor(),
            auth: NullOnly,
            context: None,
            deadline: "2026-07-18T12:00:10Z".into(),
            entry_point: EntryPoint::LocalDesktop,
            expected_revision: None,
            idempotency_key: "key".into(),
            message_kind: KcpCommandEnvelopeMessageKind::Value,
            protocol_version: KcpCommandEnvelopeProtocolVersion::Value,
            request_id: "10000000-0000-4000-8000-000000000003".into(),
            task_id: None,
            command_type: "task.create".into(),
            payload: KcpCommandPayload::TaskCreate(Box::new(request)),
        }
    }

    fn task() -> kernel_contracts::TaskSpec {
        serde_json::from_value(json!({"id":"00000000-0000-4000-8000-000000000001","origin_ref":"30000000-0000-4000-8000-000000000001","actor":actor(),"proposer":"user","goal":"goal","constraints":[],"success_criteria":["done"],"risk_hint":null,"capability_hints":[],"delegation_ref":null,"task_scope_ref":"20000000-0000-4000-8000-000000000001","parent_task_id":null,"status":"candidate","plan_version":0,"schema_version":1,"revision":1,"created_at":"2026-07-18T12:00:01Z","updated_at":"2026-07-18T12:00:01Z","failed_recovery_meta":null})).expect("valid task")
    }
}
