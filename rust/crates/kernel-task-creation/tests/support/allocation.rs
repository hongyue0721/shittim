use kernel_contracts::{
    decode_validated, validate_json, ChildTaskMaterializationAllocationV1,
    RootTaskCreateAllocationV2,
};
use kernel_task_creation::{
    AllocationConflictKind, ChildTaskMaterializationExternalUuidRefsV1,
    RootTaskCreateExternalUuidRefsV1, TaskCreationError,
};
use schema_tool::official_fixture::AllocationDomainResult;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootExternalWire {
    pub command_request_id: String,
    pub delegation_ref: Option<String>,
    pub parent_origin_refs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildExternalWire {
    pub parent_task_id: String,
    pub action_id: String,
    pub permission_decision_id: String,
    pub approval_resolution_ref: Option<String>,
    pub credential_refs: Vec<String>,
    pub challenge_refs: Vec<String>,
    pub delegation_ref: Option<String>,
    pub parent_origin_refs: Vec<String>,
}

pub fn evaluate_root(
    schema_id: &str,
    allocation: &Value,
    external: &RootTaskCreateExternalUuidRefsV1,
) -> (bool, AllocationDomainResult) {
    if validate_json(schema_id, allocation).is_err() {
        return (false, AllocationDomainResult::NotEvaluated);
    }
    let typed: RootTaskCreateAllocationV2 =
        decode_validated(schema_id, allocation).expect("Schema-valid root allocation decode");
    let result = kernel_task_creation::validate_root_task_create_allocation(&typed, external);
    (true, map_domain_result(result))
}

pub fn evaluate_child(
    schema_id: &str,
    allocation: &Value,
    external: &ChildTaskMaterializationExternalUuidRefsV1,
) -> (bool, AllocationDomainResult) {
    if validate_json(schema_id, allocation).is_err() {
        return (false, AllocationDomainResult::NotEvaluated);
    }
    let typed: ChildTaskMaterializationAllocationV1 =
        decode_validated(schema_id, allocation).expect("Schema-valid child allocation decode");
    let result =
        kernel_task_creation::validate_child_task_materialization_allocation(&typed, external);
    (true, map_domain_result(result))
}

pub fn root_external(value: &Value) -> RootTaskCreateExternalUuidRefsV1 {
    let wire: RootExternalWire =
        serde_json::from_value(value.clone()).expect("strict root external refs");
    RootTaskCreateExternalUuidRefsV1 {
        command_request_id: parse(&wire.command_request_id),
        delegation_ref: wire.delegation_ref.as_deref().map(parse),
        parent_origin_refs: wire
            .parent_origin_refs
            .iter()
            .map(|value| parse(value))
            .collect(),
    }
}

pub fn child_external(value: &Value) -> ChildTaskMaterializationExternalUuidRefsV1 {
    let wire: ChildExternalWire =
        serde_json::from_value(value.clone()).expect("strict child external refs");
    ChildTaskMaterializationExternalUuidRefsV1 {
        parent_task_id: parse(&wire.parent_task_id),
        action_id: parse(&wire.action_id),
        permission_decision_id: parse(&wire.permission_decision_id),
        approval_resolution_ref: wire.approval_resolution_ref.as_deref().map(parse),
        credential_refs: wire
            .credential_refs
            .iter()
            .map(|value| parse(value))
            .collect(),
        challenge_refs: wire
            .challenge_refs
            .iter()
            .map(|value| parse(value))
            .collect(),
        delegation_ref: wire.delegation_ref.as_deref().map(parse),
        parent_origin_refs: wire
            .parent_origin_refs
            .iter()
            .map(|value| parse(value))
            .collect(),
    }
}

pub fn assert_canonical_external_uuid_text(value: &Value) {
    fn walk(value: &Value) {
        match value {
            Value::String(text) => assert_eq!(Uuid::parse_str(text).unwrap().to_string(), *text),
            Value::Array(values) => values.iter().for_each(walk),
            Value::Object(values) => values.values().for_each(walk),
            Value::Null => {}
            _ => panic!("external UUID snapshot contains non-string scalar"),
        }
    }
    walk(value);
}

fn parse(value: &str) -> Uuid {
    let parsed = Uuid::parse_str(value).expect("external UUID parses");
    assert_eq!(
        parsed.to_string(),
        value,
        "external UUID text must be canonical lowercase"
    );
    parsed
}

fn map_domain_result(result: Result<(), TaskCreationError>) -> AllocationDomainResult {
    match result {
        Ok(()) => AllocationDomainResult::Accepted,
        Err(TaskCreationError::AllocationConflict {
            kind: AllocationConflictKind::DuplicateInternalUuid,
            ..
        }) => AllocationDomainResult::DuplicateInternalUuid,
        Err(TaskCreationError::AllocationConflict {
            kind: AllocationConflictKind::ExternalUuidCollision,
            ..
        }) => AllocationDomainResult::ExternalUuidCollision,
        Err(TaskCreationError::AllocationConflict {
            kind: AllocationConflictKind::DuplicateOpaque,
            ..
        }) => AllocationDomainResult::DuplicateOpaque,
        Err(TaskCreationError::InvalidAllocationContract { .. }) => {
            panic!("production validator rejected allocation after prior Schema success")
        }
        Err(TaskCreationError::InvalidUuid { .. }) => {
            panic!("Schema-valid allocation contained invalid UUID")
        }
        Err(TaskCreationError::RawContract(_))
        | Err(TaskCreationError::InvalidOriginSourceUri)
        | Err(TaskCreationError::InvalidResourcePattern { .. })
        | Err(TaskCreationError::InvalidExclusion { .. })
        | Err(TaskCreationError::InternalContract(_))
        | Err(TaskCreationError::InternalJson(_)) => {
            panic!("unexpected non-allocation error from allocation validator")
        }
    }
}
