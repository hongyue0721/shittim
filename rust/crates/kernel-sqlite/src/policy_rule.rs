//! PolicyRuleV2 append-only revision store + global PolicySet revision counter (IC §6.7).
//!
//! Empty PolicySet bootstrap is revision 0 (authoritative empty state). Every successful
//! rule mutation increments the global counter in the same transaction. Physical delete
//! is forbidden; disable by appending a new revision with `enabled=false`.

use crate::task::encode_contract_document;
use crate::{StoreError, StoreErrorCode, WriteTransaction};
use chrono::{DateTime, SecondsFormat, Utc};
use kernel_contracts::{canonical_json_string, validate_json, PolicyRuleV2};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::Value;

const POLICY_RULE_SCHEMA: &str = "https://schemas.shittim.local/policy/policy_rule/v2";
const POLICY_RULE_SAVEPOINT: &str = "kernel_sqlite_policy_rule_mutation";

/// Result of a PolicyRule mutation that also advances the global PolicySet revision.
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyRuleMutationResult {
    /// Canonical stored rule revision.
    pub rule: PolicyRuleV2,
    /// Global PolicySet revision after this mutation.
    pub policy_set_revision: i64,
}

impl WriteTransaction<'_> {
    /// Appends a PolicyRuleV2 revision and increments the global PolicySet revision.
    ///
    /// Continuity: first revision for a `rule.id` must be `1`; subsequent must be
    /// `max(existing)+1`. Same `(id, revision)` is rejected. Canonical JCS readback.
    pub fn append_policy_rule_revision(
        &self,
        rule: PolicyRuleV2,
    ) -> Result<PolicyRuleMutationResult, StoreError> {
        self.with_savepoint(POLICY_RULE_SAVEPOINT, |connection| {
            append_policy_rule_revision_inside(connection, rule)
        })
    }
}

impl crate::SqliteStore {
    /// Reads one stored PolicyRuleV2 revision (canonical JCS + Schema).
    pub fn get_policy_rule_revision(
        &self,
        rule_id: &str,
        revision: i64,
    ) -> Result<Option<PolicyRuleV2>, StoreError> {
        let connection = self.lock_connection()?;
        get_policy_rule_revision(&connection, rule_id, revision)
    }

    /// Reads the highest revision for a rule id (current head), if any.
    pub fn get_current_policy_rule(
        &self,
        rule_id: &str,
    ) -> Result<Option<PolicyRuleV2>, StoreError> {
        let connection = self.lock_connection()?;
        get_current_policy_rule(&connection, rule_id)
    }

    /// Lists current heads for all rules (max revision per id), ordered by id.
    ///
    /// Includes disabled heads. Evaluation filters `enabled`.
    pub fn list_current_policy_rules(&self) -> Result<Vec<PolicyRuleV2>, StoreError> {
        let connection = self.lock_connection()?;
        list_current_policy_rules(&connection)
    }

    /// Returns the global PolicySet revision (0 when empty / bootstrap).
    pub fn get_policy_set_revision(&self) -> Result<i64, StoreError> {
        let connection = self.lock_connection()?;
        get_policy_set_revision(&connection)
    }
}

pub(crate) fn get_policy_set_revision(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .query_row(
            "SELECT revision FROM policy_set_metadata WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(read_error)
}

pub(crate) fn list_current_policy_rules(
    connection: &Connection,
) -> Result<Vec<PolicyRuleV2>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT pr.record_json FROM policy_rules pr
             INNER JOIN (
                SELECT rule_id, MAX(revision) AS revision
                FROM policy_rules
                GROUP BY rule_id
             ) head ON pr.rule_id = head.rule_id AND pr.revision = head.revision
             ORDER BY pr.rule_id",
        )
        .map_err(read_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(read_error)?;
    let mut rules = Vec::new();
    for row in rows {
        let stored = row.map_err(read_error)?;
        rules.push(decode_policy_rule_document(&stored)?);
    }
    Ok(rules)
}

pub(crate) fn list_enabled_current_policy_rules(
    connection: &Connection,
) -> Result<Vec<PolicyRuleV2>, StoreError> {
    Ok(list_current_policy_rules(connection)?
        .into_iter()
        .filter(|rule| rule.enabled)
        .collect())
}

fn append_policy_rule_revision_inside(
    connection: &Connection,
    rule: PolicyRuleV2,
) -> Result<PolicyRuleMutationResult, StoreError> {
    if rule.id.trim().is_empty() || rule.revision < 1 {
        return Err(contract_error());
    }
    let current_max = max_rule_revision(connection, &rule.id)?;
    let expected = current_max.map_or(1, |max| max + 1);
    if rule.revision != expected {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "policy rule revision is not continuous",
        ));
    }

    let record_json = encode_contract_document(POLICY_RULE_SCHEMA, &rule)?;
    connection
        .execute(
            "INSERT INTO policy_rules(record_json) VALUES (?1)",
            params![record_json],
        )
        .map_err(write_error)?;

    let policy_set_revision = increment_policy_set_revision(connection, &rule.updated_at)?;
    let stored = get_policy_rule_revision(connection, &rule.id, rule.revision)?
        .ok_or_else(stored_invalid)?;
    if stored != rule {
        return Err(stored_invalid());
    }
    Ok(PolicyRuleMutationResult {
        rule: stored,
        policy_set_revision,
    })
}

fn increment_policy_set_revision(
    connection: &Connection,
    updated_at: &str,
) -> Result<i64, StoreError> {
    let current = get_policy_set_revision(connection)?;
    let next = current.checked_add(1).ok_or_else(|| {
        StoreError::new(StoreErrorCode::InternalStoreError, "policy set overflow")
    })?;
    let changed = connection
        .execute(
            "UPDATE policy_set_metadata SET revision = ?1, updated_at = ?2 \
             WHERE id = 1 AND revision = ?3",
            params![next, updated_at, current],
        )
        .map_err(write_error)?;
    if changed != 1 {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "policy set revision CAS failed",
        ));
    }
    Ok(next)
}

fn max_rule_revision(connection: &Connection, rule_id: &str) -> Result<Option<i64>, StoreError> {
    connection
        .query_row(
            "SELECT MAX(revision) FROM policy_rules WHERE rule_id = ?1",
            [rule_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(read_error)
}

fn get_policy_rule_revision(
    connection: &Connection,
    rule_id: &str,
    revision: i64,
) -> Result<Option<PolicyRuleV2>, StoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT record_json FROM policy_rules WHERE rule_id = ?1 AND revision = ?2",
            params![rule_id, revision],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(|stored| decode_policy_rule_document(&stored))
        .transpose()
}

fn get_current_policy_rule(
    connection: &Connection,
    rule_id: &str,
) -> Result<Option<PolicyRuleV2>, StoreError> {
    let Some(max) = max_rule_revision(connection, rule_id)? else {
        return Ok(None);
    };
    get_policy_rule_revision(connection, rule_id, max)
}

fn decode_policy_rule_document(stored: &str) -> Result<PolicyRuleV2, StoreError> {
    decode_contract_document(POLICY_RULE_SCHEMA, stored)
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

/// Formats a UTC timestamp for PolicyRule / PolicySet metadata (second precision).
#[allow(dead_code)]
pub(crate) fn format_policy_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn contract_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::ContractInvalid,
        "policy rule repository facts violate a generated JSON contract",
    )
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored policy rule repository data failed integrity validation",
    )
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}
