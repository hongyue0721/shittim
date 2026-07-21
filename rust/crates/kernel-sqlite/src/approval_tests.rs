//! Approval v2 + Identity repository tests (slice 4c).

use super::*;
use crate::identity::StoredChallenge;
use chrono::{TimeZone, Utc};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, ApprovalEventAllocationV1,
    ApprovalRecordKindV2, ApprovalRecordV2, ApprovalRecordV2InvalidationRecord,
    ApprovalRecordV2InvalidationRecordReasonCode, ApprovalRecordV2InvalidationSchemaVersion,
    ApprovalRecordV2InvalidationSubject, ApprovalRecordV2RequestRecord,
    ApprovalRecordV2RequestSchemaVersion, ApprovalRecordV2RequestSubject,
    ApprovalRecordV2ResolutionRecord, ApprovalRecordV2ResolutionRecordDecision,
    ApprovalRecordV2ResolutionSchemaVersion, ApprovalRecordV2ResolutionSubject,
    ApprovalStateChangedPayloadV1ChangeKind, ApprovalSubjectKindV2, AuditRecordV2AuditType,
    CausationRefV2, ConfirmationModeV1, CredentialRefV1, CredentialRefV1SchemaVersion,
    CredentialRefV1Status, EntryPoint, EventEnvelopeV2Payload, InputContentOriginV1,
    InputContentOriginV1Kind, InputContentOriginV1ProducerRef, InputContentOriginV1ProducerRefKind,
    InputContentOriginV1SchemaVersion, InputTaskScopeV1, InputTaskScopeV1SchemaVersion,
    LocalPresenceEvidenceV1, LocalPresenceEvidenceV1PresenceKind,
    LocalPresenceEvidenceV1SchemaVersion, LocalPresenceEvidenceV1TransportKind,
    LocalPresenceEvidenceV1VerifierKind, NormalizedRootTaskCreatePayloadV2Proposer,
    RemoteApprovalChallengeV1, RemoteApprovalChallengeV1AllowedDecisions,
    RemoteApprovalChallengeV1NonceEncoding, RemoteApprovalChallengeV1SchemaVersion,
    RemoteApprovalChallengeV1State, RemoteSignatureAlgorithmV1,
    RemoteSignatureAlgorithmV1Ed25519PublicKeyEncoding, RootTaskCreateAllocationV2,
    RootTaskCreateAllocationV2SchemaVersion, SideEffectClass, TaskCreateRequestV2,
    TaskCreateRequestV2SchemaVersion,
};
use rusqlite::Connection;
use serde_json::{json, Map};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

struct ApprovalDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl ApprovalDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        Self {
            path: directory.path().join("approval.sqlite3"),
            _directory: directory,
            config: SqliteConfig::new(Duration::from_secs(2)).expect("config"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path, self.config).expect("open")
    }

    #[allow(dead_code)]
    fn raw(&self) -> Connection {
        Connection::open(&self.path).expect("raw")
    }
}

fn uuid(number: u32) -> String {
    format!("{number:08x}-0000-4000-8000-{number:012x}")
}

fn actor() -> Actor {
    Actor {
        authentication_level: ActorAuthenticationLevel::PlatformVerified,
        confidence: Some(0.9),
        id: "actor".into(),
        kind: ActorKind::KnownUser,
        revision: 1,
        schema_version: ActorSchemaVersion,
        source: "actor-source://local/desktop".into(),
    }
}

