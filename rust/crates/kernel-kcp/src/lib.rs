//! Non-connectable KCP Value preflight, registration narrowing, typed dispatcher, and handlers.
//!
//! This crate accepts only already parsed `serde_json::Value` at its preflight boundary and
//! generated typed envelopes at its handler boundary. It intentionally provides no bytes/UTF-8
//! parser, frame codec, socket, named pipe, transport server, or `agentd` composition root.
//!
//! Slice 3a (`V2InitialBuildActive`) activates production `METHOD_VERSION_BINDINGS` and the
//! generated `select_request_version` library selectors. This crate still consumes the retained
//! v1 preflight/dispatcher path; method-aware V2 preflight/dispatcher/handler wiring is slice 3b.
//! Non-empty bindings therefore do not imply this runtime path is active or server-ready.

#![deny(missing_docs)]

mod dispatcher;
mod handlers;
mod ports;
mod preflight;
mod response;
mod runtime;
pub mod sqlite_adapter;

pub use dispatcher::{
    narrow_to_registered, KnownCatalogMethodNotImplemented, RegisteredMethod, RegisteredRequest,
    RegistrationResult, TypedDispatcher,
};
pub use handlers::{handle_system_ping, handle_task_create, handle_task_get};
pub use ports::{
    BackendError, ClockError, IdGenerationError, KernelClock, KernelIdGenerator, OpaqueIdPurpose,
    TaskApplicationBackend, TaskCreateBackendResult, TaskCreateOperation, UuidPurpose,
};
pub use preflight::{
    preflight_value, PreflightLocalRejection, PreflightLocalRejectionKind, PreflightResult,
    TypedCatalogRequest, TypedCatalogRequestFamily,
};
pub use response::{
    HandledResponse, HandlerContractFailure, HandlerContractFailureKind, HandlerResult,
    PostCommitNotificationIntent,
};
pub use runtime::{RandomKernelIdGenerator, SystemKernelClock};
