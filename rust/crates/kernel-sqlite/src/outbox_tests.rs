use super::*;
use chrono::{TimeZone, Utc};
use kernel_contracts::{
    ActionStateChangedPayloadV1, ActionStateChangedPayloadV1SchemaVersion, ActionStatus,
    ApprovalRecordKindV2, ApprovalStateChangedPayloadV1, ApprovalStateChangedPayloadV1ChangeKind,
    ApprovalStateChangedPayloadV1SchemaVersion, ApprovalSubjectKindV2, CausationRefV2,
    ConfirmationModeV1, EntryPoint, EventEnvelopeV2Payload, StopFenceActivatedPayload,
    StopFenceActivatedPayloadSchemaVersion, TaskCreatedPayload, TaskCreatedPayloadProposer,
    TaskCreatedPayloadSchemaVersion, TaskStateChangedPayload, TaskStateChangedPayloadSchemaVersion,
    TaskStatus,
};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

struct OutboxDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl OutboxDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("directory");
        Self {
            path: directory.path().join("outbox.sqlite3"),
            _directory: directory,
            config: SqliteConfig::new(Duration::from_secs(2)).expect("config"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path, self.config).expect("open")
    }
}

#[test]
fn active_v2_five_type_matrix_proves_sequence_position_and_shared_stream() {
    let database = OutboxDatabase::new();
    let store = database.open();
    let task_id = uuid(1);
    let action_id = uuid(2);
    let approval_id = uuid(3);
    // Five active types each appear at least once; task aggregate spans created+state_changed;
    // stop_fence aggregate spans two activations; positions are one shared AUTOINCREMENT stream.
    let records = store
        .with_write_transaction(|transaction| {
            Ok(vec![
                transaction.append_active_event_v2(active_task_created(task_id, 1))?,
                transaction.append_active_event_v2(active_task_state_changed(task_id, 2))?,
                transaction.append_active_event_v2(active_stop(3))?,
                transaction.append_active_event_v2(active_stop(4))?,
                transaction.append_active_event_v2(active_action(action_id, task_id, 5))?,
                transaction.append_active_event_v2(active_approval(approval_id, 6))?,
            ])
        })
        .expect("active append");
    assert_eq!(records.len(), 6);

    // Per-type sequence/position proofs (indexes match append order above).
    assert_eq!(active_type(&records[0]), "task.created");
    assert_eq!(records[0].envelope.sequence(), 0);
    assert_eq!(records[0].envelope.outbox_position(), "1");
    assert_eq!(active_type(&records[1]), "task.state_changed");
    assert_eq!(records[1].envelope.sequence(), 1);
    assert_eq!(records[1].envelope.outbox_position(), "2");
    assert_eq!(active_type(&records[2]), "stop_fence.activated");
    assert_eq!(records[2].envelope.sequence(), 0);
    assert_eq!(records[2].envelope.outbox_position(), "3");
    assert_eq!(active_type(&records[3]), "stop_fence.activated");
    assert_eq!(records[3].envelope.sequence(), 1);
    assert_eq!(records[3].envelope.outbox_position(), "4");
    assert_eq!(active_type(&records[4]), "action.state_changed");
    assert_eq!(records[4].envelope.sequence(), 0);
    assert_eq!(records[4].envelope.outbox_position(), "5");
    assert_eq!(active_type(&records[5]), "approval.state_changed");
    assert_eq!(records[5].envelope.sequence(), 0);
    assert_eq!(records[5].envelope.outbox_position(), "6");

    // Same task aggregate: multi-type sequence strictly increases on one stream.
    assert_eq!(active_aggregate_id(&records[0]), task_id.to_string());
    assert_eq!(active_aggregate_id(&records[1]), task_id.to_string());
    assert!(records[0].envelope.sequence() < records[1].envelope.sequence());
    // Same stop_fence aggregate: two events, sequence 0 then 1.
    assert_eq!(active_aggregate_id(&records[2]), "global");
    assert_eq!(active_aggregate_id(&records[3]), "global");
    assert_eq!(
        records[2].envelope.sequence() + 1,
        records[3].envelope.sequence()
    );

    for (index, record) in records.iter().enumerate() {
        assert_eq!(record.envelope.outbox_position(), (index + 1).to_string());
        assert!(matches!(record.envelope, StoredEventEnvelope::ActiveV2(_)));
    }

    let page = store
        .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
        .expect("read page");
    assert_eq!(page, records);
    assert_eq!(
        store
            .latest_position()
            .expect("latest")
            .expect("non-empty")
            .get(),
        6
    );
}

