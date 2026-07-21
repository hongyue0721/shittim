//! Identity repositories: Credential / Challenge / Evidence (IC §6.10.2–§6.10.4).
//!
//! Credential history is append-only revisions; current status transitions
//! (revoke/rotate) are repository-only CAS rewrites with canonical readback.
//! Challenges are single-row facts with terminal CAS discipline
//! (`issued -> consumed|expired|revoked`, terminal states irreversible).
//! `get` never mutates state because time passed; expiry happens only through
//! `expire_challenge_with_expected_state` inside the caller's transaction, which also
//! writes the sole `identity.challenge_expired` Audit and never an Approval event.
//! Evidence facts are immutable canonical records from trusted producers.

use crate::root_task_create_v2::get_audit_v2;
use crate::task::encode_contract_document;
use crate::{StoreError, StoreErrorCode, WriteTransaction};
use chrono::{DateTime, SecondsFormat, Utc};
use kernel_contracts::{
    canonical_json_string, validate_json, Actor, AuditAllocationV2, AuditRecordV2,
    AuditRecordV2AuditType, AuditRecordV2ExternalContentStatus, AuditRecordV2Level,
    AuditRecordV2Outcome, AuditRecordV2RollbackCapability, AuditRecordV2SchemaVersion,
    CredentialRefV1, CredentialRefV1Status, EntryPoint, LocalPresenceEvidenceV1,
    RemoteApprovalChallengeV1, RemoteApprovalChallengeV1State, SystemAuthenticationChallengeV1,
    SystemAuthenticationChallengeV1State, SystemAuthenticationEvidenceV1,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::Value;

const CREDENTIAL_SCHEMA: &str = "https://schemas.shittim.local/policy/credential_ref/v1";
const REMOTE_CHALLENGE_SCHEMA: &str =
    "https://schemas.shittim.local/policy/remote_approval_challenge/v1";
const SYSTEM_CHALLENGE_SCHEMA: &str =
    "https://schemas.shittim.local/policy/system_authentication_challenge/v1";
const LOCAL_EVIDENCE_SCHEMA: &str =
    "https://schemas.shittim.local/policy/local_presence_evidence/v1";
const SYSTEM_EVIDENCE_SCHEMA: &str =
    "https://schemas.shittim.local/policy/system_authentication_evidence/v1";
const AUDIT_V2_SCHEMA: &str = "https://schemas.shittim.local/audit/audit_record/v2";

const CREDENTIAL_SAVEPOINT: &str = "kernel_sqlite_identity_credential";
const CHALLENGE_SAVEPOINT: &str = "kernel_sqlite_identity_challenge";
const EVIDENCE_SAVEPOINT: &str = "kernel_sqlite_identity_evidence";

/// Stored challenge fact: remote or system, distinguished by the explicit mapping column.
#[derive(Debug, Clone, PartialEq)]
pub enum StoredChallenge {
    /// RemoteApprovalChallengeV1.
    Remote(Box<RemoteApprovalChallengeV1>),
    /// SystemAuthenticationChallengeV1.
    System(Box<SystemAuthenticationChallengeV1>),
}

impl StoredChallenge {
    /// Current state name for CAS checks.
    pub fn state(&self) -> &'static str {
        match self {
            StoredChallenge::Remote(challenge) => challenge.state.as_str(),
            StoredChallenge::System(challenge) => challenge.state.as_str(),
        }
    }

    /// Challenge id.
    pub fn challenge_id(&self) -> &str {
        match self {
            StoredChallenge::Remote(challenge) => &challenge.challenge_id,
            StoredChallenge::System(challenge) => &challenge.challenge_id,
        }
    }

    /// Expiry timestamp.
    pub fn expires_at(&self) -> &str {
        match self {
            StoredChallenge::Remote(challenge) => &challenge.expires_at,
            StoredChallenge::System(challenge) => &challenge.expires_at,
        }
    }
}

/// Result of a successful challenge expiry CAS.
#[derive(Debug, Clone, PartialEq)]
pub struct ChallengeExpiredResult {
    /// The challenge that transitioned to `expired`.
    pub challenge_id: String,
    /// Its `expires_at` (expiry moment).
    pub expires_at: String,
    /// The `identity.challenge_expired` Audit written in the same transaction.
    pub audit: AuditRecordV2,
}

