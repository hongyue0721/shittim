//! Active ActionRequestV2 current-snapshot repository (IC §6.10.6 closed set).
//!
//! Slice 4a implements `insert_pending`, `get`, and crate-private
//! `transition_with_expected_revision` (internal CAS helper only).
//! Status-changing transitions that emit `action.state_changed` are authoritative only through
//! [`crate::action_transition::WriteTransaction::mark_committed_with_event`] (IC §6.14).
//! Policy binding CAS, lease, child materialization, and recovery listing remain future slices.
//! Domain edge legality and evidence gates are owned by `domain-task`.

use crate::task::{encode_contract_document, get_task_shallow};
use crate::{StoreError, StoreErrorCode, WriteTransaction};
use chrono::{DateTime, SecondsFormat, Utc};
use domain_task::{
    apply_action_transition, is_action_transition_allowed, ActionEffects, ActionEvidence,
    ActionTransitionCommand,
};
use kernel_contracts::{
    canonical_json_string, validate_json, ActionRequestV2, ActionRequestV2Result,
    ActionRequestV2SchemaVersion, ActionStatus, SideEffectClass,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

const ACTION_SCHEMA: &str = "https://schemas.shittim.local/task/action_request/v2";
const ACTION_INSERT_SAVEPOINT: &str = "kernel_sqlite_insert_pending_action";
// Crate-private CAS helper path (no Outbox). Production status-event edges use mark_committed_with_event.
#[cfg_attr(not(test), allow(dead_code))]
const ACTION_TRANSITION_SAVEPOINT: &str = "kernel_sqlite_action_transition_cas";

/// Draft facts for inserting a pending Action (revision fixed to 1, status fixed to pending).
#[derive(Debug, Clone, PartialEq)]
pub struct InsertPendingActionCommand {
    /// Caller-allocated Action UUID.
    pub action_id: String,
    /// Owning Task UUID; must already exist.
    pub task_id: String,
    /// Optional step identifier.
    pub step_id: Option<String>,
    /// Optional parent Action (compensation). `None` for original Actions.
    pub parent_action_id: Option<String>,
    /// Capability identifier.
    pub capability_id: String,
    /// Operation name.
    pub operation: String,
    /// Structured arguments object.
    pub structured_arguments: Map<String, Value>,
    /// Resource refs.
    pub resource_refs: Vec<String>,
    /// TaskScope UUID referenced by this Action.
    pub task_scope_ref: String,
    /// Side-effect class.
    pub side_effect_class: SideEffectClass,
    /// Non-empty idempotency key.
    pub idempotency_key: String,
    /// Initial execution generation (typically 0).
    pub execution_generation: i64,
    /// Verification policy required fields.
    pub verification_policy: ActionRequestV2VerificationPolicyInput,
    /// Optional rollback policy.
    pub rollback_policy: Option<kernel_contracts::ActionRequestV2RollbackPolicy>,
    /// Optional recovery meta; pending drafts usually null.
    pub recovery_meta: Option<kernel_contracts::ActionRequestV2RecoveryMeta>,
    /// Caller-injected creation time (UTC second precision preferred by producers).
    pub created_at: DateTime<Utc>,
}

/// Verification policy input for pending insert (mirrors generated type without Schema const).
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRequestV2VerificationPolicyInput {
    /// Strategy string.
    pub strategy: String,
    /// Expected outcome JSON value.
    pub expected_outcome: Value,
    /// Timeout string.
    pub timeout: String,
}

/// CAS transition command for Action current snapshot (crate-private helper input).
///
/// Not a public dual authority for status-event edges. Status-changing transitions that
/// emit `action.state_changed` must go through
/// [`crate::action_transition::WriteTransaction::mark_committed_with_event`].
/// Edges whose domain outcome requires lease/lock effects currently fail closed until the
/// lease API lands.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct TransitionActionCommand {
    /// Action UUID.
    pub action_id: String,
    /// Expected current revision for CAS.
    pub expected_revision: i64,
    /// Expected current status (must match stored).
    pub expected_status: ActionStatus,
    /// Desired target status.
    pub target_status: ActionStatus,
    /// Non-empty structured reason.
    pub reason: String,
    /// Domain evidence bag (permission/approval/verification/dispatch).
    pub evidence: ActionEvidence,
    /// Optional result fields to apply on this transition (e.g. completed).
    pub result: Option<ActionRequestV2Result>,
    /// Optional permission decision to bind (if domain requires it).
    pub permission_decision_ref: Option<String>,
    /// Optional approval chain to bind.
    pub approval_chain_id: Option<String>,
    /// Optional lease update.
    pub lease: Option<kernel_contracts::ActionRequestV2Lease>,
    /// Optional recovery meta update.
    pub recovery_meta: Option<kernel_contracts::ActionRequestV2RecoveryMeta>,
    /// Caller-injected transition time.
    pub updated_at: DateTime<Utc>,
}

