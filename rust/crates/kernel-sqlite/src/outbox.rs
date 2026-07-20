//! Atomic versioned Event Outbox storage and cursor types (v2-only production API).

use crate::{StoreError, StoreErrorCode, WriteTransaction};
use chrono::{DateTime, Utc};
use kernel_contracts::{
    canonical_json_string, CausationRefV2, EventEnvelopeV2Payload, TypedEventEnvelopeV2,
    EVENT_ACTIVE_BINDINGS,
};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

const ACTIVE_EVENT_SCHEMA: &str = "https://schemas.shittim.local/event/event_envelope/v2";

/// A strictly decoded stored EventEnvelope in its exact persisted version.
///
/// Production store is v2-only (ADR-0009). Rows with `schema_version=1` fail as
/// `stored_data_invalid` at decode time; open also refuses them as reinitialize-required.
#[derive(Debug, Clone, PartialEq)]
pub enum StoredEventEnvelope {
    /// Active EventEnvelope v2.
    ActiveV2(TypedEventEnvelopeV2),
}

impl StoredEventEnvelope {
    /// Returns the globally allocated Outbox position string.
    pub fn outbox_position(&self) -> &str {
        match self {
            Self::ActiveV2(envelope) => &envelope.outbox_position,
        }
    }

    /// Returns the aggregate sequence.
    pub fn sequence(&self) -> i64 {
        match self {
            Self::ActiveV2(envelope) => envelope.sequence,
        }
    }
}

/// Closed aggregate identity accepted by the active Event v2 append API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAggregateId {
    /// Task aggregate UUID.
    Task(Uuid),
    /// Action aggregate UUID.
    Action(Uuid),
    /// Approval chain aggregate UUID.
    ApprovalChain(Uuid),
    /// Singleton global Stop Fence aggregate.
    StopFenceGlobal,
}

/// Complete caller-owned active v2 facts before derived mapping and allocation.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingActiveEventV2 {
    /// Caller-allocated event UUID.
    pub event_id: Uuid,
    /// Closed aggregate identity, checked against the payload variant and payload ID.
    pub aggregate_id: EventAggregateId,
    /// Kernel-supplied occurrence instant.
    pub occurred_at: DateTime<Utc>,
    /// Active v2 direct cause.
    pub causation_ref: CausationRefV2,
    /// Non-empty correlation ID.
    pub correlation_id: String,
    /// Non-empty consumer idempotency key.
    pub dedup_key: String,
    /// Generated closed active payload variant.
    pub payload: EventEnvelopeV2Payload,
}

/// Positive, globally allocated Outbox row position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutboxPosition(i64);

impl OutboxPosition {
    /// Creates a positive position.
    pub fn new(value: i64) -> Result<Self, StoreError> {
        if value <= 0 {
            return Err(invalid_cursor());
        }
        Ok(Self(value))
    }

    /// Returns the numeric position.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for OutboxPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Non-negative subscription cursor representing the last processed position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct OutboxCursor(i64);

impl OutboxCursor {
    /// Cursor before any event has been processed.
    pub const START: Self = Self(0);

    /// Creates a non-negative cursor.
    pub fn new(value: i64) -> Result<Self, StoreError> {
        if value < 0 {
            return Err(invalid_cursor());
        }
        Ok(Self(value))
    }

    /// Returns the numeric cursor.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for OutboxCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for OutboxCursor {
    type Err = StoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || !value.as_bytes().iter().all(u8::is_ascii_digit) {
            return Err(invalid_cursor());
        }
        value.parse::<i64>().map(Self).map_err(|_| invalid_cursor())
    }
}

/// Bounded event page size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLimit(u16);

impl PageLimit {
    /// Creates a limit in `1..=500`.
    pub fn new(value: u16) -> Result<Self, StoreError> {
        if !(1..=500).contains(&value) {
            return Err(invalid_cursor());
        }
        Ok(Self(value))
    }

    pub(crate) const fn as_i64(self) -> i64 {
        self.0 as i64
    }
}

