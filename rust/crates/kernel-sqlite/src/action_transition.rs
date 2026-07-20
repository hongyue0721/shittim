//! ActionTransitionIntentV1 repository and action.state_changed producer (IC §6.14).
//!
//! Closed method set: `insert_intent`, `get_intent`, `get_for_action_revision`,
//! `mark_committed_with_event`, `reconcile_intent`. No update/delete of intent body.
//!
//! Status-event authority is sole: only `mark_committed_with_event` may CAS Action and append
//! `action.state_changed` together. Committed verification compares intent ↔ outbox event snapshot
//! fields only; current Action head may advance later and must not map to Corrupt.

use crate::action::{
    cas_transition_for_intent, format_time, get_action, get_action_shallow, map_domain_error,
    parse_uuid, reject_unhandled_action_effects,
};
use crate::outbox::{decode_versioned_row_at, EventAggregateId, PendingActiveEventV2};
use crate::{OutboxRecord, StoreError, StoreErrorCode, StoredEventEnvelope, WriteTransaction};
use chrono::{DateTime, Utc};
use domain_task::{
    apply_action_transition, is_action_transition_allowed, ActionEventIntent, ActionEvidence,
    ActionTransitionCommand, EventIntent,
};
use kernel_contracts::{
    canonical_json_string, validate_json, ActionRequestV2, ActionRequestV2Result,
    ActionStateChangedPayloadV1, ActionStateChangedPayloadV1SchemaVersion, ActionStatus,
    ActionTransitionIntentV1, CausationRefV2, EventEnvelopeV2Payload,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::Value;
use uuid::Uuid;

const INTENT_SCHEMA: &str = "https://schemas.shittim.local/task/action_transition_intent/v1";
const INSERT_INTENT_SAVEPOINT: &str = "kernel_sqlite_insert_action_transition_intent";
const MARK_COMMITTED_SAVEPOINT: &str = "kernel_sqlite_mark_action_transition_committed";

/// Result of inserting an ActionTransitionIntent.
#[derive(Debug, Clone, PartialEq)]
pub enum InsertIntentResult {
    /// New intent row was written.
    Inserted(ActionTransitionIntentV1),
    /// Same dual unique keys already held this exact intent; returned stored fact.
    Replayed(ActionTransitionIntentV1),
}

/// Reconciliation outcome for a transition intent (IC §6.14).
///
/// Never fabricates events or rewrites transition ids.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconcileIntentResult {
    /// Intent exists, committed_event_id is null, no committed event linked.
    Prepared {
        /// Canonical intent.
        intent: ActionTransitionIntentV1,
    },
    /// Intent exists with committed_event_id and matching action.state_changed event.
    Committed {
        /// Canonical intent.
        intent: ActionTransitionIntentV1,
        /// Linked event id.
        event_id: String,
    },
    /// Partial / conflicting / unreadable stored relationship.
    Corrupt {
        /// Diagnostic reason (non-sensitive).
        reason: &'static str,
    },
}

/// Facts required to commit an intent together with Action CAS and Outbox append.
///
/// `evidence` is the domain closed set (PD / verification / dispatch_certainty / …).
/// Intent is only the anchor + uniqueness key; it does **not** substitute for evidence gates.
/// Edges whose domain outcome requires lease/lock release effects currently fail closed until
/// the lease API lands (no silent half-commit).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkCommittedCommand {
    /// Transition intent id.
    pub transition_id: String,
    /// Caller-allocated event UUID for action.state_changed.
    pub event_id: Uuid,
    /// Non-empty consumer dedup key for the event.
    pub dedup_key: String,
    /// Business time projected to Action.updated_at and event.occurred_at / payload.changed_at.
    pub changed_at: DateTime<Utc>,
    /// Domain evidence bag required by `apply_action_transition` for this edge.
    pub evidence: ActionEvidence,
    /// Optional result projection applied during CAS (e.g. completed child refs).
    pub result: Option<ActionRequestV2Result>,
    /// Optional permission decision projection applied during CAS.
    pub permission_decision_ref: Option<String>,
    /// Optional approval chain projection applied during CAS.
    pub approval_chain_id: Option<String>,
    /// Optional approval resolution ref projected only into the event payload (not Action field).
    pub approval_resolution_ref: Option<String>,
}

