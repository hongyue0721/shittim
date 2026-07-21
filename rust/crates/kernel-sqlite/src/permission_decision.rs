//! PermissionDecisionV2 immutable append-only repository (IC §6.6 / §6.10.6).
//!
//! `decision_revision` is allocated by the repository as max(action)+1 and must be continuous.
//! Physical update/delete are forbidden. Action.permission_decision_ref must equal the current
//! mapping after evaluation binds it (enforced by evaluation orchestration / CAS).

use crate::action::get_action_shallow;
use crate::task::encode_contract_document;
use crate::{StoreError, StoreErrorCode, WriteTransaction};
use kernel_contracts::{canonical_json_string, validate_json, PermissionDecisionV2};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::Value;

const PERMISSION_DECISION_SCHEMA: &str =
    "https://schemas.shittim.local/policy/permission_decision/v2";
const PERMISSION_DECISION_SAVEPOINT: &str = "kernel_sqlite_permission_decision_append";

impl WriteTransaction<'_> {
    /// Appends an immutable PermissionDecisionV2 for an Action.
    ///
    /// Allocates `decision_revision = max(existing for action)+1` (first = 1). Rejects gaps.
    /// Owning Action must exist. Canonical JCS readback. No update/delete path.
    pub fn append_permission_decision(
        &self,
        mut decision: PermissionDecisionV2,
    ) -> Result<PermissionDecisionV2, StoreError> {
        self.with_savepoint(PERMISSION_DECISION_SAVEPOINT, |connection| {
            let allocated = next_decision_revision(connection, &decision.action_id)?;
            // Fail closed if caller claimed a non-zero revision that is not the next continuous.
            if decision.decision_revision != 0 && decision.decision_revision != allocated {
                return Err(StoreError::new(
                    StoreErrorCode::ConstraintViolation,
                    "permission decision_revision is not continuous",
                ));
            }
            decision.decision_revision = allocated;
            append_permission_decision_inside(connection, decision)
        })
    }
}

impl crate::SqliteStore {
    /// Reads one PermissionDecisionV2 by id.
    pub fn get_permission_decision(
        &self,
        id: &str,
    ) -> Result<Option<PermissionDecisionV2>, StoreError> {
        let connection = self.lock_connection()?;
        get_permission_decision(&connection, id)
    }

    /// Reads the current (highest decision_revision) PermissionDecision for an Action.
    pub fn get_current_permission_decision_for_action(
        &self,
        action_id: &str,
    ) -> Result<Option<PermissionDecisionV2>, StoreError> {
        let connection = self.lock_connection()?;
        get_current_for_action(&connection, action_id)
    }

    /// Lists all PermissionDecisions for an Action ordered by decision_revision ascending.
    pub fn list_permission_decisions_for_action(
        &self,
        action_id: &str,
    ) -> Result<Vec<PermissionDecisionV2>, StoreError> {
        let connection = self.lock_connection()?;
        list_for_action(&connection, action_id)
    }

    /// Validates that Action.permission_decision_ref equals PD repository current mapping.
    ///
    /// Returns the current decision when consistent. Fail closed on mismatch or corruption.
    pub fn validate_current_permission_decision_for_action(
        &self,
        action_id: &str,
    ) -> Result<Option<PermissionDecisionV2>, StoreError> {
        let connection = self.lock_connection()?;
        validate_current_for_action(&connection, action_id)
    }
}

pub(crate) fn get_permission_decision(
    connection: &Connection,
    id: &str,
) -> Result<Option<PermissionDecisionV2>, StoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT record_json FROM permission_decisions WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let decision = decode_permission_decision_document(&stored)?;
    if decision.id != id {
        return Err(stored_invalid());
    }
    Ok(Some(decision))
}

pub(crate) fn get_current_for_action(
    connection: &Connection,
    action_id: &str,
) -> Result<Option<PermissionDecisionV2>, StoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT record_json FROM permission_decisions \
             WHERE action_id = ?1 \
             ORDER BY decision_revision DESC LIMIT 1",
            [action_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(|stored| decode_permission_decision_document(&stored))
        .transpose()
}

