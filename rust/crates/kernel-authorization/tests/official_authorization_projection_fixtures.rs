//! Production-owner harness for official authorization projection fixtures.
//!
//! Boundary: decode JSON Facts → production project_* → compare normalized object /
//! JCS / sha256. Schema validation of the normalized projection object is performed
//! by production finalize_projection (via kernel-contracts). Tamper cases assert
//! raw_input decode failure or domain InvalidFact only — never unexecuted-layer errors.

use kernel_authorization::{
    project_child_task_delta, project_material_authorization, project_observation_evidence,
    project_subject_projection, AuthorizationProjectionError, CanonicalProjection,
    SubjectProjectionFactsV1,
};
use schema_tool::official_fixture::{
    load_approval_event_allocation_fixture, load_projection_fixture,
    load_subject_projection_fixture, HashRelation, MutationOperation, Preimage,
    ProjectionDomainError, ProjectionExpected, ProjectionFixture, ProjectionResult,
    SubjectProjectionSide, APPROVAL_EVENT_ALLOCATION_TAMPER_CASE_COUNT,
    CHILD_DELTA_TAMPER_CASE_COUNT, MATERIAL_TAMPER_CASE_COUNT,
    OBSERVATION_NOT_APPLICABLE_TAMPER_CASE_COUNT, OBSERVATION_OBSERVED_TAMPER_CASE_COUNT,
    SUBJECT_OPERATION_TAMPER_CASE_COUNT, SUBJECT_PLAN_REVISION_TAMPER_CASE_COUNT,
    SUBJECT_TASK_PROPOSAL_TAMPER_CASE_COUNT,
};
use schema_tool::{apply_json_mutation, JsonPointer};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const CHILD_DELTA_FIXTURE: &str = "schemas/fixtures/task/child_task_delta_projection.v1.json";
const MATERIAL_FIXTURE: &str = "schemas/fixtures/policy/material_authorization_projection.v1.json";
const APPROVAL_EVENT_ALLOCATION_FIXTURE: &str =
    "schemas/fixtures/policy/approval_event_allocation.v1.json";
const SUBJECT_FIXTURE: &str = "schemas/fixtures/policy/subject_projection.v1.json";
const OBSERVATION_NA_FIXTURE: &str =
    "schemas/fixtures/policy/observation_evidence_not_applicable.v1.json";
const OBSERVATION_OBSERVED_FIXTURE: &str =
    "schemas/fixtures/policy/observation_evidence_observed.v1.json";

fn repo_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.pop();
    path
}

fn read_fixture(relative: &str) -> ProjectionFixture {
    load_projection_fixture(repo_root().join(relative)).expect("load validated projection fixture")
}

fn mutate(
    document: &Value,
    operation: MutationOperation,
    pointer: &JsonPointer,
    value: Value,
) -> Value {
    let mut mutated = document.clone();
    apply_json_mutation(&mut mutated, operation.into(), pointer, value)
        .expect("fixture mutation must be structurally valid");
    mutated
}

fn assert_preimage_integrity(value: &Value, stored: &Preimage) {
    assert!(!stored.jcs_utf8_hex.is_empty());
    assert_eq!(stored.sha256.len(), 64);
    assert!(stored
        .jcs_utf8_hex
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert!(stored
        .sha256
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    let bytes = hex::decode(&stored.jcs_utf8_hex).expect("strict lowercase JCS hex");
    assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]), "JCS has BOM");
    assert_ne!(bytes.last(), Some(&b'\n'), "JCS has trailing newline");
    let decoded: Value = serde_json::from_slice(&bytes).expect("JCS is UTF-8 JSON");
    assert_eq!(&decoded, value);
    assert_eq!(hex::encode(Sha256::digest(&bytes)), stored.sha256);
}