/// Validated event reconstructed from normalized Outbox columns.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxRecord {
    /// Exact typed envelope version.
    pub envelope: StoredEventEnvelope,
    /// Publisher completion time; not a subscriber acknowledgement.
    pub delivered_at: Option<DateTime<Utc>>,
}

/// Result of idempotently marking a row delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkDeliveredResult {
    /// The first publisher completion time was stored.
    Marked,
    /// The row was already marked; its first time was retained.
    AlreadyMarked,
    /// No Outbox row has this position.
    NotFound,
}

#[derive(Debug)]
struct VersionedAppend {
    schema_version: i64,
    event_id: String,
    event_type: &'static str,
    aggregate_type: &'static str,
    aggregate_id: String,
    occurred_at: DateTime<Utc>,
    causation: Value,
    correlation_id: String,
    dedup_key: String,
    payload: Value,
}

pub(crate) fn append_active_event_v2(
    transaction: &WriteTransaction<'_>,
    event: PendingActiveEventV2,
) -> Result<OutboxRecord, StoreError> {
    let input = active_input(event)?;
    append_versioned_event(transaction, input)
}

fn active_input(event: PendingActiveEventV2) -> Result<VersionedAppend, StoreError> {
    if event.correlation_id.is_empty() || event.dedup_key.is_empty() {
        return Err(caller_contract());
    }
    let (binding_index, payload_id) = active_payload_identity(&event.payload)?;
    let binding = EVENT_ACTIVE_BINDINGS
        .get(binding_index)
        .ok_or_else(caller_contract)?;
    let aggregate_id = match (binding_index, event.aggregate_id) {
        (0 | 1, EventAggregateId::Task(id)) if id == payload_id => id.to_string(),
        (2, EventAggregateId::Action(id)) if id == payload_id => id.to_string(),
        (3, EventAggregateId::ApprovalChain(id)) if id == payload_id => id.to_string(),
        (4, EventAggregateId::StopFenceGlobal) => "global".to_owned(),
        _ => return Err(caller_contract()),
    };
    let payload = serialize_active_payload(&event.payload)?;
    let causation =
        serde_json::to_value(&event.causation_ref).map_err(|_| caller_serialization())?;
    let input = VersionedAppend {
        schema_version: 2,
        event_id: event.event_id.to_string(),
        event_type: binding.event_type,
        aggregate_type: binding.aggregate_type,
        aggregate_id,
        occurred_at: event.occurred_at,
        causation,
        correlation_id: event.correlation_id,
        dedup_key: event.dedup_key,
        payload,
    };
    prevalidate(&input)?;
    Ok(input)
}

fn active_payload_identity(payload: &EventEnvelopeV2Payload) -> Result<(usize, Uuid), StoreError> {
    let (index, id) = match payload {
        EventEnvelopeV2Payload::TaskCreated(payload) => (0, payload.task_id.as_str()),
        EventEnvelopeV2Payload::TaskStateChanged(payload) => (1, payload.task_id.as_str()),
        EventEnvelopeV2Payload::ActionStateChanged(payload) => (2, payload.action_id.as_str()),
        EventEnvelopeV2Payload::ApprovalStateChanged(payload) => {
            (3, payload.approval_chain_id.as_str())
        }
        EventEnvelopeV2Payload::StopFenceActivated(_) => {
            return Ok((4, Uuid::nil()));
        }
    };
    Uuid::parse_str(id)
        .map(|id| (index, id))
        .map_err(|_| caller_contract())
}

fn serialize_active_payload(payload: &EventEnvelopeV2Payload) -> Result<Value, StoreError> {
    match payload {
        EventEnvelopeV2Payload::TaskCreated(value) => to_value(value.as_ref()),
        EventEnvelopeV2Payload::TaskStateChanged(value) => to_value(value.as_ref()),
        EventEnvelopeV2Payload::ActionStateChanged(value) => to_value(value.as_ref()),
        EventEnvelopeV2Payload::ApprovalStateChanged(value) => to_value(value.as_ref()),
        EventEnvelopeV2Payload::StopFenceActivated(value) => to_value(value.as_ref()),
    }
}