impl WriteTransaction<'_> {
    /// Inserts an immutable ActionTransitionIntentV1.
    ///
    /// Dual unique keys: `transition_id` and
    /// `(action_id, expected_action_revision, execution_generation, from_status, to_status, reason_code)`.
    /// Same-fact replay returns the stored intent; conflicting key collision fails closed.
    pub fn insert_intent(
        &self,
        intent: ActionTransitionIntentV1,
    ) -> Result<InsertIntentResult, StoreError> {
        self.with_savepoint(INSERT_INTENT_SAVEPOINT, |connection| {
            insert_intent_inside(connection, intent)
        })
    }

    /// CAS Action + append unique `action.state_changed` + write `committed_event_id` in one unit.
    ///
    /// Sole public authority for status-changing Action edges that emit Outbox events.
    /// Before CAS, full domain evidence invariants are enforced via `apply_action_transition`.
    /// Event causation is exactly `ActionTransitionRefV1` for this intent. Payload is projected
    /// from the post-CAS Action + intent + domain `ActionEventIntent`. Sequence/position roll
    /// back with the savepoint on failure. Same-event idempotent replay verifies only the
    /// intent ↔ event linkage (not that Action head still sits on this revision).
    pub fn mark_committed_with_event(
        &self,
        command: MarkCommittedCommand,
    ) -> Result<(ActionRequestV2, OutboxRecord), StoreError> {
        self.with_savepoint(MARK_COMMITTED_SAVEPOINT, |_| {
            mark_committed_inside(self, command)
        })
    }
}

impl crate::SqliteStore {
    /// Reads and revalidates an ActionTransitionIntentV1 by transition_id.
    pub fn get_intent(
        &self,
        transition_id: &str,
    ) -> Result<Option<ActionTransitionIntentV1>, StoreError> {
        let connection = self.lock_connection()?;
        get_intent(&connection, transition_id)
    }

    /// Looks up intent by business unique key including expected Action revision.
    pub fn get_for_action_revision(
        &self,
        action_id: &str,
        expected_action_revision: i64,
        execution_generation: i64,
        from_status: ActionStatus,
        to_status: ActionStatus,
        reason_code: &str,
    ) -> Result<Option<ActionTransitionIntentV1>, StoreError> {
        let connection = self.lock_connection()?;
        get_for_action_revision(
            &connection,
            action_id,
            expected_action_revision,
            execution_generation,
            from_status,
            to_status,
            reason_code,
        )
    }

    /// Reconciles intent commit state without fabricating missing facts.
    pub fn reconcile_intent(
        &self,
        transition_id: &str,
    ) -> Result<ReconcileIntentResult, StoreError> {
        let connection = self.lock_connection()?;
        reconcile_intent(&connection, transition_id)
    }
}

fn insert_intent_inside(
    connection: &Connection,
    intent: ActionTransitionIntentV1,
) -> Result<InsertIntentResult, StoreError> {
    validate_intent_shape(&intent)?;
    if !is_action_transition_allowed(intent.from_status, intent.to_status) {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "illegal action transition edge for intent",
        ));
    }
    // Action must exist.
    let action = get_action(connection, &intent.action_id)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::NotFound,
            "action was not found for transition intent",
        )
    })?;
    if action.execution_generation != intent.execution_generation {
        // Intent generation must match Action current generation at prepare time.
        // (Strict: fail closed rather than accept stale generation.)
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "intent execution_generation does not match action",
        ));
    }

    if let Some(existing) = get_intent_shallow(connection, &intent.transition_id)? {
        return replay_or_conflict(connection, &intent, existing);
    }
    if let Some(existing) = get_for_action_revision_shallow(
        connection,
        &intent.action_id,
        intent.expected_action_revision,
        intent.execution_generation,
        intent.from_status,
        intent.to_status,
        &intent.reason_code,
    )? {
        return replay_or_conflict(connection, &intent, existing);
    }

    let record_json = encode_intent(&intent)?;
    connection
        .execute(
            "INSERT INTO action_transition_intents(record_json, committed_event_id) VALUES (?1, NULL)",
            params![record_json],
        )
        .map_err(write_error)?;

    let stored = get_intent(connection, &intent.transition_id)?.ok_or_else(stored_invalid)?;
    Ok(InsertIntentResult::Inserted(stored))
}