#[test]
fn active_aggregate_mismatch_fails_before_allocations_and_causes_no_holes() {
    let database = OutboxDatabase::new();
    let store = database.open();
    let task_id = uuid(10);
    let wrong = uuid(11);
    store
        .with_write_transaction(|transaction| {
            let mut pending = active_task_created(task_id, 1);
            pending.aggregate_id = EventAggregateId::Task(wrong);
            assert_eq!(
                transaction
                    .append_active_event_v2(pending)
                    .expect_err("mismatch")
                    .code,
                StoreErrorCode::ContractInvalid
            );
            Ok(())
        })
        .expect("caller swallowed prevalidation error");
    let record = store
        .with_write_transaction(|transaction| {
            transaction.append_active_event_v2(active_task_created(task_id, 2))
        })
        .expect("valid append");
    assert_eq!(record.envelope.sequence(), 0);
    assert_eq!(record.envelope.outbox_position(), "1");
}

#[test]
fn corrupt_payload_causation_version_timestamp_or_relation_fails_whole_page() {
    for mutation in [
        "UPDATE outbox SET payload_json = '{\"schema_version\":1,\"task_id\":\"bad\"}' WHERE outbox_position = 1",
        "UPDATE outbox SET causation_json = '{\"kind\":\"command_request\", \"id\":\"11111111-1111-4111-8111-111111111111\"}' WHERE outbox_position = 1",
        "PRAGMA ignore_check_constraints=ON; UPDATE outbox SET schema_version = 9 WHERE outbox_position = 1; PRAGMA ignore_check_constraints=OFF",
        "PRAGMA ignore_check_constraints=ON; UPDATE outbox SET schema_version = 1 WHERE outbox_position = 1; PRAGMA ignore_check_constraints=OFF",
        "UPDATE outbox SET occurred_at = 'not-a-time' WHERE outbox_position = 1",
        "PRAGMA ignore_check_constraints=ON; UPDATE outbox SET aggregate_id = '22222222-2222-4222-8222-222222222222' WHERE outbox_position = 1; PRAGMA ignore_check_constraints=OFF",
    ] {
        let database = OutboxDatabase::new();
        let store = database.open();
        let task_id = uuid(20);
        store
            .with_write_transaction(|transaction| {
                transaction.append_active_event_v2(active_task_created(task_id, 1))?;
                transaction.append_active_event_v2(active_task_state_changed(task_id, 2))?;
                Ok(())
            })
            .expect("append page");
        database
            .open()
            .lock_connection()
            .expect("connection")
            .execute_batch(mutation)
            .expect("mutate");
        assert_eq!(
            store
                .read_after(OutboxCursor::START, PageLimit::new(10).expect("limit"))
                .expect_err("whole page fails")
                .code,
            StoreErrorCode::StoredDataInvalid,
            "mutation: {mutation}"
        );
    }
}

