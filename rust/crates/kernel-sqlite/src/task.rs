//! Transactional Task create repository and strict Task/TaskScope/ContentOrigin reads.

use crate::{PendingEvent, StoreError, StoreErrorCode, WriteTransaction};
use chrono::{DateTime, Utc};
use domain_policy::{normalize_uri, normalize_uri_pattern};
use kernel_contracts::{
    canonical_json_string, sha256_canonical, validate_json, Actor, AuditRecord,
    AuditRecordAuditType, AuditRecordExternalContentStatus, AuditRecordLevel, AuditRecordOutcome,
    AuditRecordRollbackCapability, AuditRecordSchemaVersion, AuditRecordTaskCreationContext,
    AuditRecordTaskCreationContextTaskRevision, CausationRef, CausationRefKind, ContentOrigin,
    ContentOriginCarrierRef, ContentOriginCarrierRefKind, ContentOriginKernelReceipt,
    ContentOriginProducerRef, ContentOriginSchemaVersion, EntryPoint, EventEnvelopeType,
    EventPayload, TaskCreateRequest, TaskCreatedPayload, TaskCreatedPayloadSchemaVersion,
    TaskScope, TaskScopeCreatedBy, TaskScopeSchemaVersion, TaskSpec, TaskSpecSchemaVersion,
    TaskStatus,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

const CREATE_TASK_SAVEPOINT: &str = "kernel_sqlite_create_task";
const TASK_CREATE_SCHEMA: &str = "https://schemas.shittim.local/v1/kcp/task_create_request.json";
const ACTOR_SCHEMA: &str = "https://schemas.shittim.local/v1/common/actor.json";
const CONTENT_ORIGIN_SCHEMA: &str = "https://schemas.shittim.local/v1/common/content_origin.json";
const TASK_SCOPE_SCHEMA: &str = "https://schemas.shittim.local/v1/task/task_scope.json";
const TASK_SCHEMA: &str = "https://schemas.shittim.local/v1/task/task_spec.json";

/// Command-envelope facts that affect task.create materialization or idempotency.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCreateEnvelopeFacts {
    /// Complete Actor revision snapshot.
    pub actor: Actor,
    /// Current KCP entry point.
    pub entry_point: EntryPoint,
    /// Caller request UUID used as direct causation.
    pub request_id: String,
    /// Optional Envelope task ID retained only in the idempotency projection.
    pub envelope_task_id: Option<String>,
    /// Optional Envelope context retained only in the idempotency projection.
    pub context: Option<Value>,
    /// Optional expected revision retained only in the idempotency projection.
    pub expected_revision: Option<i64>,
    /// Non-empty idempotency key scoped by actor ID, entry point, and command type.
    pub idempotency_key: String,
}

/// Caller-allocated identities and the single accepted instant for task.create.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCreateAllocation {
    /// New Task UUID.
    pub task_id: String,
    /// New TaskScope UUID.
    pub task_scope_id: String,
    /// New ContentOrigin UUID.
    pub content_origin_id: String,
    /// New kernel receipt UUID.
    pub receipt_id: String,
    /// New AuditRecord UUID.
    pub audit_id: String,
    /// New Event UUID.
    pub event_id: String,
    /// Non-empty event correlation ID.
    pub correlation_id: String,
    /// Non-empty event consumer deduplication key.
    pub dedup_key: String,
    /// Kernel-fixed acceptance instant reused by every created fact.
    pub accepted_at: DateTime<Utc>,
}

/// Complete repository command for one task.create attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCreateCommand {
    /// Envelope-owned facts.
    pub envelope: TaskCreateEnvelopeFacts,
    /// Generated TaskCreateRequest payload.
    pub request: TaskCreateRequest,
    /// Caller-owned identities and acceptance time.
    pub allocation: TaskCreateAllocation,
}

/// Result of a task.create repository operation.
#[derive(Debug, Clone, PartialEq)]
pub enum CreateTaskResult {
    /// New Task facts were committed to the surrounding transaction.
    Created {
        /// Strictly validated current Task.
        task: TaskSpec,
    },
    /// The same canonical business projection already created this Task.
    Replayed {
        /// Strictly validated current Task.
        task: TaskSpec,
    },
}