fn replay_or_conflict(
    connection: &Connection,
    requested: &ActionTransitionIntentV1,
    existing: ActionTransitionIntentV1,
) -> Result<InsertIntentResult, StoreError> {
    if intents_equal_business(requested, &existing) {
        // Canonical readback of stored fact (including committed_event_id relation if any).
        let stored = get_intent(connection, &existing.transition_id)?.ok_or_else(stored_invalid)?;
        return Ok(InsertIntentResult::Replayed(stored));
    }
    Err(StoreError::new(
        StoreErrorCode::ConstraintViolation,
        "action transition intent unique key conflict",
    ))
}

fn mark_committed_inside(
    transaction: &WriteTransaction<'_>,
    command: MarkCommittedCommand,
) -> Result<(ActionRequestV2, OutboxRecord), StoreError> {
    let connection = transaction.connection();
    parse_uuid(&command.transition_id)?;
    if command.dedup_key.is_empty() {
        return Err(contract_error());
    }
    if command.changed_at.timestamp_subsec_nanos() != 0 {
        return Err(contract_error());
    }

    let intent = get_intent(connection, &command.transition_id)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::NotFound,
            "transition intent was not found for commit",
        )
    })?;

    let existing_committed = load_committed_event_id(connection, &intent.transition_id)?;
    if let Some(existing_event_id) = existing_committed {
        // Idempotent same-fact commit: event id must match; only intent↔event linkage is required.
        if existing_event_id != command.event_id.to_string() {
            return Err(StoreError::new(
                StoreErrorCode::ConstraintViolation,
                "transition intent already committed with a different event id",
            ));
        }
        let action = get_action(connection, &intent.action_id)?.ok_or_else(stored_invalid)?;
        let event = load_action_state_changed(connection, &existing_event_id)?
            .ok_or_else(stored_invalid)?;
        verify_event_matches_intent(
            &event,
            &intent,
            &existing_event_id,
            command.approval_resolution_ref.as_deref(),
            true,
        )?;
        return Ok((action, event));
    }

    if !is_action_transition_allowed(intent.from_status, intent.to_status) {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "illegal action transition edge for intent commit",
        ));
    }

    // Read current Action head for domain evidence gate + CAS preconditions.
    let current = get_action(connection, &intent.action_id)?.ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::NotFound,
            "action was not found for intent commit",
        )
    })?;
    if current.revision != intent.expected_action_revision {
        return Err(crate::action::revision_conflict(
            &intent.action_id,
            intent.expected_action_revision,
            current.revision,
        ));
    }
    if current.status != intent.from_status {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action status does not match transition intent from_status",
        ));
    }
    if current.execution_generation != intent.execution_generation {
        return Err(StoreError::new(
            StoreErrorCode::ContractInvalid,
            "intent execution_generation does not match action",
        ));
    }

    // Evidence closed set: intent is only anchor/unique key, never a substitute for domain gates.
    let mut evidence = command.evidence.clone();
    if evidence.permission_decision_ref.is_none() {
        if let Some(permission) = command.permission_decision_ref.clone() {
            evidence.permission_decision_ref = Some(permission);
        }
    }
    if evidence.reason_code.is_none() {
        evidence.reason_code = Some(intent.reason_code.clone());
    }
    let domain_cmd = ActionTransitionCommand {
        action_id: current.action_id.clone(),
        parent_action_id: current.parent_action_id.clone(),
        current_status: current.status,
        current_revision: current.revision as u64,
        expected_revision: Some(intent.expected_action_revision as u64),
        target_status: intent.to_status,
        reason: intent.reason_code.clone(),
        evidence,
    };
    let outcome = apply_action_transition(&domain_cmd).map_err(map_domain_error)?;
    if outcome.new_status != intent.to_status
        || !outcome.status_changed
        || outcome.new_revision as i64 != intent.expected_action_revision + 1
    {
        return Err(StoreError::new(
            StoreErrorCode::InternalStoreError,
            "domain outcome does not match transition intent",
        ));
    }
    // Lease/lock release effects are not persisted in this slice; refuse rather than half-commit.
    reject_unhandled_action_effects(&outcome.effects)?;

    // Domain ActionEventIntent projects status-change facts that Outbox payload must honor.
    let domain_event_intent = extract_action_event_intent(&outcome.event_intents)?;

    let action_id = intent.action_id.clone();
    let expected_revision = intent.expected_action_revision;
    let from_status = intent.from_status;
    let to_status = intent.to_status;
    let result = command.result.clone();
    let permission = command
        .permission_decision_ref
        .clone()
        .or_else(|| command.evidence.permission_decision_ref.clone());
    let chain = command.approval_chain_id.clone();

    let action = cas_transition_for_intent(
        connection,
        &action_id,
        expected_revision,
        from_status,
        to_status,
        command.changed_at,
        |next| {
            if let Some(result) = result.clone() {
                next.result = Some(result);
            }
            if let Some(permission) = permission.clone() {
                next.permission_decision_ref = Some(permission);
            }
            if let Some(chain) = chain.clone() {
                next.approval_chain_id = Some(chain);
            }
        },
    )?;

    // Cross-check post-CAS Action against intent + domain event intent (this commit only).
    if action.status != intent.to_status
        || action.revision != intent.expected_action_revision + 1
        || action.execution_generation != intent.execution_generation
        || action.action_id != intent.action_id
        || action.status != domain_event_intent.to_status
        || action.revision as u64 != domain_event_intent.revision
        || intent.from_status != domain_event_intent.from_status
    {
        return Err(stored_invalid());
    }

    let payload = project_action_state_changed_payload(
        &action,
        &intent,
        &domain_event_intent,
        command.approval_resolution_ref.clone(),
        format_time(command.changed_at),
    )?;

    let pending = PendingActiveEventV2 {
        event_id: command.event_id,
        aggregate_id: EventAggregateId::Action(
            Uuid::parse_str(&action.action_id).map_err(|_| contract_error())?,
        ),
        occurred_at: command.changed_at,
        causation_ref: CausationRefV2::ActionTransition {
            action_id: intent.action_id.clone(),
            transition_id: intent.transition_id.clone(),
        },
        correlation_id: intent.correlation_id.clone(),
        dedup_key: command.dedup_key.clone(),
        payload: EventEnvelopeV2Payload::ActionStateChanged(Box::new(payload)),
    };
    let outbox = transaction.append_active_event_v2(pending)?;
    // Fresh commit: event snapshot must match intent + this Action head (same transaction).
    verify_event_matches_intent(
        &outbox,
        &intent,
        &command.event_id.to_string(),
        command.approval_resolution_ref.as_deref(),
        false,
    )?;
    verify_fresh_commit_action_snapshot(&outbox, &action, &intent)?;

    let changed = connection
        .execute(
            "UPDATE action_transition_intents SET committed_event_id = ?1 \
             WHERE transition_id = ?2 AND committed_event_id IS NULL",
            params![command.event_id.to_string(), intent.transition_id],
        )
        .map_err(write_error)?;
    if changed != 1 {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "transition intent commit marker was not updated",
        ));
    }

    // Final canonical readback: intent ↔ event linkage only (head may later advance).
    let stored_intent =
        get_intent(connection, &intent.transition_id)?.ok_or_else(stored_invalid)?;
    let stored_event_id = load_committed_event_id(connection, &stored_intent.transition_id)?
        .ok_or_else(stored_invalid)?;
    if stored_event_id != command.event_id.to_string() {
        return Err(stored_invalid());
    }
    let stored_action = get_action(connection, &action.action_id)?.ok_or_else(stored_invalid)?;
    let stored_event =
        load_action_state_changed(connection, &stored_event_id)?.ok_or_else(stored_invalid)?;
    verify_event_matches_intent(
        &stored_event,
        &stored_intent,
        &stored_event_id,
        command.approval_resolution_ref.as_deref(),
        false,
    )?;
    verify_fresh_commit_action_snapshot(&stored_event, &stored_action, &stored_intent)?;

    Ok((stored_action, stored_event))
}

