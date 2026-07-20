//! Strict Task / TaskScope / ContentOrigin read repository.
//!
//! Active root TaskCreate v2 write path lives in `root_task_create_v2`.
//! Legacy v1 TaskCreate write path (`create_task` / `TaskCreateCommand` / legacy
//! producers) was deleted under ADR-0009 (V2InitialBuildActive slice 3c).

use crate::{StoreError, StoreErrorCode};
use kernel_contracts::{
    canonical_json_string, validate_json, ContentOrigin, ContentOriginV2, TaskScope, TaskSpec,
};
use rusqlite::{Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

const CONTENT_ORIGIN_SCHEMA: &str = "https://schemas.shittim.local/v1/common/content_origin.json";
pub(crate) const TASK_SCOPE_SCHEMA: &str = "https://schemas.shittim.local/v1/task/task_scope.json";
pub(crate) const TASK_SCHEMA: &str = "https://schemas.shittim.local/v1/task/task_spec.json";

pub(crate) fn get_task(connection: &Connection, id: &str) -> Result<Option<TaskSpec>, StoreError> {
    let Some(task) = get_task_shallow(connection, id)? else {
        return Ok(None);
    };
    // Prefer full v1 origin validation when the legacy table still exists (pre-0005 open
    // paths are refused separately); otherwise require a fully validated ContentOriginV2.
    // After migration 0005 the v1 origin tables are dropped, so only v2 remains.
    if table_exists(connection, "content_origins")?
        && get_origin_shallow(connection, &task.origin_ref)?.is_some()
    {
        let origin =
            get_content_origin(connection, &task.origin_ref)?.ok_or_else(stored_invalid)?;
        if origin.id != task.origin_ref {
            return Err(stored_invalid());
        }
    } else {
        let origin =
            get_content_origin_v2(connection, &task.origin_ref)?.ok_or_else(stored_invalid)?;
        if origin.id != task.origin_ref {
            return Err(stored_invalid());
        }
    }
    let scope =
        get_task_scope_shallow(connection, &task.task_scope_ref)?.ok_or_else(stored_invalid)?;
    validate_scope_relations(connection, &scope)?;
    if scope.id != task.task_scope_ref
        || scope.task_id != task.id
        || task.task_scope_ref != scope.id
    {
        return Err(stored_invalid());
    }
    if let Some(parent_id) = &task.parent_task_id {
        if get_task_shallow(connection, parent_id)?.is_none() {
            return Err(stored_invalid());
        }
    }
    Ok(Some(task))
}

pub(crate) fn get_task_scope(
    connection: &Connection,
    id: &str,
) -> Result<Option<TaskScope>, StoreError> {
    let Some(scope) = get_task_scope_shallow(connection, id)? else {
        return Ok(None);
    };
    validate_scope_relations(connection, &scope)?;
    let task = get_task_shallow(connection, &scope.task_id)?.ok_or_else(stored_invalid)?;
    if task.task_scope_ref != scope.id {
        return Err(stored_invalid());
    }
    Ok(Some(scope))
}

/// Reads a legacy ContentOrigin v1 when the table still exists.
///
/// After migration 0005 the table is dropped; this returns `None` without error so
/// shared Task readers can fall through to ContentOriginV2. Open refuses non-empty
/// legacy tables, so production open never serves v1 origin rows.
pub(crate) fn get_content_origin(
    connection: &Connection,
    id: &str,
) -> Result<Option<ContentOrigin>, StoreError> {
    if !table_exists(connection, "content_origins")? {
        return Ok(None);
    }
    let Some(origin) = get_origin_shallow(connection, id)? else {
        return Ok(None);
    };
    if !table_exists(connection, "content_origin_parent_refs")? {
        return Err(stored_invalid());
    }
    let relation_ids = relation_ids(
        connection,
        "SELECT ordinal, parent_origin_id FROM content_origin_parent_refs \
         WHERE origin_id = ?1 ORDER BY ordinal",
        id,
    )?;
    if relation_ids != origin.parent_origin_refs {
        return Err(stored_invalid());
    }
    for parent_id in &relation_ids {
        if get_origin_shallow(connection, parent_id)?.is_none() {
            return Err(stored_invalid());
        }
    }
    Ok(Some(origin))
}

pub(crate) fn encode_contract_document<T: Serialize>(
    schema: &str,
    document: &T,
) -> Result<String, StoreError> {
    let value = serde_json::to_value(document).map_err(|_| serialization_error())?;
    validate_json(schema, &value).map_err(|_| contract_error())?;
    canonical_json_string(&value).map_err(|_| serialization_error())
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

pub(crate) fn get_task_shallow(
    connection: &Connection,
    id: &str,
) -> Result<Option<TaskSpec>, StoreError> {
    get_document(connection, "tasks", TASK_SCHEMA, id)
}

fn get_task_scope_shallow(
    connection: &Connection,
    id: &str,
) -> Result<Option<TaskScope>, StoreError> {
    get_document(connection, "task_scopes", TASK_SCOPE_SCHEMA, id)
}

fn get_origin_shallow(
    connection: &Connection,
    id: &str,
) -> Result<Option<ContentOrigin>, StoreError> {
    if !table_exists(connection, "content_origins")? {
        return Ok(None);
    }
    get_document(connection, "content_origins", CONTENT_ORIGIN_SCHEMA, id)
}

fn get_document<T: DeserializeOwned>(
    connection: &Connection,
    table: &str,
    schema: &str,
    id: &str,
) -> Result<Option<T>, StoreError> {
    let sql = format!("SELECT record_json FROM {table} WHERE id = ?1");
    let stored: Option<String> = connection
        .query_row(&sql, [id], |row| row.get(0))
        .optional()
        .map_err(read_error)?;
    stored
        .map(|stored| decode_contract_document(schema, &stored))
        .transpose()
}

fn validate_scope_relations(connection: &Connection, scope: &TaskScope) -> Result<(), StoreError> {
    let relation_ids = relation_ids(
        connection,
        "SELECT ordinal, origin_id FROM task_scope_source_refs \
         WHERE scope_id = ?1 ORDER BY ordinal",
        &scope.id,
    )?;
    if relation_ids != scope.source_refs {
        return Err(stored_invalid());
    }
    for origin_id in &relation_ids {
        // Scope source_refs must resolve through the same strict origin readers as Task.origin_ref.
        if table_exists(connection, "content_origins")?
            && get_origin_shallow(connection, origin_id)?.is_some()
        {
            let _ = get_content_origin(connection, origin_id)?.ok_or_else(stored_invalid)?;
        } else {
            let _ = get_content_origin_v2(connection, origin_id)?.ok_or_else(stored_invalid)?;
        }
    }
    Ok(())
}

/// True when a ContentOrigin exists in either the legacy v1 table (if present) or active v2 store.
pub(crate) fn origin_exists(connection: &Connection, id: &str) -> Result<bool, StoreError> {
    if table_exists(connection, "content_origins")? && get_origin_shallow(connection, id)?.is_some()
    {
        return Ok(true);
    }
    Ok(get_origin_v2_shallow(connection, id)?.is_some())
}

/// Reads a ContentOriginV2 and validates ordered parent mirrors and parent existence.
pub(crate) fn get_content_origin_v2(
    connection: &Connection,
    id: &str,
) -> Result<Option<ContentOriginV2>, StoreError> {
    let Some(origin) = get_origin_v2_shallow(connection, id)? else {
        return Ok(None);
    };
    let relation_ids = relation_ids(
        connection,
        "SELECT ordinal, parent_origin_id FROM content_origin_v2_parent_refs \
         WHERE origin_id = ?1 ORDER BY ordinal",
        id,
    )?;
    if relation_ids != origin.parent_origin_refs {
        return Err(stored_invalid());
    }
    for parent_id in &relation_ids {
        if !origin_exists(connection, parent_id)? {
            return Err(stored_invalid());
        }
    }
    Ok(Some(origin))
}

fn get_origin_v2_shallow(
    connection: &Connection,
    id: &str,
) -> Result<Option<ContentOriginV2>, StoreError> {
    get_document(
        connection,
        "content_origins_v2",
        "https://schemas.shittim.local/common/content_origin/v2",
        id,
    )
}

fn relation_ids(
    connection: &Connection,
    sql: &str,
    owner_id: &str,
) -> Result<Vec<String>, StoreError> {
    let mut statement = connection.prepare(sql).map_err(read_error)?;
    let mut rows = statement.query([owner_id]).map_err(read_error)?;
    let mut result = Vec::new();
    let mut expected_ordinal = 0_i64;
    while let Some(row) = rows.next().map_err(read_error)? {
        let ordinal: i64 = row.get(0).map_err(read_error)?;
        let id: String = row.get(1).map_err(read_error)?;
        if ordinal != expected_ordinal {
            return Err(stored_invalid());
        }
        result.push(id);
        expected_ordinal += 1;
    }
    Ok(result)
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StoreError> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .map_err(read_error)?;
    Ok(count == 1)
}

fn contract_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::ContractInvalid,
        "task repository facts violate a generated JSON contract",
    )
}

fn serialization_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::SerializationFailed,
        "task repository JSON serialization failed",
    )
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored task repository data failed integrity validation",
    )
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}