impl WriteTransaction<'_> {
    /// Registers a new credential at revision 1. The id must not exist.
    pub fn register_credential(
        &self,
        credential: CredentialRefV1,
    ) -> Result<CredentialRefV1, StoreError> {
        self.with_savepoint(CREDENTIAL_SAVEPOINT, |connection| {
            if credential.credential_revision != 1 {
                return Err(contract_error(
                    "credential register must start at revision 1",
                ));
            }
            if get_credential(connection, &credential.credential_id, None)?.is_some() {
                return Err(StoreError::new(
                    StoreErrorCode::ConstraintViolation,
                    "credential id already exists",
                ));
            }
            insert_credential(connection, &credential)?;
            get_credential(connection, &credential.credential_id, Some(1))?
                .ok_or_else(stored_invalid)
        })
    }

    /// Rotates a credential: CAS-marks the expected current revision replaced and
    /// appends the new revision (`expected_revision + 1`, status active).
    pub fn rotate_credential(
        &self,
        expected_revision: i64,
        new_credential: CredentialRefV1,
        replaced_at: DateTime<Utc>,
    ) -> Result<CredentialRefV1, StoreError> {
        self.with_savepoint(CREDENTIAL_SAVEPOINT, |connection| {
            let current = get_credential(connection, &new_credential.credential_id, None)?
                .ok_or_else(|| contract_error("credential was not found for rotation"))?;
            if current.credential_revision != expected_revision
                || current.status != CredentialRefV1Status::Active
            {
                return Err(StoreError::new(
                    StoreErrorCode::ConstraintViolation,
                    "credential rotation expected current active revision",
                ));
            }
            if new_credential.credential_revision != expected_revision + 1
                || new_credential.status != CredentialRefV1Status::Active
            {
                return Err(contract_error(
                    "rotated credential must be next revision with status active",
                ));
            }
            if new_credential.replaced_by_ref.is_some() {
                return Err(contract_error(
                    "new credential must not carry replaced_by_ref",
                ));
            }
            let mut old = current.clone();
            old.status = CredentialRefV1Status::Revoked;
            old.replaced_by_ref = Some(new_credential.credential_id.clone());
            rewrite_credential(connection, &old, expected_revision, replaced_at)?;
            insert_credential(connection, &new_credential)?;
            get_credential(
                connection,
                &new_credential.credential_id,
                Some(new_credential.credential_revision),
            )?
            .ok_or_else(stored_invalid)
        })
    }

    /// Revokes the current active credential revision (CAS on expected revision).
    pub fn revoke_credential(
        &self,
        credential_id: &str,
        expected_revision: i64,
        revoked_at: DateTime<Utc>,
    ) -> Result<CredentialRefV1, StoreError> {
        self.with_savepoint(CREDENTIAL_SAVEPOINT, |connection| {
            let current = get_credential(connection, credential_id, None)?
                .ok_or_else(|| contract_error("credential was not found for revocation"))?;
            if current.credential_revision != expected_revision
                || current.status != CredentialRefV1Status::Active
            {
                return Err(StoreError::new(
                    StoreErrorCode::ConstraintViolation,
                    "credential revoke expected current active revision",
                ));
            }
            let mut revoked = current;
            revoked.status = CredentialRefV1Status::Revoked;
            rewrite_credential(connection, &revoked, expected_revision, revoked_at)?;
            get_credential(connection, credential_id, Some(expected_revision))?
                .ok_or_else(stored_invalid)
        })
    }

    /// Issues a remote or system challenge (single row, unique id / nonce / request).
    pub fn issue_challenge(
        &self,
        challenge: StoredChallenge,
    ) -> Result<StoredChallenge, StoreError> {
        self.with_savepoint(CHALLENGE_SAVEPOINT, |connection| {
            let (schema, challenge_type) = match &challenge {
                StoredChallenge::Remote(_) => (REMOTE_CHALLENGE_SCHEMA, "remote"),
                StoredChallenge::System(_) => (SYSTEM_CHALLENGE_SCHEMA, "system"),
            };
            let record_json = encode_contract_document(schema, &challenge_value(&challenge))?;
            connection
                .execute(
                    "INSERT INTO identity_challenges(record_json, challenge_type) VALUES (?1, ?2)",
                    params![record_json, challenge_type],
                )
                .map_err(write_error)?;
            get_challenge(connection, challenge.challenge_id())?.ok_or_else(stored_invalid)
        })
    }

    /// Consumes an issued challenge (CAS `issued -> consumed`).
    pub fn consume_challenge(
        &self,
        challenge_id: &str,
        consumed_at: DateTime<Utc>,
    ) -> Result<StoredChallenge, StoreError> {
        self.with_savepoint(CHALLENGE_SAVEPOINT, |connection| {
            let challenge = get_challenge(connection, challenge_id)?
                .ok_or_else(|| contract_error("challenge was not found for consume"))?;
            if challenge.state() != "issued" {
                return Err(terminal_challenge_error(challenge.state()));
            }
            let updated = challenge_with_state(&challenge, "consumed", Some(consumed_at), None)?;
            rewrite_challenge(connection, &challenge, &updated)?;
            get_challenge(connection, challenge_id)?.ok_or_else(stored_invalid)
        })
    }

    /// Revokes an issued challenge (CAS `issued -> revoked`).
    pub fn revoke_challenge(
        &self,
        challenge_id: &str,
        reason: &str,
        revoked_at: DateTime<Utc>,
    ) -> Result<StoredChallenge, StoreError> {
        self.with_savepoint(CHALLENGE_SAVEPOINT, |connection| {
            if reason.trim().is_empty() {
                return Err(contract_error(
                    "challenge revocation reason must be non-empty",
                ));
            }
            let challenge = get_challenge(connection, challenge_id)?
                .ok_or_else(|| contract_error("challenge was not found for revoke"))?;
            if challenge.state() != "issued" {
                return Err(terminal_challenge_error(challenge.state()));
            }
            let updated =
                challenge_with_state(&challenge, "revoked", Some(revoked_at), Some(reason))?;
            rewrite_challenge(connection, &challenge, &updated)?;
            get_challenge(connection, challenge_id)?.ok_or_else(stored_invalid)
        })
    }

    /// Expires an issued challenge (CAS `issued -> expired`).
    ///
    /// Only accepts current `issued` state; requires `expires_at <= expired_at`.
    /// Writes the sole `identity.challenge_expired` Audit (from the caller-injected
    /// `AuditAllocationV2`) in the same transaction; never emits an Approval event.
    pub fn expire_challenge_with_expected_state(
        &self,
        challenge_id: &str,
        expired_at: DateTime<Utc>,
        audit_allocation: AuditAllocationV2,
        entry_point: EntryPoint,
        actor: Option<Actor>,
    ) -> Result<ChallengeExpiredResult, StoreError> {
        self.with_savepoint(CHALLENGE_SAVEPOINT, |connection| {
            let challenge = get_challenge(connection, challenge_id)?
                .ok_or_else(|| contract_error("challenge was not found for expiry"))?;
            if challenge.state() != "issued" {
                return Err(terminal_challenge_error(challenge.state()));
            }
            let expires_at = challenge.expires_at().to_owned();
            let expired_at_text = format_identity_time(expired_at);
            if expired_at_text < expires_at {
                return Err(contract_error(
                    "challenge expiry requires expires_at <= expired_at",
                ));
            }
            let updated = challenge_with_state(&challenge, "expired", None, None)?;
            rewrite_challenge(connection, &challenge, &updated)?;
            let audit =
                build_challenge_expired_audit(&challenge, &audit_allocation, entry_point, actor)?;
            let audit_json = encode_contract_document(AUDIT_V2_SCHEMA, &audit)?;
            connection
                .execute(
                    "INSERT INTO audit_records_v2(record_json) VALUES (?1)",
                    [&audit_json],
                )
                .map_err(write_error)?;
            let stored_audit = get_audit_v2(connection, &audit.id)?.ok_or_else(stored_invalid)?;
            if stored_audit != audit {
                return Err(stored_invalid());
            }
            Ok(ChallengeExpiredResult {
                challenge_id: challenge_id.to_owned(),
                expires_at,
                audit: stored_audit,
            })
        })
    }

    /// Inserts an immutable local-presence evidence record (trusted local producer only).
    pub fn insert_local_presence(
        &self,
        evidence: LocalPresenceEvidenceV1,
    ) -> Result<LocalPresenceEvidenceV1, StoreError> {
        self.with_savepoint(EVIDENCE_SAVEPOINT, |connection| {
            let record_json = encode_contract_document(LOCAL_EVIDENCE_SCHEMA, &evidence)?;
            connection
                .execute(
                    "INSERT INTO identity_evidence(record_json, evidence_type) VALUES (?1, 'local_presence')",
                    [record_json],
                )
                .map_err(write_error)?;
            get_local_presence(connection, &evidence.id)?.ok_or_else(stored_invalid)
        })
    }

    /// Inserts an immutable system-authentication evidence record (registered OS adapter only).
    pub fn insert_system_authentication(
        &self,
        evidence: SystemAuthenticationEvidenceV1,
    ) -> Result<SystemAuthenticationEvidenceV1, StoreError> {
        self.with_savepoint(EVIDENCE_SAVEPOINT, |connection| {
            let record_json = encode_contract_document(SYSTEM_EVIDENCE_SCHEMA, &evidence)?;
            connection
                .execute(
                    "INSERT INTO identity_evidence(record_json, evidence_type) VALUES (?1, 'system_authentication')",
                    [record_json],
                )
                .map_err(write_error)?;
            get_system_authentication(connection, &evidence.id)?.ok_or_else(stored_invalid)
        })
    }
}

