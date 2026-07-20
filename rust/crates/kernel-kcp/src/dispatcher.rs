//! Registration narrowing and the explicit three-method typed dispatcher.
//!
//! Active path (slice 3b): method-aware preflight selects versions via
//! `METHOD_VERSION_BINDINGS` / `select_request_version`. `task.create` is registered only as
//! the root-only v2 typed request; v1 create never enters this dispatcher.

use crate::handlers::{
    handle_system_ping_with_validator, handle_task_create_with_validator,
    handle_task_get_with_validator,
};
use crate::ports::{ResponseContractValidator, SchemaResponseContractValidator};
use crate::preflight::{TaskCreateCommandRequestV2, TypedCatalogRequest, TypedCatalogRequestKind};
use crate::{HandlerResult, KernelClock, KernelIdGenerator, TaskApplicationBackend};
use kernel_contracts::{KcpCommandPayload, KcpQueryPayload, TypedKcpQueryEnvelope};

/// Result of narrowing a fully typed catalog request to implemented handlers.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum RegistrationResult {
    /// A request backed by a formal typed handler.
    Registered(RegisteredRequest),
    /// A fully valid first-batch catalog method without a registered handler.
    KnownCatalogMethodNotImplemented(KnownCatalogMethodNotImplemented),
    /// An internal contract violation that the active method-aware preflight pipeline
    /// must never produce.
    ///
    /// This local registration result intentionally does not implement `serde::Serialize`
    /// and never masquerades as another catalog method.
    InternalContractViolation(InternalContractViolation),
}

/// Stable identities for internal contract violations detected during narrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalContractViolation {
    /// A typed `task.create` v1 envelope reached registration narrowing even though
    /// active method-aware preflight never accepts it.
    TaskCreateV1AfterActivePreflight,
}

/// First-batch methods that have Schema and generated types but no handler yet.
///
/// This local registration result intentionally does not implement `serde::Serialize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownCatalogMethodNotImplemented {
    /// `task.list`.
    TaskList,
    /// `event.subscribe`.
    EventSubscribe,
    /// `event.poll`.
    EventPoll,
    /// `stop.activate`.
    StopActivate,
    /// `stop.status`.
    StopStatus,
}

/// Registered method identity exposed without exposing constructible envelope variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisteredMethod {
    /// `system.ping`.
    SystemPing,
    /// `task.create` (active root-only v2).
    TaskCreate,
    /// `task.get`.
    TaskGet,
}

/// A request whose family, discriminator, and generated payload variant are guaranteed aligned.
///
/// Values of this type are created only by [`narrow_to_registered`].
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredRequest(RegisteredRequestKind);

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
enum RegisteredRequestKind {
    SystemPing(TypedKcpQueryEnvelope),
    /// Active root-only task.create v2 — not a generic TypedKcpCommandEnvelopeV2 wrapper.
    TaskCreate(TaskCreateCommandRequestV2),
    TaskGet(TypedKcpQueryEnvelope),
}

impl RegisteredRequest {
    /// Returns the registered handler method.
    pub fn method(&self) -> RegisteredMethod {
        match &self.0 {
            RegisteredRequestKind::SystemPing(_) => RegisteredMethod::SystemPing,
            RegisteredRequestKind::TaskCreate(_) => RegisteredMethod::TaskCreate,
            RegisteredRequestKind::TaskGet(_) => RegisteredMethod::TaskGet,
        }
    }
}

