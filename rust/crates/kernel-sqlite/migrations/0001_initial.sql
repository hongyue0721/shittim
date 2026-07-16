CREATE TABLE aggregate_event_sequences (
    aggregate_type TEXT NOT NULL CHECK(length(aggregate_type) > 0),
    aggregate_id TEXT NOT NULL CHECK(length(aggregate_id) > 0),
    last_sequence INTEGER NOT NULL CHECK(last_sequence >= 0),
    PRIMARY KEY (aggregate_type, aggregate_id)
) WITHOUT ROWID;

CREATE TABLE outbox (
    outbox_position INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL CHECK(event_type IN ('task.created', 'task.state_changed', 'stop_fence.activated')),
    schema_version INTEGER NOT NULL CHECK(schema_version = 1),
    aggregate_type TEXT NOT NULL CHECK(length(aggregate_type) > 0),
    aggregate_id TEXT NOT NULL CHECK(length(aggregate_id) > 0),
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    occurred_at TEXT NOT NULL CHECK(length(occurred_at) > 0),
    causation_kind TEXT NOT NULL CHECK(causation_kind IN ('command_request', 'event')),
    causation_id TEXT NOT NULL CHECK(length(causation_id) > 0),
    correlation_id TEXT NOT NULL CHECK(length(correlation_id) > 0),
    dedup_key TEXT NOT NULL CHECK(length(dedup_key) > 0),
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
    delivered_at TEXT,
    UNIQUE (aggregate_type, aggregate_id, sequence),
    CHECK(
        (event_type IN ('task.created', 'task.state_changed') AND aggregate_type = 'task') OR
        (event_type = 'stop_fence.activated' AND aggregate_type = 'stop_fence' AND aggregate_id = 'global')
    )
);

CREATE INDEX outbox_undelivered_position_idx
    ON outbox(outbox_position) WHERE delivered_at IS NULL;
CREATE INDEX outbox_aggregate_idx
    ON outbox(aggregate_type, aggregate_id, sequence);

CREATE TABLE audit_records (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        json_extract(record_json, '$.schema_version') = 1
    )
);

CREATE UNIQUE INDEX audit_records_id_idx
    ON audit_records(json_extract(record_json, '$.id'));
CREATE INDEX audit_records_type_idx
    ON audit_records(json_extract(record_json, '$.audit_type'));
CREATE INDEX audit_records_time_idx
    ON audit_records(json_extract(record_json, '$.occurred_at'));
CREATE INDEX audit_records_task_idx
    ON audit_records(json_extract(record_json, '$.task_id'));
CREATE INDEX audit_records_action_idx
    ON audit_records(json_extract(record_json, '$.action_id'));

CREATE TRIGGER audit_records_immutable_update
BEFORE UPDATE ON audit_records
BEGIN
    SELECT RAISE(ABORT, 'audit_records are immutable');
END;

CREATE TRIGGER audit_records_immutable_delete
BEFORE DELETE ON audit_records
BEGIN
    SELECT RAISE(ABORT, 'audit_records are immutable');
END;

CREATE TABLE policy_rate_limit_consumptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_version INTEGER NOT NULL CHECK(schema_version = 1),
    rule_id TEXT NOT NULL CHECK(length(rule_id) > 0),
    rule_revision INTEGER NOT NULL CHECK(rule_revision > 0),
    rate_key TEXT NOT NULL CHECK(length(rate_key) > 0),
    consumed_at_micros INTEGER NOT NULL
);

CREATE INDEX policy_rate_limit_window_idx
    ON policy_rate_limit_consumptions(rule_id, rule_revision, rate_key, consumed_at_micros);