pub(crate) fn list_for_action(
    connection: &Connection,
    action_id: &str,
) -> Result<Vec<PermissionDecisionV2>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT record_json FROM permission_decisions \
             WHERE action_id = ?1 ORDER BY decision_revision ASC",
        )
        .map_err(read_error)?;
    let rows = statement
        .query_map([action_id], |row| row.get::<_, String>(0))
        .map_err(read_error)?;
    let mut decisions = Vec::new();
    let mut expected = 1i64;
    for row in rows {
        let stored = row.map_err(read_error)?;
        let decision = decode_permission_decision_document(&stored)?;
        if decision.action_id != action_id || decision.decision_revision != expected {
            return Err(stored_invalid());
        }
        expected = expected.checked_add(1).ok_or_else(|| {
            StoreError::new(StoreErrorCode::StoredDataInvalid, "revision overflow")
        })?;
        decisions.push(decision);
    }
    Ok(decisions)
}

pub(crate) fn validate_current_for_action(
    connection: &Connection,
    action_id: &str,
) -> Result<Option<PermissionDecisionV2>, StoreError> {
    let action = get_action_shallow(connection, action_id)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::NotFound,
            "action was not found for permission decision validation",
        )
    })?;
    let current = get_current_for_action(connection, action_id)?;
    match (&action.permission_decision_ref, &current) {
        (None, None) => Ok(None),
        (Some(ref_id), Some(decision)) if ref_id == &decision.id => Ok(Some(decision.clone())),
        (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) => Err(StoreError::new(
            StoreErrorCode::StoredDataInvalid,
            "action permission_decision_ref is not consistent with permission decision current mapping",
        )),
    }
}

fn next_decision_revision(connection: &Connection, action_id: &str) -> Result<i64, StoreError> {
    let max: Option<i64> = connection
        .query_row(
            "SELECT MAX(decision_revision) FROM permission_decisions WHERE action_id = ?1",
            [action_id],
            |row| row.get(0),
        )
        .map_err(read_error)?;
    Ok(max.map_or(1, |value| value + 1))
}

fn append_permission_decision_inside(
    connection: &Connection,
    decision: PermissionDecisionV2,
) -> Result<PermissionDecisionV2, StoreError> {
    validate_decision_shape(&decision)?;
    if get_action_shallow(connection, &decision.action_id)?.is_none() {
        return Err(StoreError::new(
            StoreErrorCode::NotFound,
            "owning action was not found for permission decision append",
        ));
    }
    if get_permission_decision(connection, &decision.id)?.is_some() {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "permission decision id already exists",
        ));
    }
    let expected = next_decision_revision(connection, &decision.action_id)?;
    // After allocation in outer path, expected equals decision.decision_revision.
    // Re-check under the same savepoint for races inside nested callers.
    if decision.decision_revision != expected {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "permission decision_revision is not continuous",
        ));
    }

    let record_json = encode_contract_document(PERMISSION_DECISION_SCHEMA, &decision)?;
    connection
        .execute(
            "INSERT INTO permission_decisions(record_json) VALUES (?1)",
            params![record_json],
        )
        .map_err(write_error)?;

    let stored = get_permission_decision(connection, &decision.id)?.ok_or_else(stored_invalid)?;
    if stored != decision {
        return Err(stored_invalid());
    }
    Ok(stored)
}

fn validate_decision_shape(decision: &PermissionDecisionV2) -> Result<(), StoreError> {
    if decision.id.trim().is_empty()
        || decision.action_id.trim().is_empty()
        || decision.decision_revision < 1
        || decision.policy_set_revision < 0
        || decision.material_authorization_fingerprint.len() != 64
        || decision.observation_evidence_fingerprint.len() != 64
        || decision.binding.action_id != decision.action_id
    {
        return Err(contract_error());
    }
    uuid::Uuid::parse_str(&decision.id).map_err(|_| contract_error())?;
    uuid::Uuid::parse_str(&decision.action_id).map_err(|_| contract_error())?;
    Ok(())
}

fn decode_permission_decision_document(stored: &str) -> Result<PermissionDecisionV2, StoreError> {
    decode_contract_document(PERMISSION_DECISION_SCHEMA, stored)
}

fn decode_contract_document<T: DeserializeOwned>(
    schema: &str,
    stored: &str,
) -> Result<T, StoreError> {
    let value: Value = serde_json::from_str(stored).map_err(|_| stored_invalid())?;
    validate_json(schema, &value).map_err(|_| stored_invalid())?;
    let canonical = canonical_json_string(&value).map_err(|_| stored_invalid())?;
    if canonical != stored {
        return Err(stored_invalid());
    }
    serde_json::from_value(value).map_err(|_| stored_invalid())
}

fn contract_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::ContractInvalid,
        "permission decision repository facts violate a generated JSON contract",
    )
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored permission decision repository data failed integrity validation",
    )
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}
