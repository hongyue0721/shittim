//! Approval v2 repository: immutable records + per-chain current-head CAS (IC §6.10 / §6.10.6).
//!
//! Three composite CAS methods are the only write entry points:
//! `append_request` (new chain only), `resolve` (expected head CAS), and
//! `invalidate_and_optionally_replace` (expected head CAS, optional atomic replacement).
//! Every successful head mutation writes the immutable record(s), the unique
//! `approval_chain_heads` CAS, exactly one `approval.state_changed` event (payload
//! projected from committed facts per the change_kind truth table), and one
//! `approval.requested|resolved|invalidated` Audit — all in one transaction. CAS
//! losers, validation failures, and same-fact replays consume no allocation and emit
//! no event. There is no update/delete and no head guessing by created_at / max(id).

use crate::action::get_action;
use crate::identity::{
    get_challenge, get_credential, get_local_presence, get_system_authentication, StoredChallenge,
};
use crate::outbox::{EventAggregateId, PendingActiveEventV2};
use crate::root_task_create_v2::get_audit_v2;
use crate::task::encode_contract_document;
use crate::{StoreError, StoreErrorCode, WriteTransaction};
use chrono::Utc;
use kernel_contracts::{
    canonical_json_string, validate_json, ApprovalEventAllocationV1, ApprovalRecordKindV2,
    ApprovalRecordV2, ApprovalRecordV2RequestRecord, ApprovalRecordV2ResolutionRecord,
    ApprovalRecordV2ResolutionRecordDecision, ApprovalStateChangedPayloadV1,
    ApprovalStateChangedPayloadV1ChangeKind, ApprovalStateChangedPayloadV1SchemaVersion,
    ApprovalSubjectKindV2, AuditRecordV2, AuditRecordV2AuditType,
    AuditRecordV2ExternalContentStatus, AuditRecordV2Level, AuditRecordV2Outcome,
    AuditRecordV2RollbackCapability, AuditRecordV2SchemaVersion, ConfirmationModeV1,
    EventEnvelopeV2Payload, RemoteApprovalResponseV1,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use uuid::Uuid;

const APPROVAL_SCHEMA: &str = "https://schemas.shittim.local/policy/approval_record/v2";
const AUDIT_V2_SCHEMA: &str = "https://schemas.shittim.local/audit/audit_record/v2";
const APPROVAL_SAVEPOINT: &str = "kernel_sqlite_approval_mutation";

/// Evidence attached to a resolution, validated with transaction-bound reads.
///
/// Cryptographic signature verification for `RemoteSignature` is a Provider boundary
/// and intentionally not performed here; this repository validates existence, binding,
/// terminal challenge state, credential activity, and time discipline only.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionEvidence {
    /// `generic`: resolver actor/entry is the only decision evidence.
    Generic,
    /// `local`: a stored LocalPresenceEvidenceV1 id.
    LocalPresence {
        /// Stored evidence id.
        evidence_id: String,
    },
    /// `system_authentication`: a stored SystemAuthenticationEvidenceV1 id.
    SystemAuthentication {
        /// Stored evidence id.
        evidence_id: String,
    },
    /// `remote_signature`: the typed remote response (binding checks only, no crypto here).
    RemoteSignature {
        /// Typed remote approval response.
        response: Box<RemoteApprovalResponseV1>,
    },
}

/// Command for `append_request` (new chain only).
#[derive(Debug, Clone)]
pub struct AppendApprovalRequestCommand {
    /// The request record (must be the `request` variant; `predecessor_ref` null).
    pub request: ApprovalRecordV2,
    /// Caller-allocated event allocation (drives record/Audit/Event business time).
    pub event_allocation: ApprovalEventAllocationV1,
    /// Caller-allocated AuditRecord UUID for `approval.requested`.
    pub audit_record_id: String,
}

/// Command for `resolve`.
#[derive(Debug, Clone)]
pub struct ResolveApprovalCommand {
    /// Expected current head (must be the chain's `request` head).
    pub expected_head_ref: String,
    /// The resolution record (must be the `resolution` variant).
    pub resolution: ApprovalRecordV2,
    /// Mode-matched resolution evidence.
    pub evidence: ResolutionEvidence,
    /// Caller-allocated event allocation.
    pub event_allocation: ApprovalEventAllocationV1,
    /// Caller-allocated AuditRecord UUID for `approval.resolved`.
    pub audit_record_id: String,
}

/// Command for `invalidate_and_optionally_replace`.
#[derive(Debug, Clone)]
pub struct InvalidateApprovalCommand {
    /// Expected current head (request or approved resolution).
    pub expected_head_ref: String,
    /// The invalidation record (must be the `invalidation` variant).
    pub invalidation: ApprovalRecordV2,
    /// Optional replacement request (becomes the new head atomically).
    pub replacement: Option<ApprovalRecordV2>,
    /// Caller-allocated event allocation.
    pub event_allocation: ApprovalEventAllocationV1,
    /// Caller-allocated AuditRecord UUID for `approval.invalidated`.
    pub audit_record_id: String,
}