impl WriteTransaction<'_> {
    /// Inserts a pending Action draft with `revision=1` and `status=pending`.
    ///
    /// Policy binding refs start null. Fails if the Task does not exist or Action ID collides.
    pub fn insert_pending_action(
        &self,
        command: InsertPendingActionCommand,
    ) -> Result<ActionRequestV2, StoreError> {
        self.with_savepoint(ACTION_INSERT_SAVEPOINT, |connection| {
            insert_pending_inside(connection, command)
        })
    }

    /// Crate-private CAS helper for Action current snapshot (domain evidence + expected revision).
    ///
    /// Does **not** write Outbox and is **not** a public dual write path for status events.
    /// Callers that must emit `action.state_changed` use `mark_committed_with_event` only.
    /// Domain outcomes that require lease/lock release effects fail closed until lease API lands.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn transition_with_expected_revision(
        &self,
        command: TransitionActionCommand,
    ) -> Result<ActionRequestV2, StoreError> {
        self.with_savepoint(ACTION_TRANSITION_SAVEPOINT, |connection| {
            transition_inside(connection, command)
        })
    }
}

impl crate::SqliteStore {
    /// Reads and revalidates an ActionRequestV2 current snapshot.
    pub fn get_action(&self, id: &str) -> Result<Option<ActionRequestV2>, StoreError> {
        let connection = self.lock_connection()?;
        get_action(&connection, id)
    }
}

pub(crate) fn get_action(
    connection: &Connection,
    id: &str,
) -> Result<Option<ActionRequestV2>, StoreError> {
    let Some(action) = get_action_shallow(connection, id)? else {
        return Ok(None);
    };
    if action.action_id != id {
        return Err(stored_invalid());
    }
    // Owning Task must still resolve (shallow is enough for relation existence).
    if get_task_shallow(connection, &action.task_id)?.is_none() {
        return Err(stored_invalid());
    }
    Ok(Some(action))
}

pub(crate) fn get_action_shallow(
    connection: &Connection,
    id: &str,
) -> Result<Option<ActionRequestV2>, StoreError> {
    get_action_document(connection, id)
}

fn insert_pending_inside(
    connection: &Connection,
    command: InsertPendingActionCommand,
) -> Result<ActionRequestV2, StoreError> {
    validate_insert_command(&command)?;
    if get_task_shallow(connection, &command.task_id)?.is_none() {
        return Err(StoreError::new(
            StoreErrorCode::NotFound,
            "owning task was not found for pending action insert",
        ));
    }
    if let Some(parent) = &command.parent_action_id {
        if get_action_shallow(connection, parent)?.is_none() {
            return Err(StoreError::new(
                StoreErrorCode::NotFound,
                "parent action was not found for pending action insert",
            ));
        }
    }
    if get_action_shallow(connection, &command.action_id)?.is_some() {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action id already exists",
        ));
    }

    let created_at = format_time(command.created_at);
    let action = ActionRequestV2 {
        action_id: command.action_id.clone(),
        task_id: command.task_id.clone(),
        step_id: command.step_id.clone(),
        parent_action_id: command.parent_action_id.clone(),
        capability_id: command.capability_id.clone(),
        operation: command.operation.clone(),
        structured_arguments: Value::Object(command.structured_arguments.clone()),
        resource_refs: command.resource_refs.clone(),
        task_scope_ref: command.task_scope_ref.clone(),
        side_effect_class: command.side_effect_class,
        idempotency_key: command.idempotency_key.clone(),
        execution_generation: command.execution_generation,
        permission_decision_ref: None,
        approval_chain_id: None,
        verification_policy: kernel_contracts::ActionRequestV2VerificationPolicy {
            strategy: command.verification_policy.strategy.clone(),
            expected_outcome: command.verification_policy.expected_outcome.clone(),
            timeout: command.verification_policy.timeout.clone(),
        },
        rollback_policy: command.rollback_policy.clone(),
        result: None,
        status: ActionStatus::Pending,
        recovery_meta: command.recovery_meta.clone(),
        lease: None,
        schema_version: ActionRequestV2SchemaVersion,
        revision: 1,
        created_at: created_at.clone(),
        updated_at: created_at,
    };

    let record_json = encode_contract_document(ACTION_SCHEMA, &action)?;
    connection
        .execute(
            "INSERT INTO actions(record_json) VALUES (?1)",
            params![record_json],
        )
        .map_err(write_error)?;

    get_action(connection, &action.action_id)?.ok_or_else(stored_invalid)
}