fn reconcile_intent(
    connection: &Connection,
    transition_id: &str,
) -> Result<ReconcileIntentResult, StoreError> {
    let intent = match get_intent(connection, transition_id) {
        Ok(Some(intent)) => intent,
        Ok(None) => {
            return Err(StoreError::new(
                StoreErrorCode::NotFound,
                "transition intent was not found for reconcile",
            ));
        }
        Err(_) => {
            return Ok(ReconcileIntentResult::Corrupt {
                reason: "intent stored data invalid",
            });
        }
    };

    let committed = match load_committed_event_id(connection, transition_id) {
        Ok(value) => value,
        Err(_) => {
            return Ok(ReconcileIntentResult::Corrupt {
                reason: "committed_event_id column unreadable",
            });
        }
    };

    match committed {
        None => Ok(ReconcileIntentResult::Prepared { intent }),
        Some(event_id) => {
            // Action existence is a structural relation for the intent; head status/revision is
            // diagnostic only and must never map a later legal advance to Corrupt.
            match get_action(connection, &intent.action_id) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(ReconcileIntentResult::Corrupt {
                        reason: "committed intent action missing",
                    });
                }
                Err(_) => {
                    return Ok(ReconcileIntentResult::Corrupt {
                        reason: "committed intent action invalid",
                    });
                }
            }
            let event = match load_action_state_changed(connection, &event_id) {
                Ok(Some(event)) => event,
                Ok(None) => {
                    return Ok(ReconcileIntentResult::Corrupt {
                        reason: "committed_event_id missing outbox row",
                    });
                }
                Err(_) => {
                    return Ok(ReconcileIntentResult::Corrupt {
                        reason: "committed event stored data invalid",
                    });
                }
            };
            // Committed = intent ↔ outbox event snapshot consistent. Do not require Action head
            // still equals this transition's to_status/revision.
            if verify_event_matches_intent(&event, &intent, &event_id, None, true).is_err() {
                return Ok(ReconcileIntentResult::Corrupt {
                    reason: "committed event does not match intent",
                });
            }
            Ok(ReconcileIntentResult::Committed { intent, event_id })
        }
    }
}

