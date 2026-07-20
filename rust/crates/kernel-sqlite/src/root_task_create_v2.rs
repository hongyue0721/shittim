//! Active root TaskCreate v2 repository (IC §5.5 / §6.16 / ADR-0009 slice 2).
//!
//! Pure normalization, receipt/idempotency projection, and allocation validation are owned by
//! `kernel-task-creation`. This module owns the single-write-transaction materialization bundle.

use crate::outbox::{EventAggregateId, PendingActiveEventV2};
use crate::task::{
    encode_contract_document, get_content_origin, get_content_origin_v2, get_task, get_task_scope,
    origin_exists, TASK_SCHEMA, TASK_SCOPE_SCHEMA,
};
use crate::{OutboxRecord, StoreError, StoreErrorCode, StoredEventEnvelope, WriteTransaction};
use chrono::{DateTime, SecondsFormat, Utc};
use kernel_contracts::{
    canonical_json_string, sha256_canonical, validate_json, Actor, AuditRecordV2,
    AuditRecordV2AuditType, AuditRecordV2ExternalContentStatus, AuditRecordV2Level,
    AuditRecordV2Outcome, AuditRecordV2RollbackCapability, AuditRecordV2SchemaVersion,
    AuditRecordV2TaskCreationContext, AuditRecordV2TaskCreationContextCreationKind,
    AuditRecordV2TaskCreationContextTaskRevision, CausationRefV2, ContentOriginV2,
    ContentOriginV2CarrierRef, ContentOriginV2CarrierRefKind, ContentOriginV2KernelReceipt,
    ContentOriginV2ProducerRef, ContentOriginV2SchemaVersion, EntryPoint, EventEnvelopeV2Payload,
    NullOnly, RootTaskCreateAllocationV2, TaskCreateRequestV2, TaskCreatedPayload,
    TaskCreatedPayloadSchemaVersion, TaskCreationProvenanceV1,
    TaskCreationProvenanceV1RootCommandV2SchemaVersion, TaskScope, TaskScopeCreatedBy,
    TaskScopeSchemaVersion, TaskSpec, TaskSpecSchemaVersion, TaskStatus,
};
use kernel_task_creation::{
    normalize_root_task_create, validate_root_task_create_allocation,
    RootTaskCreateExternalUuidRefsV1, RootTaskCreateProjection, RootTaskProjectionInput,
    TaskCreationError,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Map, Value};
use uuid::Uuid;

const ROOT_V2_CREATE_SAVEPOINT: &str = "kernel_sqlite_create_root_task_active";
const CONTENT_ORIGIN_V2_SCHEMA: &str = "https://schemas.shittim.local/common/content_origin/v2";
const AUDIT_RECORD_V2_SCHEMA: &str = "https://schemas.shittim.local/audit/audit_record/v2";
const PROVENANCE_SCHEMA: &str = "https://schemas.shittim.local/task/task_creation_provenance/v1";
const ROOT_ALLOCATION_SCHEMA: &str =
    "https://schemas.shittim.local/task/root_task_create_allocation/v2";

/// Envelope facts required by root TaskCreate v2 materialization and idempotency.
#[derive(Debug, Clone, PartialEq)]
pub struct RootTaskCreateV2EnvelopeFacts {
    /// Complete Actor revision snapshot from the command Envelope.
    pub actor: Actor,
    /// Current KCP entry point.
    pub entry_point: EntryPoint,
    /// Caller request UUID used as carrier and direct command_request causation.
    pub request_id: String,
    /// Required-nullable Envelope context retained only in the idempotency projection.
    pub context: Option<Map<String, Value>>,
    /// Non-empty idempotency key scoped by actor ID, entry point, and command type.
    pub idempotency_key: String,
}

/// Complete repository command for one active root task.create v2 attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct RootTaskCreateV2Command {
    /// Envelope-owned facts. `task_id` and `expected_revision` are fixed null by contract.
    pub envelope: RootTaskCreateV2EnvelopeFacts,
    /// Active TaskCreateRequestV2 payload.
    pub request: TaskCreateRequestV2,
    /// Caller-allocated RootTaskCreateAllocationV2 (seven UUIDs + two opaque IDs).
    pub allocation: RootTaskCreateAllocationV2,
    /// First and only clock read for this attempt; repository never reads the clock.
    pub accepted_at: DateTime<Utc>,
}

/// Result of an active root task.create v2 repository operation.
#[derive(Debug, Clone, PartialEq)]
pub enum CreateRootTaskV2Result {
    /// New root Task facts were committed to the surrounding transaction.
    Created {
        /// Strictly validated current TaskSpec.
        task: TaskSpec,
        /// Creation provenance UUID written in the same transaction.
        creation_provenance_ref: String,
    },
    /// The same canonical business projection already created this Task.
    Replayed {
        /// Strictly validated current TaskSpec.
        task: TaskSpec,
        /// Creation provenance UUID from the original transaction.
        creation_provenance_ref: String,
    },
}

#[derive(Debug)]
struct PreparedRootV2Create {
    projection: RootTaskCreateProjection,
    accepted_at: String,
    external: RootTaskCreateExternalUuidRefsV1,
}