/// Result of an Approval head mutation.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalMutationResult {
    /// Records appended by this mutation (1 or 2, in head order).
    pub records: Vec<ApprovalRecordV2>,
    /// The new current head id.
    pub current_head_ref: String,
    /// The `approval.*` Audit written.
    pub audit: AuditRecordV2,
    /// The Outbox position of the single `approval.state_changed` event.
    pub event_outbox_position: String,
}

impl WriteTransaction<'_> {
    /// Creates a new approval chain (head = request).
    ///
    /// Rejects an existing chain, a non-null expected head, or a request carrying a
    /// predecessor. Never usable as a replacement.
    pub fn append_request(
        &self,
        command: AppendApprovalRequestCommand,
    ) -> Result<ApprovalMutationResult, StoreError> {
        self.with_savepoint(APPROVAL_SAVEPOINT, |connection| {
            let (chain_id, request_record, _subject_value, request_id) = match &command.request {
                ApprovalRecordV2::Request {
                    approval_chain_id,
                    record,
                    subject,
                    id,
                    predecessor_ref,
                    ..
                } => {
                    if predecessor_ref.is_some() {
                        return Err(contract_error(
                            "append_request requires a null predecessor_ref",
                        ));
                    }
                    (
                        approval_chain_id.clone(),
                        record.clone(),
                        serde_json::to_value(subject).map_err(|_| contract_error("subject"))?,
                        id.clone(),
                    )
                }
                _ => return Err(contract_error("append_request requires a request record")),
            };
            validate_allocation(&command.event_allocation)?;
            if get_chain_head(connection, &chain_id)?.is_some() {
                return Err(head_conflict());
            }
            validate_uuid(&chain_id)?;
            validate_uuid(&request_id)?;
            validate_uuid(&command.audit_record_id)?;

            insert_approval_record(connection, &command.request)?;
            upsert_chain_head(
                connection,
                &chain_id,
                &request_id,
                "request",
                &command.event_allocation.changed_at,
            )?;
            bind_action_approval_chain(connection, &command.request, &chain_id)?;

            let payload = project_payload(PayloadProjection {
                change_kind: ApprovalStateChangedPayloadV1ChangeKind::InitialRequest,
                chain_id: &chain_id,
                from_head_ref: None,
                to_head_ref: &request_id,
                from_record_kind: None,
                to_record_kind: ApprovalRecordKindV2::Request,
                request_ref: Some(&request_id),
                resolution_ref: None,
                invalidation_ref: None,
                replacement_request_ref: None,
                record: &command.request,
                reason_code: first_reason_code_request(&request_record),
                changed_at: &command.event_allocation.changed_at,
            })?;
            let event_position =
                append_approval_event(self, &command.event_allocation, &chain_id, payload)?;
            let audit = build_approval_audit(
                AuditProjection {
                    audit_type: AuditRecordV2AuditType::ApprovalRequested,
                    audit_record_id: &command.audit_record_id,
                    allocation: &command.event_allocation,
                    record: &command.request,
                    approval_resolution_ref: None,
                },
                connection,
            )?;
            let audit = insert_and_verify_audit(connection, audit)?;

            Ok(ApprovalMutationResult {
                records: vec![
                    get_approval_record(connection, &request_id)?.ok_or_else(stored_invalid)?
                ],
                current_head_ref: request_id,
                audit,
                event_outbox_position: event_position,
            })
        })
    }

    /// Resolves a chain whose current head is the expected request.
    pub fn resolve(
        &self,
        command: ResolveApprovalCommand,
    ) -> Result<ApprovalMutationResult, StoreError> {
        self.with_savepoint(APPROVAL_SAVEPOINT, |connection| {
            let (chain_id, resolution_record, resolution_id) = match &command.resolution {
                ApprovalRecordV2::Resolution {
                    approval_chain_id,
                    record,
                    id,
                    ..
                } => (approval_chain_id.clone(), record.clone(), id.clone()),
                _ => return Err(contract_error("resolve requires a resolution record")),
            };
            validate_allocation(&command.event_allocation)?;
            validate_uuid(&resolution_id)?;
            validate_uuid(&command.audit_record_id)?;

            let head = get_chain_head(connection, &chain_id)?
                .ok_or_else(|| contract_error("approval chain was not found"))?;
            if head.current_head_ref != command.expected_head_ref {
                return Err(head_conflict());
            }
            let request = get_approval_record(connection, &head.current_head_ref)?
                .ok_or_else(stored_invalid)?;
            let (request_record, request_subject_value) = match &request {
                ApprovalRecordV2::Request {
                    record, subject, ..
                } => (
                    record.clone(),
                    serde_json::to_value(subject).map_err(|_| contract_error("subject"))?,
                ),
                _ => return Err(contract_error("current head must be a request for resolve")),
            };
            let resolution_subject_value = match &command.resolution {
                ApprovalRecordV2::Resolution { subject, .. } => {
                    serde_json::to_value(subject).map_err(|_| contract_error("subject"))?
                }
                _ => return Err(contract_error("resolve requires a resolution record")),
            };
            if canonical_json_string(&request_subject_value).map_err(|_| stored_invalid())?
                != canonical_json_string(&resolution_subject_value).map_err(|_| stored_invalid())?
            {
                return Err(contract_error(
                    "resolution subject must equal the chain canonical subject",
                ));
            }
            if resolution_record.request_ref != head.current_head_ref {
                return Err(contract_error(
                    "resolution request_ref must equal the current head",
                ));
            }
            match &command.resolution {
                ApprovalRecordV2::Resolution {
                    predecessor_ref, ..
                } if predecessor_ref != &head.current_head_ref => {
                    return Err(contract_error(
                        "resolution predecessor_ref must equal the current head",
                    ));
                }
                _ => {}
            }
            validate_resolution_evidence(
                connection,
                &request_record,
                &resolution_record,
                &command.evidence,
                &request_subject_value,
            )?;

            insert_approval_record(connection, &command.resolution)?;
            upsert_chain_head(
                connection,
                &chain_id,
                &resolution_id,
                "resolution",
                &command.event_allocation.changed_at,
            )?;

            let payload = project_payload(PayloadProjection {
                change_kind: ApprovalStateChangedPayloadV1ChangeKind::Resolution,
                chain_id: &chain_id,
                from_head_ref: Some(&head.current_head_ref),
                to_head_ref: &resolution_id,
                from_record_kind: Some(ApprovalRecordKindV2::Request),
                to_record_kind: ApprovalRecordKindV2::Resolution,
                request_ref: Some(&head.current_head_ref),
                resolution_ref: Some(&resolution_id),
                invalidation_ref: None,
                replacement_request_ref: None,
                record: &command.resolution,
                reason_code: first_reason_code_request(&request_record),
                changed_at: &command.event_allocation.changed_at,
            })?;
            let event_position =
                append_approval_event(self, &command.event_allocation, &chain_id, payload)?;
            let audit = build_approval_audit(
                AuditProjection {
                    audit_type: AuditRecordV2AuditType::ApprovalResolved,
                    audit_record_id: &command.audit_record_id,
                    allocation: &command.event_allocation,
                    record: &command.resolution,
                    approval_resolution_ref: match resolution_record.decision {
                        ApprovalRecordV2ResolutionRecordDecision::Approved => {
                            Some(resolution_id.clone())
                        }
                        ApprovalRecordV2ResolutionRecordDecision::Denied => None,
                    },
                },
                connection,
            )?;
            let audit = insert_and_verify_audit(connection, audit)?;

            Ok(ApprovalMutationResult {
                records: vec![
                    get_approval_record(connection, &head.current_head_ref)?
                        .ok_or_else(stored_invalid)?,
                    get_approval_record(connection, &resolution_id)?.ok_or_else(stored_invalid)?,
                ],
                current_head_ref: resolution_id,
                audit,
                event_outbox_position: event_position,
            })
        })
    }

    /// Invalidates the current head, optionally installing a replacement request atomically.
    pub fn invalidate_and_optionally_replace(
        &self,
        command: InvalidateApprovalCommand,
    ) -> Result<ApprovalMutationResult, StoreError> {
        self.with_savepoint(APPROVAL_SAVEPOINT, |connection| {
            let (chain_id, invalidation_record, invalidation_id) = match &command.invalidation {
                ApprovalRecordV2::Invalidation {
                    approval_chain_id,
                    record,
                    id,
                    ..
                } => (approval_chain_id.clone(), record.clone(), id.clone()),
                _ => {
                    return Err(contract_error("invalidate requires an invalidation record"));
                }
            };
            validate_allocation(&command.event_allocation)?;
            validate_uuid(&invalidation_id)?;
            validate_uuid(&command.audit_record_id)?;

            let head = get_chain_head(connection, &chain_id)?
                .ok_or_else(|| contract_error("approval chain was not found"))?;
            if head.current_head_ref != command.expected_head_ref {
                return Err(head_conflict());
            }
            if head.head_record_kind == "invalidation" {
                return Err(contract_error(
                    "cannot invalidate an already invalidated head",
                ));
            }
            if invalidation_record.invalidated_record_ref != head.current_head_ref {
                return Err(contract_error(
                    "invalidated_record_ref must equal the current head",
                ));
            }
            match &command.invalidation {
                ApprovalRecordV2::Invalidation {
                    predecessor_ref, ..
                } if predecessor_ref != &head.current_head_ref => {
                    return Err(contract_error(
                        "invalidation predecessor_ref must equal the current head",
                    ));
                }
                _ => {}
            }

            let old_head = get_approval_record(connection, &head.current_head_ref)?
                .ok_or_else(stored_invalid)?;
            let old_subject_value = record_subject_value(&old_head)?;
            let old_request_ref = match &old_head {
                ApprovalRecordV2::Request { id, .. } => id.clone(),
                ApprovalRecordV2::Resolution { record, .. } => record.request_ref.clone(),
                ApprovalRecordV2::Invalidation { .. } => return Err(stored_invalid()),
            };
            let old_resolution_ref = match &old_head {
                ApprovalRecordV2::Resolution { id, .. } => Some(id.clone()),
                _ => None,
            };

            let replacement_id = if let Some(replacement) = &command.replacement {
                let (replacement_record_id, replacement_subject_value, replacement_predecessor) =
                    match replacement {
                        ApprovalRecordV2::Request {
                            id,
                            subject,
                            predecessor_ref,
                            ..
                        } => (
                            id.clone(),
                            serde_json::to_value(subject).map_err(|_| contract_error("subject"))?,
                            predecessor_ref.clone(),
                        ),
                        _ => {
                            return Err(contract_error("replacement must be a request record"));
                        }
                    };
                validate_uuid(&replacement_record_id)?;
                if canonical_json_string(&old_subject_value).map_err(|_| stored_invalid())?
                    != canonical_json_string(&replacement_subject_value)
                        .map_err(|_| stored_invalid())?
                {
                    return Err(contract_error(
                        "replacement subject must equal the chain canonical subject",
                    ));
                }
                if invalidation_record.replacement_request_ref.as_deref()
                    != Some(replacement_record_id.as_str())
                {
                    return Err(contract_error(
                        "replacement_request_ref must equal the replacement request id",
                    ));
                }
                if replacement_predecessor.as_deref() != Some(invalidation_id.as_str()) {
                    return Err(contract_error(
                        "replacement predecessor_ref must equal the invalidation id",
                    ));
                }
                Some(replacement_record_id)
            } else {
                if invalidation_record.replacement_request_ref.is_some() {
                    return Err(contract_error(
                        "replacement_request_ref requires a replacement request",
                    ));
                }
                None
            };

            insert_approval_record(connection, &command.invalidation)?;
            if let Some(replacement) = &command.replacement {
                insert_approval_record(connection, replacement)?;
            }
            let (new_head_ref, new_head_kind) = match (&command.replacement, &replacement_id) {
                (Some(_), Some(id)) => (id.clone(), "request"),
                _ => (invalidation_id.clone(), "invalidation"),
            };
            upsert_chain_head(
                connection,
                &chain_id,
                &new_head_ref,
                new_head_kind,
                &command.event_allocation.changed_at,
            )?;

            let from_record_kind = match head.head_record_kind.as_str() {
                "request" => ApprovalRecordKindV2::Request,
                "resolution" => ApprovalRecordKindV2::Resolution,
                _ => return Err(stored_invalid()),
            };
            let (change_kind, to_record_kind) = if replacement_id.is_some() {
                (
                    ApprovalStateChangedPayloadV1ChangeKind::ReplacementRequest,
                    ApprovalRecordKindV2::Request,
                )
            } else {
                (
                    ApprovalStateChangedPayloadV1ChangeKind::InvalidationWithoutReplacement,
                    ApprovalRecordKindV2::Invalidation,
                )
            };
            let payload = project_payload(PayloadProjection {
                change_kind,
                chain_id: &chain_id,
                from_head_ref: Some(&head.current_head_ref),
                to_head_ref: &new_head_ref,
                from_record_kind: Some(from_record_kind),
                to_record_kind,
                request_ref: Some(&old_request_ref),
                resolution_ref: old_resolution_ref.as_ref(),
                invalidation_ref: Some(&invalidation_id),
                replacement_request_ref: replacement_id.as_ref(),
                record: &command.invalidation,
                reason_code: invalidation_record.reason_code.as_str(),
                changed_at: &command.event_allocation.changed_at,
            })?;
            let event_position =
                append_approval_event(self, &command.event_allocation, &chain_id, payload)?;
            let audit = build_approval_audit(
                AuditProjection {
                    audit_type: AuditRecordV2AuditType::ApprovalInvalidated,
                    audit_record_id: &command.audit_record_id,
                    allocation: &command.event_allocation,
                    record: &command.invalidation,
                    approval_resolution_ref: old_resolution_ref.clone(),
                },
                connection,
            )?;
            let audit = insert_and_verify_audit(connection, audit)?;

            let mut records = vec![
                get_approval_record(connection, &head.current_head_ref)?
                    .ok_or_else(stored_invalid)?,
                get_approval_record(connection, &invalidation_id)?.ok_or_else(stored_invalid)?,
            ];
            if let Some(id) = &replacement_id {
                records.push(get_approval_record(connection, id)?.ok_or_else(stored_invalid)?);
            }
            Ok(ApprovalMutationResult {
                records,
                current_head_ref: new_head_ref,
                audit,
                event_outbox_position: event_position,
            })
        })
    }
}

