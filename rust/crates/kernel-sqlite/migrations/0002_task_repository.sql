CREATE TABLE content_origins (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 1
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED UNIQUE,
    kind TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.kind')) STORED,
    receipt_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.kernel_receipt.receipt_id')) STORED UNIQUE,
    received_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.received_at')) STORED
);

CREATE INDEX content_origins_kind_idx ON content_origins(kind);
CREATE INDEX content_origins_received_at_idx ON content_origins(received_at, id);

CREATE TRIGGER content_origins_immutable_update
BEFORE UPDATE ON content_origins
BEGIN
    SELECT RAISE(ABORT, 'content_origins are immutable');
END;

CREATE TRIGGER content_origins_immutable_delete
BEFORE DELETE ON content_origins
BEGIN
    SELECT RAISE(ABORT, 'content_origins are immutable');
END;

CREATE TABLE content_origin_parent_refs (
    origin_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
    parent_origin_id TEXT NOT NULL,
    PRIMARY KEY(origin_id, ordinal),
    FOREIGN KEY(origin_id) REFERENCES content_origins(id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(parent_origin_id) REFERENCES content_origins(id)
) WITHOUT ROWID;

CREATE INDEX content_origin_parent_refs_parent_idx
    ON content_origin_parent_refs(parent_origin_id);

CREATE TRIGGER content_origin_parent_refs_no_late_insert
BEFORE INSERT ON content_origin_parent_refs
WHEN EXISTS(SELECT 1 FROM content_origins WHERE id = NEW.origin_id)
BEGIN
    SELECT RAISE(ABORT, 'content origin parent refs are fixed at creation');
END;

CREATE TRIGGER content_origin_parent_refs_immutable_update
BEFORE UPDATE ON content_origin_parent_refs
BEGIN
    SELECT RAISE(ABORT, 'content origin parent refs are immutable');
END;

CREATE TRIGGER content_origin_parent_refs_immutable_delete
BEFORE DELETE ON content_origin_parent_refs
BEGIN
    SELECT RAISE(ABORT, 'content origin parent refs are immutable');
END;

CREATE TABLE task_scopes (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 1
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED UNIQUE,
    schema_version INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.schema_version')) STORED,
    revision INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.revision')) STORED CHECK(revision >= 1),
    task_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.task_id')) STORED UNIQUE,
    created_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.created_at')) STORED,
    updated_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.updated_at')) STORED,
    FOREIGN KEY(task_id) REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX task_scopes_updated_at_idx ON task_scopes(updated_at, id);

CREATE TRIGGER task_scopes_identity_guard
BEFORE UPDATE ON task_scopes
WHEN json_extract(OLD.record_json, '$.id') IS NOT json_extract(NEW.record_json, '$.id')
  OR json_extract(OLD.record_json, '$.schema_version') IS NOT json_extract(NEW.record_json, '$.schema_version')
  OR json_extract(OLD.record_json, '$.task_id') IS NOT json_extract(NEW.record_json, '$.task_id')
BEGIN
    SELECT RAISE(ABORT, 'task scope identity is immutable');
END;

CREATE TABLE task_scope_source_refs (
    scope_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
    origin_id TEXT NOT NULL,
    PRIMARY KEY(scope_id, ordinal),
    FOREIGN KEY(scope_id) REFERENCES task_scopes(id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(origin_id) REFERENCES content_origins(id)
) WITHOUT ROWID;

CREATE INDEX task_scope_source_refs_origin_idx ON task_scope_source_refs(origin_id);

CREATE TABLE tasks (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 1
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED UNIQUE,
    schema_version INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.schema_version')) STORED,
    origin_ref TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.origin_ref')) STORED,
    task_scope_ref TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.task_scope_ref')) STORED UNIQUE,
    parent_task_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.parent_task_id')) STORED,
    status TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.status')) STORED,
    proposer TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.proposer')) STORED,
    actor_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.actor.id')) STORED,
    revision INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.revision')) STORED CHECK(revision >= 1),
    created_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.created_at')) STORED,
    updated_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.updated_at')) STORED,
    FOREIGN KEY(origin_ref) REFERENCES content_origins(id),
    FOREIGN KEY(task_scope_ref) REFERENCES task_scopes(id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(parent_task_id) REFERENCES tasks(id)
);

CREATE INDEX tasks_parent_idx ON tasks(parent_task_id);
CREATE INDEX tasks_status_created_idx ON tasks(status, created_at DESC, id);
CREATE INDEX tasks_created_idx ON tasks(created_at DESC, id);
CREATE INDEX tasks_actor_idx ON tasks(actor_id);

CREATE TRIGGER tasks_identity_guard
BEFORE UPDATE ON tasks
WHEN json_extract(OLD.record_json, '$.id') IS NOT json_extract(NEW.record_json, '$.id')
  OR json_extract(OLD.record_json, '$.schema_version') IS NOT json_extract(NEW.record_json, '$.schema_version')
BEGIN
    SELECT RAISE(ABORT, 'task identity is immutable');
END;

CREATE TABLE task_create_idempotency (
    projection_json TEXT NOT NULL CHECK(
        json_valid(projection_json) AND
        json_type(projection_json, '$') = 'object' AND
        json_type(projection_json, '$.actor.id') = 'text' AND
        json_type(projection_json, '$.entry_point') = 'text' AND
        json_type(projection_json, '$.command_type') = 'text'
    ),
    actor_id TEXT GENERATED ALWAYS AS (json_extract(projection_json, '$.actor.id')) STORED,
    entry_point TEXT GENERATED ALWAYS AS (json_extract(projection_json, '$.entry_point')) STORED,
    command_type TEXT GENERATED ALWAYS AS (json_extract(projection_json, '$.command_type')) STORED,
    idempotency_key TEXT NOT NULL CHECK(length(idempotency_key) > 0),
    projection_hash TEXT NOT NULL CHECK(length(projection_hash) = 64 AND projection_hash NOT GLOB '*[^0-9a-f]*'),
    created_task_id TEXT NOT NULL UNIQUE,
    accepted_at TEXT NOT NULL CHECK(length(accepted_at) > 0),
    UNIQUE(actor_id, entry_point, command_type, idempotency_key),
    FOREIGN KEY(created_task_id) REFERENCES tasks(id)
);
