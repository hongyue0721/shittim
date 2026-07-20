//! Typed application-handler implementations.

use crate::ports::{
    BackendError, KernelClock, KernelIdGenerator, OpaqueIdPurpose, ResponseContractValidator,
    SchemaResponseContractValidator, TaskApplicationBackend, TaskCreateBackendResult,
    TaskCreateOperation, UuidPurpose,
};
use crate::preflight::TaskCreateCommandRequestV2;
use crate::response::{
    HandledResponse, HandlerContractFailure, HandlerContractFailureKind, HandlerResult,
    PostCommitNotificationIntent,
};
use chrono::{DateTime, SecondsFormat, Utc};
use kernel_contracts::{
    KcpError, KcpErrorSchemaVersion, KcpQueryPayload, KcpResponseEnvelope,
    KcpResponseEnvelopeMessageKind, KcpResponseEnvelopeProtocolVersion, KcpResponseEnvelopeStatus,
    SystemPingResponse, SystemPingResponseProtocolVersion, SystemPingResponseSchemaVersion,
    TaskCreateResponseV2, TaskCreateResponseV2SchemaVersion, TaskGetResponse,
    TaskGetResponseSchemaVersion, TypedKcpQueryEnvelope,
};
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

const SYSTEM_PING_RESPONSE_SCHEMA: &str =
    "https://schemas.shittim.local/v1/kcp/system_ping_response.json";
const TASK_CREATE_RESPONSE_V2_SCHEMA: &str =
    "https://schemas.shittim.local/kcp/task_create_response/v2";
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

/// Handles a typed root-only `task.create` v2 command through the high-level Task backend.
pub fn handle_task_create(
    request: &TaskCreateCommandRequestV2,
    clock: &impl KernelClock,
    ids: &impl KernelIdGenerator,
    backend: &impl TaskApplicationBackend,
) -> HandlerResult {
    handle_task_create_with_validator(
        request,
        clock,
        ids,
        backend,
        &SchemaResponseContractValidator,
    )
}