fn root_command(number: u32) -> RootTaskCreateV2Command {
    RootTaskCreateV2Command {
        envelope: RootTaskCreateV2EnvelopeFacts {
            actor: actor(),
            entry_point: EntryPoint::LocalDesktop,
            request_id: uuid(0x1000_0000 + number),
            context: None,
            idempotency_key: format!("root-for-approval-{number}"),
        },
        request: TaskCreateRequestV2 {
            capability_hints: vec!["filesystem.read".into()],
            constraints: vec![],
            delegation_ref: None,
            goal: format!("goal {number}"),
            origin: InputContentOriginV1 {
                kind: InputContentOriginV1Kind::UserInput,
                parent_origin_refs: vec![],
                producer_ref: InputContentOriginV1ProducerRef {
                    id: "actor".into(),
                    kind: InputContentOriginV1ProducerRefKind::Actor,
                },
                schema_version: InputContentOriginV1SchemaVersion,
                source_uri: None,
                upstream_stable_id: None,
            },
            proposer: NormalizedRootTaskCreatePayloadV2Proposer::User,
            risk_hint: None,
            schema_version: TaskCreateRequestV2SchemaVersion,
            success_criteria: vec!["done".into()],
            task_scope: InputTaskScopeV1 {
                allowed_capability_hints: vec![],
                exclusions: vec![],
                expires_at: None,
                resource_patterns: vec!["https://example.com/a/**".into()],
                schema_version: InputTaskScopeV1SchemaVersion,
            },
        },
        allocation: RootTaskCreateAllocationV2 {
            audit_record_id: uuid(0x2000_0000 + number),
            content_origin_id: uuid(0x3000_0000 + number),
            correlation_id: format!("corr-root-{number}"),
            creation_provenance_id: uuid(0x4000_0000 + number),
            kernel_receipt_id: uuid(0x5000_0000 + number),
            schema_version: RootTaskCreateAllocationV2SchemaVersion,
            task_created_dedup_key: format!("dedup-root-{number}"),
            task_created_event_id: uuid(0x6000_0000 + number),
            task_id: uuid(0x7000_0000 + number),
            task_scope_id: uuid(0x8000_0000 + number),
        },
        accepted_at: Utc
            .with_ymd_and_hms(2026, 7, 22, 8, 0, number % 60)
            .unwrap(),
    }
}

fn seed_task_action(store: &SqliteStore, number: u32) -> (String, String) {
    let command = root_command(number);
    let task_id = command.allocation.task_id.clone();
    store
        .with_write_transaction(|tx| tx.create_root_task_v2(command))
        .expect("root");
    let task = store.get_task(&task_id).expect("task").expect("exists");
    let action_id = uuid(0x9000_0000 + number);
    store
        .with_write_transaction(|tx| {
            tx.insert_pending_action(InsertPendingActionCommand {
                action_id: action_id.clone(),
                task_id: task_id.clone(),
                step_id: None,
                parent_action_id: None,
                capability_id: "kernel.task".into(),
                operation: "task.child.create".into(),
                structured_arguments: Map::from_iter([("goal".into(), json!("child"))]),
                resource_refs: vec![format!("https://example.com/a/item-{number}")],
                task_scope_ref: task.task_scope_ref.clone(),
                side_effect_class: SideEffectClass::S1,
                idempotency_key: format!("approval-action-{number}"),
                execution_generation: 0,
                verification_policy: ActionRequestV2VerificationPolicyInput {
                    strategy: "kernel_local".into(),
                    expected_outcome: json!({"ok": true}),
                    timeout: "PT30S".into(),
                },
                rollback_policy: None,
                recovery_meta: None,
                created_at: Utc
                    .with_ymd_and_hms(2026, 7, 22, 9, 0, number % 60)
                    .unwrap(),
            })
        })
        .expect("action");
    (task_id, action_id)
}

fn operation_subject(task_id: &str, action_id: &str) -> ApprovalRecordV2RequestSubject {
    ApprovalRecordV2RequestSubject::Operation {
        action_id: action_id.to_owned(),
        action_revision: 1,
        capability_id: "kernel.task".into(),
        key_params_hash: "11".repeat(32),
        material_authorization_fingerprint: "22".repeat(32),
        operation: "task.child.create".into(),
        permission_decision_ref: uuid(0xa000_0001),
        permission_decision_revision: 1,
        policy_set_revision: 0,
        resource_refs_hash: "33".repeat(32),
        side_effect_class: SideEffectClass::S1,
        task_id: task_id.to_owned(),
        task_plan_version: 0,
        task_revision: 1,
    }
}

fn request_record(
    number: u32,
    chain_id: &str,
    task_id: &str,
    action_id: &str,
    mode: ConfirmationModeV1,
    challenge_ref: Option<String>,
) -> ApprovalRecordV2 {
    ApprovalRecordV2::Request {
        approval_chain_id: chain_id.to_owned(),
        created_at: "2026-07-22T10:00:00Z".into(),
        expires_at: "2026-07-22T10:05:00Z".into(),
        id: uuid(0xb000_0000 + number),
        predecessor_ref: None,
        record: ApprovalRecordV2RequestRecord {
            challenge_ref,
            confirmation_mode: mode,
            reason_codes: vec!["needs-confirm".into()],
            request_expires_at: "2026-07-22T10:05:00Z".into(),
            request_id: uuid(0xb000_0000 + number),
            requested_by_actor: actor(),
            requested_from_entry_point: EntryPoint::LocalDesktop,
        },
        schema_version: ApprovalRecordV2RequestSchemaVersion,
        subject: operation_subject(task_id, action_id),
    }
}