impl WriteTransaction<'_> {
    /// Active root TaskCreate v2 write path.
    ///
    /// Owns an internal SAVEPOINT so an ignored error cannot leave partial facts in the caller's
    /// surrounding write transaction. Does not read the clock; `accepted_at` is caller-injected.
    pub fn create_root_task_v2(
        &self,
        command: RootTaskCreateV2Command,
    ) -> Result<CreateRootTaskV2Result, StoreError> {
        let prepared = prepare_root_v2_create(&command)?;
        self.with_savepoint(ROOT_V2_CREATE_SAVEPOINT, |_| {
            create_root_v2_inside_savepoint(self, &command, &prepared)
        })
    }
}

fn prepare_root_v2_create(
    command: &RootTaskCreateV2Command,
) -> Result<PreparedRootV2Create, StoreError> {
    if command.envelope.idempotency_key.is_empty() {
        return Err(contract_error());
    }
    // Reject sub-second accepted_at so the unique producer clock projects at second precision.
    if command.accepted_at.timestamp_subsec_nanos() != 0 {
        return Err(contract_error());
    }
    let projection = normalize_root_task_create(
        command.request.clone(),
        RootTaskProjectionInput {
            actor: command.envelope.actor.clone(),
            entry_point: command.envelope.entry_point,
            context: command.envelope.context.clone(),
        },
    )
    .map_err(map_task_creation_error)?;
    let external = RootTaskCreateExternalUuidRefsV1 {
        command_request_id: Uuid::parse_str(&command.envelope.request_id)
            .map_err(|_| contract_error())?,
        delegation_ref: parse_optional_uuid(
            command.request.delegation_ref.as_deref().or(projection
                .receipt
                .value
                .delegation_ref
                .as_deref()),
        )?,
        parent_origin_refs: projection
            .receipt
            .value
            .origin
            .parent_origin_refs
            .iter()
            .map(|value| Uuid::parse_str(value).map_err(|_| contract_error()))
            .collect::<Result<Vec<_>, _>>()?,
    };
    validate_root_task_create_allocation(&command.allocation, &external)
        .map_err(map_task_creation_error)?;
    // Re-validate allocation Schema at the repository boundary (fail closed).
    let allocation_value =
        serde_json::to_value(&command.allocation).map_err(|_| serialization_error())?;
    validate_json(ROOT_ALLOCATION_SCHEMA, &allocation_value).map_err(|_| contract_error())?;
    Ok(PreparedRootV2Create {
        projection,
        accepted_at: format_accepted_at(command.accepted_at),
        external,
    })
}

fn create_root_v2_inside_savepoint(
    transaction: &WriteTransaction<'_>,
    command: &RootTaskCreateV2Command,
    prepared: &PreparedRootV2Create,
) -> Result<CreateRootTaskV2Result, StoreError> {
    let connection = transaction.connection();
    if let Some(result) = replay_root_v2_if_present(connection, command, prepared)? {
        return Ok(result);
    }
    if prepared.external.delegation_ref.is_some() {
        // No Delegation authority repository exists yet; non-null refs cannot be proven current.
        return Err(StoreError::new(
            StoreErrorCode::DelegationNotFound,
            "delegation was not found",
        ));
    }
    validate_parent_origins_v2(connection, &prepared.projection)?;

    let origin = build_content_origin_v2(command, prepared)?;
    let scope = build_task_scope(command, prepared)?;
    let task = build_task(command, prepared)?;
    let provenance = build_provenance(command, prepared)?;
    let audit = build_audit_v2(command, prepared, &task, &origin, &provenance)?;

    let origin_json = encode_contract_document(CONTENT_ORIGIN_V2_SCHEMA, &origin)?;
    let scope_json = encode_contract_document(TASK_SCOPE_SCHEMA, &scope)?;
    let task_json = encode_contract_document(TASK_SCHEMA, &task)?;
    let provenance_json = encode_contract_document(PROVENANCE_SCHEMA, &provenance)?;
    let audit_json = encode_contract_document(AUDIT_RECORD_V2_SCHEMA, &audit)?;
    let projection_json = String::from_utf8(prepared.projection.idempotency.jcs_utf8.clone())
        .map_err(|_| serialization_error())?;

    for (ordinal, parent_id) in origin.parent_origin_refs.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO content_origin_v2_parent_refs(origin_id, ordinal, parent_origin_id) \
                 VALUES (?1, ?2, ?3)",
                params![origin.id, ordinal as i64, parent_id],
            )
            .map_err(write_error)?;
    }
    connection
        .execute(
            "INSERT INTO content_origins_v2(record_json) VALUES (?1)",
            [&origin_json],
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
            [&scope_json],
        )
        .map_err(write_error)?;
    connection
        .execute("INSERT INTO tasks(record_json) VALUES (?1)", [&task_json])
        .map_err(write_error)?;
    connection
        .execute(
            "INSERT INTO task_creation_provenances(record_json, task_id) VALUES (?1, ?2)",
            params![provenance_json, task.id],
        )
        .map_err(write_error)?;
    connection
        .execute(
            "INSERT INTO audit_records_v2(record_json) VALUES (?1)",
            [&audit_json],
        )
        .map_err(write_error)?;
    connection
        .execute(
            "INSERT INTO root_task_create_idempotency_v2(\
                projection_json, idempotency_key, request_hash, created_task_id, \
                creation_provenance_id, accepted_at\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                projection_json,
                command.envelope.idempotency_key,
                prepared.projection.idempotency.sha256,
                task.id,
                command.allocation.creation_provenance_id,
                prepared.accepted_at,
            ],
        )
        .map_err(write_error)?;

    let pending = build_task_created_event(command, prepared, &task)?;
    let appended = transaction.append_active_event_v2(pending.clone())?;
    // Test-only: prove post-append bundle failure rolls back sequence/position with the savepoint.
    #[cfg(test)]
    if transaction.root_v2_post_append_bundle_invalid_for_test() {
        return Err(stored_invalid());
    }
    // Full closed-bundle canonical readback is the sole post-write authority for Created and Replayed.
    validate_created_bundle(
        connection,
        CreatedBundleExpectation {
            task_id: &task.id,
            scope_id: &scope.id,
            origin_id: &origin.id,
            provenance_id: &command.allocation.creation_provenance_id,
            audit_id: &audit.id,
            task_created_event_id: &command.allocation.task_created_event_id,
            correlation_id: &command.allocation.correlation_id,
            dedup_key: &command.allocation.task_created_dedup_key,
            request_id: &command.envelope.request_id,
            accepted_at: &prepared.accepted_at,
            expected_task: Some(&task),
            expected_scope: Some(&scope),
            expected_origin: Some(&origin),
            expected_provenance: Some(&provenance),
            expected_audit: Some(&audit),
            expected_pending_event: Some(&pending),
            expected_appended_event: Some(&appended),
        },
    )?;
    Ok(CreateRootTaskV2Result::Created {
        task,
        creation_provenance_ref: command.allocation.creation_provenance_id.clone(),
    })
}

