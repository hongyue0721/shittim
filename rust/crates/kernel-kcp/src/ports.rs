//! Injectable application ports and closed backend classifications.

use chrono::{DateTime, Utc};
use kernel_contracts::{TaskSpec, TypedKcpCommandEnvelope};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

/// Failure returned by the Kernel clock port.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("kernel clock failed")]
pub struct ClockError;

/// Failure returned by the Kernel identity generator.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("kernel identity generation failed")]
pub struct IdGenerationError;

/// Purpose of one task.create UUID allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UuidPurpose {
    /// Task identity.
    Task,
    /// TaskScope identity.
    TaskScope,
    /// ContentOrigin identity.
    ContentOrigin,
    /// Kernel receipt identity.
    KernelReceipt,
    /// AuditRecord identity.
    AuditRecord,
    /// task.created Event identity.
    Event,
}

/// Purpose of one task.create opaque allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpaqueIdPurpose {
    /// Audit/Event correlation identity.
    Correlation,
    /// Event consumer deduplication identity.
    EventDedup,
}

/// Clock authority used by typed handlers.
pub trait KernelClock {
    /// Returns the current parsed UTC instant.
    fn now_utc(&self) -> Result<DateTime<Utc>, ClockError>;
}

/// Identity authority used by task.create.
pub trait KernelIdGenerator {
    /// Allocates one UUID for the stated purpose.
    fn next_uuid(&self, purpose: UuidPurpose) -> Result<String, IdGenerationError>;

    /// Allocates one non-empty opaque identity for the stated purpose.
    fn next_opaque_id(&self, purpose: OpaqueIdPurpose) -> Result<String, IdGenerationError>;
}

/// Closed application-backend failure classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    /// Task scope URI normalization failed.
    InvalidScopePattern,
    /// The idempotency scope contains different task facts.
    IdempotencyConflict,
    /// Referenced Delegation is unavailable.
    DelegationNotFound,
    /// Referenced parent Task is unavailable.
    ParentTaskNotFound,
    /// Referenced parent ContentOrigin is unavailable.
    ParentOriginNotFound,
    /// SQLite lock acquisition timed out.
    SqliteBusy,
    /// SQLite or its filesystem is full.
    SqliteFull,
    /// SQLite reported corruption or an invalid database.
    SqliteCorrupt,
    /// Stored canonical task data failed integrity validation.
    StoredDataInvalid,
    /// An unclassified application/storage failure occurred.
    Internal,
}

/// Complete typed task.create operation crossing the application/backend boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCreateOperation {
    /// Fully typed command envelope, including the typed TaskCreateRequest payload.
    pub envelope: TypedKcpCommandEnvelope,
    /// The first clock reading, reused for every created fact.
    pub accepted_at: DateTime<Utc>,
    /// New Task UUID.
    pub task_id: Uuid,
    /// New TaskScope UUID.
    pub task_scope_id: Uuid,
    /// New ContentOrigin UUID.
    pub content_origin_id: Uuid,
    /// New Kernel receipt UUID.
    pub receipt_id: Uuid,
    /// New AuditRecord UUID.
    pub audit_id: Uuid,
    /// New task.created Event UUID.
    pub event_id: Uuid,
    /// Non-empty Audit/Event correlation identity.
    pub correlation_id: String,
    /// Non-empty Event deduplication identity.
    pub dedup_key: String,
}

/// Backend result for task.create after its transaction has completed.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskCreateBackendResult {
    /// New facts committed and the Event binding is proven.
    Created {
        /// Current committed Task.
        current_task: TaskSpec,
        /// Event UUID committed to Outbox.
        committed_event_id: Uuid,
    },
    /// Existing equivalent facts were replayed.
    Replayed {
        /// Current Task after replay.
        current_task: TaskSpec,
    },
}

/// High-level Task persistence boundary used by handlers.
pub trait TaskApplicationBackend {
    /// Atomically creates or replays Task facts.
    fn create_task(
        &self,
        operation: TaskCreateOperation,
    ) -> Result<TaskCreateBackendResult, BackendError>;

    /// Reads the current Task by UUID.
    fn get_task(&self, task_id: Uuid) -> Result<Option<TaskSpec>, BackendError>;
}

/// Failure from the internal response-contract validation seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("response contract validation failed")]
pub(crate) struct ResponseValidationError;

/// Internal response-contract seam used only by handler implementation and crate unit tests.
///
/// Public handlers always use the built-in generated-Schema implementation. Keeping this trait
/// crate-private prevents production callers from replacing or bypassing the response contract.
pub(crate) trait ResponseContractValidator {
    /// Validates one method-specific success payload.
    fn validate_method_payload(
        &self,
        schema_id: &str,
        value: &Value,
    ) -> Result<(), ResponseValidationError>;

    /// Validates one final generic response envelope.
    fn validate_response_envelope(&self, value: &Value) -> Result<(), ResponseValidationError>;
}

/// Production response validator backed by `kernel-contracts` generated Schema catalog.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SchemaResponseContractValidator;

impl ResponseContractValidator for SchemaResponseContractValidator {
    fn validate_method_payload(
        &self,
        schema_id: &str,
        value: &Value,
    ) -> Result<(), ResponseValidationError> {
        kernel_contracts::validate_json(schema_id, value).map_err(|_| ResponseValidationError)
    }

    fn validate_response_envelope(&self, value: &Value) -> Result<(), ResponseValidationError> {
        kernel_contracts::validate_json(
            "https://schemas.shittim.local/v1/kcp/response_envelope.json",
            value,
        )
        .map_err(|_| ResponseValidationError)
    }
}