impl crate::SqliteStore {
    /// Reads an immutable ApprovalRecord by id.
    pub fn get_approval_record(
        &self,
        record_id: &str,
    ) -> Result<Option<ApprovalRecordV2>, StoreError> {
        let connection = self.lock_connection()?;
        get_approval_record(&connection, record_id)
    }

    /// Reads the current head of an approval chain.
    pub fn get_approval_chain_head(
        &self,
        chain_id: &str,
    ) -> Result<Option<ApprovalChainHead>, StoreError> {
        let connection = self.lock_connection()?;
        get_chain_head(&connection, chain_id)
    }

    /// Lists the immutable records of a chain in insertion order.
    pub fn list_approval_chain_records(
        &self,
        chain_id: &str,
    ) -> Result<Vec<ApprovalRecordV2>, StoreError> {
        let connection = self.lock_connection()?;
        list_chain_records(&connection, chain_id)
    }
}

/// Current head pointer of an approval chain.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalChainHead {
    /// Chain id.
    pub chain_id: String,
    /// Current head record id.
    pub current_head_ref: String,
    /// Current head record kind (`request|resolution|invalidation`).
    pub head_record_kind: String,
    /// Last head mutation time.
    pub updated_at: String,
}

fn insert_approval_record(
    connection: &Connection,
    record: &ApprovalRecordV2,
) -> Result<(), StoreError> {
    let record_json = encode_contract_document(APPROVAL_SCHEMA, record)?;
    connection
        .execute(
            "INSERT INTO approval_records(record_json) VALUES (?1)",
            [record_json],
        )
        .map_err(write_error)?;
    Ok(())
}

