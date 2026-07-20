//! Adapter from the high-level Task application backend to active root TaskCreate v2.

use crate::{BackendError, TaskApplicationBackend, TaskCreateBackendResult, TaskCreateOperation};
use kernel_contracts::{RootTaskCreateAllocationV2, RootTaskCreateAllocationV2SchemaVersion};
use kernel_sqlite::{
    CreateRootTaskV2Result, RootTaskCreateV2Command, RootTaskCreateV2EnvelopeFacts, SqliteStore,
    StoreError, StoreErrorCode,
};
use serde_json::{Map, Value};
use uuid::Uuid;

/// Task backend backed by one file-based [`SqliteStore`].
#[derive(Debug)]
pub struct SqliteTaskBackend<'store> {
    store: &'store SqliteStore,
}

impl<'store> SqliteTaskBackend<'store> {
    /// Borrows a configured store without exposing its transaction or SQL surface.
    pub const fn new(store: &'store SqliteStore) -> Self {
        Self { store }
    }
}

impl TaskApplicationBackend for SqliteTaskBackend<'_> {
    fn create_task(
        &self,
        operation: TaskCreateOperation,
    ) -> Result<TaskCreateBackendResult, BackendError> {
        let expected_task_id = operation.task_id;
        let expected_event_id = operation.event_id;
        let expected_provenance = operation.creation_provenance_id.to_string();
        let context = context_as_object_map(operation.context)?;
        let command = RootTaskCreateV2Command {
            envelope: RootTaskCreateV2EnvelopeFacts {
                actor: operation.actor,
                entry_point: operation.entry_point,
                request_id: operation.request_id,
                context,
                idempotency_key: operation.idempotency_key,
            },
            request: operation.request,
            allocation: RootTaskCreateAllocationV2 {
                audit_record_id: operation.audit_id.to_string(),
                content_origin_id: operation.content_origin_id.to_string(),
                correlation_id: operation.correlation_id,
                creation_provenance_id: expected_provenance.clone(),
                kernel_receipt_id: operation.receipt_id.to_string(),
                schema_version: RootTaskCreateAllocationV2SchemaVersion,
                task_created_dedup_key: operation.dedup_key,
                task_created_event_id: operation.event_id.to_string(),
                task_id: operation.task_id.to_string(),
                task_scope_id: operation.task_scope_id.to_string(),
            },
            accepted_at: operation.accepted_at,
        };
        let result = self
            .store
            .with_write_transaction(|transaction| transaction.create_root_task_v2(command))
            .map_err(map_store_error)?;
        bind_committed_create_result(
            result,
            expected_task_id,
            expected_event_id,
            &expected_provenance,
        )
    }

    fn get_task(&self, task_id: Uuid) -> Result<Option<kernel_contracts::TaskSpec>, BackendError> {
        self.store
            .get_task(&task_id.to_string())
            .map_err(map_store_error)
    }
}

fn context_as_object_map(
    context: Option<Value>,
) -> Result<Option<Map<String, Value>>, BackendError> {
    match context {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(Value::Null) => Ok(None),
        Some(_) => Err(BackendError::Internal),
    }
}

fn bind_committed_create_result(
    result: CreateRootTaskV2Result,
    expected_task_id: Uuid,
    expected_event_id: Uuid,
    expected_provenance: &str,
) -> Result<TaskCreateBackendResult, BackendError> {
    match result {
        CreateRootTaskV2Result::Created {
            task,
            creation_provenance_ref,
        } => {
            let actual_task_id = Uuid::parse_str(&task.id).map_err(|_| BackendError::Internal)?;
            if actual_task_id != expected_task_id {
                return Err(BackendError::Internal);
            }
            if creation_provenance_ref != expected_provenance {
                return Err(BackendError::Internal);
            }
            // `create_root_task_v2` returns Created only after the repository appended and
            // verified the active Event and the surrounding write transaction committed.
            Ok(TaskCreateBackendResult::Created {
                current_task: task,
                creation_provenance_ref,
                committed_event_id: expected_event_id,
            })
        }
        CreateRootTaskV2Result::Replayed {
            task,
            creation_provenance_ref,
        } => {
            Uuid::parse_str(&task.id).map_err(|_| BackendError::Internal)?;
            Ok(TaskCreateBackendResult::Replayed {
                current_task: task,
                creation_provenance_ref,
            })
        }
    }
}

fn map_store_error(error: StoreError) -> BackendError {
    map_store_error_code(error.code)
}