#[derive(Debug)]
pub(crate) struct PreparedCreate {
    normalized_request: TaskCreateRequest,
    #[cfg(test)]
    normalized_value: Value,
    projection_json: String,
    projection_hash: String,
    receipt_hash: String,
    accepted_at: String,
}

#[cfg(test)]
impl PreparedCreate {
    pub(crate) fn normalized_value_for_test(&self) -> Value {
        self.normalized_value.clone()
    }

    pub(crate) fn receipt_hash_for_test(&self) -> Value {
        Value::String(self.receipt_hash.clone())
    }

    pub(crate) fn projection_hash_for_test(&self) -> Value {
        Value::String(self.projection_hash.clone())
    }
}

impl WriteTransaction<'_> {
    /// Creates ContentOrigin, TaskScope, Task, idempotency, Audit, and Event atomically.
    ///
    /// This method owns an internal SAVEPOINT, so an ignored error cannot leave partial facts in
    /// the caller's surrounding write transaction.
    pub fn create_task(&self, command: TaskCreateCommand) -> Result<CreateTaskResult, StoreError> {
        let prepared = prepare_create(&command)?;
        self.connection()
            .execute_batch(&format!("SAVEPOINT {CREATE_TASK_SAVEPOINT}"))
            .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
        let result = create_inside_savepoint(self.connection(), &command, &prepared);
        finish_savepoint(self.connection(), result)
    }
}

