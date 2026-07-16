//! Immutable AuditRecord persistence.

use crate::{StoreError, StoreErrorCode};
use kernel_contracts::{
    canonical_json_string, validate_json, AuditRecord, AuditRecordExternalContentStatus,
};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

const AUDIT_SCHEMA: &str = "https://schemas.shittim.local/v1/audit/audit_record.json";

pub(crate) fn prepare_audit(record: &AuditRecord) -> Result<String, StoreError> {
    enforce_sent_support(record)?;
    let value = serde_json::to_value(record).map_err(|_| {
        StoreError::new(
            StoreErrorCode::SerializationFailed,
            "AuditRecord serialization failed",
        )
    })?;
    validate_json(AUDIT_SCHEMA, &value).map_err(|_| {
        StoreError::new(
            StoreErrorCode::ContractInvalid,
            "AuditRecord violates its JSON contract",
        )
    })?;
    canonical_json_string(&value).map_err(|_| {
        StoreError::new(
            StoreErrorCode::SerializationFailed,
            "AuditRecord canonicalization failed",
        )
    })
}

pub(crate) fn insert_audit(
    connection: &Connection,
    record: &AuditRecord,
) -> Result<(), StoreError> {
    let canonical = prepare_audit(record)?;
    connection
        .execute(
            "INSERT INTO audit_records(record_json) VALUES (?1)",
            [canonical],
        )
        .map(|_| ())
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))
}

pub(crate) fn get_audit(
    connection: &Connection,
    id: &str,
) -> Result<Option<AuditRecord>, StoreError> {
    let json: Option<String> = connection
        .query_row(
            "SELECT record_json FROM audit_records \
             WHERE json_extract(record_json, '$.id') = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    json.map(|json| decode_audit(&json)).transpose()
}

fn decode_audit(json: &str) -> Result<AuditRecord, StoreError> {
    let value: Value = serde_json::from_str(json).map_err(|_| {
        StoreError::new(
            StoreErrorCode::SerializationFailed,
            "stored AuditRecord JSON cannot be parsed",
        )
    })?;
    validate_json(AUDIT_SCHEMA, &value).map_err(|_| {
        StoreError::new(
            StoreErrorCode::ContractInvalid,
            "stored AuditRecord violates its JSON contract",
        )
    })?;
    serde_json::from_value(value).map_err(|_| {
        StoreError::new(
            StoreErrorCode::SerializationFailed,
            "stored AuditRecord cannot be decoded",
        )
    })
}

fn enforce_sent_support(record: &AuditRecord) -> Result<(), StoreError> {
    if record.external_content_status == AuditRecordExternalContentStatus::Sent
        && record.content_origin_refs.is_empty()
        && record.artifact_refs.is_empty()
        && record.resource_refs.is_empty()
        && record.model_call_refs.is_empty()
        && record.payload_manifest_refs.is_empty()
        && record.causation_ref.is_none()
    {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "sent AuditRecord requires at least one stable producer reference",
        ));
    }
    Ok(())
}