fn replay_root_v2_if_present(
    connection: &Connection,
    command: &RootTaskCreateV2Command,
    prepared: &PreparedRootV2Create,
) -> Result<Option<CreateRootTaskV2Result>, StoreError> {
    let row: Option<(String, String, String, String, String, String, String)> = connection
        .query_row(
            "SELECT projection_json, request_hash, created_task_id, creation_provenance_id, \
                    actor_id, entry_point, command_type \
             FROM root_task_create_idempotency_v2 \
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
    let Some((projection_json, stored_hash, task_id, provenance_id, actor_id, entry, command_type)) =
        row
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
    {
        return Err(stored_invalid());
    }
    if stored_hash != prepared.projection.idempotency.sha256 {
        return Err(StoreError::new(
            StoreErrorCode::IdempotencyConflict,
            "idempotency key was used for different task facts",
        ));
    }
    let expected_projection = String::from_utf8(prepared.projection.idempotency.jcs_utf8.clone())
        .map_err(|_| stored_invalid())?;
    if projection_json != expected_projection {
        return Err(StoreError::new(
            StoreErrorCode::IdempotencyConflict,
            "idempotency key was used for different task facts",
        ));
    }

    // Replay must re-prove the same full closed bundle as Created; any gap is stored corruption.
    // Allocation UUIDs for audit/event/origin/scope are recovered from stored facts, not from the
    // replay caller's (possibly different) allocation payload.
    let task = get_task(connection, &task_id)?.ok_or_else(stored_invalid)?;
    let provenance = get_provenance(connection, &provenance_id)?.ok_or_else(stored_invalid)?;
    let TaskCreationProvenanceV1::RootCommandV2 {
        id: stored_provenance_id,
        command_request_id,
        ..
    } = &provenance
    else {
        return Err(stored_invalid());
    };
    if stored_provenance_id != &provenance_id || command_request_id != &command.envelope.request_id
    {
        return Err(stored_invalid());
    }
    let audit = find_root_creation_audit(connection, &task_id, &provenance_id)?
        .ok_or_else(stored_invalid)?;
    let event = find_task_created_event(connection, &task_id)?.ok_or_else(stored_invalid)?;
    let StoredEventEnvelope::ActiveV2(envelope) = &event.envelope;
    validate_created_bundle(
        connection,
        CreatedBundleExpectation {
            task_id: &task.id,
            scope_id: &task.task_scope_ref,
            origin_id: &task.origin_ref,
            provenance_id: &provenance_id,
            audit_id: &audit.id,
            task_created_event_id: &envelope.event_id,
            correlation_id: &envelope.correlation_id,
            dedup_key: &envelope.dedup_key,
            request_id: &command.envelope.request_id,
            accepted_at: &task.created_at,
            expected_task: None,
            expected_scope: None,
            expected_origin: None,
            expected_provenance: None,
            expected_audit: None,
            expected_pending_event: None,
            expected_appended_event: None,
        },
    )?;
    // Cross-check idempotency row against the closed bundle it claims to own.
    if task.id != task_id {
        return Err(stored_invalid());
    }
    Ok(Some(CreateRootTaskV2Result::Replayed {
        task,
        creation_provenance_ref: provenance_id,
    }))
}

fn validate_parent_origins_v2(
    connection: &Connection,
    projection: &RootTaskCreateProjection,
) -> Result<(), StoreError> {
    for parent_id in &projection.receipt.value.origin.parent_origin_refs {
        if !origin_exists(connection, parent_id)? {
            return Err(StoreError::new(
                StoreErrorCode::ParentOriginNotFound,
                "parent content origin was not found",
            ));
        }
    }
    Ok(())
}

fn build_content_origin_v2(
    command: &RootTaskCreateV2Command,
    prepared: &PreparedRootV2Create,
) -> Result<ContentOriginV2, StoreError> {
    let origin = &prepared.projection.receipt.value.origin;
    Ok(ContentOriginV2 {
        carrier_ref: ContentOriginV2CarrierRef {
            id: command.envelope.request_id.clone(),
            kind: ContentOriginV2CarrierRefKind::CommandRequest,
        },
        entry_point: command.envelope.entry_point,
        id: command.allocation.content_origin_id.clone(),
        kernel_receipt: ContentOriginV2KernelReceipt {
            content_hash: prepared.projection.receipt.sha256.clone(),
            receipt_id: command.allocation.kernel_receipt_id.clone(),
            recorded_at: prepared.accepted_at.clone(),
        },
        kind: enum_convert(origin.kind)?,
        parent_origin_refs: origin.parent_origin_refs.clone(),
        producer_ref: ContentOriginV2ProducerRef {
            id: origin.producer_ref.id.clone(),
            kind: enum_convert(origin.producer_ref.kind)?,
        },
        received_at: prepared.accepted_at.clone(),
        schema_version: ContentOriginV2SchemaVersion,
        source_uri: origin.source_uri.clone(),
        upstream_stable_id: origin.upstream_stable_id.clone(),
    })
}

fn build_task_scope(
    command: &RootTaskCreateV2Command,
    prepared: &PreparedRootV2Create,
) -> Result<TaskScope, StoreError> {
    let scope = &prepared.projection.receipt.value.task_scope;
    Ok(TaskScope {
        allowed_capability_hints: scope.allowed_capability_hints.clone(),
        created_at: prepared.accepted_at.clone(),
        created_by: TaskScopeCreatedBy {
            actor: command.envelope.actor.clone(),
            entry_point: command.envelope.entry_point,
        },
        exclusions: scope.exclusions.clone(),
        expires_at: scope.expires_at.clone(),
        id: command.allocation.task_scope_id.clone(),
        resource_patterns: scope.resource_patterns.clone(),
        revision: 1,
        schema_version: TaskScopeSchemaVersion,
        source_refs: vec![command.allocation.content_origin_id.clone()],
        task_id: command.allocation.task_id.clone(),
        updated_at: prepared.accepted_at.clone(),
    })
}

fn build_task(
    command: &RootTaskCreateV2Command,
    prepared: &PreparedRootV2Create,
) -> Result<TaskSpec, StoreError> {
    let payload = &prepared.projection.receipt.value;
    Ok(TaskSpec {
        actor: command.envelope.actor.clone(),
        capability_hints: payload.capability_hints.clone(),
        constraints: payload.constraints.clone(),
        created_at: prepared.accepted_at.clone(),
        delegation_ref: payload.delegation_ref.clone(),
        failed_recovery_meta: None,
        goal: payload.goal.clone(),
        id: command.allocation.task_id.clone(),
        origin_ref: command.allocation.content_origin_id.clone(),
        parent_task_id: None,
        plan_version: 0,
        proposer: enum_convert(payload.proposer)?,
        revision: 1,
        risk_hint: payload.risk_hint.clone(),
        schema_version: TaskSpecSchemaVersion,
        status: TaskStatus::Candidate,
        success_criteria: payload.success_criteria.clone(),
        task_scope_ref: command.allocation.task_scope_id.clone(),
        updated_at: prepared.accepted_at.clone(),
    })
}

fn build_provenance(
    command: &RootTaskCreateV2Command,
    prepared: &PreparedRootV2Create,
) -> Result<TaskCreationProvenanceV1, StoreError> {
    Ok(TaskCreationProvenanceV1::RootCommandV2 {
        accepted_at: prepared.accepted_at.clone(),
        action_id: NullOnly,
        actor: command.envelope.actor.clone(),
        command_request_id: command.envelope.request_id.clone(),
        entry_point: command.envelope.entry_point,
        id: command.allocation.creation_provenance_id.clone(),
        materialized_at: NullOnly,
        parent_task_id: NullOnly,
        receipt_ref: command.allocation.kernel_receipt_id.clone(),
        schema_version: TaskCreationProvenanceV1RootCommandV2SchemaVersion,
    })
}

fn build_audit_v2(
    command: &RootTaskCreateV2Command,
    prepared: &PreparedRootV2Create,
    task: &TaskSpec,
    origin: &ContentOriginV2,
    provenance: &TaskCreationProvenanceV1,
) -> Result<AuditRecordV2, StoreError> {
    let TaskCreationProvenanceV1::RootCommandV2 {
        id: provenance_id, ..
    } = provenance
    else {
        return Err(contract_error());
    };
    Ok(AuditRecordV2 {
        action_id: None,
        actor: Some(command.envelope.actor.clone()),
        approval_resolution_ref: None,
        artifact_refs: vec![],
        audit_type: AuditRecordV2AuditType::TaskCreationRecorded,
        causation_ref: Some(CausationRefV2::CommandRequest {
            id: command.envelope.request_id.clone(),
        }),
        content_origin_refs: vec![origin.id.clone()],
        correlation_id: Some(command.allocation.correlation_id.clone()),
        delegation_ref: None,
        details: json!({}),
        entry_point: command.envelope.entry_point,
        extension_id: None,
        external_content_status: AuditRecordV2ExternalContentStatus::NotSent,
        id: command.allocation.audit_record_id.clone(),
        level: AuditRecordV2Level::UserActivity,
        model_call_refs: vec![],
        occurred_at: prepared.accepted_at.clone(),
        outcome: AuditRecordV2Outcome::Succeeded,
        payload_manifest_refs: vec![],
        permission_decision_ref: None,
        policy_context: None,
        provider_id: None,
        reason_codes: vec!["task_created_root_v2".into()],
        recovery_attempt_ref: None,
        resource_refs: vec![],
        rollback_capability: AuditRecordV2RollbackCapability::Unknown,
        schema_version: AuditRecordV2SchemaVersion,
        stop_fence_generation: None,
        summary: None,
        task_creation_context: Some(AuditRecordV2TaskCreationContext {
            accepted_at: prepared.accepted_at.clone(),
            creation_kind: AuditRecordV2TaskCreationContextCreationKind::RootCommandV2,
            creation_provenance_ref: provenance_id.clone(),
            goal: task.goal.clone(),
            materialized_at: None,
            origin_ref: task.origin_ref.clone(),
            proposer: enum_convert(task.proposer)?,
            task_revision: AuditRecordV2TaskCreationContextTaskRevision,
        }),
        task_id: Some(task.id.clone()),
        verification_result_refs: vec![],
    })
}

fn build_task_created_event(
    command: &RootTaskCreateV2Command,
    _prepared: &PreparedRootV2Create,
    task: &TaskSpec,
) -> Result<PendingActiveEventV2, StoreError> {
    let event_id =
        Uuid::parse_str(&command.allocation.task_created_event_id).map_err(|_| contract_error())?;
    let task_uuid = Uuid::parse_str(&task.id).map_err(|_| contract_error())?;
    let payload = TaskCreatedPayload {
        created_at: task.created_at.clone(),
        goal: task.goal.clone(),
        proposer: enum_convert(task.proposer)?,
        schema_version: TaskCreatedPayloadSchemaVersion,
        status: task.status,
        task_id: task.id.clone(),
        task_revision: task.revision,
    };
    Ok(PendingActiveEventV2 {
        event_id,
        aggregate_id: EventAggregateId::Task(task_uuid),
        occurred_at: command.accepted_at,
        causation_ref: CausationRefV2::CommandRequest {
            id: command.envelope.request_id.clone(),
        },
        correlation_id: command.allocation.correlation_id.clone(),
        dedup_key: command.allocation.task_created_dedup_key.clone(),
        payload: EventEnvelopeV2Payload::TaskCreated(Box::new(payload)),
    })
}

/// Closed-bundle facts required by Created and Replayed readback (IC §5.5 / §6.16).
///
/// Created path supplies the in-memory values for byte-equal comparison; Replayed path leaves
/// the expected_* fields empty and relies solely on stored-object closure proofs.
struct CreatedBundleExpectation<'a> {
    task_id: &'a str,
    scope_id: &'a str,
    origin_id: &'a str,
    provenance_id: &'a str,
    audit_id: &'a str,
    task_created_event_id: &'a str,
    correlation_id: &'a str,
    dedup_key: &'a str,
    request_id: &'a str,
    accepted_at: &'a str,
    expected_task: Option<&'a TaskSpec>,
    expected_scope: Option<&'a TaskScope>,
    expected_origin: Option<&'a ContentOriginV2>,
    expected_provenance: Option<&'a TaskCreationProvenanceV1>,
    expected_audit: Option<&'a AuditRecordV2>,
    expected_pending_event: Option<&'a PendingActiveEventV2>,
    expected_appended_event: Option<&'a OutboxRecord>,
}