fn resolution_record(
    number: u32,
    chain_id: &str,
    task_id: &str,
    action_id: &str,
    request_id: &str,
    decision: ApprovalRecordV2ResolutionRecordDecision,
) -> ApprovalRecordV2 {
    ApprovalRecordV2::Resolution {
        approval_chain_id: chain_id.to_owned(),
        created_at: "2026-07-22T10:01:00Z".into(),
        expires_at: Some("2026-07-22T11:00:00Z".into()),
        id: uuid(0xc000_0000 + number),
        predecessor_ref: request_id.to_owned(),
        record: ApprovalRecordV2ResolutionRecord {
            decision,
            evidence_refs: vec![],
            local_presence_evidence_ref: None,
            remote_response_ref: None,
            request_ref: request_id.to_owned(),
            resolved_at: "2026-07-22T10:01:00Z".into(),
            resolved_by_actor: actor(),
            resolved_from_entry_point: EntryPoint::LocalDesktop,
            system_auth_evidence_ref: None,
        },
        schema_version: ApprovalRecordV2ResolutionSchemaVersion,
        subject: match operation_subject(task_id, action_id) {
            ApprovalRecordV2RequestSubject::Operation {
                action_id,
                action_revision,
                capability_id,
                key_params_hash,
                material_authorization_fingerprint,
                operation,
                permission_decision_ref,
                permission_decision_revision,
                policy_set_revision,
                resource_refs_hash,
                side_effect_class,
                task_id,
                task_plan_version,
                task_revision,
            } => ApprovalRecordV2ResolutionSubject::Operation {
                action_id,
                action_revision,
                capability_id,
                key_params_hash,
                material_authorization_fingerprint,
                operation,
                permission_decision_ref,
                permission_decision_revision,
                policy_set_revision,
                resource_refs_hash,
                side_effect_class,
                task_id,
                task_plan_version,
                task_revision,
            },
            _ => unreachable!(),
        },
    }
}

fn invalidation_record(
    number: u32,
    chain_id: &str,
    task_id: &str,
    action_id: &str,
    head_ref: &str,
    replacement_ref: Option<String>,
) -> ApprovalRecordV2 {
    ApprovalRecordV2::Invalidation {
        approval_chain_id: chain_id.to_owned(),
        created_at: "2026-07-22T10:02:00Z".into(),
        expires_at: kernel_contracts::NullOnly,
        id: uuid(0xd000_0000 + number),
        predecessor_ref: head_ref.to_owned(),
        record: ApprovalRecordV2InvalidationRecord {
            invalidated_at: "2026-07-22T10:02:00Z".into(),
            invalidated_by_actor: Some(actor()),
            invalidated_from_entry_point: EntryPoint::LocalDesktop,
            invalidated_record_ref: head_ref.to_owned(),
            reason_code: ApprovalRecordV2InvalidationRecordReasonCode::ManualRevocation,
            replacement_request_ref: replacement_ref,
        },
        schema_version: ApprovalRecordV2InvalidationSchemaVersion,
        subject: match operation_subject(task_id, action_id) {
            ApprovalRecordV2RequestSubject::Operation {
                action_id,
                action_revision,
                capability_id,
                key_params_hash,
                material_authorization_fingerprint,
                operation,
                permission_decision_ref,
                permission_decision_revision,
                policy_set_revision,
                resource_refs_hash,
                side_effect_class,
                task_id,
                task_plan_version,
                task_revision,
            } => ApprovalRecordV2InvalidationSubject::Operation {
                action_id,
                action_revision,
                capability_id,
                key_params_hash,
                material_authorization_fingerprint,
                operation,
                permission_decision_ref,
                permission_decision_revision,
                policy_set_revision,
                resource_refs_hash,
                side_effect_class,
                task_id,
                task_plan_version,
                task_revision,
            },
            _ => unreachable!(),
        },
    }
}

fn event_allocation(number: u32) -> ApprovalEventAllocationV1 {
    ApprovalEventAllocationV1 {
        causation_ref: CausationRefV2::CommandRequest {
            id: uuid(0xe000_0000 + number),
        },
        changed_at: "2026-07-22T10:00:00Z".into(),
        correlation_id: format!("corr-approval-{number}"),
        dedup_key: format!("dedup-approval-{number}"),
        event_id: uuid(0xf000_0000 + number),
    }
}