fn assert_projection_matches<T>(
    projection: &CanonicalProjection<T>,
    value: &Value,
    stored: &Preimage,
) where
    T: Serialize,
{
    assert_eq!(&serde_json::to_value(&projection.value).unwrap(), value);
    assert_eq!(hex::encode(&projection.jcs_utf8), stored.jcs_utf8_hex);
    assert_eq!(projection.sha256, stored.sha256);
    assert_preimage_integrity(value, stored);
}

fn assert_relation(actual: &str, baseline: &str, relation: HashRelation) {
    match relation {
        HashRelation::Same => assert_eq!(actual, baseline, "expected same hash"),
        HashRelation::Different => assert_ne!(actual, baseline, "expected different hash"),
        HashRelation::NotComputed => panic!("hash_computed case cannot expect not_computed"),
    }
}

enum ProjectionExecution {
    RawInputRejected,
    DomainRejected(ProjectionDomainError),
    Hash(String),
}

fn domain_error(error: AuthorizationProjectionError) -> ProjectionDomainError {
    match error {
        AuthorizationProjectionError::InvalidFact { field, reason } => ProjectionDomainError {
            field: field.to_string(),
            reason: reason.to_string(),
        },
        other => panic!("harness must not borrow non-domain errors: {other}"),
    }
}

fn execute_facts<F, T>(
    raw: &Value,
    project: impl FnOnce(F) -> Result<CanonicalProjection<T>, AuthorizationProjectionError>,
) -> ProjectionExecution
where
    F: DeserializeOwned,
{
    let facts: F = match serde_json::from_value(raw.clone()) {
        Ok(facts) => facts,
        Err(_) => return ProjectionExecution::RawInputRejected,
    };
    match project(facts) {
        Ok(projection) => ProjectionExecution::Hash(projection.sha256),
        Err(error) => ProjectionExecution::DomainRejected(domain_error(error)),
    }
}

fn assert_case(baseline_sha: &str, expected: ProjectionExpected, actual: ProjectionExecution) {
    match (expected.result, actual) {
        (ProjectionResult::RawInputRejected, ProjectionExecution::RawInputRejected) => {
            assert!(!expected.schema_valid);
            assert!(expected.domain_error.is_none());
            assert_eq!(expected.hash_relation, HashRelation::NotComputed);
        }
        (ProjectionResult::DomainRejected, ProjectionExecution::DomainRejected(actual)) => {
            assert!(!expected.schema_valid);
            assert_eq!(expected.domain_error.as_ref(), Some(&actual));
            assert_eq!(expected.hash_relation, HashRelation::NotComputed);
        }
        (ProjectionResult::HashComputed, ProjectionExecution::Hash(actual_sha)) => {
            assert!(expected.schema_valid);
            assert!(expected.domain_error.is_none());
            assert_relation(&actual_sha, baseline_sha, expected.hash_relation);
        }
        (expected, actual) => {
            let actual_label = match actual {
                ProjectionExecution::RawInputRejected => "raw_input_rejected".to_string(),
                ProjectionExecution::DomainRejected(error) => {
                    format!("domain_rejected({}/{})", error.field, error.reason)
                }
                ProjectionExecution::Hash(sha) => format!("hash_computed({sha})"),
            };
            panic!("projection result mismatch: expected {expected:?}, got {actual_label}")
        }
    }
}

fn run_projection_fixture<F, T>(
    relative: &str,
    expected_case_count: usize,
    project: impl Fn(F) -> Result<CanonicalProjection<T>, AuthorizationProjectionError> + Copy,
) where
    F: DeserializeOwned,
    T: Serialize,
{
    let fixture = read_fixture(relative);
    assert_eq!(fixture.tamper_cases.len(), expected_case_count);

    let baseline_facts: F =
        serde_json::from_value(fixture.raw_input.clone()).expect("baseline raw_input decodes");
    let baseline = project(baseline_facts).expect("baseline projection must succeed");
    assert_projection_matches(&baseline, &fixture.normalized_object, &fixture.preimage);

    for case in fixture.tamper_cases {
        let mutated = mutate(
            &fixture.raw_input,
            case.operation,
            &case.pointer,
            case.value,
        );
        let actual = execute_facts(&mutated, project);
        assert_case(&baseline.sha256, case.expected, actual);
    }
}

