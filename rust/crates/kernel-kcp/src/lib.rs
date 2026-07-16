//! Non-connectable typed KCP application handlers.
//!
//! This crate begins after JSON/schema preflight and typed envelope decoding. It intentionally
//! provides no raw JSON parser, frame codec, dispatcher, socket, named pipe, server, or `agentd`.

#![deny(missing_docs)]

mod handlers;
mod ports;
mod response;
pub mod sqlite_adapter;

pub use handlers::{handle_system_ping, handle_task_create, handle_task_get};
pub use ports::{
    BackendError, ClockError, IdGenerationError, KernelClock, KernelIdGenerator, OpaqueIdPurpose,
    TaskApplicationBackend, TaskCreateBackendResult, TaskCreateOperation, UuidPurpose,
};
pub use response::{
    HandledResponse, HandlerContractFailure, HandlerContractFailureKind, HandlerResult,
    PostCommitNotificationIntent,
};