/// Narrows a fully typed catalog request to the three registered application handlers.
pub fn narrow_to_registered(request: TypedCatalogRequest) -> RegistrationResult {
    match request.into_kind() {
        TypedCatalogRequestKind::TaskCreateV2(request) => RegistrationResult::Registered(
            RegisteredRequest(RegisteredRequestKind::TaskCreate(request)),
        ),
        TypedCatalogRequestKind::CommandV1(envelope) => match &envelope.payload {
            KcpCommandPayload::StopActivate(_) => {
                RegistrationResult::KnownCatalogMethodNotImplemented(
                    KnownCatalogMethodNotImplemented::StopActivate,
                )
            }
            // Active method-aware preflight never Accepts task.create v1. A residual typed
            // v1 create envelope can only appear via private construction bypass; fail closed
            // with an honest internal-contract violation instead of inventing a wire error or
            // masquerading as another catalog method.
            KcpCommandPayload::TaskCreate(_) => RegistrationResult::InternalContractViolation(
                InternalContractViolation::TaskCreateV1AfterActivePreflight,
            ),
        },
        TypedCatalogRequestKind::QueryV1(envelope) => match &envelope.payload {
            KcpQueryPayload::SystemPing(_) => RegistrationResult::Registered(RegisteredRequest(
                RegisteredRequestKind::SystemPing(envelope),
            )),
            KcpQueryPayload::TaskGet(_) => RegistrationResult::Registered(RegisteredRequest(
                RegisteredRequestKind::TaskGet(envelope),
            )),
            KcpQueryPayload::TaskList(_) => RegistrationResult::KnownCatalogMethodNotImplemented(
                KnownCatalogMethodNotImplemented::TaskList,
            ),
            KcpQueryPayload::EventSubscribe(_) => {
                RegistrationResult::KnownCatalogMethodNotImplemented(
                    KnownCatalogMethodNotImplemented::EventSubscribe,
                )
            }
            KcpQueryPayload::EventPoll(_) => RegistrationResult::KnownCatalogMethodNotImplemented(
                KnownCatalogMethodNotImplemented::EventPoll,
            ),
            KcpQueryPayload::StopStatus(_) => RegistrationResult::KnownCatalogMethodNotImplemented(
                KnownCatalogMethodNotImplemented::StopStatus,
            ),
        },
    }
}

/// Borrowing dispatcher for the three formally registered typed handlers.
#[derive(Debug)]
pub struct TypedDispatcher<'ports, C, G, B> {
    clock: &'ports C,
    ids: &'ports G,
    task_backend: &'ports B,
}