fn to_value(value: &impl Serialize) -> Result<Value, StoreError> {
    serde_json::to_value(value).map_err(|_| caller_serialization())
}

fn prevalidate(input: &VersionedAppend) -> Result<(), StoreError> {
    if input.correlation_id.is_empty() || input.dedup_key.is_empty() {
        return Err(caller_contract());
    }
    decode_envelope(
        envelope_value(input, OutboxPosition::new(1)?, 0),
        DecodeContext::Caller,
    )?;
    Ok(())
}

fn append_versioned_event(
    transaction: &WriteTransaction<'_>,
    input: VersionedAppend,
) -> Result<OutboxRecord, StoreError> {
    transaction.with_savepoint("append_versioned_event", |connection| {
        let sequence: i64 = connection
            .query_row(
                "INSERT INTO aggregate_event_sequences(aggregate_type, aggregate_id, last_sequence) \
                 VALUES (?1, ?2, 0) \
                 ON CONFLICT(aggregate_type, aggregate_id) DO UPDATE \
                 SET last_sequence = last_sequence + 1 RETURNING last_sequence",
                params![input.aggregate_type, input.aggregate_id],
                |row| row.get(0),
            )
            .map_err(write_error)?;
        let payload_json = canonical_json_string(&input.payload).map_err(|_| caller_serialization())?;
        let causation_json = canonical_json_string(&input.causation).map_err(|_| caller_serialization())?;
        connection
            .execute(
                "INSERT INTO outbox(\
                    event_id, event_type, schema_version, aggregate_type, aggregate_id, sequence, \
                    occurred_at, causation_json, correlation_id, dedup_key, payload_json\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    input.event_id,
                    input.event_type,
                    input.schema_version,
                    input.aggregate_type,
                    input.aggregate_id,
                    sequence,
                    input.occurred_at.to_rfc3339(),
                    causation_json,
                    input.correlation_id,
                    input.dedup_key,
                    payload_json,
                ],
            )
            .map_err(write_error)?;
        let position = connection.last_insert_rowid();
        decode_versioned_row_at(connection, "outbox", position)?.ok_or_else(stored_invalid)
    })
}

pub(crate) fn read_after(
    connection: &Connection,
    cursor: OutboxCursor,
    limit: PageLimit,
) -> Result<Vec<OutboxRecord>, StoreError> {
    read_records(
        connection,
        "SELECT outbox_position, event_id, event_type, schema_version, aggregate_type, aggregate_id, \
                sequence, occurred_at, causation_json, correlation_id, dedup_key, payload_json, delivered_at \
         FROM outbox WHERE outbox_position > ?1 ORDER BY outbox_position ASC LIMIT ?2",
        cursor.get(),
        limit,
    )
}

pub(crate) fn read_undelivered(
    connection: &Connection,
    cursor: OutboxCursor,
    limit: PageLimit,
) -> Result<Vec<OutboxRecord>, StoreError> {
    read_records(
        connection,
        "SELECT outbox_position, event_id, event_type, schema_version, aggregate_type, aggregate_id, \
                sequence, occurred_at, causation_json, correlation_id, dedup_key, payload_json, delivered_at \
         FROM outbox WHERE delivered_at IS NULL AND outbox_position > ?1 \
         ORDER BY outbox_position ASC LIMIT ?2",
        cursor.get(),
        limit,
    )
}

pub(crate) fn latest_position(
    connection: &Connection,
) -> Result<Option<OutboxPosition>, StoreError> {
    let value: Option<i64> = connection
        .query_row("SELECT MAX(outbox_position) FROM outbox", [], |row| {
            row.get(0)
        })
        .map_err(read_error)?;
    value.map(OutboxPosition::new).transpose()
}