pub(crate) fn handle_task_create_with_validator(
    request: &TaskCreateCommandRequestV2,
    clock: &impl KernelClock,
    ids: &impl KernelIdGenerator,
    backend: &impl TaskApplicationBackend,
    validator: &impl ResponseContractValidator,
) -> HandlerResult {
    let accepted_at = match clock.now_utc() {
        Ok(value) => value,
        Err(_) => return error_result(&request.request_id, ErrorKind::Internal, vec![], validator),
    };
    if request.command_type != "task.create" {
        return input_mismatch();
    }
    // Root-only active create: Envelope.task_id and expected_revision must be null.
    // Fail closed as invalid_request (not internal) so callers get a protocol-visible rejection.
    if request.task_id.is_some() || request.expected_revision.is_some() {
        return error_result(
            &request.request_id,
            ErrorKind::InvalidRequest,
            vec![],
            validator,
        );
    }
    let deadline = match parse_deadline(&request.deadline) {
        Some(value) => value,
        None => return error_result(&request.request_id, ErrorKind::Internal, vec![], validator),
    };
    if accepted_at >= deadline {
        return error_result(
            &request.request_id,
            ErrorKind::DeadlineExceeded,
            vec![],
            validator,
        );
    }
    let allocation = match allocate_task_create_v2(ids) {
        Some(value) => value,
        None => return error_result(&request.request_id, ErrorKind::Internal, vec![], validator),
    };
    let operation = TaskCreateOperation {
        actor: request.actor.clone(),
        entry_point: request.entry_point,
        request_id: request.request_id.clone(),
        context: request.context.clone(),
        idempotency_key: request.idempotency_key.clone(),
        request: request.payload.clone(),
        accepted_at,
        task_id: allocation.task_id,
        task_scope_id: allocation.task_scope_id,
        content_origin_id: allocation.content_origin_id,
        receipt_id: allocation.receipt_id,
        creation_provenance_id: allocation.creation_provenance_id,
        audit_id: allocation.audit_id,
        event_id: allocation.event_id,
        correlation_id: allocation.correlation_id,
        dedup_key: allocation.dedup_key,
    };
    let backend_result = backend.create_task(operation);
    let (result_kind, intents) = match backend_result {
        Ok(TaskCreateBackendResult::Created {
            current_task,
            creation_provenance_ref,
            committed_event_id,
        }) => {
            let intent = PostCommitNotificationIntent::TaskCreatedCommitted {
                task_id: current_task.id.clone(),
                event_id: committed_event_id,
            };
            (
                CreateCompletion::Created {
                    task: current_task,
                    creation_provenance_ref,
                },
                vec![intent],
            )
        }
        Ok(TaskCreateBackendResult::Replayed {
            current_task,
            creation_provenance_ref,
        }) => (
            CreateCompletion::Replayed {
                task: current_task,
                creation_provenance_ref,
            },
            vec![],
        ),
        Err(error) => (CreateCompletion::Failed(error), vec![]),
    };
    let completed_at = match clock.now_utc() {
        Ok(value) => value,
        Err(_) => {
            return error_result(&request.request_id, ErrorKind::Internal, intents, validator)
        }
    };
    if completed_at >= deadline {
        return error_result(
            &request.request_id,
            ErrorKind::DeadlineExceeded,
            intents,
            validator,
        );
    }
    match result_kind {
        CreateCompletion::Created {
            task,
            creation_provenance_ref,
        }
        | CreateCompletion::Replayed {
            task,
            creation_provenance_ref,
        } => {
            let payload = TaskCreateResponseV2 {
                schema_version: TaskCreateResponseV2SchemaVersion,
                task,
                creation_provenance_ref,
            };
            success_result(
                &request.request_id,
                &payload,
                TASK_CREATE_RESPONSE_V2_SCHEMA,
                intents,
                validator,
            )
        }
        CreateCompletion::Failed(error) => error_result(
            &request.request_id,
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

struct CreateAllocationV2 {
    task_id: Uuid,
    task_scope_id: Uuid,
    content_origin_id: Uuid,
    receipt_id: Uuid,
    creation_provenance_id: Uuid,
    audit_id: Uuid,
    event_id: Uuid,
    correlation_id: String,
    dedup_key: String,
}

fn allocate_task_create_v2(ids: &impl KernelIdGenerator) -> Option<CreateAllocationV2> {
    let purposes = [
        UuidPurpose::Task,
        UuidPurpose::TaskScope,
        UuidPurpose::ContentOrigin,
        UuidPurpose::KernelReceipt,
        UuidPurpose::CreationProvenance,
        UuidPurpose::AuditRecord,
        UuidPurpose::Event,
    ];
    let mut uuids = Vec::with_capacity(purposes.len());
    for purpose in purposes {
        let text = ids.next_uuid(purpose).ok()?;
        uuids.push(Uuid::parse_str(&text).ok()?);
    }
    if uuids.iter().copied().collect::<HashSet<_>>().len() != uuids.len() {
        return None;
    }
    let correlation_id = ids.next_opaque_id(OpaqueIdPurpose::Correlation).ok()?;
    let dedup_key = ids.next_opaque_id(OpaqueIdPurpose::EventDedup).ok()?;
    if correlation_id.is_empty() || dedup_key.is_empty() {
        return None;
    }
    Some(CreateAllocationV2 {
        task_id: uuids[0],
        task_scope_id: uuids[1],
        content_origin_id: uuids[2],
        receipt_id: uuids[3],
        creation_provenance_id: uuids[4],
        audit_id: uuids[5],
        event_id: uuids[6],
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
    Created {
        task: kernel_contracts::TaskSpec,
        creation_provenance_ref: String,
    },
    Replayed {
        task: kernel_contracts::TaskSpec,
        creation_provenance_ref: String,
    },
    Failed(BackendError),
}

enum ErrorKind {
    DeadlineExceeded,
    TaskNotFound,
    InvalidRequest,
    CreateBackend(BackendError),
    GetBackend(BackendError),
    Internal,
}

impl ErrorKind {
    fn kcp_error(self) -> KcpError {
        let (code, message, retryable) = match self {
            Self::DeadlineExceeded => ("deadline_exceeded", "request deadline exceeded", true),
            Self::TaskNotFound => ("task_not_found", "task was not found", false),
            Self::InvalidRequest => ("invalid_request", "request is invalid", false),
            Self::Internal
            | Self::CreateBackend(BackendError::Internal)
            | Self::GetBackend(BackendError::Internal)
            | Self::GetBackend(
                BackendError::InvalidScopePattern
                | BackendError::IdempotencyConflict
                | BackendError::DelegationNotFound
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
        InputContentOriginV1, InputContentOriginV1Kind, InputContentOriginV1ProducerRef,
        InputContentOriginV1ProducerRefKind, InputContentOriginV1SchemaVersion, InputTaskScopeV1,
        InputTaskScopeV1SchemaVersion, NormalizedRootTaskCreatePayloadV2Proposer, NullOnly,
        TaskCreateRequestV2, TaskCreateRequestV2SchemaVersion,
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
                creation_provenance_ref: "00000000-0000-4000-8000-000000000005".into(),
                committed_event_id: Uuid::parse_str("00000000-0000-4000-8000-000000000007")
                    .expect("valid event uuid"),
            })));
            let result = handle_task_create_with_validator(
                &create_request(),
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

    fn create_request() -> TaskCreateCommandRequestV2 {
        TaskCreateCommandRequestV2 {
            actor: actor(),
            auth: NullOnly,
            context: Some(json!({"conversation":1})),
            deadline: "2026-07-18T12:00:10Z".into(),
            entry_point: EntryPoint::LocalDesktop,
            expected_revision: None,
            idempotency_key: "key".into(),
            request_id: "10000000-0000-4000-8000-000000000003".into(),
            task_id: None,
            command_type: "task.create".into(),
            payload: TaskCreateRequestV2 {
                capability_hints: vec![],
                constraints: vec![],
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
                    source_uri: None,
                    upstream_stable_id: None,
                },
                proposer: NormalizedRootTaskCreatePayloadV2Proposer::User,
                risk_hint: None,
                schema_version: TaskCreateRequestV2SchemaVersion,
                success_criteria: vec!["done".into()],
                task_scope: InputTaskScopeV1 {
                    allowed_capability_hints: vec![],
                    exclusions: vec![],
                    expires_at: None,
                    resource_patterns: vec![],
                    schema_version: InputTaskScopeV1SchemaVersion,
                },
            },
        }
    }

    fn task() -> kernel_contracts::TaskSpec {
        serde_json::from_value(json!({"id":"00000000-0000-4000-8000-000000000001","origin_ref":"30000000-0000-4000-8000-000000000001","actor":actor(),"proposer":"user","goal":"goal","constraints":[],"success_criteria":["done"],"risk_hint":null,"capability_hints":[],"delegation_ref":null,"task_scope_ref":"20000000-0000-4000-8000-000000000001","parent_task_id":null,"status":"candidate","plan_version":0,"schema_version":1,"revision":1,"created_at":"2026-07-18T12:00:01Z","updated_at":"2026-07-18T12:00:01Z","failed_recovery_meta":null})).expect("valid task")
    }
}
