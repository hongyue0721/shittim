-- kernel-sqlite migration phase: schema
-- Fresh-baseline root TaskCreate v2 tables (ADR-0009 / IC §5.5 / §6.16).
-- 0001-0003 assets remain byte-stable. Legacy v1 write paths stay usable in this slice.
--
-- TaskSpec/TaskScope remain the active retained v1 shapes. ContentOrigin active store is v2:
-- tasks.origin_ref and task_scope source_refs therefore lose hard FKs to content_origins(v1)
-- so a single Task row can reference either legacy ContentOrigin v1 or ContentOriginV2.
-- Existence and relation closure are enforced by repository canonical readback.

-- ContentOrigin v2: canonical record_json is the sole source of truth; generated columns project.
CREATE TABLE content_origins_v2 (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 2
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED UNIQUE,
    kind TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.kind')) STORED,
    receipt_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.kernel_receipt.receipt_id')) STORED UNIQUE,
    received_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.received_at')) STORED
);

CREATE INDEX content_origins_v2_kind_idx ON content_origins_v2(kind);
CREATE INDEX content_origins_v2_received_at_idx ON content_origins_v2(received_at, id);

CREATE TRIGGER content_origins_v2_immutable_update
BEFORE UPDATE ON content_origins_v2
BEGIN
    SELECT RAISE(ABORT, 'content_origins_v2 are immutable');
END;

CREATE TRIGGER content_origins_v2_immutable_delete
BEFORE DELETE ON content_origins_v2
BEGIN
    SELECT RAISE(ABORT, 'content_origins_v2 are immutable');
END;

CREATE TABLE content_origin_v2_parent_refs (
    origin_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
    parent_origin_id TEXT NOT NULL,
    PRIMARY KEY(origin_id, ordinal),
    FOREIGN KEY(origin_id) REFERENCES content_origins_v2(id) DEFERRABLE INITIALLY DEFERRED
) WITHOUT ROWID;

CREATE INDEX content_origin_v2_parent_refs_parent_idx
    ON content_origin_v2_parent_refs(parent_origin_id);

CREATE TRIGGER content_origin_v2_parent_refs_no_late_insert
BEFORE INSERT ON content_origin_v2_parent_refs
WHEN EXISTS(SELECT 1 FROM content_origins_v2 WHERE id = NEW.origin_id)
BEGIN
    SELECT RAISE(ABORT, 'content origin v2 parent refs are fixed at creation');
END;

CREATE TRIGGER content_origin_v2_parent_refs_immutable_update
BEFORE UPDATE ON content_origin_v2_parent_refs
BEGIN
    SELECT RAISE(ABORT, 'content origin v2 parent refs are immutable');
END;

CREATE TRIGGER content_origin_v2_parent_refs_immutable_delete
BEFORE DELETE ON content_origin_v2_parent_refs
BEGIN
    SELECT RAISE(ABORT, 'content origin v2 parent refs are immutable');
END;

-- Rebuild tasks without origin_ref FK so origin may live in content_origins or content_origins_v2.
CREATE TABLE tasks_replacement (
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
    FOREIGN KEY(parent_task_id) REFERENCES tasks_replacement(id)
    -- task_scope_ref deferred FK is restored after task_scopes_replacement exists (see below).
    -- SQLite with PRAGMA foreign_keys=ON rejects CREATE TABLE FK to a not-yet-created table.
);

INSERT INTO tasks_replacement(record_json) SELECT record_json FROM tasks;

-- Rebuild task_scopes to retarget task_id FK to tasks_replacement.
CREATE TABLE task_scopes_replacement (
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
    FOREIGN KEY(task_id) REFERENCES tasks_replacement(id) DEFERRABLE INITIALLY DEFERRED
);

INSERT INTO task_scopes_replacement(record_json) SELECT record_json FROM task_scopes;

-- Second rebuild of tasks: now that task_scopes_replacement exists, restore 0002's
-- deferred tasks.task_scope_ref → task_scopes cycle (paired with task_scopes.task_id → tasks).
CREATE TABLE tasks_with_scope_fk (
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
    FOREIGN KEY(parent_task_id) REFERENCES tasks_with_scope_fk(id),
    FOREIGN KEY(task_scope_ref) REFERENCES task_scopes_replacement(id) DEFERRABLE INITIALLY DEFERRED
);