fn seed_chain(store: &SqliteStore, number: u32) -> (String, String, String, ApprovalRecordV2) {
    let (task_id, action_id) = seed_task_action(store, number);
    let chain_id = uuid(0x1100_0000 + number);
    let request = request_record(
        number,
        &chain_id,
        &task_id,
        &action_id,
        ConfirmationModeV1::Generic,
        None,
    );
    let result = store
        .with_write_transaction(|tx| {
            tx.append_request(AppendApprovalRequestCommand {
                request: request.clone(),
                event_allocation: event_allocation(number),
                audit_record_id: uuid(0x1200_0000 + number),
            })
        })
        .expect("append_request");
    assert_eq!(result.current_head_ref, uuid(0xb000_0000 + number));
    (task_id, action_id, chain_id, request)
}

#[test]
fn append_request_creates_chain_event_audit_and_binds_action() {
    let database = ApprovalDatabase::new();
    let store = database.open();
    let (_task_id, action_id, chain_id, request) = seed_chain(&store, 1);

    let head = store
        .get_approval_chain_head(&chain_id)
        .expect("head")
        .expect("exists");
    assert_eq!(head.current_head_ref, uuid(0xb000_0001));
    assert_eq!(head.head_record_kind, "request");

    // Action bound to the chain (operation subject), with revision bumped as a fact change.
    let action = store
        .get_action(&action_id)
        .expect("action")
        .expect("exists");
    assert_eq!(action.approval_chain_id.as_deref(), Some(chain_id.as_str()));
    assert_eq!(action.revision, 2);

    // One approval.state_changed event with initial_request payload.
    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("outbox");
    assert_eq!(events.len(), 2); // root task.created + approval.state_changed
    let StoredEventEnvelope::ActiveV2(envelope) = &events[1].envelope;
    assert_eq!(envelope.aggregate_id, chain_id);
    let EventEnvelopeV2Payload::ApprovalStateChanged(payload) = &envelope.payload else {
        panic!("expected approval payload");
    };
    assert_eq!(
        payload.change_kind,
        ApprovalStateChangedPayloadV1ChangeKind::InitialRequest
    );
    assert_eq!(payload.from_head_ref, None);
    assert_eq!(payload.to_head_ref, uuid(0xb000_0001));
    assert_eq!(payload.to_record_kind, ApprovalRecordKindV2::Request);
    assert_eq!(
        payload.request_ref.as_deref(),
        Some(uuid(0xb000_0001).as_str())
    );
    assert_eq!(payload.resolution_ref, None);
    assert_eq!(payload.subject_kind, ApprovalSubjectKindV2::Operation);
    assert_eq!(payload.confirmation_mode, ConfirmationModeV1::Generic);
    assert_eq!(payload.action_id.as_deref(), Some(action_id.as_str()));

    // approval.requested audit.
    assert_eq!(
        result_audit_type(&store, &uuid(0x1200_0001)),
        AuditRecordV2AuditType::ApprovalRequested
    );

    // Request record canonical readback.
    let stored = store
        .get_approval_record(&uuid(0xb000_0001))
        .expect("record")
        .expect("exists");
    assert_eq!(stored, request);
}

fn result_audit_type(store: &SqliteStore, audit_id: &str) -> AuditRecordV2AuditType {
    store
        .get_audit_v2(audit_id)
        .expect("audit")
        .expect("exists")
        .audit_type
}

#[test]
fn append_request_rejects_existing_chain_and_predecessor() {
    let database = ApprovalDatabase::new();
    let store = database.open();
    let (task_id, action_id, chain_id, request) = seed_chain(&store, 2);

    // Same chain again -> head_conflict.
    let conflict = store
        .with_write_transaction(|tx| {
            tx.append_request(AppendApprovalRequestCommand {
                request: request_record(
                    2,
                    &chain_id,
                    &task_id,
                    &action_id,
                    ConfirmationModeV1::Generic,
                    None,
                ),
                event_allocation: event_allocation(102),
                audit_record_id: uuid(0x1200_0102),
            })
        })
        .expect_err("existing chain");
    assert_eq!(conflict.code, StoreErrorCode::ConstraintViolation);

    // Request carrying predecessor is rejected even on a fresh chain.
    let mut bad = request.clone();
    let ApprovalRecordV2::Request {
        predecessor_ref, ..
    } = &mut bad
    else {
        panic!()
    };
    *predecessor_ref = Some(uuid(0xdead));
    let rejected = store
        .with_write_transaction(|tx| {
            tx.append_request(AppendApprovalRequestCommand {
                request: bad,
                event_allocation: event_allocation(103),
                audit_record_id: uuid(0x1200_0103),
            })
        })
        .expect_err("predecessor");
    assert_eq!(rejected.code, StoreErrorCode::ContractInvalid);
}