fn get_intent(
    connection: &Connection,
    transition_id: &str,
) -> Result<Option<ActionTransitionIntentV1>, StoreError> {
    let Some(intent) = get_intent_shallow(connection, transition_id)? else {
        return Ok(None);
    };
    if intent.transition_id != transition_id {
        return Err(stored_invalid());
    }
    // Action must exist for a consistent intent read.
    if get_action_shallow(connection, &intent.action_id)?.is_none() {
        return Err(stored_invalid());
    }
    Ok(Some(intent))
}

fn get_for_action_revision(
    connection: &Connection,
    action_id: &str,
    expected_action_revision: i64,
    execution_generation: i64,
    from_status: ActionStatus,
    to_status: ActionStatus,
    reason_code: &str,
) -> Result<Option<ActionTransitionIntentV1>, StoreError> {
    let Some(intent) = get_for_action_revision_shallow(
        connection,
        action_id,
        expected_action_revision,
        execution_generation,
        from_status,
        to_status,
        reason_code,
    )?
    else {
        return Ok(None);
    };
    get_intent(connection, &intent.transition_id)
}

fn get_intent_shallow(
    connection: &Connection,
    transition_id: &str,
) -> Result<Option<ActionTransitionIntentV1>, StoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT record_json FROM action_transition_intents WHERE transition_id = ?1",
            [transition_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(|stored| decode_intent_document(&stored))
        .transpose()
}

fn get_for_action_revision_shallow(
    connection: &Connection,
    action_id: &str,
    expected_action_revision: i64,
    execution_generation: i64,
    from_status: ActionStatus,
    to_status: ActionStatus,
    reason_code: &str,
) -> Result<Option<ActionTransitionIntentV1>, StoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT record_json FROM action_transition_intents \
             WHERE action_id = ?1 \
               AND expected_action_revision = ?2 \
               AND execution_generation = ?3 \
               AND from_status = ?4 \
               AND to_status = ?5 \
               AND reason_code = ?6",
            params![
                action_id,
                expected_action_revision,
                execution_generation,
                from_status.as_str(),
                to_status.as_str(),
                reason_code,
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(|stored| decode_intent_document(&stored))
        .transpose()
}

