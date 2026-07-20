use kernel_authorization::{project_subject_projection, SubjectProjectionFactsV1};
use kernel_contracts::{SideEffectClass, SubjectProjectionV1};
use serde_json::json;
use uuid::Uuid;

fn uuid(n: u8) -> Uuid {
    Uuid::parse_str(&format!("00000000-0000-4000-8000-0000000000{n:02}")).unwrap()
}

fn hash(character: char) -> String {
    std::iter::repeat_n(character, 64).collect()
}

fn operation() -> SubjectProjectionFactsV1 {
    SubjectProjectionFactsV1::Operation {
        task_id: uuid(1),
        task_revision: 2,
        task_plan_version: 1,
        action_id: uuid(2),
        action_revision: 3,
        permission_decision_ref: uuid(3),
        permission_decision_revision: 4,
        policy_set_revision: 5,
        material_authorization_fingerprint: hash('a'),
        capability_id: "computer.input".into(),
        operation: "click".into(),
        side_effect_class: SideEffectClass::S2,
        resource_refs_hash: hash('b'),
        key_params_hash: hash('c'),
    }
}

#[test]
fn projects_all_subject_branches_with_exact_schema_shapes() {
    let projected = project_subject_projection(operation()).expect("operation");
    assert!(matches!(
        projected.value,
        SubjectProjectionV1::Operation { .. }
    ));

    let projected = project_subject_projection(SubjectProjectionFactsV1::TaskProposal {
        candidate_task_id: uuid(4),
        candidate_revision: 1,
        proposal_hash: hash('d'),
        proposer_actor_ref: "actor-local-1".into(),
        task_scope_hash: hash('e'),
        delegation_ref: None,
        policy_set_revision: 5,
    })
    .expect("task proposal");
    assert!(matches!(
        projected.value,
        SubjectProjectionV1::TaskProposal { .. }
    ));

    let projected = project_subject_projection(SubjectProjectionFactsV1::PlanRevision {
        task_id: uuid(1),
        task_revision: 2,
        base_plan_version: 1,
        proposed_plan_version: 2,
        proposed_plan_hash: hash('f'),
        policy_set_revision: 5,
    })
    .expect("plan revision");
    assert!(matches!(
        projected.value,
        SubjectProjectionV1::PlanRevision { .. }
    ));
}

#[test]
fn every_operation_subject_field_is_hash_bound() {
    let baseline = serde_json::to_value(operation()).unwrap();
    let baseline_projection = project_subject_projection(operation()).unwrap();
    let mutations = [
        ("/task_id", json!("00000000-0000-4000-8000-000000000014")),
        ("/task_revision", json!(3)),
        ("/task_plan_version", json!(2)),
        ("/action_id", json!("00000000-0000-4000-8000-000000000015")),
        ("/action_revision", json!(4)),
        (
            "/permission_decision_ref",
            json!("00000000-0000-4000-8000-000000000016"),
        ),
        ("/permission_decision_revision", json!(5)),
        ("/policy_set_revision", json!(6)),
        ("/material_authorization_fingerprint", json!(hash('1'))),
        ("/capability_id", json!("computer.keyboard")),
        ("/operation", json!("type")),
        ("/side_effect_class", json!("S3")),
        ("/resource_refs_hash", json!(hash('2'))),
        ("/key_params_hash", json!(hash('3'))),
    ];
    for (pointer, replacement) in mutations {
        let mut mutated = baseline.clone();
        *mutated.pointer_mut(pointer).expect("pointer") = replacement;
        let facts: SubjectProjectionFactsV1 = serde_json::from_value(mutated).unwrap();
        let projected = project_subject_projection(facts).unwrap();
        assert_ne!(projected.sha256, baseline_projection.sha256, "{pointer}");
    }
}

#[test]
fn invalid_subject_facts_fail_before_hashing() {
    let mut invalid = operation();
    if let SubjectProjectionFactsV1::Operation { task_revision, .. } = &mut invalid {
        *task_revision = 0;
    }
    assert!(project_subject_projection(invalid).is_err());

    let invalid = SubjectProjectionFactsV1::TaskProposal {
        candidate_task_id: uuid(4),
        candidate_revision: 1,
        proposal_hash: hash('D'),
        proposer_actor_ref: "actor-local-1".into(),
        task_scope_hash: hash('e'),
        delegation_ref: None,
        policy_set_revision: 5,
    };
    assert!(project_subject_projection(invalid).is_err());
}
