use kernel_contracts::{decode_validated, validate_json, Actor, EntryPoint, TaskCreateRequestV2};
use kernel_task_creation::{RootTaskCreateProjection, TaskCreationError};
use schema_tool::official_fixture::{HashRelation, PublicError};
use serde::Deserialize;
use serde_json::{Map, Value};

const ENVELOPE_SCHEMA: &str = "https://schemas.shittim.local/kcp/command_envelope/v2";
const PAYLOAD_SCHEMA: &str = "https://schemas.shittim.local/kcp/task_create_request/v2";

#[derive(Debug)]
pub enum RootExecution {
    RawSchemaRejected(PublicError),
    NormalizationRejected(PublicError),
    Hashes(Box<RootTaskCreateProjection>),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootEnvelopeFacts {
    actor: Actor,
    context: Option<Map<String, Value>>,
    entry_point: EntryPoint,
}

pub fn execute(raw_envelope: &Value) -> RootExecution {
    if validate_json(ENVELOPE_SCHEMA, raw_envelope).is_err() {
        return RootExecution::RawSchemaRejected(invalid_request());
    }
    let payload = raw_envelope
        .get("payload")
        .expect("validated envelope payload");
    let request: TaskCreateRequestV2 = match decode_validated(PAYLOAD_SCHEMA, payload) {
        Ok(request) => request,
        Err(_) => return RootExecution::RawSchemaRejected(invalid_request()),
    };
    let facts: RootEnvelopeFacts = serde_json::from_value(select_envelope_facts(raw_envelope))
        .expect("validated envelope facts decode");
    let input = kernel_task_creation::RootTaskProjectionInput {
        actor: facts.actor,
        context: facts.context,
        entry_point: facts.entry_point,
    };
    match kernel_task_creation::normalize_root_task_create(request, input) {
        Ok(projection) => RootExecution::Hashes(Box::new(projection)),
        Err(error) => RootExecution::NormalizationRejected(public_error(&error)),
    }
}

fn select_envelope_facts(raw_envelope: &Value) -> Value {
    let object = raw_envelope.as_object().expect("validated envelope object");
    Value::Object(Map::from_iter([
        ("actor".to_owned(), object["actor"].clone()),
        ("context".to_owned(), object["context"].clone()),
        ("entry_point".to_owned(), object["entry_point"].clone()),
    ]))
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