pub(crate) fn mark_delivered(
    transaction: &WriteTransaction<'_>,
    position: OutboxPosition,
    delivered_at: DateTime<Utc>,
) -> Result<MarkDeliveredResult, StoreError> {
    let connection = transaction.connection();
    let Some(record) = decode_versioned_row_at(connection, "outbox", position.get())? else {
        return Ok(MarkDeliveredResult::NotFound);
    };
    if record.delivered_at.is_some() {
        return Ok(MarkDeliveredResult::AlreadyMarked);
    }
    let changed = connection
        .execute(
            "UPDATE outbox SET delivered_at = ?1 \
             WHERE outbox_position = ?2 AND delivered_at IS NULL",
            params![delivered_at.to_rfc3339(), position.get()],
        )
        .map_err(write_error)?;
    Ok(if changed == 1 {
        MarkDeliveredResult::Marked
    } else {
        MarkDeliveredResult::AlreadyMarked
    })
}

fn read_records(
    connection: &Connection,
    sql: &str,
    after: i64,
    limit: PageLimit,
) -> Result<Vec<OutboxRecord>, StoreError> {
    let mut statement = connection.prepare(sql).map_err(read_error)?;
    let mut rows = statement
        .query(params![after, limit.as_i64()])
        .map_err(read_error)?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().map_err(read_error)? {
        records.push(decode_versioned_row(row).map_err(read_error)?);
    }
    Ok(records)
}

pub(crate) fn decode_versioned_row_at(
    connection: &Connection,
    table: &str,
    position: i64,
) -> Result<Option<OutboxRecord>, StoreError> {
    let sql = format!(
        "SELECT outbox_position, event_id, event_type, schema_version, aggregate_type, aggregate_id, \
                sequence, occurred_at, causation_json, correlation_id, dedup_key, payload_json, delivered_at \
         FROM {table} WHERE outbox_position = ?1"
    );
    let mut statement = connection.prepare(&sql).map_err(read_error)?;
    let outcome = statement.query_row([position], decode_versioned_row);
    match outcome {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(rusqlite::Error::FromSqlConversionFailure(_, _, source))
            if source.downcast_ref::<StoreError>().is_some() =>
        {
            Err(stored_invalid())
        }
        Err(error) => Err(read_error(error)),
    }
}

fn decode_versioned_row(row: &rusqlite::Row<'_>) -> Result<OutboxRecord, rusqlite::Error> {
    decode_versioned_parts(
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
    )
    .map_err(to_sqlite_decode_error)
}

#[allow(clippy::too_many_arguments)]
fn decode_versioned_parts(
    position: i64,
    event_id: String,
    event_type: String,
    schema_version: i64,
    aggregate_type: String,
    aggregate_id: String,
    sequence: i64,
    occurred_at: String,
    causation_json: String,
    correlation_id: String,
    dedup_key: String,
    payload_json: String,
    delivered_at: Option<String>,
) -> Result<OutboxRecord, StoreError> {
    // ADR-0009: production store is v2-only. schema_version=1 is stored_data_invalid
    // (fresh baseline cannot produce such rows; old DBs are refused at open).
    if schema_version != 2 {
        return Err(stored_invalid());
    }
    let causation = parse_canonical_object(&causation_json)?;
    let payload = parse_canonical_object(&payload_json)?;
    let input = VersionedAppend {
        schema_version,
        event_id,
        event_type: stored_event_type(&event_type)?,
        aggregate_type: stored_aggregate_type(&aggregate_type)?,
        aggregate_id,
        occurred_at: parse_datetime(&occurred_at)?,
        causation,
        correlation_id,
        dedup_key,
        payload,
    };
    let envelope = decode_envelope(
        envelope_value(
            &input,
            OutboxPosition::new(position).map_err(|_| stored_invalid())?,
            sequence,
        ),
        DecodeContext::Stored,
    )?;
    let delivered_at = delivered_at
        .map(|value| parse_datetime(&value))
        .transpose()?;
    Ok(OutboxRecord {
        envelope,
        delivered_at,
    })
}

fn parse_canonical_object(stored: &str) -> Result<Value, StoreError> {
    let value: Value = serde_json::from_str(stored).map_err(|_| stored_invalid())?;
    if !value.is_object() || canonical_json_string(&value).map_err(|_| stored_invalid())? != stored
    {
        return Err(stored_invalid());
    }
    Ok(value)
}