impl crate::SqliteStore {
    /// Reads a credential: exact revision when given, else the highest revision.
    pub fn get_credential(
        &self,
        credential_id: &str,
        revision: Option<i64>,
    ) -> Result<Option<CredentialRefV1>, StoreError> {
        let connection = self.lock_connection()?;
        get_credential(&connection, credential_id, revision)
    }

    /// Reads a challenge by id (remote or system). Never mutates state.
    pub fn get_challenge(&self, challenge_id: &str) -> Result<Option<StoredChallenge>, StoreError> {
        let connection = self.lock_connection()?;
        get_challenge(&connection, challenge_id)
    }

    /// Reads a local-presence evidence record by id.
    pub fn get_local_presence(
        &self,
        evidence_id: &str,
    ) -> Result<Option<LocalPresenceEvidenceV1>, StoreError> {
        let connection = self.lock_connection()?;
        get_local_presence(&connection, evidence_id)
    }

    /// Reads a system-authentication evidence record by id.
    pub fn get_system_authentication(
        &self,
        evidence_id: &str,
    ) -> Result<Option<SystemAuthenticationEvidenceV1>, StoreError> {
        let connection = self.lock_connection()?;
        get_system_authentication(&connection, evidence_id)
    }
}

fn insert_credential(
    connection: &Connection,
    credential: &CredentialRefV1,
) -> Result<(), StoreError> {
    let record_json = encode_contract_document(CREDENTIAL_SCHEMA, credential)?;
    connection
        .execute(
            "INSERT INTO identity_credentials(record_json) VALUES (?1)",
            [record_json],
        )
        .map_err(write_error)?;
    Ok(())
}