#[test]
fn child_delta_official_fixture_recomputes_and_enforces_tamper_matrix() {
    run_projection_fixture(
        CHILD_DELTA_FIXTURE,
        CHILD_DELTA_TAMPER_CASE_COUNT,
        project_child_task_delta,
    );
}

#[test]
fn material_official_fixture_recomputes_and_enforces_tamper_matrix() {
    run_projection_fixture(
        MATERIAL_FIXTURE,
        MATERIAL_TAMPER_CASE_COUNT,
        project_material_authorization,
    );
}

fn run_subject_side(side: SubjectProjectionSide, expected_case_count: usize) {
    assert_eq!(side.tamper_cases.len(), expected_case_count);
    let baseline_facts: SubjectProjectionFactsV1 =
        serde_json::from_value(side.raw_input.clone()).expect("subject facts decode");
    let baseline = project_subject_projection(baseline_facts).expect("subject projection");
    assert_projection_matches(&baseline, &side.normalized_object, &side.preimage);
    let projected_subject = match serde_json::to_value(&baseline.value).expect("projection value") {
        Value::Object(mut object) => {
            object.remove("schema_version");
            Value::Object(object)
        }
        _ => panic!("subject projection must be object"),
    };
    assert_eq!(projected_subject, side.subject);

    for case in side.tamper_cases {
        let mutated = mutate(&side.raw_input, case.operation, &case.pointer, case.value);
        let actual = execute_facts(&mutated, project_subject_projection);
        assert_case(&baseline.sha256, case.expected, actual);
    }
}

#[test]
fn subject_projection_official_fixture_recomputes_all_branches_and_tampers() {
    let fixture = load_subject_projection_fixture(repo_root().join(SUBJECT_FIXTURE))
        .expect("load subject projection fixture");
    run_subject_side(
        fixture.branches.operation,
        SUBJECT_OPERATION_TAMPER_CASE_COUNT,
    );
    run_subject_side(
        fixture.branches.task_proposal,
        SUBJECT_TASK_PROPOSAL_TAMPER_CASE_COUNT,
    );
    run_subject_side(
        fixture.branches.plan_revision,
        SUBJECT_PLAN_REVISION_TAMPER_CASE_COUNT,
    );
}

#[test]
fn approval_event_allocation_official_fixture_is_schema_only_and_has_no_preimage() {
    let fixture =
        load_approval_event_allocation_fixture(repo_root().join(APPROVAL_EVENT_ALLOCATION_FIXTURE))
            .expect("load approval event allocation fixture");
    assert_eq!(
        fixture.tamper_cases.len(),
        APPROVAL_EVENT_ALLOCATION_TAMPER_CASE_COUNT
    );
    kernel_contracts::validate_json(&fixture.schema_id, &fixture.valid_allocation)
        .expect("baseline allocation Schema");
    for case in fixture.tamper_cases {
        let mutated = mutate(
            &fixture.valid_allocation,
            case.operation,
            &case.pointer,
            case.value,
        );
        assert_eq!(
            kernel_contracts::validate_json(&fixture.schema_id, &mutated).is_ok(),
            case.schema_valid,
            "{}",
            case.case_id
        );
    }
}

#[test]
fn observation_not_applicable_official_fixture_recomputes_and_enforces_tamper_matrix() {
    run_projection_fixture(
        OBSERVATION_NA_FIXTURE,
        OBSERVATION_NOT_APPLICABLE_TAMPER_CASE_COUNT,
        project_observation_evidence,
    );
}

#[test]
fn observation_observed_official_fixture_recomputes_and_enforces_tamper_matrix() {
    run_projection_fixture(
        OBSERVATION_OBSERVED_FIXTURE,
        OBSERVATION_OBSERVED_TAMPER_CASE_COUNT,
        project_observation_evidence,
    );
}