pub(crate) fn get_approval_record(
    connection: &Connection,
    record_id: &str,
) -> Result<Option<ApprovalRecordV2>, StoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT record_json FROM approval_records WHERE id = ?1",
            [record_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(|stored| decode_approval_document(&stored))
        .transpose()
}

fn list_chain_records(
    connection: &Connection,
    chain_id: &str,
) -> Result<Vec<ApprovalRecordV2>, StoreError> {
    let mut statement = connection
        .prepare("SELECT record_json FROM approval_records WHERE chain_id = ?1 ORDER BY rowid")
        .map_err(read_error)?;
    let rows = statement
        .query_map([chain_id], |row| row.get::<_, String>(0))
        .map_err(read_error)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(decode_approval_document(&row.map_err(read_error)?)?);
    }
    Ok(records)
}

pub(crate) fn get_chain_head(
    connection: &Connection,
    chain_id: &str,
) -> Result<Option<ApprovalChainHead>, StoreError> {
    connection
        .query_row(
            "SELECT chain_id, current_head_ref, head_record_kind, updated_at \
             FROM approval_chain_heads WHERE chain_id = ?1",
            [chain_id],
            |row| {
                Ok(ApprovalChainHead {
                    chain_id: row.get(0)?,
                    current_head_ref: row.get(1)?,
                    head_record_kind: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(read_error)
}

fn upsert_chain_head(
    connection: &Connection,
    chain_id: &str,
    head_ref: &str,
    head_kind: &str,
    updated_at: &str,
) -> Result<(), StoreError> {
    connection
        .execute(
            "INSERT INTO approval_chain_heads(chain_id, current_head_ref, head_record_kind, updated_at) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(chain_id) DO UPDATE SET \
                current_head_ref = excluded.current_head_ref, \
                head_record_kind = excluded.head_record_kind, \
                updated_at = excluded.updated_at",
            params![chain_id, head_ref, head_kind, updated_at],
        )
        .map_err(write_error)?;
    Ok(())
}

fn bind_action_approval_chain(
    connection: &Connection,
    request: &ApprovalRecordV2,
    chain_id: &str,
) -> Result<(), StoreError> {
    let action_id = match request {
        ApprovalRecordV2::Request {
            subject: kernel_contracts::ApprovalRecordV2RequestSubject::Operation { action_id, .. },
            ..
        } => Some(action_id.clone()),
        _ => None,
    };
    let Some(action_id) = action_id else {
        return Ok(());
    };
    let action = get_action(connection, &action_id)?
        .ok_or_else(|| contract_error("operation subject action was not found"))?;
    // The approval-chain binding is an Action fact change: bump revision in the same
    // snapshot write (never a silent in-place edit), then canonical readback.
    let next_revision = action.revision.checked_add(1).ok_or_else(stored_invalid)?;
    let changed = connection
        .execute(
            "UPDATE actions SET record_json = json_set( \
                record_json, '$.approval_chain_id', ?1, '$.revision', ?2) \
             WHERE id = ?3 AND revision = ?4",
            params![chain_id, next_revision, action.action_id, action.revision],
        )
        .map_err(write_error)?;
    if changed != 1 {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "action approval-chain binding CAS failed",
        ));
    }
    let updated = get_action(connection, &action.action_id)?.ok_or_else(stored_invalid)?;
    if updated.approval_chain_id.as_deref() != Some(chain_id) || updated.revision != next_revision {
        return Err(stored_invalid());
    }
    Ok(())
}

fn validate_allocation(allocation: &ApprovalEventAllocationV1) -> Result<(), StoreError> {
    if allocation.correlation_id.trim().is_empty() || allocation.dedup_key.trim().is_empty() {
        return Err(contract_error(
            "event allocation correlation_id and dedup_key must be non-empty",
        ));
    }
    validate_uuid(&allocation.event_id)?;
    Ok(())
}

fn validate_uuid(value: &str) -> Result<(), StoreError> {
    Uuid::parse_str(value).map_err(|_| contract_error("expected a UUID"))?;
    Ok(())
}

fn head_conflict() -> StoreError {
    StoreError::new(
        StoreErrorCode::ConstraintViolation,
        "approval_head_conflict",
    )
}

fn record_subject_value(record: &ApprovalRecordV2) -> Result<Value, StoreError> {
    match record {
        ApprovalRecordV2::Request { subject, .. } => {
            serde_json::to_value(subject).map_err(|_| contract_error("subject"))
        }
        ApprovalRecordV2::Resolution { subject, .. } => {
            serde_json::to_value(subject).map_err(|_| contract_error("subject"))
        }
        ApprovalRecordV2::Invalidation { subject, .. } => {
            serde_json::to_value(subject).map_err(|_| contract_error("subject"))
        }
    }
}

fn first_reason_code_request(record: &ApprovalRecordV2RequestRecord) -> &str {
    record
        .reason_codes
        .first()
        .map(String::as_str)
        .unwrap_or("approval")
}

struct PayloadProjection<'a> {
    change_kind: ApprovalStateChangedPayloadV1ChangeKind,
    chain_id: &'a str,
    from_head_ref: Option<&'a String>,
    to_head_ref: &'a str,
    from_record_kind: Option<ApprovalRecordKindV2>,
    to_record_kind: ApprovalRecordKindV2,
    request_ref: Option<&'a String>,
    resolution_ref: Option<&'a String>,
    invalidation_ref: Option<&'a String>,
    replacement_request_ref: Option<&'a String>,
    record: &'a ApprovalRecordV2,
    reason_code: &'a str,
    changed_at: &'a str,
}

fn project_payload(
    projection: PayloadProjection<'_>,
) -> Result<ApprovalStateChangedPayloadV1, StoreError> {
    let (subject_kind, confirmation_mode, permission_decision_ref, action_id) =
        match projection.record {
            ApprovalRecordV2::Request {
                subject, record, ..
            } => {
                let subject_kind = match subject {
                    kernel_contracts::ApprovalRecordV2RequestSubject::Operation { .. } => {
                        ApprovalSubjectKindV2::Operation
                    }
                    kernel_contracts::ApprovalRecordV2RequestSubject::TaskProposal { .. } => {
                        ApprovalSubjectKindV2::TaskProposal
                    }
                    kernel_contracts::ApprovalRecordV2RequestSubject::PlanRevision { .. } => {
                        ApprovalSubjectKindV2::PlanRevision
                    }
                };
                let (pd, action) = match subject {
                    kernel_contracts::ApprovalRecordV2RequestSubject::Operation {
                        permission_decision_ref,
                        action_id,
                        ..
                    } => (
                        Some(permission_decision_ref.clone()),
                        Some(action_id.clone()),
                    ),
                    _ => (None, None),
                };
                (subject_kind, record.confirmation_mode, pd, action)
            }
            ApprovalRecordV2::Resolution { subject, .. } => {
                let subject_kind = match subject {
                    kernel_contracts::ApprovalRecordV2ResolutionSubject::Operation { .. } => {
                        ApprovalSubjectKindV2::Operation
                    }
                    kernel_contracts::ApprovalRecordV2ResolutionSubject::TaskProposal {
                        ..
                    } => ApprovalSubjectKindV2::TaskProposal,
                    kernel_contracts::ApprovalRecordV2ResolutionSubject::PlanRevision {
                        ..
                    } => ApprovalSubjectKindV2::PlanRevision,
                };
                let (pd, action) = match subject {
                    kernel_contracts::ApprovalRecordV2ResolutionSubject::Operation {
                        permission_decision_ref,
                        action_id,
                        ..
                    } => (
                        Some(permission_decision_ref.clone()),
                        Some(action_id.clone()),
                    ),
                    _ => (None, None),
                };
                (subject_kind, ConfirmationModeV1::Generic, pd, action)
            }
            ApprovalRecordV2::Invalidation { subject, .. } => {
                let subject_kind = match subject {
                    kernel_contracts::ApprovalRecordV2InvalidationSubject::Operation { .. } => {
                        ApprovalSubjectKindV2::Operation
                    }
                    kernel_contracts::ApprovalRecordV2InvalidationSubject::TaskProposal {
                        ..
                    } => ApprovalSubjectKindV2::TaskProposal,
                    kernel_contracts::ApprovalRecordV2InvalidationSubject::PlanRevision {
                        ..
                    } => ApprovalSubjectKindV2::PlanRevision,
                };
                let (pd, action) = match subject {
                    kernel_contracts::ApprovalRecordV2InvalidationSubject::Operation {
                        permission_decision_ref,
                        action_id,
                        ..
                    } => (
                        Some(permission_decision_ref.clone()),
                        Some(action_id.clone()),
                    ),
                    _ => (None, None),
                };
                (subject_kind, ConfirmationModeV1::Generic, pd, action)
            }
        };
    Ok(ApprovalStateChangedPayloadV1 {
        schema_version: ApprovalStateChangedPayloadV1SchemaVersion,
        change_kind: projection.change_kind,
        approval_chain_id: projection.chain_id.to_owned(),
        from_head_ref: projection.from_head_ref.cloned(),
        to_head_ref: projection.to_head_ref.to_owned(),
        from_record_kind: projection.from_record_kind,
        to_record_kind: projection.to_record_kind,
        subject_kind,
        confirmation_mode,
        request_ref: projection.request_ref.cloned(),
        resolution_ref: projection.resolution_ref.cloned(),
        invalidation_ref: projection.invalidation_ref.cloned(),
        replacement_request_ref: projection.replacement_request_ref.cloned(),
        permission_decision_ref,
        action_id,
        reason_code: projection.reason_code.to_owned(),
        changed_at: projection.changed_at.to_owned(),
    })
}

fn append_approval_event(
    transaction: &WriteTransaction<'_>,
    allocation: &ApprovalEventAllocationV1,
    chain_id: &str,
    payload: ApprovalStateChangedPayloadV1,
) -> Result<String, StoreError> {
    let occurred_at = chrono::DateTime::parse_from_rfc3339(&allocation.changed_at)
        .map_err(|_| contract_error("event allocation changed_at must be RFC 3339"))?
        .with_timezone(&Utc);
    let record = transaction.append_active_event_v2(PendingActiveEventV2 {
        event_id: Uuid::parse_str(&allocation.event_id).map_err(|_| contract_error("event id"))?,
        aggregate_id: EventAggregateId::ApprovalChain(
            Uuid::parse_str(chain_id).map_err(|_| contract_error("chain id"))?,
        ),
        occurred_at,
        causation_ref: allocation.causation_ref.clone(),
        correlation_id: allocation.correlation_id.clone(),
        dedup_key: allocation.dedup_key.clone(),
        payload: EventEnvelopeV2Payload::ApprovalStateChanged(Box::new(payload)),
    })?;
    Ok(record.envelope.outbox_position().to_owned())
}

struct AuditProjection<'a> {
    audit_type: AuditRecordV2AuditType,
    audit_record_id: &'a str,
    allocation: &'a ApprovalEventAllocationV1,
    record: &'a ApprovalRecordV2,
    approval_resolution_ref: Option<String>,
}