#[test]
fn resolve_advances_head_with_event_and_audit() {
    let database = ApprovalDatabase::new();
    let store = database.open();
    let (task_id, action_id, chain_id, _request) = seed_chain(&store, 3);
    let request_id = uuid(0xb000_0003);

    let resolution = resolution_record(
        3,
        &chain_id,
        &task_id,
        &action_id,
        &request_id,
        ApprovalRecordV2ResolutionRecordDecision::Approved,
    );
    let result = store
        .with_write_transaction(|tx| {
            tx.resolve(ResolveApprovalCommand {
                expected_head_ref: request_id.clone(),
                resolution: resolution.clone(),
                evidence: ResolutionEvidence::Generic,
                event_allocation: event_allocation(0x103),
                audit_record_id: uuid(0x1200_0103),
            })
        })
        .expect("resolve");
    assert_eq!(result.current_head_ref, uuid(0xc000_0003));

    let head = store
        .get_approval_chain_head(&chain_id)
        .expect("head")
        .expect("exists");
    assert_eq!(head.head_record_kind, "resolution");

    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("outbox");
    assert_eq!(events.len(), 3);
    let StoredEventEnvelope::ActiveV2(envelope) = &events[2].envelope;
    let EventEnvelopeV2Payload::ApprovalStateChanged(payload) = &envelope.payload else {
        panic!()
    };
    assert_eq!(
        payload.change_kind,
        ApprovalStateChangedPayloadV1ChangeKind::Resolution
    );
    assert_eq!(payload.from_head_ref.as_deref(), Some(request_id.as_str()));
    assert_eq!(payload.to_head_ref, uuid(0xc000_0003));
    assert_eq!(
        payload.resolution_ref.as_deref(),
        Some(uuid(0xc000_0003).as_str())
    );
    assert_eq!(payload.invalidation_ref, None);

    assert_eq!(
        result_audit_type(&store, &uuid(0x1200_0103)),
        AuditRecordV2AuditType::ApprovalResolved
    );
    let stored = store
        .get_approval_record(&uuid(0xc000_0003))
        .expect("record")
        .expect("exists");
    assert_eq!(stored, resolution);
}

#[test]
fn resolve_head_conflict_and_subject_mismatch_fail_closed() {
    let database = ApprovalDatabase::new();
    let store = database.open();
    let (task_id, action_id, chain_id, _request) = seed_chain(&store, 4);
    let request_id = uuid(0xb000_0004);

    // Wrong expected head.
    let conflict = store
        .with_write_transaction(|tx| {
            tx.resolve(ResolveApprovalCommand {
                expected_head_ref: uuid(0xdead),
                resolution: resolution_record(
                    4,
                    &chain_id,
                    &task_id,
                    &action_id,
                    &request_id,
                    ApprovalRecordV2ResolutionRecordDecision::Approved,
                ),
                evidence: ResolutionEvidence::Generic,
                event_allocation: event_allocation(0x104),
                audit_record_id: uuid(0x1200_0104),
            })
        })
        .expect_err("head conflict");
    assert_eq!(conflict.code, StoreErrorCode::ConstraintViolation);

    // Subject mismatch (different task).
    let (other_task, _other_action) = seed_task_action(&store, 0x104);
    let mismatched = resolution_record(
        4,
        &chain_id,
        &other_task,
        &action_id,
        &request_id,
        ApprovalRecordV2ResolutionRecordDecision::Approved,
    );
    let mismatch = store
        .with_write_transaction(|tx| {
            tx.resolve(ResolveApprovalCommand {
                expected_head_ref: request_id.clone(),
                resolution: mismatched,
                evidence: ResolutionEvidence::Generic,
                event_allocation: event_allocation(0x204),
                audit_record_id: uuid(0x1200_0204),
            })
        })
        .expect_err("subject mismatch");
    assert_eq!(mismatch.code, StoreErrorCode::ContractInvalid);

    // Head unchanged after failures.
    let head = store
        .get_approval_chain_head(&chain_id)
        .expect("head")
        .expect("exists");
    assert_eq!(head.current_head_ref, request_id);
}