#[cfg_attr(not(test), allow(dead_code))]
fn transition_inside(
    connection: &Connection,
    command: TransitionActionCommand,
) -> Result<ActionRequestV2, StoreError> {
    validate_transition_command(&command)?;
    let current = get_action(connection, &command.action_id)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::NotFound,
            "action was not found for transition",
        )
    })?;

    if current.revision != command.expected_revision {
        return Err(revision_conflict(
            &command.action_id,
            command.expected_revision,
            current.revision,
        ));
    }
    if current.status != command.expected_status {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action expected status does not match stored status",
        ));
    }
    if !is_action_transition_allowed(current.status, command.target_status) {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "illegal action status transition",
        ));
    }

    let domain_cmd = ActionTransitionCommand {
        action_id: current.action_id.clone(),
        parent_action_id: current.parent_action_id.clone(),
        current_status: current.status,
        current_revision: current.revision as u64,
        expected_revision: Some(command.expected_revision as u64),
        target_status: command.target_status,
        reason: command.reason.clone(),
        evidence: command.evidence.clone(),
    };
    let outcome = apply_action_transition(&domain_cmd).map_err(map_domain_error)?;
    if outcome.new_status != command.target_status {
        return Err(StoreError::new(
            StoreErrorCode::InternalStoreError,
            "domain outcome status mismatch",
        ));
    }
    if outcome.new_revision as i64 != current.revision + 1 {
        return Err(StoreError::new(
            StoreErrorCode::InternalStoreError,
            "domain outcome revision mismatch",
        ));
    }
    // Lease/lock release effects are not persisted in this slice; refuse rather than half-commit.
    reject_unhandled_action_effects(&outcome.effects)?;

    let mut next = current.clone();
    next.status = outcome.new_status;
    next.revision = outcome.new_revision as i64;
    next.updated_at = format_time(command.updated_at);
    if let Some(permission) = command.permission_decision_ref.clone() {
        next.permission_decision_ref = Some(permission);
    } else if let Some(permission) = command.evidence.permission_decision_ref.clone() {
        // Domain evidence may carry the decision that authorized this edge.
        if next.permission_decision_ref.is_none() {
            next.permission_decision_ref = Some(permission);
        }
    }
    if let Some(chain) = command.approval_chain_id.clone() {
        next.approval_chain_id = Some(chain);
    }
    if let Some(result) = command.result.clone() {
        next.result = Some(result);
    }
    if let Some(lease) = command.lease.clone() {
        next.lease = Some(lease);
    }
    if let Some(meta) = command.recovery_meta.clone() {
        next.recovery_meta = Some(meta);
    }

    // CAS update: only replace when stored revision still matches expected.
    let record_json = encode_contract_document(ACTION_SCHEMA, &next)?;
    let changed = connection
        .execute(
            "UPDATE actions SET record_json = ?1 \
             WHERE id = ?2 AND revision = ?3 AND status = ?4",
            params![
                record_json,
                command.action_id,
                command.expected_revision,
                command.expected_status.as_str(),
            ],
        )
        .map_err(write_error)?;
    if changed != 1 {
        // Re-read to distinguish race vs corruption.
        let latest = get_action_shallow(connection, &command.action_id)?.ok_or_else(|| {
            StoreError::new(
                StoreErrorCode::NotFound,
                "action disappeared during transition CAS",
            )
        })?;
        return Err(revision_conflict(
            &command.action_id,
            command.expected_revision,
            latest.revision,
        ));
    }

    let stored = get_action(connection, &command.action_id)?.ok_or_else(stored_invalid)?;
    if stored.revision != next.revision
        || stored.status != next.status
        || stored.action_id != next.action_id
    {
        return Err(stored_invalid());
    }
    Ok(stored)
}

