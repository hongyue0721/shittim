-- kernel-sqlite migration phase: schema
-- PolicyRuleV2 history + PolicySet revision counter + immutable PermissionDecisionV2 (IC §6.6 / §6.7 / §6.10.6).
-- Descriptor v1 SchemaOnly. Fresh baseline only; no row transform.
-- Empty PolicySet bootstrap is revision 0 (authoritative empty state, not a forged rule set).

-- Global PolicySet revision counter. Exactly one row (id = 1). Bootstrap revision = 0.
CREATE TABLE policy_set_metadata (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    updated_at TEXT NOT NULL CHECK(length(updated_at) > 0)
);

INSERT INTO policy_set_metadata(id, revision, updated_at)
VALUES (1, 0, '1970-01-01T00:00:00Z');

-- PolicyRuleV2 revisions: canonical record_json is sole source of truth; projections for indexes.
-- Unique (rule_id, revision). Current enabled head is the max revision per rule_id
-- (repository selects MAX(revision) GROUP BY rule_id); disabled rules remain stored.
CREATE TABLE policy_rules (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        length(json_extract(record_json, '$.id')) > 0 AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 2 AND
        json_type(record_json, '$.revision') = 'integer' AND
        json_extract(record_json, '$.revision') >= 1 AND
        json_type(record_json, '$.effect') = 'text' AND
        json_type(record_json, '$.priority') = 'integer' AND
        json_type(record_json, '$.enabled') IN ('true', 'false', 'integer') AND
        json_type(record_json, '$.created_at') = 'text'
    ),
    rule_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED,
    revision INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.revision')) STORED CHECK(revision >= 1),
    effect TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.effect')) STORED,
    priority INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.priority')) STORED,
    enabled INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.enabled')) STORED,
    created_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.created_at')) STORED,
    UNIQUE(rule_id, revision)
);

CREATE INDEX policy_rules_rule_id_revision_idx ON policy_rules(rule_id, revision DESC);
CREATE INDEX policy_rules_enabled_priority_idx ON policy_rules(enabled, priority DESC, rule_id);

CREATE TRIGGER policy_rules_immutable_update
BEFORE UPDATE ON policy_rules
BEGIN
    SELECT RAISE(ABORT, 'policy_rules are immutable');
END;

CREATE TRIGGER policy_rules_immutable_delete
BEFORE DELETE ON policy_rules
BEGIN
    SELECT RAISE(ABORT, 'policy_rules are not deletable');
END;

-- PermissionDecisionV2: immutable append-only; unique (action_id, decision_revision) continuous by repository.
CREATE TABLE permission_decisions (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        json_type(record_json, '$.schema_version') = 'integer' AND
        json_extract(record_json, '$.schema_version') = 2 AND
        json_type(record_json, '$.action_id') = 'text' AND
        json_type(record_json, '$.decision_revision') = 'integer' AND
        json_extract(record_json, '$.decision_revision') >= 1 AND
        json_type(record_json, '$.evaluated_at') = 'text' AND
        json_type(record_json, '$.decision') = 'text' AND
        json_type(record_json, '$.material_authorization_fingerprint') = 'text' AND
        json_type(record_json, '$.observation_evidence_fingerprint') = 'text' AND
        json_type(record_json, '$.policy_set_revision') = 'integer' AND
        json_extract(record_json, '$.policy_set_revision') >= 0
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED UNIQUE,
    action_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.action_id')) STORED,
    decision_revision INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.decision_revision')) STORED CHECK(decision_revision >= 1),
    evaluated_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.evaluated_at')) STORED,
    decision TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.decision')) STORED,
    material_fingerprint TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.material_authorization_fingerprint')) STORED,
    observation_fingerprint TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.observation_evidence_fingerprint')) STORED,
    policy_set_revision INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.policy_set_revision')) STORED CHECK(policy_set_revision >= 0),
    UNIQUE(action_id, decision_revision),
    FOREIGN KEY(action_id) REFERENCES actions(id)
);

CREATE INDEX permission_decisions_action_revision_idx
    ON permission_decisions(action_id, decision_revision DESC);
CREATE INDEX permission_decisions_evaluated_at_idx
    ON permission_decisions(evaluated_at, id);

CREATE TRIGGER permission_decisions_immutable_update
BEFORE UPDATE ON permission_decisions
BEGIN
    SELECT RAISE(ABORT, 'permission_decisions are immutable');
END;

CREATE TRIGGER permission_decisions_immutable_delete
BEFORE DELETE ON permission_decisions
BEGIN
    SELECT RAISE(ABORT, 'permission_decisions are not deletable');
END;