impl<'ports, C, G, B> TypedDispatcher<'ports, C, G, B>
where
    C: KernelClock,
    G: KernelIdGenerator,
    B: TaskApplicationBackend,
{
    /// Borrows the existing handler ports without creating parallel abstractions.
    pub const fn new(clock: &'ports C, ids: &'ports G, task_backend: &'ports B) -> Self {
        Self {
            clock,
            ids,
            task_backend,
        }
    }

    /// Dispatches one registered request and returns the handler result unchanged.
    pub fn dispatch(&self, request: RegisteredRequest) -> HandlerResult {
        self.dispatch_with_validator(request, &SchemaResponseContractValidator)
    }

    pub(crate) fn dispatch_with_validator(
        &self,
        request: RegisteredRequest,
        validator: &impl ResponseContractValidator,
    ) -> HandlerResult {
        match request.0 {
            RegisteredRequestKind::SystemPing(envelope) => {
                handle_system_ping_with_validator(&envelope, self.clock, validator)
            }
            RegisteredRequestKind::TaskCreate(request) => handle_task_create_with_validator(
                &request,
                self.clock,
                self.ids,
                self.task_backend,
                validator,
            ),
            RegisteredRequestKind::TaskGet(envelope) => {
                handle_task_get_with_validator(&envelope, self.clock, self.task_backend, validator)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        ClockError, IdGenerationError, OpaqueIdPurpose, ResponseValidationError,
        TaskCreateBackendResult, TaskCreateOperation, UuidPurpose,
    };
    use crate::{
        narrow_to_registered, preflight_value, BackendError, HandlerContractFailureKind,
        PreflightResult, RegistrationResult,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::Value;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use uuid::Uuid;

    struct RejectFinalResponse;

    impl ResponseContractValidator for RejectFinalResponse {
        fn validate_method_payload(
            &self,
            _schema_id: &str,
            _value: &Value,
        ) -> Result<(), ResponseValidationError> {
            Ok(())
        }

        fn validate_response_envelope(
            &self,
            _value: &Value,
        ) -> Result<(), ResponseValidationError> {
            Err(ResponseValidationError)
        }
    }

    struct Clock(RefCell<VecDeque<DateTime<Utc>>>);
    impl KernelClock for Clock {
        fn now_utc(&self) -> Result<DateTime<Utc>, ClockError> {
            self.0.borrow_mut().pop_front().ok_or(ClockError)
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

    struct Backend(Cell<usize>);
    impl TaskApplicationBackend for Backend {
        fn create_task(
            &self,
            _operation: TaskCreateOperation,
        ) -> Result<TaskCreateBackendResult, BackendError> {
            Err(BackendError::Internal)
        }
        fn get_task(
            &self,
            _task_id: Uuid,
        ) -> Result<Option<kernel_contracts::TaskSpec>, BackendError> {
            self.0.set(self.0.get() + 1);
            Ok(None)
        }
    }

    #[test]
    fn residual_v1_create_narrowing_is_internal_violation_not_stop_activate() {
        // Active method-aware preflight never produces a typed task.create v1 envelope.
        // If a bypassed construction reaches narrowing, it must surface as an honest
        // internal contract violation and never masquerade as another catalog method.
        let envelope = kernel_contracts::TypedKcpCommandEnvelope::decode_after_validation(
            serde_json::json!({
                "protocol_version": "1.0",
                "message_kind": "command",
                "request_id": "11111111-1111-4111-8111-111111111111",
                "actor": {"schema_version":1,"revision":1,"id":"actor","kind":"known_user","source":"actor-source://local/desktop","authentication_level":"platform_verified","confidence":0.9},
                "entry_point": "local_desktop",
                "auth": null,
                "task_id": null,
                "context": null,
                "deadline": "2026-07-18T12:00:10Z",
                "idempotency_key": "key",
                "expected_revision": null,
                "command_type": "task.create",
                "payload": {
                    "schema_version":1,
                    "proposer":"user",
                    "goal":"goal",
                    "constraints":[],
                    "success_criteria":["done"],
                    "risk_hint":null,
                    "capability_hints":[],
                    "task_scope":{"schema_version":1,"resource_patterns":[],"exclusions":[],"allowed_capability_hints":[],"expires_at":null},
                    "delegation_ref":null,
                    "parent_task_id":null,
                    "origin":{"schema_version":1,"kind":"user_input","source_uri":null,"upstream_stable_id":null,"producer_ref":{"kind":"actor","id":"actor"},"parent_origin_refs":[]}
                }
            }),
        )
        .expect("typed v1 create envelope");
        let request =
            TypedCatalogRequest::from_kind_for_test(TypedCatalogRequestKind::CommandV1(envelope));
        assert_eq!(
            narrow_to_registered(request),
            RegistrationResult::InternalContractViolation(
                crate::InternalContractViolation::TaskCreateV1AfterActivePreflight
            )
        );
    }

    #[test]
    fn dispatcher_preserves_handler_contract_failure_without_extra_clock_reads() {
        let value = serde_json::json!({
            "protocol_version":"1.0",
            "message_kind":"query",
            "request_id":"11111111-1111-4111-8111-111111111111",
            "actor":{"schema_version":1,"revision":1,"id":"actor","kind":"known_user","source":"actor-source://local/desktop","authentication_level":"platform_verified","confidence":0.9},
            "entry_point":"local_desktop",
            "auth":null,
            "task_id":null,
            "deadline":"2026-07-18T12:00:10Z",
            "query_type":"task.get",
            "payload":{"schema_version":1,"task_id":"00000000-0000-4000-8000-000000000001"}
        });
        let PreflightResult::Accepted(request) = preflight_value(value) else {
            panic!("accepted")
        };
        let RegistrationResult::Registered(request) = narrow_to_registered(request) else {
            panic!("registered")
        };
        let clock = Clock(RefCell::new([instant(1), instant(2)].into()));
        let backend = Backend(Cell::new(0));
        let dispatcher = TypedDispatcher::new(&clock, &Ids, &backend);
        let HandlerResult::ContractFailure {
            failure,
            post_commit_notification_intents,
        } = dispatcher.dispatch_with_validator(request, &RejectFinalResponse)
        else {
            panic!("contract failure")
        };
        assert_eq!(
            failure.kind,
            HandlerContractFailureKind::FinalResponseInvalid
        );
        assert!(post_commit_notification_intents.is_empty());
        assert!(clock.0.borrow().is_empty());
        assert_eq!(backend.0.get(), 1);
    }

    fn instant(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, second)
            .single()
            .expect("valid time")
    }
}