pub(crate) fn get_task(connection: &Connection, id: &str) -> Result<Option<TaskSpec>, StoreError> {
    let Some(task) = get_task_shallow(connection, id)? else {
        return Ok(None);
    };
    let origin = get_content_origin(connection, &task.origin_ref)?.ok_or_else(stored_invalid)?;
    let scope =
        get_task_scope_shallow(connection, &task.task_scope_ref)?.ok_or_else(stored_invalid)?;
    validate_scope_relations(connection, &scope)?;
    if origin.id != task.origin_ref
        || scope.id != task.task_scope_ref
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

pub(crate) fn get_content_origin(
    connection: &Connection,
    id: &str,
) -> Result<Option<ContentOrigin>, StoreError> {
    let Some(origin) = get_origin_shallow(connection, id)? else {
        return Ok(None);
    };
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

pub(crate) fn prepare_create(command: &TaskCreateCommand) -> Result<PreparedCreate, StoreError> {
    validate_command_facts(command)?;
    let original = serde_json::to_value(&command.request).map_err(|_| serialization_error())?;
    validate_json(TASK_CREATE_SCHEMA, &original).map_err(|_| contract_error())?;

    let mut normalized_request = command.request.clone();
    if let Some(source_uri) = &normalized_request.origin.source_uri {
        normalized_request.origin.source_uri = Some(normalize_uri(source_uri).map_err(|_| {
            StoreError::new(
                StoreErrorCode::InvalidScopePattern,
                "invalid origin source URI",
            )
        })?);
    }
    normalized_request.task_scope.resource_patterns =
        normalize_patterns(&normalized_request.task_scope.resource_patterns)?;
    normalized_request.task_scope.exclusions =
        normalize_patterns(&normalized_request.task_scope.exclusions)?;

    let normalized_value =
        serde_json::to_value(&normalized_request).map_err(|_| serialization_error())?;
    validate_json(TASK_CREATE_SCHEMA, &normalized_value).map_err(|_| contract_error())?;
    let receipt_hash = sha256_canonical(&normalized_value).map_err(|_| serialization_error())?;
    let normalized_value_for_projection = normalized_value.clone();
    let projection = json!({
        "actor": command.envelope.actor,
        "entry_point": command.envelope.entry_point,
        "command_type": "task.create",
        "task_id": command.envelope.envelope_task_id,
        "context": command.envelope.context,
        "expected_revision": command.envelope.expected_revision,
        "payload": normalized_value_for_projection,
    });
    let projection_json = canonical_json_string(&projection).map_err(|_| serialization_error())?;
    let projection_hash = sha256_canonical(&projection).map_err(|_| serialization_error())?;
    let accepted_at = command.allocation.accepted_at.to_rfc3339();
    Ok(PreparedCreate {
        normalized_request,
        #[cfg(test)]
        normalized_value,
        projection_json,
        projection_hash,
        receipt_hash,
        accepted_at,
    })
}

fn validate_command_facts(command: &TaskCreateCommand) -> Result<(), StoreError> {
    let actor = serde_json::to_value(&command.envelope.actor).map_err(|_| serialization_error())?;
    validate_json(ACTOR_SCHEMA, &actor).map_err(|_| contract_error())?;
    for value in [
        &command.envelope.request_id,
        &command.allocation.task_id,
        &command.allocation.task_scope_id,
        &command.allocation.content_origin_id,
        &command.allocation.receipt_id,
        &command.allocation.audit_id,
        &command.allocation.event_id,
    ] {
        Uuid::parse_str(value).map_err(|_| contract_error())?;
    }
    if let Some(value) = &command.envelope.envelope_task_id {
        Uuid::parse_str(value).map_err(|_| contract_error())?;
    }
    if command
        .envelope
        .context
        .as_ref()
        .is_some_and(|value| !value.is_object())
        || command
            .envelope
            .expected_revision
            .is_some_and(|revision| revision < 0)
        || command.envelope.idempotency_key.is_empty()
        || command.allocation.correlation_id.is_empty()
        || command.allocation.dedup_key.is_empty()
    {
        return Err(contract_error());
    }
    Ok(())
}

fn normalize_patterns(patterns: &[String]) -> Result<Vec<String>, StoreError> {
    patterns
        .iter()
        .map(|pattern| {
            normalize_uri_pattern(pattern).map_err(|_| {
                StoreError::new(
                    StoreErrorCode::InvalidScopePattern,
                    "task scope contains an invalid URI pattern",
                )
            })
        })
        .collect()
}

fn create_inside_savepoint(
    connection: &Connection,
    command: &TaskCreateCommand,
    prepared: &PreparedCreate,
) -> Result<CreateTaskResult, StoreError> {
    if let Some(result) = replay_if_present(connection, command, prepared)? {
        return Ok(result);
    }
    if prepared.normalized_request.delegation_ref.is_some() {
        return Err(StoreError::new(
            StoreErrorCode::DelegationNotFound,
            "delegation was not found",
        ));
    }
    validate_parent_refs(connection, &prepared.normalized_request)?;

    let origin = build_origin(command, prepared)?;
    let scope = build_scope(command, prepared)?;
    let task = build_task(command, prepared)?;
    let origin_json = encode_contract_document(CONTENT_ORIGIN_SCHEMA, &origin)?;
    let scope_json = encode_contract_document(TASK_SCOPE_SCHEMA, &scope)?;
    let task_json = encode_contract_document(TASK_SCHEMA, &task)?;

    for (ordinal, parent_id) in origin.parent_origin_refs.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO content_origin_parent_refs(origin_id, ordinal, parent_origin_id) \
                 VALUES (?1, ?2, ?3)",
                params![origin.id, ordinal as i64, parent_id],
            )
            .map_err(write_error)?;
    }
    connection
        .execute(
            "INSERT INTO content_origins(record_json) VALUES (?1)",
            [origin_json],
        )
        .map_err(write_error)?;
    for (ordinal, origin_id) in scope.source_refs.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO task_scope_source_refs(scope_id, ordinal, origin_id) \
                 VALUES (?1, ?2, ?3)",
                params![scope.id, ordinal as i64, origin_id],
            )
            .map_err(write_error)?;
    }
    connection
        .execute(
            "INSERT INTO task_scopes(record_json) VALUES (?1)",
            [scope_json],
        )
        .map_err(write_error)?;
    connection
        .execute("INSERT INTO tasks(record_json) VALUES (?1)", [task_json])
        .map_err(write_error)?;
    connection
        .execute(
            "INSERT INTO task_create_idempotency(\
                projection_json, idempotency_key, projection_hash, created_task_id, accepted_at\
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                prepared.projection_json,
                command.envelope.idempotency_key,
                prepared.projection_hash,
                task.id,
                prepared.accepted_at,
            ],
        )
        .map_err(write_error)?;

    let audit = build_audit(command, &task, &origin, prepared)?;
    verify_creation_audit(&audit, command, &task, &origin, prepared)?;
    crate::audit::insert_audit(connection, &audit)?;
    let pending = build_event(command, &task)?;
    let appended = crate::outbox::append_event(connection, pending.clone())?;
    verify_created_event(&appended, &pending, &task, prepared)?;
    validate_created_relations(connection, &task, &scope, &origin)?;
    Ok(CreateTaskResult::Created { task })
}

