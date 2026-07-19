mod support;

use schema_tool::official_fixture::{
    AllocationFixture, ChildExpected, ChildFixture, ChildResult, HashRelation, RootExpected,
    RootFixture, RootResult, CHILD_ALLOCATION_TAMPER_CASE_COUNT, CHILD_TAMPER_CASE_COUNT,
    ROOT_ALLOCATION_TAMPER_CASE_COUNT, ROOT_TAMPER_CASE_COUNT,
};
use support::allocation::{
    assert_canonical_external_uuid_text, child_external, evaluate_child, evaluate_root,
    root_external,
};
use support::child::ChildExecution;
use support::preimage::{assert_projection_matches, mutate, read_fixture};
use support::root::RootExecution;

const ROOT_FIXTURE: &str = "schemas/fixtures/kcp/task_create_normalized_hash.v2.json";
const CHILD_FIXTURE: &str = "schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json";
const ALLOCATION_FIXTURE: &str = "schemas/fixtures/task/task_creation_allocations.v1.json";

#[test]
fn root_official_fixture_recomputes_and_enforces_tamper_matrix() {
    let fixture: RootFixture = read_fixture(ROOT_FIXTURE);
    assert_eq!(fixture.tamper_cases.len(), ROOT_TAMPER_CASE_COUNT);

    let baseline = match support::root::execute(&fixture.raw_envelope) {
        RootExecution::Hashes(projection) => projection,
        _ => panic!("official root baseline must compute hashes"),
    };
    assert_projection_matches(
        &baseline.receipt,
        &fixture.normalized_payload,
        &fixture.receipt_preimage,
    );
    assert_projection_matches(
        &baseline.idempotency,
        &fixture.idempotency_projection,
        &fixture.idempotency_preimage,
    );

    for case in fixture.tamper_cases {
        let mutated = mutate(
            &fixture.raw_envelope,
            case.operation,
            &case.pointer,
            case.value,
        );
        assert_root_case(&baseline, case.expected, support::root::execute(&mutated));
    }
}

#[test]
fn child_official_fixture_recomputes_and_enforces_tamper_matrix() {
    let fixture: ChildFixture = read_fixture(CHILD_FIXTURE);
    assert_eq!(fixture.tamper_cases.len(), CHILD_TAMPER_CASE_COUNT);

    let baseline = match support::child::execute(&fixture.raw_proposal) {
        ChildExecution::Hash(projection) => projection,
        _ => panic!("official child baseline must compute hash"),
    };
    assert_projection_matches(
        &baseline.proposal,
        &fixture.normalized_proposal,
        &fixture.proposal_preimage,
    );

    for case in fixture.tamper_cases {
        let mutated = mutate(
            &fixture.raw_proposal,
            case.operation,
            &case.pointer,
            case.value,
        );
        assert_child_case(&baseline, case.expected, support::child::execute(&mutated));
    }
}

#[test]
fn allocation_official_fixture_is_schema_first_and_domain_exhaustive() {
    let fixture: AllocationFixture = read_fixture(ALLOCATION_FIXTURE);
    assert_eq!(
        fixture.root.tamper_cases.len(),
        ROOT_ALLOCATION_TAMPER_CASE_COUNT
    );
    assert_eq!(
        fixture.child.tamper_cases.len(),
        CHILD_ALLOCATION_TAMPER_CASE_COUNT
    );
    assert_canonical_external_uuid_text(&fixture.root.external_uuid_refs);
    assert_canonical_external_uuid_text(&fixture.child.external_uuid_refs);

    let root_external = root_external(&fixture.root.external_uuid_refs);
    assert_eq!(
        evaluate_root(
            &fixture.root.schema_id,
            &fixture.root.valid_allocation,
            &root_external,
        ),
        (
            true,
            schema_tool::official_fixture::AllocationDomainResult::Accepted
        )
    );
    for case in fixture.root.tamper_cases {
        let value = mutate(
            &fixture.root.valid_allocation,
            case.operation,
            &case.pointer,
            case.value,
        );
        assert_eq!(
            evaluate_root(&fixture.root.schema_id, &value, &root_external),
            (case.expected.schema_valid, case.expected.domain_result)
        );
    }

    let child_external = child_external(&fixture.child.external_uuid_refs);
    assert_eq!(
        evaluate_child(
            &fixture.child.schema_id,
            &fixture.child.valid_allocation,
            &child_external,
        ),
        (
            true,
            schema_tool::official_fixture::AllocationDomainResult::Accepted
        )
    );
    for case in fixture.child.tamper_cases {
        let value = mutate(
            &fixture.child.valid_allocation,
            case.operation,
            &case.pointer,
            case.value,
        );
        assert_eq!(
            evaluate_child(&fixture.child.schema_id, &value, &child_external),
            (case.expected.schema_valid, case.expected.domain_result)
        );
    }
}

fn assert_root_case(
    baseline: &kernel_task_creation::RootTaskCreateProjection,
    expected: RootExpected,
    actual: RootExecution,
) {
    match (expected.result, actual) {
        (RootResult::RawSchemaRejected, RootExecution::RawSchemaRejected(error))
        | (RootResult::NormalizationRejected, RootExecution::NormalizationRejected(error)) => {
            assert_eq!(expected.public_error, Some(error));
            assert_eq!(expected.hash_relations.receipt, HashRelation::NotComputed);
            assert_eq!(
                expected.hash_relations.idempotency,
                HashRelation::NotComputed
            );
        }
        (RootResult::HashesComputed, RootExecution::Hashes(actual)) => {
            assert_eq!(expected.public_error, None);
            support::root::assert_relation(
                &actual.receipt.sha256,
                &baseline.receipt.sha256,
                expected.hash_relations.receipt,
            );
            support::root::assert_relation(
                &actual.idempotency.sha256,
                &baseline.idempotency.sha256,
                expected.hash_relations.idempotency,
            );
        }
        (expected, actual) => panic!("root result mismatch: expected {expected:?}, got {actual:?}"),
    }
}

fn assert_child_case(
    baseline: &kernel_task_creation::ChildTaskCreationProjection,
    expected: ChildExpected,
    actual: ChildExecution,
) {
    match (expected.result, actual) {
        (ChildResult::RawSchemaRejected, ChildExecution::RawSchemaRejected(error))
        | (ChildResult::NormalizationRejected, ChildExecution::NormalizationRejected(error)) => {
            assert_eq!(expected.public_error, Some(error));
            assert_eq!(expected.hash_relation, HashRelation::NotComputed);
        }
        (ChildResult::HashComputed, ChildExecution::Hash(actual)) => {
            assert_eq!(expected.public_error, None);
            support::child::assert_relation(
                &actual.proposal.sha256,
                &baseline.proposal.sha256,
                expected.hash_relation,
            );
        }
        (expected, actual) => {
            panic!("child result mismatch: expected {expected:?}, got {actual:?}")
        }
    }
}