#[test]
fn invalidate_without_and_with_replacement() {
    let database = ApprovalDatabase::new();
    let store = database.open();
    let (task_id, action_id, chain_id, _request) = seed_chain(&store, 5);
    let request_id = uuid(0xb000_0005);

    // Without replacement.
    let invalidation = invalidation_record(5, &chain_id, &task_id, &action_id, &request_id, None);
    let result = store
        .with_write_transaction(|tx| {
            tx.invalidate_and_optionally_replace(InvalidateApprovalCommand {
                expected_head_ref: request_id.clone(),
                invalidation: invalidation.clone(),
                replacement: None,
                event_allocation: event_allocation(0x105),
                audit_record_id: uuid(0x1200_0105),
            })
        })
        .expect("invalidate");
    assert_eq!(result.current_head_ref, uuid(0xd000_0005));
    let head = store
        .get_approval_chain_head(&chain_id)
        .expect("head")
        .expect("exists");
    assert_eq!(head.head_record_kind, "invalidation");

    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("outbox");
    let StoredEventEnvelope::ActiveV2(envelope) = &events[2].envelope;
    let EventEnvelopeV2Payload::ApprovalStateChanged(payload) = &envelope.payload else {
        panic!()
    };
    assert_eq!(
        payload.change_kind,
        ApprovalStateChangedPayloadV1ChangeKind::InvalidationWithoutReplacement
    );
    assert_eq!(payload.replacement_request_ref, None);
    assert_eq!(
        result_audit_type(&store, &uuid(0x1200_0105)),
        AuditRecordV2AuditType::ApprovalInvalidated
    );

    // Second chain: invalidate with replacement.
    let (task2, action2, chain2, _request2) = seed_chain(&store, 0x205);
    let request2_id = uuid(0xb000_0205);
    let replacement_id = uuid(0xb000_0305);
    let invalidation2 = invalidation_record(
        0x205,
        &chain2,
        &task2,
        &action2,
        &request2_id,
        Some(replacement_id.clone()),
    );
    let mut replacement = request_record(
        0x305,
        &chain2,
        &task2,
        &action2,
        ConfirmationModeV1::Generic,
        None,
    );
    let ApprovalRecordV2::Request {
        id,
        predecessor_ref,
        ..
    } = &mut replacement
    else {
        panic!()
    };
    *id = replacement_id.clone();
    *predecessor_ref = Some(uuid(0xd000_0205));
    let ApprovalRecordV2::Request { record, .. } = &mut replacement else {
        panic!()
    };
    record.request_id = replacement_id.clone();

    let result2 = store
        .with_write_transaction(|tx| {
            tx.invalidate_and_optionally_replace(InvalidateApprovalCommand {
                expected_head_ref: request2_id.clone(),
                invalidation: invalidation2,
                replacement: Some(replacement),
                event_allocation: event_allocation(0x305),
                audit_record_id: uuid(0x1200_0305),
            })
        })
        .expect("replacement");
    assert_eq!(result2.current_head_ref, replacement_id);
    let head2 = store
        .get_approval_chain_head(&chain2)
        .expect("head")
        .expect("exists");
    assert_eq!(head2.head_record_kind, "request");

    let events2 = store
        .read_after(OutboxCursor::START, PageLimit::new(20).expect("limit"))
        .expect("outbox");
    let last = events2.last().expect("last");
    let StoredEventEnvelope::ActiveV2(envelope2) = &last.envelope;
    let EventEnvelopeV2Payload::ApprovalStateChanged(payload2) = &envelope2.payload else {
        panic!()
    };
    assert_eq!(
        payload2.change_kind,
        ApprovalStateChangedPayloadV1ChangeKind::ReplacementRequest
    );
    assert_eq!(payload2.to_record_kind, ApprovalRecordKindV2::Request);
    assert_eq!(
        payload2.replacement_request_ref.as_deref(),
        Some(replacement_id.as_str())
    );
    assert_eq!(
        payload2.invalidation_ref.as_deref(),
        Some(uuid(0xd000_0205).as_str())
    );
}