fn replay_if_present(
    connection: &Connection,
    command: &TaskCreateCommand,
    prepared: &PreparedCreate,
) -> Result<Option<CreateTaskResult>, StoreError> {
    let row: Option<(String, String, String, String, String, String, String)> = connection
        .query_row(
            "SELECT projection_json, projection_hash, created_task_id, \
                    actor_id, entry_point, command_type, idempotency_key \
             FROM task_create_idempotency \
             WHERE actor_id = ?1 AND entry_point = ?2 AND command_type = 'task.create' \
               AND idempotency_key = ?3",
            params![
                command.envelope.actor.id,
                command.envelope.entry_point.as_str(),
                command.envelope.idempotency_key,
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(read_error)?;
    let Some((projection_json, stored_hash, task_id, actor_id, entry, command_type, key)) = row
    else {
        return Ok(None);
    };
    let projection: Value = serde_json::from_str(&projection_json).map_err(|_| stored_invalid())?;
    let canonical = canonical_json_string(&projection).map_err(|_| stored_invalid())?;
    let recomputed = sha256_canonical(&projection).map_err(|_| stored_invalid())?;
    if canonical != projection_json
        || recomputed != stored_hash
        || actor_id != command.envelope.actor.id
        || entry != command.envelope.entry_point.as_str()
        || command_type != "task.create"
        || key != command.envelope.idempotency_key
    {
        return Err(stored_invalid());
    }
    if stored_hash != prepared.projection_hash || projection_json != prepared.projection_json {
        return Err(StoreError::new(
            StoreErrorCode::IdempotencyConflict,
            "idempotency key was used for different task facts",
        ));
    }
    let task = get_task(connection, &task_id)?.ok_or_else(stored_invalid)?;
    Ok(Some(CreateTaskResult::Replayed { task }))
}

fn validate_parent_refs(
    connection: &Connection,
    request: &TaskCreateRequest,
) -> Result<(), StoreError> {
    if let Some(parent_id) = &request.parent_task_id {
        if get_task(connection, parent_id)?.is_none() {
            return Err(StoreError::new(
                StoreErrorCode::ParentTaskNotFound,
                "parent task was not found",
            ));
        }
    }
    for parent_id in &request.origin.parent_origin_refs {
        if get_content_origin(connection, parent_id)?.is_none() {
            return Err(StoreError::new(
                StoreErrorCode::ParentOriginNotFound,
                "parent content origin was not found",
            ));
        }
    }
    Ok(())
}

fn build_origin(
    command: &TaskCreateCommand,
    prepared: &PreparedCreate,
) -> Result<ContentOrigin, StoreError> {
    Ok(ContentOrigin {
        carrier_ref: ContentOriginCarrierRef {
            id: command.envelope.request_id.clone(),
            kind: ContentOriginCarrierRefKind::CommandRequest,
        },
        entry_point: command.envelope.entry_point,
        id: command.allocation.content_origin_id.clone(),
        kernel_receipt: ContentOriginKernelReceipt {
            content_hash: prepared.receipt_hash.clone(),
            receipt_id: command.allocation.receipt_id.clone(),
            recorded_at: prepared.accepted_at.clone(),
        },
        kind: enum_convert(prepared.normalized_request.origin.kind)?,
        parent_origin_refs: prepared
            .normalized_request
            .origin
            .parent_origin_refs
            .clone(),
        producer_ref: ContentOriginProducerRef {
            id: prepared.normalized_request.origin.producer_ref.id.clone(),
            kind: enum_convert(prepared.normalized_request.origin.producer_ref.kind)?,
        },
        received_at: prepared.accepted_at.clone(),
        schema_version: ContentOriginSchemaVersion,
        source_uri: prepared.normalized_request.origin.source_uri.clone(),
        upstream_stable_id: prepared
            .normalized_request
            .origin
            .upstream_stable_id
            .clone(),
    })
}

fn build_scope(
    command: &TaskCreateCommand,
    prepared: &PreparedCreate,
) -> Result<TaskScope, StoreError> {
    Ok(TaskScope {
        allowed_capability_hints: prepared
            .normalized_request
            .task_scope
            .allowed_capability_hints
            .clone(),
        created_at: prepared.accepted_at.clone(),
        created_by: TaskScopeCreatedBy {
            actor: command.envelope.actor.clone(),
            entry_point: command.envelope.entry_point,
        },
        exclusions: prepared.normalized_request.task_scope.exclusions.clone(),
        expires_at: prepared.normalized_request.task_scope.expires_at.clone(),
        id: command.allocation.task_scope_id.clone(),
        resource_patterns: prepared
            .normalized_request
            .task_scope
            .resource_patterns
            .clone(),
        revision: 1,
        schema_version: TaskScopeSchemaVersion,
        source_refs: vec![command.allocation.content_origin_id.clone()],
        task_id: command.allocation.task_id.clone(),
        updated_at: prepared.accepted_at.clone(),
    })
}

fn build_task(
    command: &TaskCreateCommand,
    prepared: &PreparedCreate,
) -> Result<TaskSpec, StoreError> {
    Ok(TaskSpec {
        actor: command.envelope.actor.clone(),
        capability_hints: prepared.normalized_request.capability_hints.clone(),
        constraints: prepared.normalized_request.constraints.clone(),
        created_at: prepared.accepted_at.clone(),
        delegation_ref: prepared.normalized_request.delegation_ref.clone(),
        failed_recovery_meta: None,
        goal: prepared.normalized_request.goal.clone(),
        id: command.allocation.task_id.clone(),
        origin_ref: command.allocation.content_origin_id.clone(),
        parent_task_id: prepared.normalized_request.parent_task_id.clone(),
        plan_version: 0,
        proposer: enum_convert(prepared.normalized_request.proposer)?,
        revision: 1,
        risk_hint: prepared.normalized_request.risk_hint.clone(),
        schema_version: TaskSpecSchemaVersion,
        status: TaskStatus::Candidate,
        success_criteria: prepared.normalized_request.success_criteria.clone(),
        task_scope_ref: command.allocation.task_scope_id.clone(),
        updated_at: prepared.accepted_at.clone(),
    })
}

fn build_audit(
    command: &TaskCreateCommand,
    task: &TaskSpec,
    origin: &ContentOrigin,
    prepared: &PreparedCreate,
) -> Result<AuditRecord, StoreError> {
    Ok(AuditRecord {
        action_id: None,
        actor: Some(command.envelope.actor.clone()),
        approval_record_ref: None,
        artifact_refs: vec![],
        audit_type: AuditRecordAuditType::TaskCreationRecorded,
        causation_ref: Some(CausationRef {
            id: command.envelope.request_id.clone(),
            kind: CausationRefKind::CommandRequest,
        }),
        content_origin_refs: vec![origin.id.clone()],
        correlation_id: Some(command.allocation.correlation_id.clone()),
        delegation_ref: task.delegation_ref.clone(),
        details: json!({}),
        entry_point: command.envelope.entry_point,
        extension_id: None,
        external_content_status: AuditRecordExternalContentStatus::NotSent,
        id: command.allocation.audit_id.clone(),
        level: AuditRecordLevel::UserActivity,
        model_call_refs: vec![],
        occurred_at: prepared.accepted_at.clone(),
        outcome: AuditRecordOutcome::Succeeded,
        payload_manifest_refs: vec![],
        permission_decision_ref: None,
        policy_context: None,
        provider_id: None,
        reason_codes: vec!["task_created".into()],
        recovery_attempt_ref: None,
        resource_refs: vec![],
        rollback_capability: AuditRecordRollbackCapability::Unknown,
        schema_version: AuditRecordSchemaVersion,
        stop_fence_generation: None,
        summary: None,
        task_creation_context: Some(AuditRecordTaskCreationContext {
            goal: task.goal.clone(),
            origin_ref: task.origin_ref.clone(),
            proposer: enum_convert(task.proposer)?,
            task_revision: AuditRecordTaskCreationContextTaskRevision,
        }),
        task_id: Some(task.id.clone()),
        verification_result_refs: vec![],
    })
}

fn build_event(command: &TaskCreateCommand, task: &TaskSpec) -> Result<PendingEvent, StoreError> {
    let payload = TaskCreatedPayload {
        created_at: task.created_at.clone(),
        goal: task.goal.clone(),
        proposer: enum_convert(task.proposer)?,
        schema_version: TaskCreatedPayloadSchemaVersion,
        status: task.status,
        task_id: task.id.clone(),
        task_revision: task.revision,
    };
    Ok(PendingEvent {
        event_id: command.allocation.event_id.clone(),
        event_type: EventEnvelopeType::TaskCreated,
        aggregate_type: "task".into(),
        aggregate_id: task.id.clone(),
        occurred_at: command.allocation.accepted_at,
        causation_ref: CausationRef {
            id: command.envelope.request_id.clone(),
            kind: CausationRefKind::CommandRequest,
        },
        correlation_id: command.allocation.correlation_id.clone(),
        dedup_key: command.allocation.dedup_key.clone(),
        payload: serde_json::to_value(payload).map_err(|_| serialization_error())?,
    })
}

fn verify_creation_audit(
    audit: &AuditRecord,
    command: &TaskCreateCommand,
    task: &TaskSpec,
    origin: &ContentOrigin,
    prepared: &PreparedCreate,
) -> Result<(), StoreError> {
    let context = audit
        .task_creation_context
        .as_ref()
        .ok_or_else(contract_error)?;
    if audit.schema_version != AuditRecordSchemaVersion
        || audit.id != command.allocation.audit_id
        || audit.audit_type != AuditRecordAuditType::TaskCreationRecorded
        || audit.level != AuditRecordLevel::UserActivity
        || audit.actor.as_ref() != Some(&command.envelope.actor)
        || audit.entry_point != command.envelope.entry_point
        || audit.occurred_at != prepared.accepted_at
        || audit.task_id.as_deref() != Some(task.id.as_str())
        || audit.action_id.is_some()
        || audit.delegation_ref != task.delegation_ref
        || audit.permission_decision_ref.is_some()
        || audit.approval_record_ref.is_some()
        || audit.recovery_attempt_ref.is_some()
        || audit.model_call_refs != Vec::<String>::new()
        || audit.payload_manifest_refs != Vec::<String>::new()
        || audit.verification_result_refs != Vec::<String>::new()
        || audit.content_origin_refs != [origin.id.clone()]
        || audit.artifact_refs != Vec::<String>::new()
        || audit.resource_refs != Vec::<String>::new()
        || audit.extension_id.is_some()
        || audit.provider_id.is_some()
        || audit.causation_ref
            != Some(CausationRef {
                id: command.envelope.request_id.clone(),
                kind: CausationRefKind::CommandRequest,
            })
        || audit.correlation_id.as_deref() != Some(command.allocation.correlation_id.as_str())
        || audit.external_content_status != AuditRecordExternalContentStatus::NotSent
        || audit.rollback_capability != AuditRecordRollbackCapability::Unknown
        || audit.stop_fence_generation.is_some()
        || audit.policy_context.is_some()
        || audit.outcome != AuditRecordOutcome::Succeeded
        || audit.reason_codes != ["task_created"]
        || audit.summary.is_some()
        || audit.details != json!({})
        || context.task_revision != AuditRecordTaskCreationContextTaskRevision
        || context.goal != task.goal
        || context.origin_ref != task.origin_ref
        || context.proposer.as_str() != task.proposer.as_str()
    {
        return Err(contract_error());
    }
    Ok(())
}

fn verify_created_event(
    record: &crate::OutboxRecord,
    pending: &PendingEvent,
    task: &TaskSpec,
    prepared: &PreparedCreate,
) -> Result<(), StoreError> {
    let envelope = &record.envelope;
    let EventPayload::TaskCreated(payload) = &envelope.payload else {
        return Err(contract_error());
    };
    if envelope.sequence != 0
        || envelope.event_id != pending.event_id
        || envelope.type_ != pending.event_type.as_str()
        || envelope.aggregate_type != pending.aggregate_type
        || envelope.aggregate_id != pending.aggregate_id
        || envelope.occurred_at != prepared.accepted_at
        || envelope.causation_ref != pending.causation_ref
        || envelope.correlation_id != pending.correlation_id
        || envelope.dedup_key != pending.dedup_key
        || payload.task_id != task.id
        || payload.status != task.status
        || payload.proposer.as_str() != task.proposer.as_str()
        || payload.goal != task.goal
        || payload.task_revision != task.revision
        || payload.created_at != task.created_at
    {
        return Err(contract_error());
    }
    Ok(())
}

fn validate_created_relations(
    connection: &Connection,
    task: &TaskSpec,
    scope: &TaskScope,
    origin: &ContentOrigin,
) -> Result<(), StoreError> {
    let loaded_task = get_task(connection, &task.id)?.ok_or_else(stored_invalid)?;
    let loaded_scope = get_task_scope(connection, &scope.id)?.ok_or_else(stored_invalid)?;
    let loaded_origin = get_content_origin(connection, &origin.id)?.ok_or_else(stored_invalid)?;
    if loaded_task != *task || loaded_scope != *scope || loaded_origin != *origin {
        return Err(stored_invalid());
    }
    let violations: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(read_error)?;
    if violations != 0 {
        return Err(stored_invalid());
    }
    Ok(())
}

fn finish_savepoint<T>(
    connection: &Connection,
    result: Result<T, StoreError>,
) -> Result<T, StoreError> {
    match result {
        Ok(value) => {
            match connection.execute_batch(&format!("RELEASE SAVEPOINT {CREATE_TASK_SAVEPOINT}")) {
                Ok(()) => Ok(value),
                Err(error) => {
                    let original = StoreError::sqlite(error, StoreErrorCode::InternalStoreError);
                    rollback_savepoint(connection, Some(&original))?;
                    Err(original)
                }
            }
        }
        Err(error) => {
            rollback_savepoint(connection, Some(&error))?;
            Err(error)
        }
    }
}

fn rollback_savepoint(
    connection: &Connection,
    original: Option<&StoreError>,
) -> Result<(), StoreError> {
    connection
        .execute_batch(&format!(
            "ROLLBACK TO SAVEPOINT {CREATE_TASK_SAVEPOINT}; \
             RELEASE SAVEPOINT {CREATE_TASK_SAVEPOINT}"
        ))
        .map_err(|_| {
            StoreError::new(
                StoreErrorCode::InternalStoreError,
                format!(
                    "create_task failed with {} and savepoint rollback also failed",
                    original.map_or("internal_store_error", |error| error.code.as_str())
                ),
            )
        })
}

fn encode_contract_document<T: Serialize>(
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

fn get_task_shallow(connection: &Connection, id: &str) -> Result<Option<TaskSpec>, StoreError> {
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
        if get_content_origin(connection, origin_id)?.is_none() {
            return Err(stored_invalid());
        }
    }
    Ok(())
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

fn enum_convert<S: Serialize, T: DeserializeOwned>(value: S) -> Result<T, StoreError> {
    let value = serde_json::to_value(value).map_err(|_| serialization_error())?;
    serde_json::from_value(value).map_err(|_| contract_error())
}

fn contract_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::ContractInvalid,
        "task.create facts violate a generated JSON contract",
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

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}