fn build_approval_audit(
    projection: AuditProjection<'_>,
    _connection: &Connection,
) -> Result<AuditRecordV2, StoreError> {
    let (task_id, action_id, actor, entry_point) = match projection.record {
        ApprovalRecordV2::Request {
            subject, record, ..
        } => {
            let (task, action) = match subject {
                kernel_contracts::ApprovalRecordV2RequestSubject::Operation {
                    task_id,
                    action_id,
                    ..
                } => (Some(task_id.clone()), Some(action_id.clone())),
                kernel_contracts::ApprovalRecordV2RequestSubject::TaskProposal {
                    candidate_task_id,
                    ..
                } => (Some(candidate_task_id.clone()), None),
                kernel_contracts::ApprovalRecordV2RequestSubject::PlanRevision {
                    task_id, ..
                } => (Some(task_id.clone()), None),
            };
            (
                task,
                action,
                Some(record.requested_by_actor.clone()),
                record.requested_from_entry_point,
            )
        }
        ApprovalRecordV2::Resolution {
            subject, record, ..
        } => {
            let (task, action) = match subject {
                kernel_contracts::ApprovalRecordV2ResolutionSubject::Operation {
                    task_id,
                    action_id,
                    ..
                } => (Some(task_id.clone()), Some(action_id.clone())),
                kernel_contracts::ApprovalRecordV2ResolutionSubject::TaskProposal {
                    candidate_task_id,
                    ..
                } => (Some(candidate_task_id.clone()), None),
                kernel_contracts::ApprovalRecordV2ResolutionSubject::PlanRevision {
                    task_id,
                    ..
                } => (Some(task_id.clone()), None),
            };
            (
                task,
                action,
                Some(record.resolved_by_actor.clone()),
                record.resolved_from_entry_point,
            )
        }
        ApprovalRecordV2::Invalidation {
            subject, record, ..
        } => {
            let (task, action) = match subject {
                kernel_contracts::ApprovalRecordV2InvalidationSubject::Operation {
                    task_id,
                    action_id,
                    ..
                } => (Some(task_id.clone()), Some(action_id.clone())),
                kernel_contracts::ApprovalRecordV2InvalidationSubject::TaskProposal {
                    candidate_task_id,
                    ..
                } => (Some(candidate_task_id.clone()), None),
                kernel_contracts::ApprovalRecordV2InvalidationSubject::PlanRevision {
                    task_id,
                    ..
                } => (Some(task_id.clone()), None),
            };
            (
                task,
                action,
                record.invalidated_by_actor.clone(),
                record.invalidated_from_entry_point,
            )
        }
    };
    Ok(AuditRecordV2 {
        action_id,
        actor,
        approval_resolution_ref: projection.approval_resolution_ref,
        artifact_refs: vec![],
        audit_type: projection.audit_type,
        causation_ref: Some(projection.allocation.causation_ref.clone()),
        content_origin_refs: vec![],
        correlation_id: Some(projection.allocation.correlation_id.clone()),
        delegation_ref: None,
        details: serde_json::json!({}),
        entry_point,
        extension_id: None,
        external_content_status: AuditRecordV2ExternalContentStatus::NotSent,
        id: projection.audit_record_id.to_owned(),
        level: AuditRecordV2Level::Security,
        model_call_refs: vec![],
        occurred_at: projection.allocation.changed_at.clone(),
        outcome: AuditRecordV2Outcome::Succeeded,
        payload_manifest_refs: vec![],
        permission_decision_ref: None,
        policy_context: None,
        provider_id: None,
        reason_codes: vec![projection.audit_type.as_str().to_owned()],
        recovery_attempt_ref: None,
        resource_refs: vec![],
        rollback_capability: AuditRecordV2RollbackCapability::Unknown,
        schema_version: AuditRecordV2SchemaVersion,
        stop_fence_generation: None,
        summary: None,
        task_creation_context: None,
        task_id,
        verification_result_refs: vec![],
    })
}

