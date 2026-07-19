use kernel_contracts::{decode_validated, ChildTaskProposalV1};
use kernel_task_creation::{ChildTaskCreationProjection, TaskCreationError};
use schema_tool::official_fixture::{HashRelation, PublicError};
use serde_json::Value;

const PROPOSAL_SCHEMA: &str = "https://schemas.shittim.local/task/child_task_proposal/v1";

#[derive(Debug)]
pub enum ChildExecution {
    RawSchemaRejected(PublicError),
    NormalizationRejected(PublicError),
    Hash(Box<ChildTaskCreationProjection>),
}

pub fn execute(raw_proposal: &Value) -> ChildExecution {
    let proposal: ChildTaskProposalV1 = match decode_validated(PROPOSAL_SCHEMA, raw_proposal) {
        Ok(proposal) => proposal,
        Err(_) => return ChildExecution::RawSchemaRejected(invalid_request()),
    };
    match kernel_task_creation::normalize_child_task_proposal(proposal) {
        Ok(projection) => ChildExecution::Hash(Box::new(projection)),
        Err(error) => ChildExecution::NormalizationRejected(public_error(&error)),
    }
}

fn public_error(error: &TaskCreationError) -> PublicError {
    let error = error
        .public_error()
        .expect("normalization error must be public");
    PublicError {
        code: error.code.to_owned(),
        details: error.details,
    }
}

fn invalid_request() -> PublicError {
    PublicError {
        code: "invalid_request".to_owned(),
        details: None,
    }
}

pub fn assert_relation(actual: &str, baseline: &str, relation: HashRelation) {
    match relation {
        HashRelation::Same => assert_eq!(actual, baseline),
        HashRelation::Different => assert_ne!(actual, baseline),
        HashRelation::NotComputed => panic!("computed hash for not_computed expectation"),
    }
}