fn validate_created_bundle(
    connection: &Connection,
    expected: CreatedBundleExpectation<'_>,
) -> Result<(), StoreError> {
    // Task (origin/scope closure via get_task).
    let loaded_task = get_task(connection, expected.task_id)?.ok_or_else(stored_invalid)?;
    if let Some(task) = expected.expected_task {
        if loaded_task != *task {
            return Err(stored_invalid());
        }
    }
    if loaded_task.id != expected.task_id
        || loaded_task.task_scope_ref != expected.scope_id
        || loaded_task.origin_ref != expected.origin_id
        || loaded_task.parent_task_id.is_some()
        || loaded_task.revision != 1
        || loaded_task.status != TaskStatus::Candidate
        || loaded_task.created_at != expected.accepted_at
        || loaded_task.updated_at != expected.accepted_at
    {
        return Err(stored_invalid());
    }

    // TaskScope + ordered source_refs (via get_task_scope).
    let loaded_scope = get_task_scope(connection, expected.scope_id)?.ok_or_else(stored_invalid)?;
    if let Some(scope) = expected.expected_scope {
        if loaded_scope != *scope {
            return Err(stored_invalid());
        }
    }
    if loaded_scope.id != expected.scope_id
        || loaded_scope.task_id != expected.task_id
        || loaded_scope.source_refs != [expected.origin_id.to_owned()]
        || loaded_scope.revision != 1
        || loaded_scope.created_at != expected.accepted_at
        || loaded_scope.updated_at != expected.accepted_at
    {
        return Err(stored_invalid());
    }

    // ContentOriginV2 + ordered parent_refs; must not also exist as legacy v1.
    let loaded_origin =
        get_content_origin_v2(connection, expected.origin_id)?.ok_or_else(stored_invalid)?;
    if let Some(origin) = expected.expected_origin {
        if loaded_origin != *origin {
            return Err(stored_invalid());
        }
    }
    if loaded_origin.id != expected.origin_id
        || loaded_origin.received_at != expected.accepted_at
        || loaded_origin.kernel_receipt.recorded_at != expected.accepted_at
        || loaded_origin.carrier_ref.kind != ContentOriginV2CarrierRefKind::CommandRequest
        || loaded_origin.carrier_ref.id != expected.request_id
    {
        return Err(stored_invalid());
    }
    if get_content_origin(connection, expected.origin_id)?.is_some() {
        return Err(stored_invalid());
    }

    // Provenance: wire + free column task_id must both point at the same Task.
    let loaded_provenance =
        get_provenance(connection, expected.provenance_id)?.ok_or_else(stored_invalid)?;
    if let Some(provenance) = expected.expected_provenance {
        if loaded_provenance != *provenance {
            return Err(stored_invalid());
        }
    }
    let TaskCreationProvenanceV1::RootCommandV2 {
        id: provenance_id,
        command_request_id,
        accepted_at: provenance_accepted_at,
        receipt_ref: _,
        actor: _,
        entry_point: _,
        action_id: _,
        materialized_at: _,
        parent_task_id: _,
        schema_version: _,
    } = &loaded_provenance
    else {
        return Err(stored_invalid());
    };
    if provenance_id != expected.provenance_id
        || command_request_id != expected.request_id
        || provenance_accepted_at != expected.accepted_at
    {
        return Err(stored_invalid());
    }
    let column_task_id = provenance_column_task_id(connection, expected.provenance_id)?
        .ok_or_else(stored_invalid)?;
    if column_task_id != expected.task_id {
        return Err(stored_invalid());
    }

    // AuditRecordV2(task.creation_recorded) with projection consistent with Task/Origin/Provenance.
    let loaded_audit = get_audit_v2(connection, expected.audit_id)?.ok_or_else(stored_invalid)?;
    if let Some(audit) = expected.expected_audit {
        if loaded_audit != *audit {
            return Err(stored_invalid());
        }
    }
    validate_root_creation_audit(
        &loaded_audit,
        &loaded_task,
        &loaded_origin,
        RootCreationAuditRefs {
            provenance_id: expected.provenance_id,
            audit_id: expected.audit_id,
            correlation_id: expected.correlation_id,
            request_id: expected.request_id,
            accepted_at: expected.accepted_at,
        },
    )?;

    // task.created Outbox Event: id / correlation / dedup / causation consistent with the bundle.
    let loaded_event =
        find_task_created_event(connection, expected.task_id)?.ok_or_else(stored_invalid)?;
    if let Some(appended) = expected.expected_appended_event {
        if loaded_event != *appended {
            return Err(stored_invalid());
        }
    }
    validate_root_task_created_event(
        &loaded_event,
        &loaded_task,
        RootTaskCreatedEventRefs {
            event_id: expected.task_created_event_id,
            correlation_id: expected.correlation_id,
            dedup_key: expected.dedup_key,
            request_id: expected.request_id,
            accepted_at: expected.accepted_at,
            expected_pending: expected.expected_pending_event,
        },
    )?;

    // Idempotency → created_task_id / creation_provenance_id cross-check.
    let (mapped_task_id, mapped_provenance_id) =
        idempotency_created_ids(connection, expected.task_id)?.ok_or_else(stored_invalid)?;
    if mapped_task_id != expected.task_id || mapped_provenance_id != expected.provenance_id {
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

struct RootCreationAuditRefs<'a> {
    provenance_id: &'a str,
    audit_id: &'a str,
    correlation_id: &'a str,
    request_id: &'a str,
    accepted_at: &'a str,
}

fn validate_root_creation_audit(
    audit: &AuditRecordV2,
    task: &TaskSpec,
    origin: &ContentOriginV2,
    refs: RootCreationAuditRefs<'_>,
) -> Result<(), StoreError> {
    let context = audit
        .task_creation_context
        .as_ref()
        .ok_or_else(stored_invalid)?;
    if audit.id != refs.audit_id
        || audit.audit_type != AuditRecordV2AuditType::TaskCreationRecorded
        || audit.level != AuditRecordV2Level::UserActivity
        || audit.occurred_at != refs.accepted_at
        || audit.task_id.as_deref() != Some(task.id.as_str())
        || audit.action_id.is_some()
        || audit.permission_decision_ref.is_some()
        || audit.approval_resolution_ref.is_some()
        || audit.recovery_attempt_ref.is_some()
        || audit.delegation_ref.is_some()
        || audit.model_call_refs != Vec::<String>::new()
        || audit.payload_manifest_refs != Vec::<String>::new()
        || audit.verification_result_refs != Vec::<String>::new()
        || audit.content_origin_refs != [origin.id.clone()]
        || audit.artifact_refs != Vec::<String>::new()
        || audit.resource_refs != Vec::<String>::new()
        || audit.extension_id.is_some()
        || audit.provider_id.is_some()
        || audit.causation_ref
            != Some(CausationRefV2::CommandRequest {
                id: refs.request_id.to_owned(),
            })
        || audit.correlation_id.as_deref() != Some(refs.correlation_id)
        || audit.external_content_status != AuditRecordV2ExternalContentStatus::NotSent
        || audit.rollback_capability != AuditRecordV2RollbackCapability::Unknown
        || audit.stop_fence_generation.is_some()
        || audit.policy_context.is_some()
        || audit.outcome != AuditRecordV2Outcome::Succeeded
        || audit.reason_codes != ["task_created_root_v2"]
        || audit.summary.is_some()
        || audit.details != json!({})
        || context.task_revision != AuditRecordV2TaskCreationContextTaskRevision
        || context.goal != task.goal
        || context.origin_ref != task.origin_ref
        || context.creation_provenance_ref != refs.provenance_id
        || context.creation_kind != AuditRecordV2TaskCreationContextCreationKind::RootCommandV2
        || context.accepted_at != refs.accepted_at
        || context.materialized_at.is_some()
        || context.proposer.as_str() != task.proposer.as_str()
    {
        return Err(stored_invalid());
    }
    Ok(())
}

struct RootTaskCreatedEventRefs<'a> {
    event_id: &'a str,
    correlation_id: &'a str,
    dedup_key: &'a str,
    request_id: &'a str,
    accepted_at: &'a str,
    expected_pending: Option<&'a PendingActiveEventV2>,
}

