//! Non-connectable KCP Value preflight, registration narrowing, typed dispatcher, and handlers.
//!
//! This crate accepts only already parsed `serde_json::Value` at its preflight boundary and
//! generated typed envelopes at its handler boundary. It intentionally provides no bytes/UTF-8
//! parser, frame codec, socket, named pipe, transport server, or `agentd` composition root.

#![deny(missing_docs)]

mod dispatcher;
mod handlers;
mod ports;
mod preflight;
mod response;
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