INSERT INTO tasks_with_scope_fk(record_json) SELECT record_json FROM tasks_replacement;
DROP TABLE tasks_replacement;
ALTER TABLE tasks_with_scope_fk RENAME TO tasks_replacement;

-- Rebuild scope source_refs without content_origins FK; keep deferred FK to scope.
CREATE TABLE task_scope_source_refs_replacement (
    scope_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
    origin_id TEXT NOT NULL,
    PRIMARY KEY(scope_id, ordinal),
    FOREIGN KEY(scope_id) REFERENCES task_scopes_replacement(id) DEFERRABLE INITIALLY DEFERRED
) WITHOUT ROWID;

INSERT INTO task_scope_source_refs_replacement(scope_id, ordinal, origin_id)
SELECT scope_id, ordinal, origin_id FROM task_scope_source_refs;

-- Rebuild v1 idempotency to retarget task FK.
CREATE TABLE task_create_idempotency_replacement (
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
    FOREIGN KEY(created_task_id) REFERENCES tasks_replacement(id)
);

INSERT INTO task_create_idempotency_replacement(
    projection_json, idempotency_key, projection_hash, created_task_id, accepted_at
)
SELECT projection_json, idempotency_key, projection_hash, created_task_id, accepted_at
FROM task_create_idempotency;

-- Drop old graph in dependency-safe order, then rename replacements.
DROP TRIGGER IF EXISTS tasks_identity_guard;
DROP TRIGGER IF EXISTS task_scopes_identity_guard;
DROP INDEX IF EXISTS tasks_parent_idx;
DROP INDEX IF EXISTS tasks_status_created_idx;
DROP INDEX IF EXISTS tasks_created_idx;
DROP INDEX IF EXISTS tasks_actor_idx;
DROP INDEX IF EXISTS task_scopes_updated_at_idx;
DROP INDEX IF EXISTS task_scope_source_refs_origin_idx;
DROP TABLE task_create_idempotency;
DROP TABLE task_scope_source_refs;
DROP TABLE task_scopes;
DROP TABLE tasks;

ALTER TABLE tasks_replacement RENAME TO tasks;
ALTER TABLE task_scopes_replacement RENAME TO task_scopes;
ALTER TABLE task_scope_source_refs_replacement RENAME TO task_scope_source_refs;
ALTER TABLE task_create_idempotency_replacement RENAME TO task_create_idempotency;

-- tasks.task_scope_ref → task_scopes deferred FK was restored via tasks_with_scope_fk above;
-- after rename it is the live tasks.task_scope_ref constraint (mirrors 0002 cycle with
-- task_scopes.task_id → tasks). Repository canonical readback remains the full authority
-- for origin existence and cross-object closure beyond what SQLite FK can express.

CREATE INDEX tasks_parent_idx ON tasks(parent_task_id);
CREATE INDEX tasks_status_created_idx ON tasks(status, created_at DESC, id);
CREATE INDEX tasks_created_idx ON tasks(created_at DESC, id);
CREATE INDEX tasks_actor_idx ON tasks(actor_id);
CREATE INDEX task_scopes_updated_at_idx ON task_scopes(updated_at, id);
CREATE INDEX task_scope_source_refs_origin_idx ON task_scope_source_refs(origin_id);

CREATE TRIGGER tasks_identity_guard
BEFORE UPDATE ON tasks
WHEN json_extract(OLD.record_json, '$.id') IS NOT json_extract(NEW.record_json, '$.id')
  OR json_extract(OLD.record_json, '$.schema_version') IS NOT json_extract(NEW.record_json, '$.schema_version')
BEGIN
    SELECT RAISE(ABORT, 'task identity is immutable');
END;

CREATE TRIGGER task_scopes_identity_guard
BEFORE UPDATE ON task_scopes
WHEN json_extract(OLD.record_json, '$.id') IS NOT json_extract(NEW.record_json, '$.id')
  OR json_extract(OLD.record_json, '$.schema_version') IS NOT json_extract(NEW.record_json, '$.schema_version')
  OR json_extract(OLD.record_json, '$.task_id') IS NOT json_extract(NEW.record_json, '$.task_id')