#[test]
fn credential_lifecycle_register_rotate_revoke() {
    let database = ApprovalDatabase::new();
    let store = database.open();

    let credential = CredentialRefV1 {
        actor_ref: "actor".into(),
        credential_id: uuid(0x2000_0001),
        credential_revision: 1,
        expires_at: "2027-01-01T00:00:00Z".into(),
        issuer_ref: "issuer".into(),
        not_before: "2026-01-01T00:00:00Z".into(),
        replaced_by_ref: None,
        schema_version: CredentialRefV1SchemaVersion,
        signature_algorithm: RemoteSignatureAlgorithmV1::Ed25519 {
            public_key: "a".repeat(43),
            public_key_encoding: RemoteSignatureAlgorithmV1Ed25519PublicKeyEncoding::Value,
        },
        status: CredentialRefV1Status::Active,
    };
    let registered = store
        .with_write_transaction(|tx| tx.register_credential(credential.clone()))
        .expect("register");
    assert_eq!(registered, credential);

    // Duplicate register rejected.
    assert!(store
        .with_write_transaction(|tx| tx.register_credential(credential.clone()))
        .is_err());

    // Rotate.
    let mut next = credential.clone();
    next.credential_revision = 2;
    let rotated = store
        .with_write_transaction(|tx| {
            tx.rotate_credential(
                1,
                next.clone(),
                Utc.with_ymd_and_hms(2026, 7, 22, 10, 3, 0).unwrap(),
            )
        })
        .expect("rotate");
    assert_eq!(rotated.status, CredentialRefV1Status::Active);
    let old = store
        .get_credential(&credential.credential_id, Some(1))
        .expect("old")
        .expect("exists");
    assert_eq!(old.status, CredentialRefV1Status::Revoked);
    assert_eq!(
        old.replaced_by_ref.as_deref(),
        Some(credential.credential_id.as_str())
    );

    // Revoke.
    let revoked = store
        .with_write_transaction(|tx| {
            tx.revoke_credential(
                &credential.credential_id,
                2,
                Utc.with_ymd_and_hms(2026, 7, 22, 10, 3, 0).unwrap(),
            )
        })
        .expect("revoke");
    assert_eq!(revoked.status, CredentialRefV1Status::Revoked);

    // Terminal: rotate/revoke again fails.
    assert!(store
        .with_write_transaction(|tx| tx.rotate_credential(
            2,
            next,
            Utc.with_ymd_and_hms(2026, 7, 22, 10, 3, 0).unwrap()
        ))
        .is_err());
    assert!(store
        .with_write_transaction(|tx| tx.revoke_credential(
            &credential.credential_id,
            2,
            Utc.with_ymd_and_hms(2026, 7, 22, 10, 3, 0).unwrap()
        ))
        .is_err());
}

