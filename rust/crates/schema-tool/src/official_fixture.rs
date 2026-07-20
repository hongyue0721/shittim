//! Shared parser and invariant owner for official projection and task-creation fixtures.
//!
//! This module is a public, non-stable test-artifact API. It exists so the
//! `schema-tool` CLI oracle and production-owner harnesses read exactly the same
//! wrapper contract. These Rust types are not production business contracts, are
//! not generated SDK types, and must not be added to the Schema manifest or
//! generated artifacts.
//!
//! Authorization projection fixtures reuse the same strict RFC6901 pointer,
//! case-id uniqueness, and expected cross-invariant discipline as the three
//! task-creation wrappers. Their outer shape is the generic single-object form
//! `raw_input` / `normalized_object` / `preimage` / `tamper_cases`.

use crate::{JsonMutationOperation, JsonPointer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const ROOT_TAMPER_CASE_COUNT: usize = 43;
pub const CHILD_TAMPER_CASE_COUNT: usize = 34;
pub const ROOT_ALLOCATION_TAMPER_CASE_COUNT: usize = 7;
pub const CHILD_ALLOCATION_TAMPER_CASE_COUNT: usize = 7;
pub const CHILD_DELTA_TAMPER_CASE_COUNT: usize = 16;
pub const MATERIAL_TAMPER_CASE_COUNT: usize = 16;
pub const OBSERVATION_NOT_APPLICABLE_TAMPER_CASE_COUNT: usize = 3;
pub const OBSERVATION_OBSERVED_TAMPER_CASE_COUNT: usize = 15;

#[derive(Debug, Error)]
pub enum OfficialFixtureError {
    #[error("read official fixture {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("decode official {fixture_kind} fixture: {source}")]
    Decode {
        fixture_kind: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid official fixture contract: {0}")]
    Contract(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootFixture {
    pub fixture_version: u32,
    pub raw_envelope: Value,
    pub normalized_payload: Value,
    pub receipt_preimage: Preimage,
    pub idempotency_projection: Value,
    pub idempotency_preimage: Preimage,
    pub tamper_cases: Vec<RootTamperCase>,
}

impl RootFixture {
    pub fn validate(&self) -> Result<(), OfficialFixtureError> {
        require_version(self.fixture_version, "root")?;
        require_object(&self.raw_envelope, "root.raw_envelope")?;
        require_object(&self.normalized_payload, "root.normalized_payload")?;
        require_object(&self.idempotency_projection, "root.idempotency_projection")?;
        require_nonempty_cases(&self.tamper_cases, "root")?;
        require_unique_case_ids(self.tamper_cases.iter().map(|case| case.case_id.as_str()))?;
        for case in &self.tamper_cases {
            case.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildFixture {
    pub fixture_version: u32,
    pub raw_proposal: Value,
    pub normalized_proposal: Value,
    pub proposal_preimage: Preimage,
    pub tamper_cases: Vec<ChildTamperCase>,
}

impl ChildFixture {
    pub fn validate(&self) -> Result<(), OfficialFixtureError> {
        require_version(self.fixture_version, "child")?;
        require_object(&self.raw_proposal, "child.raw_proposal")?;
        require_object(&self.normalized_proposal, "child.normalized_proposal")?;
        require_nonempty_cases(&self.tamper_cases, "child")?;
        require_unique_case_ids(self.tamper_cases.iter().map(|case| case.case_id.as_str()))?;
        for case in &self.tamper_cases {
            case.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationFixture {
    pub fixture_version: u32,
    pub root: AllocationSide,
    pub child: AllocationSide,
}

/// Generic single-object official fixture used by authorization projections.
///
/// `raw_input` is a JSON encoding of production typed Facts (not a business Schema).
/// `normalized_object` is the Schema-validated projection value. Preimage fields hold
/// exact lowercase JCS hex and SHA-256 only — never a canonical JSON string.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionFixture {
    pub fixture_version: u32,
    pub schema_id: String,
    pub raw_input: Value,
    pub normalized_object: Value,
    pub preimage: Preimage,
    pub tamper_cases: Vec<ProjectionTamperCase>,
}

impl ProjectionFixture {
    pub fn validate(&self) -> Result<(), OfficialFixtureError> {
        require_version(self.fixture_version, "projection")?;
        require_nonempty_text(&self.schema_id, "projection.schema_id")?;
        require_object(&self.raw_input, "projection.raw_input")?;
        require_object(&self.normalized_object, "projection.normalized_object")?;
        require_nonempty_cases(&self.tamper_cases, "projection")?;
        require_unique_case_ids(self.tamper_cases.iter().map(|case| case.case_id.as_str()))?;
        for case in &self.tamper_cases {
            case.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionTamperCase {
    pub case_id: String,
    pub operation: MutationOperation,
    pub pointer: JsonPointer,
    pub value: Value,
    pub expected: ProjectionExpected,
}

impl ProjectionTamperCase {
    fn validate(&self) -> Result<(), OfficialFixtureError> {
        validate_case_identity(&self.case_id, &self.pointer)?;
        self.expected.validate(&self.case_id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionExpected {
    pub result: ProjectionResult,
    pub schema_valid: bool,
    pub domain_error: Option<ProjectionDomainError>,
    pub hash_relation: HashRelation,
}

impl ProjectionExpected {
    fn validate(&self, case_id: &str) -> Result<(), OfficialFixtureError> {
        match self.result {
            ProjectionResult::RawInputRejected => {
                if self.schema_valid {
                    return contract_error(format!(
                        "{case_id}: raw_input_rejected forbids schema_valid true"
                    ));
                }
                require_no_domain_error(case_id, self.domain_error.as_ref(), "raw_input_rejected")?;
                require_relation(
                    case_id,
                    "rejected projection hash",
                    self.hash_relation,
                    HashRelation::NotComputed,
                )
            }
            ProjectionResult::DomainRejected => {
                // Domain rejection may leave the mutated raw input Schema-invalid for the
                // projection object; harness never Schema-validates raw Facts. schema_valid
                // here describes the *normalized_object* path and is fixed false when domain
                // fails before a projection object exists.
                if self.schema_valid {
                    return contract_error(format!(
                        "{case_id}: domain_rejected forbids schema_valid true"
                    ));
                }
                let error = self.domain_error.as_ref().ok_or_else(|| {
                    OfficialFixtureError::Contract(format!(
                        "{case_id}: domain_rejected must carry domain_error"
                    ))
                })?;
                require_nonempty_text(&error.field, &format!("{case_id}.domain_error.field"))?;
                require_nonempty_text(&error.reason, &format!("{case_id}.domain_error.reason"))?;
                require_relation(
                    case_id,
                    "rejected projection hash",
                    self.hash_relation,
                    HashRelation::NotComputed,
                )
            }
            ProjectionResult::HashComputed => {
                if !self.schema_valid {
                    return contract_error(format!(
                        "{case_id}: hash_computed requires schema_valid true"
                    ));
                }
                require_no_domain_error(case_id, self.domain_error.as_ref(), "hash_computed")?;
                require_computed_relation(case_id, "computed projection hash", self.hash_relation)
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectionDomainError {
    pub field: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionResult {
    /// Mutated raw_input cannot be decoded into production typed Facts.
    RawInputRejected,
    /// Typed Facts decode, but production projection owner rejects them.
    DomainRejected,
    /// Projection succeeds and a hash is available for relation checks.
    HashComputed,
}

impl AllocationFixture {
    pub fn validate(&self) -> Result<(), OfficialFixtureError> {
        require_version(self.fixture_version, "allocation")?;
        self.root.validate("allocation.root")?;
        self.child.validate("allocation.child")?;
        require_unique_case_ids(
            self.root
                .tamper_cases
                .iter()
                .chain(self.child.tamper_cases.iter())
                .map(|case| case.case_id.as_str()),
        )?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationSide {
    pub schema_id: String,
    pub external_uuid_refs: Value,
    pub valid_allocation: Value,
    pub tamper_cases: Vec<AllocationTamperCase>,
}

impl AllocationSide {
    fn validate(&self, location: &str) -> Result<(), OfficialFixtureError> {
        require_nonempty_text(&self.schema_id, &format!("{location}.schema_id"))?;
        require_object(
            &self.external_uuid_refs,
            &format!("{location}.external_uuid_refs"),
        )?;
        require_object(
            &self.valid_allocation,
            &format!("{location}.valid_allocation"),
        )?;
        require_nonempty_cases(&self.tamper_cases, location)?;
        for case in &self.tamper_cases {
            case.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preimage {
    pub jcs_utf8_hex: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootTamperCase {
    pub case_id: String,
    pub operation: MutationOperation,
    pub pointer: JsonPointer,
    pub value: Value,
    pub expected: RootExpected,
}

impl RootTamperCase {
    fn validate(&self) -> Result<(), OfficialFixtureError> {
        validate_case_identity(&self.case_id, &self.pointer)?;
        self.expected.validate(&self.case_id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootExpected {
    pub result: RootResult,
    pub public_error: Option<PublicError>,
    pub hash_relations: RootHashRelations,
}

impl RootExpected {
    fn validate(&self, case_id: &str) -> Result<(), OfficialFixtureError> {
        match self.result {
            RootResult::RawSchemaRejected | RootResult::NormalizationRejected => {
                validate_rejection_error(case_id, self.public_error.as_ref())?;
                require_relation(
                    case_id,
                    "rejected receipt",
                    self.hash_relations.receipt,
                    HashRelation::NotComputed,
                )?;
                require_relation(
                    case_id,
                    "rejected idempotency",
                    self.hash_relations.idempotency,
                    HashRelation::NotComputed,
                )
            }
            RootResult::HashesComputed => {
                require_no_public_error(case_id, self.public_error.as_ref(), "hashes_computed")?;
                require_computed_relation(
                    case_id,
                    "computed receipt",
                    self.hash_relations.receipt,
                )?;
                require_computed_relation(
                    case_id,
                    "computed idempotency",
                    self.hash_relations.idempotency,
                )
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootHashRelations {
    pub receipt: HashRelation,
    pub idempotency: HashRelation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildTamperCase {
    pub case_id: String,
    pub operation: MutationOperation,
    pub pointer: JsonPointer,
    pub value: Value,
    pub expected: ChildExpected,
}

impl ChildTamperCase {
    fn validate(&self) -> Result<(), OfficialFixtureError> {
        validate_case_identity(&self.case_id, &self.pointer)?;
        self.expected.validate(&self.case_id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildExpected {
    pub result: ChildResult,
    pub public_error: Option<PublicError>,
    pub hash_relation: HashRelation,
}

impl ChildExpected {
    fn validate(&self, case_id: &str) -> Result<(), OfficialFixtureError> {
        match self.result {
            ChildResult::RawSchemaRejected | ChildResult::NormalizationRejected => {
                validate_rejection_error(case_id, self.public_error.as_ref())?;
                require_relation(
                    case_id,
                    "rejected child hash",
                    self.hash_relation,
                    HashRelation::NotComputed,
                )
            }
            ChildResult::HashComputed => {
                require_no_public_error(case_id, self.public_error.as_ref(), "hash_computed")?;
                require_computed_relation(case_id, "computed child hash", self.hash_relation)
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationTamperCase {
    pub case_id: String,
    pub operation: MutationOperation,
    pub pointer: JsonPointer,
    pub value: Value,
    pub expected: AllocationExpected,
}

impl AllocationTamperCase {
    fn validate(&self) -> Result<(), OfficialFixtureError> {
        validate_case_identity(&self.case_id, &self.pointer)?;
        self.expected.validate(&self.case_id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationExpected {
    pub schema_valid: bool,
    pub domain_result: AllocationDomainResult,
}

impl AllocationExpected {
    fn validate(&self, case_id: &str) -> Result<(), OfficialFixtureError> {
        match (self.schema_valid, self.domain_result) {
            (true, AllocationDomainResult::NotEvaluated) => contract_error(format!(
                "{case_id}: schema_valid true forbids not_evaluated domain_result"
            )),
            (false, AllocationDomainResult::NotEvaluated) | (true, _) => Ok(()),
            (false, _) => contract_error(format!(
                "{case_id}: schema_valid false requires not_evaluated domain_result"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationOperation {
    Add,
    Replace,
}

impl From<MutationOperation> for JsonMutationOperation {
    fn from(value: MutationOperation) -> Self {
        match value {
            MutationOperation::Add => Self::Add,
            MutationOperation::Replace => Self::Replace,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HashRelation {
    Same,
    Different,
    NotComputed,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RootResult {
    RawSchemaRejected,
    NormalizationRejected,
    HashesComputed,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChildResult {
    RawSchemaRejected,
    NormalizationRejected,
    HashComputed,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AllocationDomainResult {
    Accepted,
    DuplicateInternalUuid,
    ExternalUuidCollision,
    DuplicateOpaque,
    NotEvaluated,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PublicError {
    pub code: String,
    pub details: Option<Value>,
}

pub fn load_root_fixture(path: impl AsRef<Path>) -> Result<RootFixture, OfficialFixtureError> {
    let bytes = read_fixture(path)?;
    parse_root_fixture(&bytes)
}

pub fn load_child_fixture(path: impl AsRef<Path>) -> Result<ChildFixture, OfficialFixtureError> {
    let bytes = read_fixture(path)?;
    parse_child_fixture(&bytes)
}

pub fn load_allocation_fixture(
    path: impl AsRef<Path>,
) -> Result<AllocationFixture, OfficialFixtureError> {
    let bytes = read_fixture(path)?;
    parse_allocation_fixture(&bytes)
}

pub fn load_projection_fixture(
    path: impl AsRef<Path>,
) -> Result<ProjectionFixture, OfficialFixtureError> {
    let bytes = read_fixture(path)?;
    parse_projection_fixture(&bytes)
}

pub fn parse_root_fixture(bytes: &[u8]) -> Result<RootFixture, OfficialFixtureError> {
    parse_validated(bytes, "root", RootFixture::validate)
}

pub fn parse_child_fixture(bytes: &[u8]) -> Result<ChildFixture, OfficialFixtureError> {
    parse_validated(bytes, "child", ChildFixture::validate)
}

pub fn parse_allocation_fixture(bytes: &[u8]) -> Result<AllocationFixture, OfficialFixtureError> {
    parse_validated(bytes, "allocation", AllocationFixture::validate)
}

pub fn parse_projection_fixture(bytes: &[u8]) -> Result<ProjectionFixture, OfficialFixtureError> {
    parse_validated(bytes, "projection", ProjectionFixture::validate)
}

fn read_fixture(path: impl AsRef<Path>) -> Result<Vec<u8>, OfficialFixtureError> {
    let path = path.as_ref();
    std::fs::read(path).map_err(|source| OfficialFixtureError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn parse_validated<T>(
    bytes: &[u8],
    fixture_kind: &'static str,
    validate: impl FnOnce(&T) -> Result<(), OfficialFixtureError>,
) -> Result<T, OfficialFixtureError>
where
    T: for<'de> Deserialize<'de>,
{
    let fixture = serde_json::from_slice(bytes).map_err(|source| OfficialFixtureError::Decode {
        fixture_kind,
        source,
    })?;
    validate(&fixture)?;
    Ok(fixture)
}

fn require_version(version: u32, fixture_kind: &str) -> Result<(), OfficialFixtureError> {
    if version == 1 {
        Ok(())
    } else {
        contract_error(format!(
            "{fixture_kind}.fixture_version must be 1, got {version}"
        ))
    }
}

fn require_object(value: &Value, location: &str) -> Result<(), OfficialFixtureError> {
    if value.is_object() {
        Ok(())
    } else {
        contract_error(format!("{location} must be an object"))
    }
}

fn require_nonempty_text(value: &str, location: &str) -> Result<(), OfficialFixtureError> {
    if value.is_empty() {
        contract_error(format!("{location} must be non-empty"))
    } else {
        Ok(())
    }
}

fn require_nonempty_cases<T>(cases: &[T], location: &str) -> Result<(), OfficialFixtureError> {
    if cases.is_empty() {
        contract_error(format!("{location}.tamper_cases must be non-empty"))
    } else {
        Ok(())
    }
}

fn require_unique_case_ids<'a>(
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), OfficialFixtureError> {
    let mut unique = HashSet::new();
    for case_id in ids {
        require_nonempty_text(case_id, "tamper case_id")?;
        if !unique.insert(case_id) {
            return contract_error(format!("duplicate tamper case_id: {case_id}"));
        }
    }
    Ok(())
}

fn validate_case_identity(
    case_id: &str,
    pointer: &JsonPointer,
) -> Result<(), OfficialFixtureError> {
    require_nonempty_text(case_id, "tamper case_id")?;
    if pointer.is_root() {
        contract_error(format!("{case_id}: pointer must not be document root"))
    } else {
        Ok(())
    }
}

fn validate_rejection_error(
    case_id: &str,
    error: Option<&PublicError>,
) -> Result<(), OfficialFixtureError> {
    let error = error.ok_or_else(|| {
        OfficialFixtureError::Contract(format!("{case_id}: rejection must carry public_error"))
    })?;
    require_nonempty_text(&error.code, &format!("{case_id}.public_error.code"))
}

fn require_no_public_error(
    case_id: &str,
    error: Option<&PublicError>,
    result: &str,
) -> Result<(), OfficialFixtureError> {
    if error.is_none() {
        Ok(())
    } else {
        contract_error(format!("{case_id}: {result} forbids public_error"))
    }
}

fn require_no_domain_error(
    case_id: &str,
    error: Option<&ProjectionDomainError>,
    result: &str,
) -> Result<(), OfficialFixtureError> {
    if error.is_none() {
        Ok(())
    } else {
        contract_error(format!("{case_id}: {result} forbids domain_error"))
    }
}

fn require_relation(
    case_id: &str,
    label: &str,
    actual: HashRelation,
    expected: HashRelation,
) -> Result<(), OfficialFixtureError> {
    if actual == expected {
        Ok(())
    } else {
        contract_error(format!(
            "{case_id}: {label} relation must be {expected:?}, got {actual:?}"
        ))
    }
}

fn require_computed_relation(
    case_id: &str,
    label: &str,
    relation: HashRelation,
) -> Result<(), OfficialFixtureError> {
    if relation == HashRelation::NotComputed {
        contract_error(format!("{case_id}: {label} cannot be not_computed"))
    } else {
        Ok(())
    }
}

fn contract_error<T>(message: String) -> Result<T, OfficialFixtureError> {
    Err(OfficialFixtureError::Contract(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_root_case() -> Value {
        json!({
            "case_id": "case",
            "operation": "replace",
            "pointer": "/value",
            "value": true,
            "expected": {
                "result": "hashes_computed",
                "public_error": null,
                "hash_relations": { "receipt": "same", "idempotency": "different" }
            }
        })
    }

    fn minimal_root_fixture() -> Value {
        json!({
            "fixture_version": 1,
            "raw_envelope": {},
            "normalized_payload": {},
            "receipt_preimage": { "jcs_utf8_hex": "00", "sha256": "00" },
            "idempotency_projection": {},
            "idempotency_preimage": { "jcs_utf8_hex": "00", "sha256": "00" },
            "tamper_cases": [minimal_root_case()]
        })
    }

    fn minimal_allocation_fixture() -> Value {
        json!({
            "fixture_version": 1,
            "root": {
                "schema_id": "https://example.test/root",
                "external_uuid_refs": {},
                "valid_allocation": {},
                "tamper_cases": [{
                    "case_id": "root_case",
                    "operation": "replace",
                    "pointer": "/value",
                    "value": true,
                    "expected": { "schema_valid": true, "domain_result": "accepted" }
                }]
            },
            "child": {
                "schema_id": "https://example.test/child",
                "external_uuid_refs": {},
                "valid_allocation": {},
                "tamper_cases": [{
                    "case_id": "child_case",
                    "operation": "add",
                    "pointer": "/value",
                    "value": true,
                    "expected": { "schema_valid": false, "domain_result": "not_evaluated" }
                }]
            }
        })
    }

    #[test]
    fn parser_enforces_version_and_wrapper_shapes() {
        let mut fixture = minimal_root_fixture();
        fixture["fixture_version"] = json!(2);
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("fixture_version must be 1"));

        fixture["fixture_version"] = json!(1);
        fixture["raw_envelope"] = json!([]);
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("raw_envelope must be an object"));
    }

    #[test]
    fn allocation_parser_enforces_cross_side_uniqueness_and_domain_short_circuit() {
        let mut fixture = minimal_allocation_fixture();
        fixture["child"]["tamper_cases"][0]["case_id"] = json!("root_case");
        let error = parse_allocation_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("duplicate tamper case_id"));

        fixture["child"]["tamper_cases"][0]["case_id"] = json!("child_case");
        fixture["child"]["tamper_cases"][0]["expected"]["domain_result"] = json!("accepted");
        let error = parse_allocation_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error
            .to_string()
            .contains("schema_valid false requires not_evaluated"));
    }

    #[test]
    fn rejection_requires_nonempty_public_error_and_not_computed_hashes() {
        let mut fixture = minimal_root_fixture();
        fixture["tamper_cases"][0]["expected"] = json!({
            "result": "raw_schema_rejected",
            "public_error": null,
            "hash_relations": { "receipt": "not_computed", "idempotency": "not_computed" }
        });
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("must carry public_error"));

        fixture["tamper_cases"][0]["expected"]["public_error"] =
            json!({ "code": "", "details": null });
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error
            .to_string()
            .contains("public_error.code must be non-empty"));
    }

    #[test]
    fn parser_enforces_strict_non_root_pointer() {
        let mut fixture = minimal_root_fixture();
        fixture["tamper_cases"][0]["pointer"] = json!("");
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("must not be document root"));

        fixture["tamper_cases"][0]["pointer"] = json!("/bad~2escape");
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(matches!(error, OfficialFixtureError::Decode { .. }));
    }

    #[test]
    fn parser_enforces_unique_nonempty_case_ids() {
        let mut fixture = minimal_root_fixture();
        fixture["tamper_cases"] = json!([minimal_root_case(), minimal_root_case()]);
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("duplicate tamper case_id"));

        fixture["tamper_cases"][1]["case_id"] = json!("");
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("case_id must be non-empty"));
    }

    #[test]
    fn parser_enforces_expected_cross_invariants_and_enum_closure() {
        let mut fixture = minimal_root_fixture();
        fixture["tamper_cases"][0]["expected"]["hash_relations"]["receipt"] = json!("not_computed");
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("cannot be not_computed"));

        fixture["tamper_cases"][0]["expected"]["hash_relations"]["receipt"] = json!("future");
        let error = parse_root_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(matches!(error, OfficialFixtureError::Decode { .. }));
    }

    fn minimal_projection_case() -> Value {
        json!({
            "case_id": "case",
            "operation": "replace",
            "pointer": "/value",
            "value": true,
            "expected": {
                "result": "hash_computed",
                "schema_valid": true,
                "domain_error": null,
                "hash_relation": "same"
            }
        })
    }

    fn minimal_projection_fixture() -> Value {
        json!({
            "fixture_version": 1,
            "schema_id": "https://example.test/projection",
            "raw_input": {},
            "normalized_object": {},
            "preimage": { "jcs_utf8_hex": "00", "sha256": "00" },
            "tamper_cases": [minimal_projection_case()]
        })
    }

    #[test]
    fn projection_parser_enforces_wrapper_and_domain_cross_invariants() {
        let mut fixture = minimal_projection_fixture();
        fixture["schema_id"] = json!("");
        let error = parse_projection_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("schema_id must be non-empty"));

        fixture = minimal_projection_fixture();
        fixture["tamper_cases"][0]["expected"] = json!({
            "result": "domain_rejected",
            "schema_valid": true,
            "domain_error": { "field": "x", "reason": "y" },
            "hash_relation": "not_computed"
        });
        let error = parse_projection_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error
            .to_string()
            .contains("domain_rejected forbids schema_valid true"));

        fixture["tamper_cases"][0]["expected"] = json!({
            "result": "domain_rejected",
            "schema_valid": false,
            "domain_error": null,
            "hash_relation": "not_computed"
        });
        let error = parse_projection_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error.to_string().contains("must carry domain_error"));

        fixture["tamper_cases"][0]["expected"] = json!({
            "result": "raw_input_rejected",
            "schema_valid": false,
            "domain_error": { "field": "x", "reason": "y" },
            "hash_relation": "not_computed"
        });
        let error = parse_projection_fixture(&serde_json::to_vec(&fixture).unwrap()).unwrap_err();
        assert!(error
            .to_string()
            .contains("raw_input_rejected forbids domain_error"));
    }
}
