//! Non-connectable KCP Value preflight, registration narrowing, typed dispatcher, and handlers.
//!
//! This crate accepts only already parsed `serde_json::Value` at its preflight boundary and
//! selected typed requests at its handler boundary. It intentionally provides no bytes/UTF-8
//! parser, frame codec, socket, named pipe, transport server, or `agentd` composition root.
//!
//! Slice 3b (`V2InitialBuildActive`) switches runtime onto method-aware production
//! `METHOD_VERSION_BINDINGS` / `select_request_version`: active `task.create` is root-only v2,
//! legacy create v1 is rejected as `unsupported_schema_version`, and the remaining seven methods
//! stay on retained active v1 payloads. Non-empty bindings plus these handlers still do not make
//! the KCP server connectable while five catalog methods lack formal handlers.

#![deny(missing_docs)]

mod dispatcher;
mod handlers;
mod ports;
mod preflight;
mod response;
mod runtime;
pub mod sqlite_adapter;

pub use dispatcher::{
    narrow_to_registered, InternalContractViolation, KnownCatalogMethodNotImplemented,
    RegisteredMethod, RegisteredRequest, RegistrationResult, TypedDispatcher,
};
pub use handlers::{handle_system_ping, handle_task_create, handle_task_get};
pub use ports::{
    BackendError, ClockError, IdGenerationError, KernelClock, KernelIdGenerator, OpaqueIdPurpose,
    TaskApplicationBackend, TaskCreateBackendResult, TaskCreateOperation, UuidPurpose,
};
pub use preflight::{
    preflight_value, PreflightLocalRejection, PreflightLocalRejectionKind, PreflightResult,
    TaskCreateCommandRequestV2, TypedCatalogRequest, TypedCatalogRequestFamily,
};
pub use response::{
    HandledResponse, HandlerContractFailure, HandlerContractFailureKind, HandlerResult,
    PostCommitNotificationIntent,
};
pub use runtime::{RandomKernelIdGenerator, SystemKernelClock};