fn map_store_error_code(code: StoreErrorCode) -> BackendError {
    match code {
        StoreErrorCode::InvalidScopePattern => BackendError::InvalidScopePattern,
        StoreErrorCode::IdempotencyConflict => BackendError::IdempotencyConflict,
        StoreErrorCode::DelegationNotFound => BackendError::DelegationNotFound,
        StoreErrorCode::ParentOriginNotFound => BackendError::ParentOriginNotFound,
        StoreErrorCode::SqliteBusy => BackendError::SqliteBusy,
        StoreErrorCode::SqliteFull => BackendError::SqliteFull,
        StoreErrorCode::SqliteCorrupt => BackendError::SqliteCorrupt,
        StoreErrorCode::StoredDataInvalid => BackendError::StoredDataInvalid,
        StoreErrorCode::InvalidDatabasePath
        | StoreErrorCode::SqliteOpenFailed
        | StoreErrorCode::SqliteConfigurationFailed
        | StoreErrorCode::MigrationFailed
        | StoreErrorCode::MigrationDrift
        | StoreErrorCode::DatabaseSchemaTooNew
        | StoreErrorCode::ConstraintViolation
        | StoreErrorCode::SerializationFailed
        | StoreErrorCode::ContractInvalid
        | StoreErrorCode::InvalidCursor
        | StoreErrorCode::NotFound
        | StoreErrorCode::InternalStoreError => BackendError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_result_rejects_task_identity_mismatch() {
        let task = serde_json::from_value(serde_json::json!({
            "id":"00000000-0000-4000-8000-000000000002",
            "origin_ref":"30000000-0000-4000-8000-000000000001",
            "actor":{"schema_version":1,"id":"actor","kind":"known_user","source":"actor-source://local/desktop","revision":1,"authentication_level":"platform_verified","confidence":0.9},
            "proposer":"user","goal":"goal","constraints":[],"success_criteria":["done"],
            "risk_hint":null,"capability_hints":[],"delegation_ref":null,
            "task_scope_ref":"20000000-0000-4000-8000-000000000001","parent_task_id":null,
            "status":"candidate","plan_version":0,"schema_version":1,"revision":1,
            "created_at":"2026-07-18T12:00:01Z","updated_at":"2026-07-18T12:00:01Z",
            "failed_recovery_meta":null
        }))
        .expect("valid task fixture");
        let result = bind_committed_create_result(
            CreateRootTaskV2Result::Created {
                task,
                creation_provenance_ref: "00000000-0000-4000-8000-000000000005".into(),
            },
            Uuid::parse_str("00000000-0000-4000-8000-000000000001").expect("expected task uuid"),
            Uuid::parse_str("00000000-0000-4000-8000-000000000007").expect("expected event uuid"),
            "00000000-0000-4000-8000-000000000005",
        );
        assert_eq!(result, Err(BackendError::Internal));
    }

    #[test]
    fn every_current_store_error_code_has_an_explicit_mapping() {
        let cases = [
            (
                StoreErrorCode::InvalidScopePattern,
                BackendError::InvalidScopePattern,
            ),
            (
                StoreErrorCode::IdempotencyConflict,
                BackendError::IdempotencyConflict,
            ),
            (
                StoreErrorCode::DelegationNotFound,
                BackendError::DelegationNotFound,
            ),
            (
                StoreErrorCode::ParentOriginNotFound,
                BackendError::ParentOriginNotFound,
            ),
            (StoreErrorCode::SqliteBusy, BackendError::SqliteBusy),
            (StoreErrorCode::SqliteFull, BackendError::SqliteFull),
            (StoreErrorCode::SqliteCorrupt, BackendError::SqliteCorrupt),
            (
                StoreErrorCode::StoredDataInvalid,
                BackendError::StoredDataInvalid,
            ),
            (StoreErrorCode::InvalidDatabasePath, BackendError::Internal),
            (StoreErrorCode::SqliteOpenFailed, BackendError::Internal),
            (
                StoreErrorCode::SqliteConfigurationFailed,
                BackendError::Internal,
            ),
            (StoreErrorCode::MigrationFailed, BackendError::Internal),
            (StoreErrorCode::MigrationDrift, BackendError::Internal),
            (StoreErrorCode::DatabaseSchemaTooNew, BackendError::Internal),
            (StoreErrorCode::ConstraintViolation, BackendError::Internal),
            (StoreErrorCode::SerializationFailed, BackendError::Internal),
            (StoreErrorCode::ContractInvalid, BackendError::Internal),
            (StoreErrorCode::InvalidCursor, BackendError::Internal),
            (StoreErrorCode::NotFound, BackendError::Internal),
            (StoreErrorCode::InternalStoreError, BackendError::Internal),
        ];
        for (code, expected) in cases {
            assert_eq!(map_store_error_code(code), expected);
        }
    }
}