#[test]
fn challenge_terminal_discipline_and_expire_audit() {
    let database = ApprovalDatabase::new();
    let store = database.open();
    let (task_id, _action_id, chain_id, _request) = seed_chain(&store, 6);

    let challenge = RemoteApprovalChallengeV1 {
        allowed_decisions: RemoteApprovalChallengeV1AllowedDecisions,
        approval_chain_id: chain_id.clone(),
        audience: "https://approval.example.com".into(),
        challenge_id: uuid(0x3000_0006),
        consumed_at: None,
        credential_ref: CredentialRefV1 {
            actor_ref: "actor".into(),
            credential_id: uuid(0x3000_0007),
            credential_revision: 1,
            expires_at: "2027-01-01T00:00:00Z".into(),
            issuer_ref: "issuer".into(),
            not_before: "2026-01-01T00:00:00Z".into(),
            replaced_by_ref: None,
            schema_version: CredentialRefV1SchemaVersion,
            signature_algorithm: RemoteSignatureAlgorithmV1::Ed25519 {
                public_key: "b".repeat(43),
                public_key_encoding: RemoteSignatureAlgorithmV1Ed25519PublicKeyEncoding::Value,
            },
            status: CredentialRefV1Status::Active,
        },
        expires_at: "2026-07-22T10:05:00Z".into(),
        issued_at: "2026-07-22T10:00:00Z".into(),
        material_authorization_fingerprint: "22".repeat(32),
        nonce: "n".repeat(43),
        nonce_encoding: RemoteApprovalChallengeV1NonceEncoding::Value,
        request_ref: uuid(0xb000_0006),
        revocation_reason: None,
        revoked_at: None,
        schema_version: RemoteApprovalChallengeV1SchemaVersion,
        state: RemoteApprovalChallengeV1State::Issued,
        subject_hash: "44".repeat(32),
        task_id: task_id.clone(),
    };
    let issued = store
        .with_write_transaction(|tx| {
            tx.issue_challenge(StoredChallenge::Remote(Box::new(challenge.clone())))
        })
        .expect("issue");
    assert_eq!(issued, StoredChallenge::Remote(Box::new(challenge.clone())));

    // Consume.
    let consumed = store
        .with_write_transaction(|tx| {
            tx.consume_challenge(
                &challenge.challenge_id,
                Utc.with_ymd_and_hms(2026, 7, 22, 10, 3, 0).unwrap(),
            )
        })
        .expect("consume");
    assert_eq!(
        consumed.state(),
        RemoteApprovalChallengeV1State::Consumed.as_str()
    );

    // Terminal: consume/expire/revoke all rejected.
    assert!(store
        .with_write_transaction(|tx| tx.consume_challenge(
            &challenge.challenge_id,
            Utc.with_ymd_and_hms(2026, 7, 22, 10, 3, 0).unwrap()
        ))
        .is_err());
    assert!(store
        .with_write_transaction(|tx| {
            tx.expire_challenge_with_expected_state(
                &challenge.challenge_id,
                Utc.with_ymd_and_hms(2026, 7, 22, 10, 3, 0).unwrap(),
                kernel_contracts::AuditAllocationV2 {
                    audit_record_id: uuid(0x3000_0008),
                    causation_ref: CausationRefV2::CommandRequest {
                        id: uuid(0x3000_0009),
                    },
                    correlation_id: "corr-expire".into(),
                    occurred_at: "2026-07-22T10:06:00Z".into(),
                },
                EntryPoint::LocalDesktop,
                Some(actor()),
            )
        })
        .is_err());

    // Fresh challenge: expire writes identity.challenge_expired audit, no approval event.
    let mut challenge2 = challenge.clone();
    challenge2.challenge_id = uuid(0x3000_0016);
    challenge2.nonce = "m".repeat(43);
    challenge2.request_ref = uuid(0x3000_0017);
    let challenge2_id = challenge2.challenge_id.clone();
    store
        .with_write_transaction(|tx| {
            tx.issue_challenge(StoredChallenge::Remote(Box::new(challenge2)))
        })
        .expect("issue2");
    let before_events = store
        .read_after(OutboxCursor::START, PageLimit::new(20).expect("limit"))
        .expect("outbox")
        .len();
    let expired = store
        .with_write_transaction(|tx| {
            tx.expire_challenge_with_expected_state(
                &challenge2_id,
                Utc.with_ymd_and_hms(2026, 7, 22, 10, 6, 0).unwrap(),
                kernel_contracts::AuditAllocationV2 {
                    audit_record_id: uuid(0x3000_0018),
                    causation_ref: CausationRefV2::CommandRequest {
                        id: uuid(0x3000_0019),
                    },
                    correlation_id: "corr-expire-2".into(),
                    occurred_at: "2026-07-22T10:06:00Z".into(),
                },
                EntryPoint::LocalDesktop,
                Some(actor()),
            )
        })
        .expect("expire");
    assert_eq!(
        expired.audit.audit_type,
        AuditRecordV2AuditType::IdentityChallengeExpired
    );
    let after_events = store
        .read_after(OutboxCursor::START, PageLimit::new(20).expect("limit"))
        .expect("outbox")
        .len();
    assert_eq!(before_events, after_events, "expiry must not emit events");
}

#[test]
fn local_presence_evidence_immutable() {
    let database = ApprovalDatabase::new();
    let store = database.open();
    let evidence = LocalPresenceEvidenceV1 {
        challenge_ref: None,
        entry_point: EntryPoint::LocalDesktop,
        evidence_hash: "55".repeat(32),
        id: uuid(0x4000_0001),
        observed_actor: actor(),
        observed_at: "2026-07-22T10:00:00Z".into(),
        peer_principal_ref: None,
        presence_kind: LocalPresenceEvidenceV1PresenceKind::InteractiveSession,
        schema_version: LocalPresenceEvidenceV1SchemaVersion,
        session_ref: "session-1".into(),
        transport_kind: LocalPresenceEvidenceV1TransportKind::UnixPeer,
        valid_until: "2026-07-22T12:00:00Z".into(),
        verifier_kind: LocalPresenceEvidenceV1VerifierKind::KernelTransport,
    };
    let inserted = store
        .with_write_transaction(|tx| tx.insert_local_presence(evidence.clone()))
        .expect("insert");
    assert_eq!(inserted, evidence.clone());
    let stored = store
        .get_local_presence(&evidence.id)
        .expect("get")
        .expect("exists");
    assert_eq!(stored, evidence);
    // Duplicate insert rejected (immutable unique id).
    assert!(store
        .with_write_transaction(|tx| tx.insert_local_presence(evidence.clone()))
        .is_err());
}