fn rewrite_credential(
    connection: &Connection,
    credential: &CredentialRefV1,
    expected_revision: i64,
    _at: DateTime<Utc>,
) -> Result<(), StoreError> {
    let record_json = encode_contract_document(CREDENTIAL_SCHEMA, credential)?;
    let changed = connection
        .execute(
            "UPDATE identity_credentials SET record_json = ?1 \
             WHERE credential_id = ?2 AND revision = ?3",
            params![record_json, credential.credential_id, expected_revision],
        )
        .map_err(write_error)?;
    if changed != 1 {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "credential CAS update failed",
        ));
    }
    Ok(())
}

pub(crate) fn get_credential(
    connection: &Connection,
    credential_id: &str,
    revision: Option<i64>,
) -> Result<Option<CredentialRefV1>, StoreError> {
    let stored: Option<String> = match revision {
        Some(revision) => connection
            .query_row(
                "SELECT record_json FROM identity_credentials \
                 WHERE credential_id = ?1 AND revision = ?2",
                params![credential_id, revision],
                |row| row.get(0),
            )
            .optional()
            .map_err(read_error)?,
        None => connection
            .query_row(
                "SELECT record_json FROM identity_credentials \
                 WHERE credential_id = ?1 ORDER BY revision DESC LIMIT 1",
                [credential_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(read_error)?,
    };
    stored
        .map(|stored| decode_document(CREDENTIAL_SCHEMA, &stored))
        .transpose()
}

fn challenge_value(challenge: &StoredChallenge) -> Value {
    match challenge {
        StoredChallenge::Remote(challenge) => serde_json::to_value(challenge).expect("challenge"),
        StoredChallenge::System(challenge) => serde_json::to_value(challenge).expect("challenge"),
    }
}

fn challenge_with_state(
    challenge: &StoredChallenge,
    state: &str,
    at: Option<DateTime<Utc>>,
    reason: Option<&str>,
) -> Result<StoredChallenge, StoreError> {
    let at_text = at.map(format_identity_time);
    match challenge {
        StoredChallenge::Remote(challenge) => {
            let mut next = challenge.clone();
            next.state = match state {
                "consumed" => RemoteApprovalChallengeV1State::Consumed,
                "expired" => RemoteApprovalChallengeV1State::Expired,
                "revoked" => RemoteApprovalChallengeV1State::Revoked,
                _ => return Err(contract_error("unsupported challenge terminal state")),
            };
            if state == "consumed" {
                next.consumed_at = at_text.clone();
            }
            if state == "revoked" {
                next.revoked_at = at_text.clone();
                next.revocation_reason = reason.map(str::to_owned);
            }
            Ok(StoredChallenge::Remote(next))
        }
        StoredChallenge::System(challenge) => {
            let mut next = challenge.clone();
            next.state = match state {
                "consumed" => SystemAuthenticationChallengeV1State::Consumed,
                "expired" => SystemAuthenticationChallengeV1State::Expired,
                "revoked" => SystemAuthenticationChallengeV1State::Revoked,
                _ => return Err(contract_error("unsupported challenge terminal state")),
            };
            if state == "consumed" {
                next.consumed_at = at_text.clone();
            }
            if state == "revoked" {
                next.revoked_at = at_text;
                next.revocation_reason = reason.map(str::to_owned);
            }
            Ok(StoredChallenge::System(next))
        }
    }
}

fn rewrite_challenge(
    connection: &Connection,
    previous: &StoredChallenge,
    next: &StoredChallenge,
) -> Result<(), StoreError> {
    let (schema, challenge_type) = match next {
        StoredChallenge::Remote(_) => (REMOTE_CHALLENGE_SCHEMA, "remote"),
        StoredChallenge::System(_) => (SYSTEM_CHALLENGE_SCHEMA, "system"),
    };
    if previous.challenge_id() != next.challenge_id() {
        return Err(stored_invalid());
    }
    let record_json = encode_contract_document(schema, &challenge_value(next))?;
    let changed = connection
        .execute(
            "UPDATE identity_challenges SET record_json = ?1 \
             WHERE challenge_id = ?2 AND challenge_type = ?3",
            params![record_json, next.challenge_id(), challenge_type],
        )
        .map_err(write_error)?;
    if changed != 1 {
        return Err(StoreError::new(
            StoreErrorCode::ConstraintViolation,
            "challenge CAS update failed",
        ));
    }
    Ok(())
}

pub(crate) fn get_challenge(
    connection: &Connection,
    challenge_id: &str,
) -> Result<Option<StoredChallenge>, StoreError> {
    let stored: Option<(String, String)> = connection
        .query_row(
            "SELECT record_json, challenge_type FROM identity_challenges WHERE challenge_id = ?1",
            [challenge_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(
            |(record_json, challenge_type)| match challenge_type.as_str() {
                "remote" => Ok(StoredChallenge::Remote(decode_document(
                    REMOTE_CHALLENGE_SCHEMA,
                    &record_json,
                )?)),
                "system" => Ok(StoredChallenge::System(decode_document(
                    SYSTEM_CHALLENGE_SCHEMA,
                    &record_json,
                )?)),
                _ => Err(stored_invalid()),
            },
        )
        .transpose()
}

pub(crate) fn get_local_presence(
    connection: &Connection,
    evidence_id: &str,
) -> Result<Option<LocalPresenceEvidenceV1>, StoreError> {
    let stored: Option<(String, String)> = connection
        .query_row(
            "SELECT record_json, evidence_type FROM identity_evidence WHERE id = ?1",
            [evidence_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(|(record_json, evidence_type)| {
            if evidence_type != "local_presence" {
                return Err(stored_invalid());
            }
            decode_document(LOCAL_EVIDENCE_SCHEMA, &record_json)
        })
        .transpose()
}

pub(crate) fn get_system_authentication(
    connection: &Connection,
    evidence_id: &str,
) -> Result<Option<SystemAuthenticationEvidenceV1>, StoreError> {
    let stored: Option<(String, String)> = connection
        .query_row(
            "SELECT record_json, evidence_type FROM identity_evidence WHERE id = ?1",
            [evidence_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(read_error)?;
    stored
        .map(|(record_json, evidence_type)| {
            if evidence_type != "system_authentication" {
                return Err(stored_invalid());
            }
            decode_document(SYSTEM_EVIDENCE_SCHEMA, &record_json)
        })
        .transpose()
}

fn build_challenge_expired_audit(
    challenge: &StoredChallenge,
    allocation: &AuditAllocationV2,
    entry_point: EntryPoint,
    actor: Option<Actor>,
) -> Result<AuditRecordV2, StoreError> {
    if allocation.audit_record_id.trim().is_empty() || allocation.correlation_id.trim().is_empty() {
        return Err(contract_error("audit allocation ids must be non-empty"));
    }
    if actor.is_none() && entry_point != EntryPoint::SystemInternal {
        return Err(contract_error(
            "a null audit actor requires the system_internal entry point",
        ));
    }
    Ok(AuditRecordV2 {
        action_id: None,
        actor,
        approval_resolution_ref: None,
        artifact_refs: vec![],
        audit_type: AuditRecordV2AuditType::IdentityChallengeExpired,
        causation_ref: Some(allocation.causation_ref.clone()),
        content_origin_refs: vec![],
        correlation_id: Some(allocation.correlation_id.clone()),
        delegation_ref: None,
        details: serde_json::json!({}),
        entry_point,
        extension_id: None,
        external_content_status: AuditRecordV2ExternalContentStatus::NotSent,
        id: allocation.audit_record_id.clone(),
        level: AuditRecordV2Level::Security,
        model_call_refs: vec![],
        occurred_at: allocation.occurred_at.clone(),
        outcome: AuditRecordV2Outcome::Succeeded,
        payload_manifest_refs: vec![],
        permission_decision_ref: None,
        policy_context: None,
        provider_id: None,
        reason_codes: vec!["identity.challenge_expired".to_owned()],
        recovery_attempt_ref: None,
        resource_refs: vec![challenge.challenge_id().to_owned()],
        rollback_capability: AuditRecordV2RollbackCapability::Unknown,
        schema_version: AuditRecordV2SchemaVersion,
        stop_fence_generation: None,
        summary: None,
        task_creation_context: None,
        task_id: None,
        verification_result_refs: vec![],
    })
}

/// Formats a UTC timestamp for Identity records (second precision).
pub(crate) fn format_identity_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn decode_document<T: DeserializeOwned>(schema: &str, stored: &str) -> Result<T, StoreError> {
    let value: Value = serde_json::from_str(stored).map_err(|_| stored_invalid())?;
    validate_json(schema, &value).map_err(|_| stored_invalid())?;
    let canonical = canonical_json_string(&value).map_err(|_| stored_invalid())?;
    if canonical != stored {
        return Err(stored_invalid());
    }
    serde_json::from_value(value).map_err(|_| stored_invalid())
}

fn terminal_challenge_error(state: &str) -> StoreError {
    let code = match state {
        "consumed" => StoreErrorCode::ConstraintViolation,
        "expired" => StoreErrorCode::ConstraintViolation,
        "revoked" => StoreErrorCode::ConstraintViolation,
        _ => StoreErrorCode::ContractInvalid,
    };
    StoreError::new(code, "challenge is already in a terminal state")
}

fn contract_error(message: &'static str) -> StoreError {
    StoreError::new(StoreErrorCode::ContractInvalid, message)
}

fn stored_invalid() -> StoreError {
    StoreError::new(
        StoreErrorCode::StoredDataInvalid,
        "stored identity repository data failed integrity validation",
    )
}

fn read_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::StoredDataInvalid)
}

fn write_error(error: rusqlite::Error) -> StoreError {
    StoreError::sqlite(error, StoreErrorCode::InternalStoreError)
}