fn insert_and_verify_audit(
    connection: &Connection,
    audit: AuditRecordV2,
) -> Result<AuditRecordV2, StoreError> {
    let audit_json = encode_contract_document(AUDIT_V2_SCHEMA, &audit)?;
    connection
        .execute(
            "INSERT INTO audit_records_v2(record_json) VALUES (?1)",
            [&audit_json],
        )
        .map_err(write_error)?;
    let stored = get_audit_v2(connection, &audit.id)?.ok_or_else(stored_invalid)?;
    if stored != audit {
        return Err(stored_invalid());
    }
    Ok(stored)
}

fn validate_resolution_evidence(
    connection: &Connection,
    request_record: &ApprovalRecordV2RequestRecord,
    resolution_record: &ApprovalRecordV2ResolutionRecord,
    evidence: &ResolutionEvidence,
    _subject_value: &Value,
) -> Result<(), StoreError> {
    match (request_record.confirmation_mode, evidence) {
        (
            ConfirmationModeV1::Generic | ConfirmationModeV1::PlanRevision,
            ResolutionEvidence::Generic,
        ) => {
            if !resolution_record.evidence_refs.is_empty() {
                return Err(contract_error(
                    "generic/plan_revision resolution must carry empty evidence_refs",
                ));
            }
            Ok(())
        }
        (ConfirmationModeV1::Local, ResolutionEvidence::LocalPresence { evidence_id }) => {
            let evidence_record = get_local_presence(connection, evidence_id)?
                .ok_or_else(|| contract_error("local presence evidence was not found"))?;
            if resolution_record.local_presence_evidence_ref.as_deref()
                != Some(evidence_id.as_str())
                || resolution_record.evidence_refs != vec![evidence_id.clone()]
            {
                return Err(contract_error(
                    "local resolution must bind exactly the local presence evidence",
                ));
            }
            if evidence_record.valid_until < resolution_record.resolved_at {
                return Err(contract_error("local presence evidence is expired"));
            }
            Ok(())
        }
        (
            ConfirmationModeV1::SystemAuthentication,
            ResolutionEvidence::SystemAuthentication { evidence_id },
        ) => {
            let evidence_record = get_system_authentication(connection, evidence_id)?
                .ok_or_else(|| contract_error("system authentication evidence was not found"))?;
            if resolution_record.system_auth_evidence_ref.as_deref() != Some(evidence_id.as_str())
                || resolution_record.evidence_refs != vec![evidence_id.clone()]
            {
                return Err(contract_error(
                    "system resolution must bind exactly the system authentication evidence",
                ));
            }
            let challenge = get_challenge(connection, &evidence_record.challenge_ref)?
                .ok_or_else(|| contract_error("system challenge was not found"))?;
            let StoredChallenge::System(challenge) = challenge else {
                return Err(contract_error(
                    "system evidence must bind a system challenge",
                ));
            };
            if challenge.request_ref != resolution_record.request_ref {
                return Err(contract_error(
                    "system challenge must bind the original request",
                ));
            }
            if !matches!(
                challenge.state,
                kernel_contracts::SystemAuthenticationChallengeV1State::Consumed
            ) {
                return Err(contract_error(
                    "system challenge must be consumed in the same transaction flow",
                ));
            }
            if evidence_record.valid_until < resolution_record.resolved_at {
                return Err(contract_error("system authentication evidence is expired"));
            }
            Ok(())
        }
        (ConfirmationModeV1::RemoteSignature, ResolutionEvidence::RemoteSignature { response }) => {
            if resolution_record.remote_response_ref.as_deref()
                != Some(response.challenge_id.as_str())
            {
                return Err(contract_error(
                    "remote resolution must bind the remote response",
                ));
            }
            if response.request_ref != resolution_record.request_ref {
                return Err(contract_error(
                    "remote response must bind the original request",
                ));
            }
            let challenge = get_challenge(connection, &response.challenge_id)?
                .ok_or_else(|| contract_error("remote challenge was not found"))?;
            let StoredChallenge::Remote(challenge) = challenge else {
                return Err(contract_error(
                    "remote response must bind a remote challenge",
                ));
            };
            if !matches!(
                challenge.state,
                kernel_contracts::RemoteApprovalChallengeV1State::Consumed
            ) {
                return Err(contract_error(
                    "remote challenge must be consumed in the same transaction flow",
                ));
            }
            let credential = get_credential(
                connection,
                &challenge.credential_ref.credential_id,
                Some(challenge.credential_ref.credential_revision),
            )?
            .ok_or_else(|| contract_error("remote credential was not found"))?;
            if credential.status != kernel_contracts::CredentialRefV1Status::Active {
                return Err(contract_error("remote credential is not active"));
            }
            // Cryptographic verification is a Provider boundary and not performed here.
            Ok(())
        }
        _ => Err(contract_error(
            "resolution evidence does not match the request confirmation mode",
        )),
    }
}

fn decode_approval_document(stored: &str) -> Result<ApprovalRecordV2, StoreError> {
    let value: Value = serde_json::from_str(stored).map_err(|_| stored_invalid())?;
    validate_json(APPROVAL_SCHEMA, &value).map_err(|_| stored_invalid())?;
    let canonical = canonical_json_string(&value).map_err(|_| stored_invalid())?;
    if canonical != stored {
        return Err(stored_invalid());
    }
    serde_json::from_value(value).map_err(|_| stored_invalid())
}

fn contract_error(message: &'static str) -> StoreError {
    StoreError::new(StoreErrorCode::ContractInvalid, message)
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored approval repository data failed integrity validation",
    )
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}