BEGIN
    SELECT RAISE(ABORT, 'task scope identity is immutable');
END;

-- TaskCreationProvenance: tagged union stored as canonical JSON; branch + task_id projected.
CREATE TABLE task_creation_provenances (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 1 AND
        json_type(record_json, '$.kind') = 'text' AND
        json_extract(record_json, '$.kind') IN ('root_command_v2', 'child_action_v2')
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED UNIQUE,
    kind TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.kind')) STORED,
    task_id TEXT NOT NULL UNIQUE,
    accepted_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.accepted_at')) STORED,
    FOREIGN KEY(task_id) REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX task_creation_provenances_kind_idx ON task_creation_provenances(kind);
CREATE INDEX task_creation_provenances_accepted_at_idx
    ON task_creation_provenances(accepted_at, id);

CREATE TRIGGER task_creation_provenances_immutable_update
BEFORE UPDATE ON task_creation_provenances
BEGIN
    SELECT RAISE(ABORT, 'task_creation_provenances are immutable');
END;

CREATE TRIGGER task_creation_provenances_immutable_delete
BEFORE DELETE ON task_creation_provenances
BEGIN
    SELECT RAISE(ABORT, 'task_creation_provenances are immutable');
END;

-- AuditRecord v2: canonical record_json with projected lookup columns.
CREATE TABLE audit_records_v2 (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 2
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED UNIQUE,
    audit_type TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.audit_type')) STORED,
    task_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.task_id')) STORED,
    occurred_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.occurred_at')) STORED,
    correlation_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.correlation_id')) STORED
);

CREATE INDEX audit_records_v2_type_occurred_idx
    ON audit_records_v2(audit_type, occurred_at, id);
CREATE INDEX audit_records_v2_task_idx ON audit_records_v2(task_id);
CREATE INDEX audit_records_v2_correlation_idx ON audit_records_v2(correlation_id);

CREATE TRIGGER audit_records_v2_immutable_update
BEFORE UPDATE ON audit_records_v2
BEGIN
    SELECT RAISE(ABORT, 'audit_records_v2 are immutable');
END;

CREATE TRIGGER audit_records_v2_immutable_delete
BEFORE DELETE ON audit_records_v2
BEGIN
    SELECT RAISE(ABORT, 'audit_records_v2 are immutable');
END;

-- Root task.create v2 idempotency: scope 4-tuple unique + request_hash + created task.
CREATE TABLE root_task_create_idempotency_v2 (
    projection_json TEXT NOT NULL CHECK(
        json_valid(projection_json) AND
        json_type(projection_json, '$') = 'object' AND
        json_type(projection_json, '$.actor.id') = 'text' AND
        json_type(projection_json, '$.entry_point') = 'text' AND
        json_type(projection_json, '$.command_type') = 'text' AND
        json_extract(projection_json, '$.command_type') = 'task.create' AND
        json_type(projection_json, '$.schema_version') = 'integer' AND
        json_extract(projection_json, '$.schema_version') = 1
    ),
    actor_id TEXT GENERATED ALWAYS AS (json_extract(projection_json, '$.actor.id')) STORED,
    entry_point TEXT GENERATED ALWAYS AS (json_extract(projection_json, '$.entry_point')) STORED,
    command_type TEXT GENERATED ALWAYS AS (json_extract(projection_json, '$.command_type')) STORED,
    idempotency_key TEXT NOT NULL CHECK(length(idempotency_key) > 0),
    request_hash TEXT NOT NULL CHECK(length(request_hash) = 64 AND request_hash NOT GLOB '*[^0-9a-f]*'),
    created_task_id TEXT NOT NULL UNIQUE,
    creation_provenance_id TEXT NOT NULL UNIQUE,
    accepted_at TEXT NOT NULL CHECK(length(accepted_at) > 0),
    UNIQUE(actor_id, entry_point, command_type, idempotency_key),
    FOREIGN KEY(created_task_id) REFERENCES tasks(id),
    FOREIGN KEY(creation_provenance_id) REFERENCES task_creation_provenances(id) DEFERRABLE INITIALLY DEFERRED
);
