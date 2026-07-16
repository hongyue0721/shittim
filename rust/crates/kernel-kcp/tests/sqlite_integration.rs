use chrono::{TimeZone, Utc};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, AuditRecordAuditType,
    CausationRefKind, EntryPoint, EventPayload, KcpCommandEnvelopeMessageKind,
    KcpCommandEnvelopeProtocolVersion, KcpCommandPayload, NullOnly, TaskCreateRequest,
    TypedKcpCommandEnvelope,
};
use kernel_kcp::{
    handle_task_create, handle_task_get, sqlite_adapter::SqliteTaskBackend, ClockError,
    HandlerResult, IdGenerationError, KernelClock, KernelIdGenerator, OpaqueIdPurpose,
    PostCommitNotificationIntent, UuidPurpose,
};
use kernel_sqlite::{OutboxCursor, PageLimit, SqliteConfig, SqliteStore};
use serde_json::json;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;
use tempfile::TempDir;

const TASK_ID: &str = "00000000-0000-4000-8000-000000000001";
const SCOPE_ID: &str = "00000000-0000-4000-8000-000000000002";
const ORIGIN_ID: &str = "00000000-0000-4000-8000-000000000003";
const AUDIT_ID: &str = "00000000-0000-4000-8000-000000000005";
const EVENT_ID: &str = "00000000-0000-4000-8000-000000000006";
const CREATE_REQUEST_ID: &str = "10000000-0000-4000-8000-000000000001";

struct Clock(RefCell<VecDeque<chrono::DateTime<Utc>>>);
impl KernelClock for Clock {
    fn now_utc(&self) -> Result<chrono::DateTime<Utc>, ClockError> {
        self.0.borrow_mut().pop_front().ok_or(ClockError)
    }
}

struct Ids(RefCell<u32>);
impl KernelIdGenerator for Ids {
    fn next_uuid(&self, _purpose: UuidPurpose) -> Result<String, IdGenerationError> {
        let next = self.0.replace_with(|value| *value + 1);
        Ok(format!("00000000-0000-4000-8000-{next:012}"))
    }

    fn next_opaque_id(&self, purpose: OpaqueIdPurpose) -> Result<String, IdGenerationError> {
        Ok(match purpose {
            OpaqueIdPurpose::Correlation => "integration-correlation".into(),
            OpaqueIdPurpose::EventDedup => "integration-dedup".into(),
        })
    }
}

#[test]
fn sqlite_create_get_and_replay_bind_intent_event_audit_and_materialized_facts() {
    let directory = TempDir::new().unwrap();
    let store = SqliteStore::open(
        directory.path().join("kernel.db"),
        SqliteConfig::new(Duration::from_millis(100)).unwrap(),
    )
    .unwrap();
    let backend = SqliteTaskBackend::new(&store);
    let first = handle_task_create(
        &create_envelope(CREATE_REQUEST_ID, "2026-07-18T12:00:10Z"),
        &Clock(RefCell::new([instant(1), instant(2)].into())),
        &Ids(RefCell::new(1)),
        &backend,
    );
    let HandlerResult::Response(first) = first else {
        panic!("response")
    };
    let (intent_task_id, intent_event_id) = match &first.post_commit_notification_intents[..] {
        [PostCommitNotificationIntent::TaskCreatedCommitted { task_id, event_id }] => {
            (task_id.clone(), event_id.to_string())
        }
        other => panic!("unexpected intents: {other:?}"),
    };
    assert_eq!(intent_task_id, TASK_ID);
    assert_eq!(intent_event_id, EVENT_ID);

    let events = store
        .read_after(OutboxCursor::START, PageLimit::new(10).unwrap())
        .unwrap();
    assert_eq!(events.len(), 1);
    let event = &events[0].envelope;
    assert_eq!(event.event_id, intent_event_id);
    assert_eq!(event.type_, "task.created");
    assert_eq!(event.aggregate_type, "task");
    assert_eq!(event.aggregate_id, intent_task_id);
    assert_eq!(event.sequence, 0);
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(&event.occurred_at)
            .unwrap()
            .with_timezone(&Utc),
        instant(1)
    );
    assert_eq!(event.correlation_id, "integration-correlation");
    assert_eq!(event.dedup_key, "integration-dedup");
    assert_eq!(event.causation_ref.kind, CausationRefKind::CommandRequest);
    assert_eq!(event.causation_ref.id, CREATE_REQUEST_ID);
    let EventPayload::TaskCreated(payload) = &event.payload else {
        panic!("task.created payload")
    };
    assert_eq!(payload.task_id, TASK_ID);
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(&payload.created_at)
            .unwrap()
            .with_timezone(&Utc),
        instant(1)
    );

    let task = store.get_task(TASK_ID).unwrap().unwrap();
    let scope = store.get_task_scope(SCOPE_ID).unwrap().unwrap();
    let origin = store.get_content_origin(ORIGIN_ID).unwrap().unwrap();
    let audit = store.get_audit(AUDIT_ID).unwrap().unwrap();
    assert_eq!(task.id, intent_task_id);
    assert_eq!(task.task_scope_ref, scope.id);
    assert_eq!(task.origin_ref, origin.id);
    assert_eq!(scope.task_id, task.id);
    assert_eq!(scope.source_refs, vec![origin.id.clone()]);
    assert_eq!(origin.carrier_ref.id, CREATE_REQUEST_ID);
    assert_eq!(audit.audit_type, AuditRecordAuditType::TaskCreationRecorded);
    assert_eq!(audit.task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(audit.content_origin_refs, vec![origin.id.clone()]);
    assert_eq!(
        audit.correlation_id.as_deref(),
        Some(event.correlation_id.as_str())
    );
    let audit_causation = audit.causation_ref.as_ref().unwrap();
    assert_eq!(audit_causation.kind, event.causation_ref.kind);
    assert_eq!(audit_causation.id, event.causation_ref.id);
    let context = audit.task_creation_context.as_ref().unwrap();
    assert_eq!(context.origin_ref, task.origin_ref);
    assert_eq!(context.goal, task.goal);

    let get = handle_task_get(
        &get_envelope(&intent_task_id, "2026-07-18T12:00:10Z"),
        &Clock(RefCell::new([instant(3), instant(4)].into())),
        &backend,
    );
    let HandlerResult::Response(get) = get else {
        panic!("get response")
    };
    assert_eq!(get.response.payload.unwrap()["task"]["id"], intent_task_id);

    let replay = handle_task_create(
        &create_envelope(
            "10000000-0000-4000-8000-000000000099",
            "2026-07-18T12:00:20Z",
        ),
        &Clock(RefCell::new([instant(5), instant(6)].into())),
        &Ids(RefCell::new(20)),
        &backend,
    );
    let HandlerResult::Response(replay) = replay else {
        panic!("replay")
    };
    assert!(replay.post_commit_notification_intents.is_empty());
    assert_eq!(
        store
            .read_after(OutboxCursor::START, PageLimit::new(10).unwrap())
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .get_audit("00000000-0000-4000-8000-000000000024")
        .unwrap()
        .is_none());
    assert!(store
        .get_task("00000000-0000-4000-8000-000000000020")
        .unwrap()
        .is_none());
    assert!(store
        .get_task_scope("00000000-0000-4000-8000-000000000021")
        .unwrap()
        .is_none());
    assert!(store
        .get_content_origin("00000000-0000-4000-8000-000000000022")
        .unwrap()
        .is_none());
}