fn validate_root_task_created_event(
    record: &OutboxRecord,
    task: &TaskSpec,
    refs: RootTaskCreatedEventRefs<'_>,
) -> Result<(), StoreError> {
    let StoredEventEnvelope::ActiveV2(envelope) = &record.envelope;
    let EventEnvelopeV2Payload::TaskCreated(payload) = &envelope.payload else {
        return Err(stored_invalid());
    };
    if envelope.sequence != 0
        || envelope.event_id != refs.event_id
        || envelope.type_ != "task.created"
        || envelope.aggregate_type != "task"
        || envelope.aggregate_id != task.id
        || envelope.causation_ref
            != (CausationRefV2::CommandRequest {
                id: refs.request_id.to_owned(),
            })
        || envelope.correlation_id != refs.correlation_id
        || envelope.dedup_key != refs.dedup_key
        || payload.task_id != task.id
        || payload.status != task.status
        || payload.proposer.as_str() != task.proposer.as_str()
        || payload.goal != task.goal
        || payload.task_revision != task.revision
        || payload.created_at != task.created_at
        || payload.created_at != refs.accepted_at
    {
        return Err(stored_invalid());
    }
    if let Some(pending) = refs.expected_pending {
        // Outbox stores DateTime via to_rfc3339(); business facts use UTC-second accepted_at.
        let event_occurred_at = pending.occurred_at.to_rfc3339();
        if envelope.occurred_at != event_occurred_at
            || envelope.event_id != pending.event_id.to_string()
            || envelope.causation_ref != pending.causation_ref
            || envelope.correlation_id != pending.correlation_id
            || envelope.dedup_key != pending.dedup_key
        {
            return Err(stored_invalid());
        }
    }
    Ok(())
}