#[test]
fn mark_delivered_validates_corrupt_row_before_update() {
    let database = OutboxDatabase::new();
    let store = database.open();
    let task_id = uuid(30);
    let record = store
        .with_write_transaction(|transaction| {
            transaction.append_active_event_v2(active_task_created(task_id, 1))
        })
        .expect("append");
    database
        .open()
        .lock_connection()
        .expect("connection")
        .execute(
            "UPDATE outbox SET payload_json = ?1 WHERE outbox_position = 1",
            [r#"{"schema_version":1}"#],
        )
        .expect("corrupt");
    let position =
        OutboxPosition::new(record.envelope.outbox_position().parse().expect("position"))
            .expect("position type");
    assert_eq!(
        store
            .mark_delivered(position, Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap(),)
            .expect_err("corruption must block mark")
            .code,
        StoreErrorCode::StoredDataInvalid
    );
    let delivered: Option<String> = database
        .open()
        .lock_connection()
        .expect("connection")
        .query_row(
            "SELECT delivered_at FROM outbox WHERE outbox_position = 1",
            [],
            |row| row.get(0),
        )
        .expect("delivery state");
    assert_eq!(delivered, None);
}

fn active_type(record: &OutboxRecord) -> &str {
    match &record.envelope {
        StoredEventEnvelope::ActiveV2(envelope) => envelope.type_.as_str(),
    }
}

fn active_aggregate_id(record: &OutboxRecord) -> String {
    match &record.envelope {
        StoredEventEnvelope::ActiveV2(envelope) => envelope.aggregate_id.clone(),
    }
}

fn active_task_created(task_id: Uuid, number: u32) -> PendingActiveEventV2 {
    active_pending(
        number,
        EventAggregateId::Task(task_id),
        EventEnvelopeV2Payload::TaskCreated(Box::new(task_created_payload(
            task_id,
            instant(number),
        ))),
    )
}

fn active_task_state_changed(task_id: Uuid, number: u32) -> PendingActiveEventV2 {
    active_pending(
        number,
        EventAggregateId::Task(task_id),
        EventEnvelopeV2Payload::TaskStateChanged(Box::new(TaskStateChangedPayload {
            changed_at: instant(number).to_rfc3339(),
            from_status: TaskStatus::Candidate,
            reason_code: "planned".to_owned(),
            schema_version: TaskStateChangedPayloadSchemaVersion,
            task_id: task_id.to_string(),
            task_revision: 2,
            to_status: TaskStatus::Planned,
        })),
    )
}

fn active_action(action_id: Uuid, task_id: Uuid, number: u32) -> PendingActiveEventV2 {
    active_pending(
        number,
        EventAggregateId::Action(action_id),
        EventEnvelopeV2Payload::ActionStateChanged(Box::new(ActionStateChangedPayloadV1 {
            action_id: action_id.to_string(),
            action_revision: 2,
            approval_resolution_ref: None,
            changed_at: instant(number).to_rfc3339(),
            execution_generation: 0,
            from_status: ActionStatus::Pending,
            materialized_child_task_ref: None,
            permission_decision_ref: None,
            reason_code: "approved".to_owned(),
            schema_version: ActionStateChangedPayloadV1SchemaVersion,
            task_id: task_id.to_string(),
            to_status: ActionStatus::Approved,
            verification_result_refs: vec![],
        })),
    )
}

fn active_approval(approval_id: Uuid, number: u32) -> PendingActiveEventV2 {
    let request = uuid(40).to_string();
    active_pending(
        number,
        EventAggregateId::ApprovalChain(approval_id),
        EventEnvelopeV2Payload::ApprovalStateChanged(Box::new(ApprovalStateChangedPayloadV1 {
            action_id: None,
            approval_chain_id: approval_id.to_string(),
            change_kind: ApprovalStateChangedPayloadV1ChangeKind::InitialRequest,
            changed_at: instant(number).to_rfc3339(),
            confirmation_mode: ConfirmationModeV1::Generic,
            from_head_ref: None,
            from_record_kind: None,
            invalidation_ref: None,
            permission_decision_ref: None,
            reason_code: "requested".to_owned(),
            replacement_request_ref: None,
            request_ref: Some(request.clone()),
            resolution_ref: None,
            schema_version: ApprovalStateChangedPayloadV1SchemaVersion,
            subject_kind: ApprovalSubjectKindV2::TaskProposal,
            to_head_ref: request,
            to_record_kind: ApprovalRecordKindV2::Request,
        })),
    )
}

fn active_stop(number: u32) -> PendingActiveEventV2 {
    active_pending(
        number,
        EventAggregateId::StopFenceGlobal,
        EventEnvelopeV2Payload::StopFenceActivated(Box::new(StopFenceActivatedPayload {
            activated_at: instant(number).to_rfc3339(),
            activated_by_actor_id: uuid(50).to_string(),
            activated_from_entry_point: EntryPoint::SystemInternal,
            generation: 1,
            reason: "test".to_owned(),
            schema_version: StopFenceActivatedPayloadSchemaVersion,
        })),
    )
}

fn active_pending(
    number: u32,
    aggregate_id: EventAggregateId,
    payload: EventEnvelopeV2Payload,
) -> PendingActiveEventV2 {
    PendingActiveEventV2 {
        event_id: event_id(number),
        aggregate_id,
        occurred_at: instant(number),
        causation_ref: CausationRefV2::CommandRequest {
            id: uuid(90).to_string(),
        },
        correlation_id: "active-correlation".to_owned(),
        dedup_key: format!("active-dedup-{number}"),
        payload,
    }
}

fn task_created_payload(task_id: Uuid, instant: chrono::DateTime<Utc>) -> TaskCreatedPayload {
    TaskCreatedPayload {
        created_at: instant.to_rfc3339(),
        goal: "active outbox task".to_owned(),
        proposer: TaskCreatedPayloadProposer::User,
        schema_version: TaskCreatedPayloadSchemaVersion,
        status: TaskStatus::Candidate,
        task_id: task_id.to_string(),
        task_revision: 1,
    }
}

fn instant(number: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, number).unwrap()
}

fn uuid(number: u128) -> Uuid {
    Uuid::from_u128(0x1000_0000_0000_4000_8000_0000_0000_0000 + number)
}

fn event_id(number: u32) -> Uuid {
    uuid(1000 + number as u128)
}
