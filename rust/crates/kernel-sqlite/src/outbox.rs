//! Atomic Event Outbox storage and cursor types.

use crate::{StoreError, StoreErrorCode};
use chrono::{DateTime, Utc};
use kernel_contracts::{validate_json, CausationRef, EventEnvelopeType, TypedEventEnvelope};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::fmt;
use std::str::FromStr;

const EVENT_SCHEMA: &str = "https://schemas.shittim.local/v1/event/event_envelope.json";
const APPEND_EVENT_SAVEPOINT: &str = "kernel_sqlite_append_event";

/// Complete caller-owned event facts before sequence and global position allocation.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEvent {
    /// Caller-allocated UUID event ID.
    pub event_id: String,
    /// Generated closed EventEnvelope type.
    pub event_type: EventEnvelopeType,
    /// Aggregate type. Conditional schema validation enforces event/type pairing.
    pub aggregate_type: String,
    /// Aggregate root ID.
    pub aggregate_id: String,
    /// Kernel-supplied occurrence instant.
    pub occurred_at: DateTime<Utc>,
    /// Direct command/event cause.
    pub causation_ref: CausationRef,
    /// Non-empty correlation ID.
    pub correlation_id: String,
    /// Non-empty consumer idempotency key.
    pub dedup_key: String,
    /// Type-specific payload including its own schema_version.
    pub payload: Value,
}

/// Positive, globally allocated Outbox row position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutboxPosition(i64);