fn stored_event_type(value: &str) -> Result<&'static str, StoreError> {
    [
        "task.created",
        "task.state_changed",
        "action.state_changed",
        "approval.state_changed",
        "stop_fence.activated",
    ]
    .iter()
    .copied()
    .find(|candidate| *candidate == value)
    .ok_or_else(stored_invalid)
}

fn stored_aggregate_type(value: &str) -> Result<&'static str, StoreError> {
    match value {
        "task" => Ok("task"),
        "action" => Ok("action"),
        "approval_chain" => Ok("approval_chain"),
        "stop_fence" => Ok("stop_fence"),
        _ => Err(stored_invalid()),
    }
}

fn envelope_value(input: &VersionedAppend, position: OutboxPosition, sequence: i64) -> Value {
    json!({
        "event_id": input.event_id,
        "type": input.event_type,
        "schema_version": input.schema_version,
        "aggregate_type": input.aggregate_type,
        "aggregate_id": input.aggregate_id,
        "sequence": sequence,
        "outbox_position": position.to_string(),
        "occurred_at": input.occurred_at.to_rfc3339(),
        "causation_ref": input.causation,
        "correlation_id": input.correlation_id,
        "dedup_key": input.dedup_key,
        "payload": input.payload,
    })
}

#[derive(Debug, Clone, Copy)]
enum DecodeContext {
    Caller,
    Stored,
}

fn decode_envelope(
    value: Value,
    context: DecodeContext,
) -> Result<StoredEventEnvelope, StoreError> {
    let version = value
        .get("schema_version")
        .and_then(Value::as_i64)
        .ok_or_else(|| decode_error(context))?;
    match version {
        2 => {
            kernel_contracts::validate_json(ACTIVE_EVENT_SCHEMA, &value)
                .map_err(|_| decode_error(context))?;
            let envelope = TypedEventEnvelopeV2::decode_after_validation(value)
                .map_err(|_| decode_error(context))?;
            validate_active_relations(&envelope, context)?;
            Ok(StoredEventEnvelope::ActiveV2(envelope))
        }
        // schema_version=1 and any other value: stored rows are invalid for the v2-only store.
        _ => Err(decode_error(context)),
    }
}

fn validate_active_relations(
    envelope: &TypedEventEnvelopeV2,
    context: DecodeContext,
) -> Result<(), StoreError> {
    let payload_id = match &envelope.payload {
        EventEnvelopeV2Payload::TaskCreated(payload) => payload.task_id.as_str(),
        EventEnvelopeV2Payload::TaskStateChanged(payload) => payload.task_id.as_str(),
        EventEnvelopeV2Payload::ActionStateChanged(payload) => payload.action_id.as_str(),
        EventEnvelopeV2Payload::ApprovalStateChanged(payload) => payload.approval_chain_id.as_str(),
        EventEnvelopeV2Payload::StopFenceActivated(_) => "global",
    };
    if envelope.aggregate_id != payload_id {
        return Err(decode_error(context));
    }
    Ok(())
}

fn decode_error(context: DecodeContext) -> StoreError {
    match context {
        DecodeContext::Caller => caller_contract(),
        DecodeContext::Stored => stored_invalid(),
    }
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| stored_invalid())
}

fn caller_contract() -> StoreError {
    StoreError::new(
        StoreErrorCode::ContractInvalid,
        "pending event violates its exact generated contract",
    )
}

fn caller_serialization() -> StoreError {
    StoreError::new(
        StoreErrorCode::SerializationFailed,
        "pending event JSON serialization failed",
    )
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored Outbox row failed exact integrity validation",
    )
}

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}

fn read_error(error: rusqlite::Error) -> StoreError {
    if let rusqlite::Error::FromSqlConversionFailure(_, _, source) = &error {
        if source.downcast_ref::<StoreError>().is_some() {
            return stored_invalid();
        }
    }
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}

fn to_sqlite_decode_error(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn invalid_cursor() -> StoreError {
    StoreError::new(
        StoreErrorCode::InvalidCursor,
        "cursor, position, or page limit is invalid",
    )
}