/// Internal CAS used by ActionTransitionIntent `mark_committed_with_event`.
///
/// Applies the transition dictated by an intent (from→to, expected revision) **after** the mark
/// path has already run `apply_action_transition` (full evidence closed set). This helper only
/// performs optimistic CAS + optional field projection; it must not be used as a domain-evidence
/// bypass.
pub(crate) fn cas_transition_for_intent(
    connection: &Connection,
    action_id: &str,
    expected_revision: i64,
    from_status: ActionStatus,
    to_status: ActionStatus,
    updated_at: DateTime<Utc>,
    mut project: impl FnMut(&mut ActionRequestV2),
) -> Result<ActionRequestV2, StoreError> {
    let current = get_action(connection, action_id)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::NotFound,
            "action was not found for intent commit",
        )
    })?;
    if current.revision != expected_revision {
        return Err(revision_conflict(
            action_id,
            expected_revision,
            current.revision,
        ));
    }
    if current.status != from_status {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action status does not match transition intent from_status",
        ));
    }
    if !is_action_transition_allowed(from_status, to_status) {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "illegal action status transition for intent commit",
        ));
    }

    let mut next = current.clone();
    next.status = to_status;
    next.revision = expected_revision
        .checked_add(1)
        .ok_or_else(|| StoreError::new(StoreErrorCode::ContractInvalid, "revision overflow"))?;
    next.updated_at = format_time(updated_at);
    project(&mut next);

    let record_json = encode_contract_document(ACTION_SCHEMA, &next)?;
    let changed = connection
        .execute(
            "UPDATE actions SET record_json = ?1 \
             WHERE id = ?2 AND revision = ?3 AND status = ?4",
            params![
                record_json,
                action_id,
                expected_revision,
                from_status.as_str()
            ],
        )
        .map_err(write_error)?;
    if changed != 1 {
        let latest = get_action_shallow(connection, action_id)?.ok_or_else(|| {
            StoreError::new(
                StoreErrorCode::NotFound,
                "action disappeared during intent CAS",
            )
        })?;
        return Err(revision_conflict(
            action_id,
            expected_revision,
            latest.revision,
        ));
    }
    get_action(connection, action_id)?.ok_or_else(stored_invalid)
}

fn validate_insert_command(command: &InsertPendingActionCommand) -> Result<(), StoreError> {
    parse_uuid(&command.action_id)?;
    parse_uuid(&command.task_id)?;
    parse_uuid(&command.task_scope_ref)?;
    if let Some(parent) = &command.parent_action_id {
        parse_uuid(parent)?;
        if parent == &command.action_id {
            return Err(contract_error());
        }
    }
    if command.capability_id.is_empty()
        || command.operation.is_empty()
        || command.idempotency_key.is_empty()
        || command.verification_policy.strategy.is_empty()
        || command.verification_policy.timeout.is_empty()
        || command.execution_generation < 0
    {
        return Err(contract_error());
    }
    if let Some(step) = &command.step_id {
        if step.is_empty() {
            return Err(contract_error());
        }
    }
    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
fn validate_transition_command(command: &TransitionActionCommand) -> Result<(), StoreError> {
    parse_uuid(&command.action_id)?;
    if command.expected_revision < 1 || command.reason.trim().is_empty() {
        return Err(contract_error());
    }
    if command.expected_status == command.target_status {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "action transition requires distinct from and to status",
        ));
    }
    Ok(())
}

fn get_action_document(
    connection: &Connection,
    id: &str,
) -> Result<Option<ActionRequestV2>, StoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT record_json FROM actions WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(|stored| decode_action_document(&stored))
        .transpose()
}

fn decode_action_document(stored: &str) -> Result<ActionRequestV2, StoreError> {
    decode_contract_document(ACTION_SCHEMA, stored)
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

pub(crate) fn map_domain_error(error: domain_task::DomainTaskError) -> StoreError {
    use domain_task::DomainTaskErrorCode;
    match error.code {
        DomainTaskErrorCode::IllegalTransition => StoreError::new(
            StoreErrorCode::ContractInvalid,
            "illegal action status transition",
        ),
        DomainTaskErrorCode::ExpectedRevisionConflict => StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action revision conflict from domain check",
        ),
        DomainTaskErrorCode::MissingEvidence | DomainTaskErrorCode::InvariantViolation => {
            StoreError::new(
                StoreErrorCode::ContractInvalid,
                "action transition domain invariant failed",
            )
        }
        DomainTaskErrorCode::InvalidInput
        | DomainTaskErrorCode::IllegalCompensationDraft
        | DomainTaskErrorCode::IllegalRecoveryCandidate => StoreError::new(
            StoreErrorCode::ContractInvalid,
            "action transition domain input invalid",
        ),
    }
}

/// Edges whose domain outcome requires lease/lock release (or other unhandled effects) fail
/// closed until the corresponding Action repository APIs land. Silent half-commit is forbidden.
pub(crate) fn reject_unhandled_action_effects(effects: &ActionEffects) -> Result<(), StoreError> {
    if effects.release_lease_and_locks.is_some() {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "action transition requires lease release effects; lease API is not implemented",
        ));
    }
    Ok(())
}

pub(crate) fn revision_conflict(action_id: &str, expected: i64, actual: i64) -> StoreError {
    let _ = (action_id, expected, actual);
    StoreError::new(
        StoreErrorCode::ConstraintViolation,
        "action expected revision does not match stored revision",
    )
}

pub(crate) fn format_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub(crate) fn parse_uuid(value: &str) -> Result<uuid::Uuid, StoreError> {
    uuid::Uuid::parse_str(value).map_err(|_| contract_error())
}

fn contract_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::ContractInvalid,
        "action repository facts violate a generated JSON contract",
    )
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored action repository data failed integrity validation",
    )
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}