impl OutboxPosition {
    /// Creates a positive position.
    pub fn new(value: i64) -> Result<Self, StoreError> {
        if value <= 0 {
            return Err(StoreError::new(
                StoreErrorCode::InvalidCursor,
                "outbox position must be positive",
            ));
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
    /// Typed generated envelope and payload.
    pub envelope: TypedEventEnvelope,
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

pub(crate) fn append_event(
    connection: &Connection,
    event: PendingEvent,
) -> Result<OutboxRecord, StoreError> {
    prevalidate_event(&event)?;
    connection
        .execute_batch(&format!("SAVEPOINT {APPEND_EVENT_SAVEPOINT}"))
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;

    match append_event_inside_savepoint(connection, event) {
        Ok(record) => {
            match connection.execute_batch(&format!("RELEASE SAVEPOINT {APPEND_EVENT_SAVEPOINT}")) {
                Ok(()) => Ok(record),
                Err(release_error) => {
                    let original =
                        StoreError::sqlite(release_error, StoreErrorCode::InternalStoreError);
                    if connection
                        .execute_batch(&format!(
                            "ROLLBACK TO SAVEPOINT {APPEND_EVENT_SAVEPOINT}; \
                         RELEASE SAVEPOINT {APPEND_EVENT_SAVEPOINT}"
                        ))
                        .is_err()
                    {
                        return Err(StoreError::new(
                            StoreErrorCode::InternalStoreError,
                            "append_event savepoint release and rollback both failed",
                        ));
                    }
                    Err(original)
                }
            }
        }
        Err(error) => {
            let rollback = connection.execute_batch(&format!(
                "ROLLBACK TO SAVEPOINT {APPEND_EVENT_SAVEPOINT}; \
                 RELEASE SAVEPOINT {APPEND_EVENT_SAVEPOINT}"
            ));
            if rollback.is_err() {
                return Err(StoreError::new(
                    StoreErrorCode::InternalStoreError,
                    format!(
                        "append_event failed with {} and savepoint rollback also failed",
                        error.code.as_str()
                    ),
                ));
            }
            Err(error)
        }
    }
}

fn prevalidate_event(event: &PendingEvent) -> Result<(), StoreError> {
    let placeholder_position = OutboxPosition::new(1).map_err(|_| {
        StoreError::new(
            StoreErrorCode::InternalStoreError,
            "invalid internal Outbox placeholder position",
        )
    })?;
    decode_envelope(envelope_value(
        placeholder_position,
        0,
        event.event_id.clone(),
        event.event_type.as_str(),
        event.aggregate_type.clone(),
        event.aggregate_id.clone(),
        event.occurred_at.to_rfc3339(),
        event.causation_ref.kind.as_str(),
        event.causation_ref.id.clone(),
        event.correlation_id.clone(),
        event.dedup_key.clone(),
        event.payload.clone(),
    ))?;
    Ok(())
}

fn append_event_inside_savepoint(
    connection: &Connection,
    event: PendingEvent,
) -> Result<OutboxRecord, StoreError> {
    let sequence: i64 = connection
        .query_row(
            "INSERT INTO aggregate_event_sequences(aggregate_type, aggregate_id, last_sequence) \
             VALUES (?1, ?2, 0) \
             ON CONFLICT(aggregate_type, aggregate_id) DO UPDATE \
             SET last_sequence = last_sequence + 1 \
             RETURNING last_sequence",
            params![event.aggregate_type, event.aggregate_id],
            |row| row.get(0),
        )
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    let payload_json = kernel_contracts::canonical_json_string(&event.payload).map_err(|_| {
        StoreError::new(
            StoreErrorCode::SerializationFailed,
            "event payload canonicalization failed",
        )
    })?;
    connection
        .execute(
            "INSERT INTO outbox(\
                event_id, event_type, schema_version, aggregate_type, aggregate_id, sequence, \
                occurred_at, causation_kind, causation_id, correlation_id, dedup_key, payload_json\
             ) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                event.event_id,
                event.event_type.as_str(),
                event.aggregate_type,
                event.aggregate_id,
                sequence,
                event.occurred_at.to_rfc3339(),
                event.causation_ref.kind.as_str(),
                event.causation_ref.id,
                event.correlation_id,
                event.dedup_key,
                payload_json,
            ],
        )
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    let position = OutboxPosition::new(connection.last_insert_rowid())?;
    let envelope_value = envelope_value(
        position,
        sequence,
        event.event_id,
        event.event_type.as_str(),
        event.aggregate_type,
        event.aggregate_id,
        event.occurred_at.to_rfc3339(),
        event.causation_ref.kind.as_str(),
        event.causation_ref.id,
        event.correlation_id,
        event.dedup_key,
        event.payload,
    );
    let envelope = decode_envelope(envelope_value)?;
    Ok(OutboxRecord {
        envelope,
        delivered_at: None,
    })
}

pub(crate) fn read_after(
    connection: &Connection,
    cursor: OutboxCursor,
    limit: PageLimit,
) -> Result<Vec<OutboxRecord>, StoreError> {
    read_records(
        connection,
        "SELECT outbox_position, event_id, event_type, aggregate_type, aggregate_id, sequence, \
                occurred_at, causation_kind, causation_id, correlation_id, dedup_key, payload_json, \
                delivered_at FROM outbox WHERE outbox_position > ?1 \
         ORDER BY outbox_position ASC LIMIT ?2",
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
        "SELECT outbox_position, event_id, event_type, aggregate_type, aggregate_id, sequence, \
                occurred_at, causation_kind, causation_id, correlation_id, dedup_key, payload_json, \
                delivered_at FROM outbox WHERE delivered_at IS NULL AND outbox_position > ?1 \
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
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    value.map(OutboxPosition::new).transpose()
}

pub(crate) fn mark_delivered(
    connection: &Connection,
    position: OutboxPosition,
    delivered_at: DateTime<Utc>,
) -> Result<MarkDeliveredResult, StoreError> {
    let changed = connection
        .execute(
            "UPDATE outbox SET delivered_at = ?1 \
             WHERE outbox_position = ?2 AND delivered_at IS NULL",
            params![delivered_at.to_rfc3339(), position.get()],
        )
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    if changed == 1 {
        return Ok(MarkDeliveredResult::Marked);
    }
    let exists = connection
        .query_row(
            "SELECT 1 FROM outbox WHERE outbox_position = ?1",
            [position.get()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?
        .is_some();
    Ok(if exists {
        MarkDeliveredResult::AlreadyMarked
    } else {
        MarkDeliveredResult::NotFound
    })
}

fn read_records(
    connection: &Connection,
    sql: &str,
    after: i64,
    limit: PageLimit,
) -> Result<Vec<OutboxRecord>, StoreError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    let mut rows = statement
        .query(params![after, limit.as_i64()])
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?;
    let mut records = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| StoreError::sqlite(error, StoreErrorCode::InternalStoreError))?
    {
        records.push(decode_row(row)?);
    }
    Ok(records)
}

fn decode_row(row: &rusqlite::Row<'_>) -> Result<OutboxRecord, StoreError> {
    let position = OutboxPosition::new(row.get(0).map_err(row_error)?)?;
    let payload_json: String = row.get(11).map_err(row_error)?;
    let payload: Value = serde_json::from_str(&payload_json).map_err(|_| {
        StoreError::new(
            StoreErrorCode::SerializationFailed,
            "stored event payload cannot be parsed",
        )
    })?;
    let occurred_at: String = row.get(6).map_err(row_error)?;
    let envelope = decode_envelope(envelope_value(
        position,
        row.get(5).map_err(row_error)?,
        row.get(1).map_err(row_error)?,
        &row.get::<_, String>(2).map_err(row_error)?,
        row.get(3).map_err(row_error)?,
        row.get(4).map_err(row_error)?,
        occurred_at,
        &row.get::<_, String>(7).map_err(row_error)?,
        row.get(8).map_err(row_error)?,
        row.get(9).map_err(row_error)?,
        row.get(10).map_err(row_error)?,
        payload,
    ))?;
    let delivered_at: Option<String> = row.get(12).map_err(row_error)?;
    let delivered_at = delivered_at
        .map(|value| parse_datetime(&value, "stored delivered_at is invalid"))
        .transpose()?;
    Ok(OutboxRecord {
        envelope,
        delivered_at,
    })
}

#[allow(clippy::too_many_arguments)]
fn envelope_value(
    position: OutboxPosition,
    sequence: i64,
    event_id: String,
    event_type: &str,
    aggregate_type: String,
    aggregate_id: String,
    occurred_at: String,
    causation_kind: &str,
    causation_id: String,
    correlation_id: String,
    dedup_key: String,
    payload: Value,
) -> Value {
    json!({
        "event_id": event_id,
        "type": event_type,
        "schema_version": 1,
        "aggregate_type": aggregate_type,
        "aggregate_id": aggregate_id,
        "sequence": sequence,
        "outbox_position": position.to_string(),
        "occurred_at": occurred_at,
        "causation_ref": { "kind": causation_kind, "id": causation_id },
        "correlation_id": correlation_id,
        "dedup_key": dedup_key,
        "payload": payload,
    })
}

fn decode_envelope(value: Value) -> Result<TypedEventEnvelope, StoreError> {
    validate_json(EVENT_SCHEMA, &value).map_err(|_| {
        StoreError::new(
            StoreErrorCode::ContractInvalid,
            "EventEnvelope violates its JSON contract",
        )
    })?;
    TypedEventEnvelope::decode(value).map_err(|_| {
        StoreError::new(
            StoreErrorCode::ContractInvalid,
            "EventEnvelope typed decoding failed",
        )
    })
}

fn parse_datetime(value: &str, message: &'static str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| StoreError::new(StoreErrorCode::SerializationFailed, message))
}

fn row_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}

fn invalid_cursor() -> StoreError {
    StoreError::new(
        StoreErrorCode::InvalidCursor,
        "cursor, position, or page limit is invalid",
    )
}
