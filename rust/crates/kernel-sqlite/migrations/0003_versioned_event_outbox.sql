-- kernel-sqlite migration phase: ledger_upgrade
ALTER TABLE schema_migrations ADD COLUMN descriptor_hash TEXT CHECK(
    descriptor_hash IS NULL OR (
        length(descriptor_hash) = 64 AND
        descriptor_hash = lower(descriptor_hash) AND
        descriptor_hash NOT GLOB '*[^0-9a-f]*'
    )
);
ALTER TABLE schema_migrations ADD COLUMN descriptor_format_version INTEGER CHECK(
    descriptor_format_version IS NULL OR descriptor_format_version = 1
);

-- kernel-sqlite migration phase: replacement_schema
CREATE TABLE outbox_versioned_replacement (
    outbox_position INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL CHECK(event_type IN (
        'task.created',
        'task.state_changed',
        'action.state_changed',
        'approval.state_changed',
        'stop_fence.activated'
    )),
    schema_version INTEGER NOT NULL CHECK(schema_version IN (1, 2)),
    aggregate_type TEXT NOT NULL CHECK(length(aggregate_type) > 0),
    aggregate_id TEXT NOT NULL CHECK(length(aggregate_id) > 0),
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    occurred_at TEXT NOT NULL CHECK(length(occurred_at) > 0),
    causation_json TEXT NOT NULL CHECK(
        json_valid(causation_json) AND json_type(causation_json, '$') = 'object'
    ),
    correlation_id TEXT NOT NULL CHECK(length(correlation_id) > 0),
    dedup_key TEXT NOT NULL UNIQUE CHECK(length(dedup_key) > 0),
    payload_json TEXT NOT NULL CHECK(
        json_valid(payload_json) AND json_type(payload_json, '$') = 'object'
    ),
    delivered_at TEXT,
    UNIQUE (aggregate_type, aggregate_id, sequence),
    CHECK(
        (schema_version = 1 AND event_type IN (
            'task.created', 'task.state_changed', 'stop_fence.activated'
        )) OR
        (schema_version = 2 AND event_type IN (
            'task.created',
            'task.state_changed',
            'action.state_changed',
            'approval.state_changed',
            'stop_fence.activated'
        ))
    ),
    CHECK(
        (event_type IN ('task.created', 'task.state_changed') AND aggregate_type = 'task') OR
        (event_type = 'action.state_changed' AND aggregate_type = 'action') OR
        (event_type = 'approval.state_changed' AND aggregate_type = 'approval_chain') OR
        (event_type = 'stop_fence.activated' AND aggregate_type = 'stop_fence' AND aggregate_id = 'global')
    )
);

-- kernel-sqlite migration phase: table_swap
DROP INDEX outbox_undelivered_position_idx;
DROP INDEX outbox_aggregate_idx;
DROP TABLE outbox;
ALTER TABLE outbox_versioned_replacement RENAME TO outbox;
CREATE INDEX outbox_undelivered_position_idx
    ON outbox(outbox_position) WHERE delivered_at IS NULL;
CREATE INDEX outbox_aggregate_idx
    ON outbox(aggregate_type, aggregate_id, sequence);