fn provenance_column_task_id(
    connection: &Connection,
    provenance_id: &str,
) -> Result<Option<String>, StoreError> {
    connection
        .query_row(
            "SELECT task_id FROM task_creation_provenances WHERE id = ?1",
            [provenance_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)
}

fn idempotency_created_ids(
    connection: &Connection,
    created_task_id: &str,
) -> Result<Option<(String, String)>, StoreError> {
    connection
        .query_row(
            "SELECT created_task_id, creation_provenance_id \
             FROM root_task_create_idempotency_v2 WHERE created_task_id = ?1",
            [created_task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(read_error)
}

fn find_root_creation_audit(
    connection: &Connection,
    task_id: &str,
    provenance_id: &str,
) -> Result<Option<AuditRecordV2>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id FROM audit_records_v2 \
             WHERE task_id = ?1 AND audit_type = 'task.creation_recorded' \
             ORDER BY occurred_at, id",
        )
        .map_err(read_error)?;
    let mut rows = statement.query([task_id]).map_err(read_error)?;
    let mut matched: Option<AuditRecordV2> = None;
    while let Some(row) = rows.next().map_err(read_error)? {
        let id: String = row.get(0).map_err(read_error)?;
        let audit = get_audit_v2(connection, &id)?.ok_or_else(stored_invalid)?;
        let context = audit
            .task_creation_context
            .as_ref()
            .ok_or_else(stored_invalid)?;
        if context.creation_provenance_ref == provenance_id
            && context.creation_kind == AuditRecordV2TaskCreationContextCreationKind::RootCommandV2
        {
            if matched.is_some() {
                // More than one matching creation audit is structural corruption.
                return Err(stored_invalid());
            }
            matched = Some(audit);
        }
    }
    Ok(matched)
}

fn find_task_created_event(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<OutboxRecord>, StoreError> {
    // Root create always writes sequence 0 for a new aggregate; read that exact row.
    let position: Option<i64> = connection
        .query_row(
            "SELECT outbox_position FROM outbox \
             WHERE event_type = 'task.created' AND aggregate_type = 'task' \
               AND aggregate_id = ?1 AND sequence = 0",
            [task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    let Some(position) = position else {
        return Ok(None);
    };
    crate::outbox::decode_versioned_row_at(connection, "outbox", position)
}

pub(crate) fn get_audit_v2(
    connection: &Connection,
    id: &str,
) -> Result<Option<AuditRecordV2>, StoreError> {
    get_document(connection, "audit_records_v2", AUDIT_RECORD_V2_SCHEMA, id)
}

pub(crate) fn get_provenance(
    connection: &Connection,
    id: &str,
) -> Result<Option<TaskCreationProvenanceV1>, StoreError> {
    let Some(provenance) = get_document(
        connection,
        "task_creation_provenances",
        PROVENANCE_SCHEMA,
        id,
    )?
    else {
        return Ok(None);
    };
    // Free column task_id must agree with the Task the provenance is bound to via
    // root_task_create_idempotency_v2 (or any future child mapping). Mismatch is stored corruption.
    let column_task_id = provenance_column_task_id(connection, id)?.ok_or_else(stored_invalid)?;
    let mapped = connection
        .query_row(
            "SELECT created_task_id FROM root_task_create_idempotency_v2 \
             WHERE creation_provenance_id = ?1",
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(read_error)?;
    if let Some(created_task_id) = mapped {
        if column_task_id != created_task_id {
            return Err(stored_invalid());
        }
    } else {
        // Child materialization path not implemented yet; unbound free column is still a fact that
        // must reference an existing Task row so it cannot float.
        let task_exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE id = ?1",
                [&column_task_id],
                |row| row.get(0),
            )
            .map_err(read_error)?;
        if task_exists != 1 {
            return Err(stored_invalid());
        }
    }
    Ok(Some(provenance))
}

fn get_document<T: serde::de::DeserializeOwned>(
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

fn decode_contract_document<T: serde::de::DeserializeOwned>(
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

fn format_accepted_at(value: DateTime<Utc>) -> String {
    // Root producer projects one accepted_at at UTC second precision (IC §5.5 / §6.16.1).
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_optional_uuid(value: Option<&str>) -> Result<Option<Uuid>, StoreError> {
    value
        .map(|raw| Uuid::parse_str(raw).map_err(|_| contract_error()))
        .transpose()
}

fn map_task_creation_error(error: TaskCreationError) -> StoreError {
    match error {
        TaskCreationError::InvalidOriginSourceUri
        | TaskCreationError::InvalidResourcePattern { .. }
        | TaskCreationError::InvalidExclusion { .. } => StoreError::new(
            StoreErrorCode::InvalidScopePattern,
            "task scope contains an invalid URI pattern",
        ),
        TaskCreationError::RawContract(_)
        | TaskCreationError::InvalidAllocationContract { .. }
        | TaskCreationError::InvalidUuid { .. }
        | TaskCreationError::AllocationConflict { .. } => contract_error(),
        TaskCreationError::InternalContract(_) | TaskCreationError::InternalJson(_) => {
            StoreError::new(
                StoreErrorCode::InternalStoreError,
                "root task.create v2 internal projection failed",
            )
        }
    }
}

fn enum_convert<S: Serialize, T: serde::de::DeserializeOwned>(value: S) -> Result<T, StoreError> {
    let value = serde_json::to_value(value).map_err(|_| serialization_error())?;
    serde_json::from_value(value).map_err(|_| contract_error())
}

fn contract_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::ContractInvalid,
        "root task.create v2 facts violate a generated JSON contract",
    )
}

fn serialization_error() -> StoreError {
    StoreError::new(
        StoreErrorCode::SerializationFailed,
        "root task.create v2 JSON serialization failed",
    )
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored root task.create v2 data failed integrity validation",
    )
}

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}