fn instant(second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, second)
        .single()
        .unwrap()
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

fn request() -> TaskCreateRequest {
    serde_json::from_value(json!({"schema_version":1,"proposer":"user","goal":"integration goal","constraints":["keep"],"success_criteria":["done"],"risk_hint":null,"capability_hints":["filesystem.read"],"task_scope":{"schema_version":1,"resource_patterns":["https://example.com/a/**"],"exclusions":[],"allowed_capability_hints":["filesystem.read"],"expires_at":null},"delegation_ref":null,"parent_task_id":null,"origin":{"schema_version":1,"kind":"user_input","source_uri":"https://example.com/inbox","upstream_stable_id":null,"producer_ref":{"kind":"actor","id":"actor"},"parent_origin_refs":[]}})).unwrap()
}

fn create_envelope(request_id: &str, deadline: &str) -> TypedKcpCommandEnvelope {
    TypedKcpCommandEnvelope {
        actor: actor(),
        auth: NullOnly,
        context: Some(json!({"conversation":1})),
        deadline: deadline.into(),
        entry_point: EntryPoint::LocalDesktop,
        expected_revision: None,
        idempotency_key: "integration-key".into(),
        message_kind: KcpCommandEnvelopeMessageKind::Value,
        protocol_version: KcpCommandEnvelopeProtocolVersion::Value,
        request_id: request_id.into(),
        task_id: None,
        command_type: "task.create".into(),
        payload: KcpCommandPayload::TaskCreate(Box::new(request())),
    }
}

fn get_envelope(task_id: &str, deadline: &str) -> kernel_contracts::TypedKcpQueryEnvelope {
    kernel_contracts::TypedKcpQueryEnvelope {
        actor: actor(),
        auth: NullOnly,
        deadline: deadline.into(),
        entry_point: EntryPoint::LocalDesktop,
        message_kind: kernel_contracts::KcpQueryEnvelopeMessageKind::Value,
        protocol_version: kernel_contracts::KcpQueryEnvelopeProtocolVersion::Value,
        request_id: "10000000-0000-4000-8000-000000000002".into(),
        task_id: None,
        query_type: "task.get".into(),
        payload: kernel_contracts::KcpQueryPayload::TaskGet(Box::new(
            kernel_contracts::TaskGetRequest {
                schema_version: kernel_contracts::TaskGetRequestSchemaVersion,
                task_id: task_id.into(),
            },
        )),
    }
}
