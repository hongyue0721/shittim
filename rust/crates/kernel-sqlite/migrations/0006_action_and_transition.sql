-- kernel-sqlite migration phase: schema
-- Active ActionRequestV2 current snapshot + ActionTransitionIntentV1 authority (IC §6.10.6 / §6.14).
-- Descriptor v1 SchemaOnly. Fresh baseline only; no row transform.

CREATE TABLE actions (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.action_id') = 'text' AND
        json_type(record_json, '$.task_id') = 'text' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 2 AND
        json_type(record_json, '$.status') = 'text' AND
        json_type(record_json, '$.revision') = 'integer' AND
        json_extract(record_json, '$.revision') >= 1 AND
        json_type(record_json, '$.execution_generation') = 'integer' AND
        json_extract(record_json, '$.execution_generation') >= 0
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.action_id')) STORED UNIQUE,
    task_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.task_id')) STORED,
    status TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.status')) STORED,
    revision INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.revision')) STORED CHECK(revision >= 1),
    approval_chain_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.approval_chain_id')) STORED,
    permission_decision_ref TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.permission_decision_ref')) STORED,
    execution_generation INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.execution_generation')) STORED CHECK(execution_generation >= 0),
    FOREIGN KEY(task_id) REFERENCES tasks(id)
);

CREATE INDEX actions_task_idx ON actions(task_id);
CREATE INDEX actions_status_revision_idx ON actions(status, revision, id);
CREATE INDEX actions_permission_decision_idx ON actions(permission_decision_ref);
CREATE INDEX actions_approval_chain_idx ON actions(approval_chain_id);

CREATE TRIGGER actions_identity_guard
BEFORE UPDATE ON actions
WHEN json_extract(OLD.record_json, '$.action_id') IS NOT json_extract(NEW.record_json, '$.action_id')
  OR json_extract(OLD.record_json, '$.schema_version') IS NOT json_extract(NEW.record_json, '$.schema_version')
  OR json_extract(OLD.record_json, '$.task_id') IS NOT json_extract(NEW.record_json, '$.task_id')
BEGIN
    SELECT RAISE(ABORT, 'action identity is immutable');
END;

CREATE TRIGGER actions_no_delete
BEFORE DELETE ON actions
BEGIN
    SELECT RAISE(ABORT, 'actions are not deletable');
END;

CREATE TABLE action_transition_intents (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 1 AND
        json_type(record_json, '$.transition_id') = 'text' AND
        json_type(record_json, '$.action_id') = 'text' AND
        json_type(record_json, '$.expected_action_revision') = 'integer' AND
        json_extract(record_json, '$.expected_action_revision') >= 0 AND
        json_type(record_json, '$.execution_generation') = 'integer' AND
        json_extract(record_json, '$.execution_generation') >= 0 AND
        json_type(record_json, '$.from_status') = 'text' AND
        json_type(record_json, '$.to_status') = 'text' AND
        json_type(record_json, '$.reason_code') = 'text' AND
        length(json_extract(record_json, '$.reason_code')) > 0
    ),
    transition_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.transition_id')) STORED UNIQUE,
    action_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.action_id')) STORED,
    expected_action_revision INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.expected_action_revision')) STORED,
    execution_generation INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.execution_generation')) STORED,
    from_status TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.from_status')) STORED,
    to_status TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.to_status')) STORED,
    reason_code TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.reason_code')) STORED,
    committed_event_id TEXT CHECK(
        committed_event_id IS NULL OR length(committed_event_id) > 0
    ),
    UNIQUE(
        action_id,
        expected_action_revision,
        execution_generation,
        from_status,
        to_status,
        reason_code
    ),
    FOREIGN KEY(action_id) REFERENCES actions(id)
);

CREATE INDEX action_transition_intents_action_revision_idx
    ON action_transition_intents(action_id, expected_action_revision);
CREATE INDEX action_transition_intents_committed_event_idx
    ON action_transition_intents(committed_event_id);

CREATE TRIGGER action_transition_intents_record_immutable
BEFORE UPDATE ON action_transition_intents
WHEN OLD.record_json IS NOT NEW.record_json
BEGIN
    SELECT RAISE(ABORT, 'action_transition_intents record_json is immutable');
END;

CREATE TRIGGER action_transition_intents_commit_once
BEFORE UPDATE ON action_transition_intents
WHEN OLD.committed_event_id IS NOT NULL
  AND (NEW.committed_event_id IS NULL OR NEW.committed_event_id IS NOT OLD.committed_event_id)
BEGIN
    SELECT RAISE(ABORT, 'action_transition_intents committed_event_id is immutable once set');
END;

CREATE TRIGGER action_transition_intents_no_delete
BEFORE DELETE ON action_transition_intents
BEGIN
    SELECT RAISE(ABORT, 'action_transition_intents are not deletable');
END;