fn load_committed_event_id(
    connection: &Connection,
    transition_id: &str,
) -> Result<Option<String>, StoreError> {
    connection
        .query_row(
            "SELECT committed_event_id FROM action_transition_intents WHERE transition_id = ?1",
            [transition_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(read_error)?
        .ok_or_else(|| StoreError::new(StoreErrorCode::NotFound, "transition intent was not found"))
}

fn load_action_state_changed(
    connection: &Connection,
    event_id: &str,
) -> Result<Option<OutboxRecord>, StoreError> {
    let position: Option<i64> = connection
        .query_row(
            "SELECT outbox_position FROM outbox WHERE event_id = ?1",
            [event_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    let Some(position) = position else {
        return Ok(None);
    };
    decode_versioned_row_at(connection, "outbox", position)
}

/// Verifies outbox event linkage against the immutable intent snapshot.
///
/// Compares event id/type/aggregate/causation/correlation/dedup-bearing payload status fields
/// with the intent. Does **not** require current Action head to still equal this transition.
/// When `lenient_resolution` is true (idempotent replay / reconcile), `approval_resolution_ref`
/// is not re-checked against a caller-supplied expected value (historical snapshot is trusted
/// once schema-valid). When false (fresh commit), optional expected resolution is enforced.
fn verify_event_matches_intent(
    record: &OutboxRecord,
    intent: &ActionTransitionIntentV1,
    expected_event_id: &str,
    expected_approval_resolution_ref: Option<&str>,
    lenient_resolution: bool,
) -> Result<(), StoreError> {
    let StoredEventEnvelope::ActiveV2(envelope) = &record.envelope;
    if envelope.event_id != expected_event_id {
        return Err(stored_invalid());
    }
    if envelope.type_ != "action.state_changed" || envelope.aggregate_type != "action" {
        return Err(stored_invalid());
    }
    if envelope.aggregate_id != intent.action_id {
        return Err(stored_invalid());
    }
    if envelope.correlation_id != intent.correlation_id {
        return Err(stored_invalid());
    }
    match &envelope.causation_ref {
        CausationRefV2::ActionTransition {
            action_id,
            transition_id,
        } if action_id == &intent.action_id && transition_id == &intent.transition_id => {}
        _ => return Err(stored_invalid()),
    }
    let EventEnvelopeV2Payload::ActionStateChanged(payload) = &envelope.payload else {
        return Err(stored_invalid());
    };
    if payload.action_id != intent.action_id
        || payload.from_status != intent.from_status
        || payload.to_status != intent.to_status
        || payload.action_revision != intent.expected_action_revision + 1
        || payload.execution_generation != intent.execution_generation
        || payload.reason_code != intent.reason_code
    {
        return Err(stored_invalid());
    }
    // Strict approval_resolution_ref check on fresh commit (same level as PD/result integrity).
    if !lenient_resolution {
        match expected_approval_resolution_ref {
            Some(expected) => {
                if payload.approval_resolution_ref.as_deref() != Some(expected) {
                    return Err(stored_invalid());
                }
            }
            None => {
                if payload.approval_resolution_ref.is_some() {
                    // Caller did not project a resolution; payload must not invent one.
                    return Err(stored_invalid());
                }
            }
        }
    }
    Ok(())
}

/// Fresh-commit only: event payload snapshot fields that come from the Action head at commit
/// time must equal the post-CAS Action (task_id, PD, result projections).
fn verify_fresh_commit_action_snapshot(
    record: &OutboxRecord,
    action: &ActionRequestV2,
    intent: &ActionTransitionIntentV1,
) -> Result<(), StoreError> {
    let StoredEventEnvelope::ActiveV2(envelope) = &record.envelope;
    let EventEnvelopeV2Payload::ActionStateChanged(payload) = &envelope.payload else {
        return Err(stored_invalid());
    };
    if payload.action_id != action.action_id
        || payload.task_id != action.task_id
        || payload.to_status != action.status
        || payload.action_revision != action.revision
        || payload.execution_generation != action.execution_generation
        || payload.permission_decision_ref != action.permission_decision_ref
        || action.action_id != intent.action_id
        || action.status != intent.to_status
        || action.revision != intent.expected_action_revision + 1
    {
        return Err(stored_invalid());
    }
    let action_child = action
        .result
        .as_ref()
        .and_then(|result| result.materialized_child_task_ref.clone());
    let action_verification = action
        .result
        .as_ref()
        .map(|result| result.verification_result_refs.clone())
        .unwrap_or_default();
    if payload.materialized_child_task_ref != action_child
        || payload.verification_result_refs != action_verification
    {
        return Err(stored_invalid());
    }
    Ok(())
}

fn extract_action_event_intent(intents: &[EventIntent]) -> Result<ActionEventIntent, StoreError> {
    let mut action_intents = intents.iter().filter_map(|intent| match intent {
        EventIntent::Action(action_intent) => Some(action_intent.clone()),
        EventIntent::Task(_) => None,
    });
    let first = action_intents.next().ok_or_else(|| {
        StoreError::new(
            StoreErrorCode::InternalStoreError,
            "domain transition produced no ActionEventIntent for status change",
        )
    })?;
    if action_intents.next().is_some() {
        return Err(StoreError::new(
            StoreErrorCode::InternalStoreError,
            "domain transition produced multiple ActionEventIntent values",
        ));
    }
    Ok(first)
}

fn project_action_state_changed_payload(
    action: &ActionRequestV2,
    intent: &ActionTransitionIntentV1,
    domain_event_intent: &ActionEventIntent,
    approval_resolution_ref: Option<String>,
    changed_at: String,
) -> Result<ActionStateChangedPayloadV1, StoreError> {
    // Payload status fields are projected from domain ActionEventIntent (not invented here).
    if domain_event_intent.from_status != intent.from_status
        || domain_event_intent.to_status != intent.to_status
        || domain_event_intent.revision as i64 != action.revision
        || domain_event_intent.to_status != action.status
    {
        return Err(StoreError::new(
            StoreErrorCode::InternalStoreError,
            "ActionEventIntent does not match intent or post-CAS action",
        ));
    }
    // Domain reason is structured text; event reason_code is the intent's stable code.
    let _ = &domain_event_intent.reason;

    let verification_result_refs = action
        .result
        .as_ref()
        .map(|result| result.verification_result_refs.clone())
        .unwrap_or_default();
    let materialized_child_task_ref = action
        .result
        .as_ref()
        .and_then(|result| result.materialized_child_task_ref.clone());
    let payload = ActionStateChangedPayloadV1 {
        action_id: action.action_id.clone(),
        task_id: action.task_id.clone(),
        from_status: domain_event_intent.from_status,
        to_status: domain_event_intent.to_status,
        action_revision: domain_event_intent.revision as i64,
        execution_generation: action.execution_generation,
        permission_decision_ref: action.permission_decision_ref.clone(),
        approval_resolution_ref,
        materialized_child_task_ref,
        verification_result_refs,
        reason_code: intent.reason_code.clone(),
        changed_at,
        schema_version: ActionStateChangedPayloadV1SchemaVersion,
    };
    // Fail closed on Schema before append.
    let value = serde_json::to_value(&payload).map_err(|_| serialization_error())?;
    validate_json(
        "https://schemas.shittim.local/event/action_state_changed_payload/v1",
        &value,
    )
    .map_err(|_| contract_error())?;
    Ok(payload)
}

fn validate_intent_shape(intent: &ActionTransitionIntentV1) -> Result<(), StoreError> {
    parse_uuid(&intent.transition_id)?;
    parse_uuid(&intent.action_id)?;
    if intent.expected_action_revision < 0
        || intent.execution_generation < 0
        || intent.reason_code.is_empty()
        || intent.correlation_id.is_empty()
        || intent.created_at.is_empty()
        || intent.from_status == intent.to_status
    {
        return Err(contract_error());
    }
    // Re-validate via Schema.
    let value = serde_json::to_value(intent).map_err(|_| serialization_error())?;
    validate_json(INTENT_SCHEMA, &value).map_err(|_| contract_error())?;
    Ok(())
}

fn intents_equal_business(a: &ActionTransitionIntentV1, b: &ActionTransitionIntentV1) -> bool {
    a.transition_id == b.transition_id
        && a.action_id == b.action_id
        && a.expected_action_revision == b.expected_action_revision
        && a.execution_generation == b.execution_generation
        && a.from_status == b.from_status
        && a.to_status == b.to_status
        && a.reason_code == b.reason_code
        && a.correlation_id == b.correlation_id
        && a.created_at == b.created_at
}

fn encode_intent(intent: &ActionTransitionIntentV1) -> Result<String, StoreError> {
    let value = serde_json::to_value(intent).map_err(|_| serialization_error())?;
    validate_json(INTENT_SCHEMA, &value).map_err(|_| contract_error())?;
    canonical_json_string(&value).map_err(|_| serialization_error())
}

fn decode_intent_document(stored: &str) -> Result<ActionTransitionIntentV1, StoreError> {
    decode_contract_document(INTENT_SCHEMA, stored)
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
        "action transition intent violates a generated JSON contract",
    )
}

fn serialization_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::SerializationFailed,
        "action transition intent serialization failed",
    )
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored action transition data failed integrity validation",
    )
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}
