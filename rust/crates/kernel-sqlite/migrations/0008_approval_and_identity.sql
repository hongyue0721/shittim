-- kernel-sqlite migration phase: schema
-- Approval v2 current-head chain + Identity credential/challenge/evidence (IC §6.10 / §6.10.2-4 / §6.10.6).
-- Descriptor v1 SchemaOnly. Fresh baseline only; no row transform.

-- ApprovalRecordV2: immutable append-only; canonical record_json is sole source of truth.
-- record_kind in (request|resolution|invalidation); subject_kind in (operation|task_proposal|plan_revision).
CREATE TABLE approval_records (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        length(json_extract(record_json, '$.id')) > 0 AND
        json_extract(record_json, '$.schema_version') = 2 AND
        json_type(record_json, '$.approval_chain_id') = 'text' AND
        length(json_extract(record_json, '$.approval_chain_id')) > 0 AND
        json_type(record_json, '$.record_kind') = 'text' AND
        json_extract(record_json, '$.record_kind') IN ('request','resolution','invalidation') AND
        json_type(record_json, '$.subject.subject_kind') = 'text' AND
        json_extract(record_json, '$.subject.subject_kind') IN ('operation','task_proposal','plan_revision') AND
        json_type(record_json, '$.created_at') = 'text'
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED,
    chain_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.approval_chain_id')) STORED,
    record_kind TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.record_kind')) STORED,
    subject_kind TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.subject.subject_kind')) STORED,
    predecessor_ref TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.predecessor_ref')) STORED,
    created_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.created_at')) STORED,
    expires_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.expires_at')) STORED
);

CREATE UNIQUE INDEX approval_records_id_unique ON approval_records(id);
CREATE INDEX approval_records_chain_idx ON approval_records(chain_id, created_at);
-- At most one committed successor per (chain, non-null predecessor).
CREATE UNIQUE INDEX approval_records_successor_unique
    ON approval_records(chain_id, predecessor_ref)
    WHERE predecessor_ref IS NOT NULL;

CREATE TRIGGER approval_records_immutable_update
BEFORE UPDATE ON approval_records
BEGIN
    SELECT RAISE(ABORT, 'approval_records are immutable');
END;

CREATE TRIGGER approval_records_immutable_delete
BEFORE DELETE ON approval_records
BEGIN
    SELECT RAISE(ABORT, 'approval_records are not deletable');
END;

-- Exactly one current head per approval_chain_id. Head revision CAS by repository
-- (never guessed by created_at / max(id)).
CREATE TABLE approval_chain_heads (
    chain_id TEXT PRIMARY KEY,
    current_head_ref TEXT NOT NULL CHECK(length(current_head_ref) > 0),
    head_record_kind TEXT NOT NULL CHECK(head_record_kind IN ('request','resolution','invalidation')),
    updated_at TEXT NOT NULL CHECK(length(updated_at) > 0)
);

-- CredentialRefV1 history: canonical record_json; (credential_id, revision) unique;
-- revision starts at 1 and increments by 1 (repository-enforced); at most one active per id.
CREATE TABLE identity_credentials (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.credential_id') = 'text' AND
        length(json_extract(record_json, '$.credential_id')) > 0 AND
        json_extract(record_json, '$.schema_version') = 1 AND
        json_type(record_json, '$.credential_revision') = 'integer' AND
        json_extract(record_json, '$.credential_revision') >= 1 AND
        json_type(record_json, '$.status') = 'text' AND
        json_extract(record_json, '$.status') IN ('active','revoked','expired')
    ),
    credential_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.credential_id')) STORED,
    revision INTEGER GENERATED ALWAYS AS (json_extract(record_json, '$.credential_revision')) STORED CHECK(revision >= 1),
    status TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.status')) STORED,
    expires_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.expires_at')) STORED,
    UNIQUE(credential_id, revision)
);

CREATE UNIQUE INDEX identity_credentials_one_active
    ON identity_credentials(credential_id)
    WHERE status = 'active';

-- Credential status is a current-state field by contract (active|revoked|expired;
-- revoke/rotate/expiry transitions are repository-only CAS rewrites with canonical
-- readback), so no immutability trigger here; history is preserved by revision chain.

-- Challenge facts (RemoteApprovalChallengeV1 / SystemAuthenticationChallengeV1).
-- Single row per challenge; terminal transitions (issued->consumed|expired|revoked)
-- are repository-only CAS rewrites with canonical readback. challenge_type is an
-- explicit mapping column validated by repository readback (record_json has no such field).
CREATE TABLE identity_challenges (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.challenge_id') = 'text' AND
        length(json_extract(record_json, '$.challenge_id')) > 0 AND
        json_extract(record_json, '$.schema_version') = 1 AND
        json_type(record_json, '$.state') = 'text' AND
        json_extract(record_json, '$.state') IN ('issued','consumed','expired','revoked') AND
        json_type(record_json, '$.request_ref') = 'text' AND
        json_type(record_json, '$.nonce') = 'text' AND
        length(json_extract(record_json, '$.nonce')) > 0
    ),
    challenge_id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.challenge_id')) STORED,
    challenge_type TEXT NOT NULL CHECK(challenge_type IN ('remote','system')),
    state TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.state')) STORED,
    request_ref TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.request_ref')) STORED,
    nonce TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.nonce')) STORED,
    expires_at TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.expires_at')) STORED,
    UNIQUE(challenge_id)
);

CREATE UNIQUE INDEX identity_challenges_nonce_unique ON identity_challenges(challenge_type, nonce);
CREATE UNIQUE INDEX identity_challenges_request_unique ON identity_challenges(challenge_type, request_ref);

-- Evidence facts (LocalPresenceEvidenceV1 / SystemAuthenticationEvidenceV1).
-- Immutable canonical; evidence_type explicit mapping column validated by repository.
CREATE TABLE identity_evidence (
    record_json TEXT NOT NULL CHECK(
        json_valid(record_json) AND
        json_type(record_json, '$') = 'object' AND
        json_type(record_json, '$.id') = 'text' AND
        length(json_extract(record_json, '$.id')) > 0 AND
        json_extract(record_json, '$.schema_version') = 1
    ),
    id TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.id')) STORED,
    evidence_type TEXT NOT NULL CHECK(evidence_type IN ('local_presence','system_authentication')),
    challenge_ref TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.challenge_ref')) STORED,
    valid_until TEXT GENERATED ALWAYS AS (json_extract(record_json, '$.valid_until')) STORED,
    UNIQUE(id)
);

CREATE TRIGGER identity_evidence_immutable_update
BEFORE UPDATE ON identity_evidence
BEGIN
    SELECT RAISE(ABORT, 'identity_evidence is immutable');
END;

CREATE TRIGGER identity_evidence_immutable_delete
BEFORE DELETE ON identity_evidence
BEGIN
    SELECT RAISE(ABORT, 'identity_evidence is not deletable');
END;
