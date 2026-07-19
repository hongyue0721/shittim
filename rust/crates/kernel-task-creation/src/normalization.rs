use crate::projection::projection_from_typed;
use crate::{CanonicalProjection, TaskCreationError};
use domain_policy::{normalize_uri, normalize_uri_pattern};
use kernel_contracts::{
    canonicalize_rfc3339_seconds, decode_validated, validate_json, Actor, ChildTaskProposalV1,
    EntryPoint, NormalizedChildTaskProposalV1, NormalizedRootTaskCreatePayloadV2, NullOnly,
    RootTaskCreateIdempotencyProjectionV1, RootTaskCreateIdempotencyProjectionV1CommandType,
    RootTaskCreateIdempotencyProjectionV1SchemaVersion, TaskCreateRequestV2,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

const ROOT_REQUEST_SCHEMA: &str = "https://schemas.shittim.local/kcp/task_create_request/v2";
const NORMALIZED_ROOT_SCHEMA: &str =
    "https://schemas.shittim.local/task/normalized_root_task_create_payload/v2";
const CHILD_PROPOSAL_SCHEMA: &str = "https://schemas.shittim.local/task/child_task_proposal/v1";
const NORMALIZED_CHILD_SCHEMA: &str =
    "https://schemas.shittim.local/task/normalized_child_task_proposal/v1";
const IDEMPOTENCY_SCHEMA: &str =
    "https://schemas.shittim.local/task/root_task_create_idempotency_projection/v1";

/// Typed envelope facts needed for root idempotency projection.
#[derive(Debug, Clone, PartialEq)]
pub struct RootTaskProjectionInput {
    /// Complete actor revision snapshot.
    pub actor: Actor,
    /// Envelope entry point.
    pub entry_point: EntryPoint,
    /// Required-nullable context, preserved exactly.
    pub context: Option<Map<String, Value>>,
}

/// Root normalized payload, receipt preimage/hash, and idempotency preimage/hash.
#[derive(Debug, Clone, PartialEq)]
pub struct RootTaskCreateProjection {
    /// Normalized payload together with the receipt JCS bytes and hash.
    pub receipt: CanonicalProjection<NormalizedRootTaskCreatePayloadV2>,
    /// Typed idempotency projection plus canonical preimage and hash.
    pub idempotency: CanonicalProjection<RootTaskCreateIdempotencyProjectionV1>,
}

/// Child normalized proposal and its shared proposal/receipt canonical boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct ChildTaskCreationProjection {
    /// Normalized child proposal plus its canonical proposal/receipt bytes and hash.
    pub proposal: CanonicalProjection<NormalizedChildTaskProposalV1>,
}

/// Revalidates and normalizes a typed root request, then constructs receipt and idempotency hashes.
pub fn normalize_root_task_create(
    request: TaskCreateRequestV2,
    envelope: RootTaskProjectionInput,
) -> Result<RootTaskCreateProjection, TaskCreationError> {
    let raw = serialize_and_validate(ROOT_REQUEST_SCHEMA, &request, true)?;
    let normalized_json = normalize_proposal_json(raw)?;
    let normalized: NormalizedRootTaskCreatePayloadV2 =
        decode_internal_exact(NORMALIZED_ROOT_SCHEMA, &normalized_json)?;
    let receipt = projection_from_typed(normalized.clone())?;
    let idempotency = build_idempotency_projection(envelope, normalized)?;
    Ok(RootTaskCreateProjection {
        receipt,
        idempotency,
    })
}

/// Revalidates and normalizes a typed child proposal, then computes its proposal/receipt hash.
pub fn normalize_child_task_proposal(
    proposal: ChildTaskProposalV1,
) -> Result<ChildTaskCreationProjection, TaskCreationError> {
    let raw = serialize_and_validate(CHILD_PROPOSAL_SCHEMA, &proposal, true)?;
    let normalized_json = normalize_proposal_json(raw)?;
    let normalized: NormalizedChildTaskProposalV1 =
        decode_internal_exact(NORMALIZED_CHILD_SCHEMA, &normalized_json)?;
    Ok(ChildTaskCreationProjection {
        proposal: projection_from_typed(normalized)?,
    })
}

fn serialize_and_validate<T: Serialize>(
    schema_id: &str,
    value: &T,
    caller_owned: bool,
) -> Result<Value, TaskCreationError> {
    let json = serde_json::to_value(value).map_err(TaskCreationError::InternalJson)?;
    validate_json(schema_id, &json).map_err(|error| {
        if caller_owned {
            TaskCreationError::RawContract(error)
        } else {
            TaskCreationError::InternalContract(error)
        }
    })?;
    Ok(json)
}

fn decode_internal_exact<T: DeserializeOwned + Serialize>(
    schema_id: &str,
    value: &Value,
) -> Result<T, TaskCreationError> {
    let typed: T =
        decode_validated(schema_id, value).map_err(TaskCreationError::InternalContract)?;
    let typed_json = serde_json::to_value(&typed).map_err(TaskCreationError::InternalJson)?;
    if typed_json != *value {
        return Err(TaskCreationError::InternalContract(
            kernel_contracts::ContractError::InvalidJson(format!(
                "typed roundtrip changed normalized value for {schema_id}"
            )),
        ));
    }
    Ok(typed)
}

fn normalize_proposal_json(mut value: Value) -> Result<Value, TaskCreationError> {
    let root = object_mut(&mut value)?;
    normalize_origin(root)?;
    normalize_scope(root)?;
    Ok(value)
}

fn normalize_origin(root: &mut Map<String, Value>) -> Result<(), TaskCreationError> {
    let origin = member_object_mut(root, "origin")?;
    let source_uri = origin.get("source_uri").ok_or_else(internal_shape)?;
    if let Some(value) = source_uri.as_str() {
        let normalized =
            normalize_uri(value).map_err(|_| TaskCreationError::InvalidOriginSourceUri)?;
        origin.insert("source_uri".to_owned(), Value::String(normalized));
    }
    Ok(())
}

fn normalize_scope(root: &mut Map<String, Value>) -> Result<(), TaskCreationError> {
    let scope = member_object_mut(root, "task_scope")?;
    normalize_pattern_array(scope, "resource_patterns", true)?;
    normalize_pattern_array(scope, "exclusions", false)?;
    if let Some(value) = scope.get("expires_at").and_then(Value::as_str) {
        let normalized = canonicalize_rfc3339_seconds(value).map_err(|error| {
            TaskCreationError::InternalContract(kernel_contracts::ContractError::InvalidJson(
                format!(
                    "Schema-valid expires_at failed canonical timestamp normalization: {error}"
                ),
            ))
        })?;
        scope.insert("expires_at".to_owned(), Value::String(normalized));
    }
    Ok(())
}

fn normalize_pattern_array(
    scope: &mut Map<String, Value>,
    field: &'static str,
    resource_pattern: bool,
) -> Result<(), TaskCreationError> {
    let values = scope
        .get_mut(field)
        .and_then(Value::as_array_mut)
        .ok_or_else(internal_shape)?;
    for (index, value) in values.iter_mut().enumerate() {
        let raw = value.as_str().ok_or_else(internal_shape)?;
        let normalized = normalize_uri_pattern(raw).map_err(|_| {
            if resource_pattern {
                TaskCreationError::InvalidResourcePattern { index }
            } else {
                TaskCreationError::InvalidExclusion { index }
            }
        })?;
        *value = Value::String(normalized);
    }
    Ok(())
}

fn build_idempotency_projection(
    envelope: RootTaskProjectionInput,
    payload: NormalizedRootTaskCreatePayloadV2,
) -> Result<CanonicalProjection<RootTaskCreateIdempotencyProjectionV1>, TaskCreationError> {
    let projection = RootTaskCreateIdempotencyProjectionV1 {
        actor: envelope.actor,
        command_type: RootTaskCreateIdempotencyProjectionV1CommandType::Value,
        context: envelope.context.map(Value::Object),
        entry_point: envelope.entry_point,
        expected_revision: NullOnly,
        payload,
        schema_version: RootTaskCreateIdempotencyProjectionV1SchemaVersion,
        task_id: NullOnly,
    };
    let json = serialize_and_validate(IDEMPOTENCY_SCHEMA, &projection, false)?;
    let typed =
        decode_internal_exact::<RootTaskCreateIdempotencyProjectionV1>(IDEMPOTENCY_SCHEMA, &json)?;
    projection_from_typed(typed)
}

fn object_mut(value: &mut Value) -> Result<&mut Map<String, Value>, TaskCreationError> {
    value.as_object_mut().ok_or_else(internal_shape)
}

fn member_object_mut<'a>(
    root: &'a mut Map<String, Value>,
    field: &'static str,
) -> Result<&'a mut Map<String, Value>, TaskCreationError> {
    root.get_mut(field)
        .and_then(Value::as_object_mut)
        .ok_or_else(internal_shape)
}

fn internal_shape() -> TaskCreationError {
    TaskCreationError::InternalJson(serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "typed task proposal serialized to an unexpected shape",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_contracts::{
        ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, ChildTaskProposalV1SchemaVersion,
        InputContentOriginV1, InputContentOriginV1Kind, InputContentOriginV1ProducerRef,
        InputContentOriginV1ProducerRefKind, InputContentOriginV1SchemaVersion, InputTaskScopeV1,
        InputTaskScopeV1SchemaVersion, NormalizedRootTaskCreatePayloadV2Proposer,
        TaskCreateRequestV2SchemaVersion,
    };
    use proptest::prelude::*;
    use serde_json::json;

    #[test]
    fn root_projection_normalizes_only_owned_fields_and_is_deterministic() {
        let request = root_request();
        let envelope = root_envelope();
        let first = normalize_root_task_create(request.clone(), envelope.clone()).unwrap();
        let second = normalize_root_task_create(request, envelope).unwrap();

        assert_eq!(first.receipt.sha256, second.receipt.sha256);
        assert_eq!(first.receipt.jcs_utf8, second.receipt.jcs_utf8);
        assert_eq!(
            first.receipt.value.task_scope.resource_patterns,
            vec![
                "https://example.com/a/**".to_owned(),
                "https://example.com/a/**".to_owned(),
                "https://example.com/b/*".to_owned(),
            ]
        );
        assert_eq!(first.receipt.value.constraints, vec![" c ", " c "]);
        assert_eq!(
            first.receipt.value.task_scope.expires_at.as_deref(),
            Some("2030-01-01T00:00:00Z")
        );
        assert_eq!(
            first.receipt.value.origin.source_uri.as_deref(),
            Some("https://example.com/a/")
        );
        assert_eq!(
            first.idempotency.value.context,
            Some(json!({"z": [" x ", " x "], "open":{"number":1.25,"null":null}}))
        );
        assert_eq!(first.idempotency.value.task_id, NullOnly);
        assert_eq!(first.idempotency.value.expected_revision, NullOnly);
        assert_ne!(first.receipt.sha256, first.idempotency.sha256);
    }

    #[test]
    fn typed_roundtrip_preserves_required_null_float_and_open_context_exactly() {
        let result = normalize_root_task_create(root_request(), root_envelope()).unwrap();
        let receipt_json = serde_json::to_value(&result.receipt.value).unwrap();
        let idempotency_json = serde_json::to_value(&result.idempotency.value).unwrap();

        assert_eq!(receipt_json.get("delegation_ref"), Some(&Value::Null));
        assert!(receipt_json.get("risk_hint").is_some());
        for required_null in ["expected_revision", "task_id"] {
            assert_eq!(idempotency_json.get(required_null), Some(&Value::Null));
        }
        assert_eq!(idempotency_json["actor"]["confidence"], json!(0.125));
        assert_eq!(
            idempotency_json["context"],
            json!({"z":[" x "," x "],"open":{"number":1.25,"null":null}})
        );
        assert_eq!(
            kernel_contracts::canonical_json_bytes(&receipt_json).unwrap(),
            result.receipt.jcs_utf8
        );
        assert_eq!(
            kernel_contracts::canonical_json_bytes(&idempotency_json).unwrap(),
            result.idempotency.jcs_utf8
        );
    }

    #[test]
    fn uri_equivalence_and_array_sequence_have_exact_hash_semantics() {
        let baseline = normalize_root_task_create(root_request(), root_envelope()).unwrap();

        let mut equivalent = root_request();
        equivalent.origin.source_uri = Some("https://example.com/a/".to_owned());
        equivalent.task_scope.resource_patterns[0] = "https://example.com/a/**".to_owned();
        equivalent.task_scope.exclusions[0] = "https://example.com/a/secret/**".to_owned();
        equivalent.task_scope.expires_at = Some("2030-01-01T00:00:00Z".to_owned());
        let equivalent = normalize_root_task_create(equivalent, root_envelope()).unwrap();
        assert_eq!(baseline.receipt.sha256, equivalent.receipt.sha256);
        assert_eq!(baseline.idempotency.sha256, equivalent.idempotency.sha256);

        let mut reordered = root_request();
        reordered.constraints.reverse();
        reordered.constraints.push("different".to_owned());
        let reordered = normalize_root_task_create(reordered, root_envelope()).unwrap();
        assert_ne!(baseline.receipt.sha256, reordered.receipt.sha256);
        assert_ne!(baseline.idempotency.sha256, reordered.idempotency.sha256);

        let mut duplicate_count = root_request();
        duplicate_count.capability_hints.pop();
        let duplicate_count = normalize_root_task_create(duplicate_count, root_envelope()).unwrap();
        assert_ne!(baseline.receipt.sha256, duplicate_count.receipt.sha256);
    }

    type RootRequestMutation = Box<dyn Fn(&mut TaskCreateRequestV2)>;

    #[test]
    fn every_representative_payload_field_is_hash_bound() {
        let baseline = normalize_root_task_create(root_request(), root_envelope()).unwrap();
        let mutations: Vec<RootRequestMutation> = vec![
            Box::new(|value| value.goal.push('!')),
            Box::new(|value| value.constraints.push("new".to_owned())),
            Box::new(|value| value.success_criteria.push("new".to_owned())),
            Box::new(|value| value.risk_hint = None),
            Box::new(|value| value.capability_hints.push("new".to_owned())),
            Box::new(|value| {
                value.delegation_ref = Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_owned())
            }),
            Box::new(|value| {
                value
                    .task_scope
                    .allowed_capability_hints
                    .push("new".to_owned())
            }),
            Box::new(|value| value.origin.upstream_stable_id = None),
        ];
        for mutate in mutations {
            let mut request = root_request();
            mutate(&mut request);
            let changed = normalize_root_task_create(request, root_envelope()).unwrap();
            assert_ne!(baseline.receipt.sha256, changed.receipt.sha256);
            assert_ne!(baseline.idempotency.sha256, changed.idempotency.sha256);
        }
    }

    #[test]
    fn canonical_timestamp_failure_after_raw_schema_is_internal_only() {
        let mut value = serde_json::to_value(root_request()).unwrap();
        value["task_scope"]["expires_at"] = json!("2030-01-01T00:00:00.001Z");
        let error = normalize_proposal_json(value).unwrap_err();
        assert!(matches!(error, TaskCreationError::InternalContract(_)));
        assert!(error.public_error().is_none());
    }

    #[test]
    fn invalid_normalized_shape_is_internal_contract_failure() {
        let mut normalized = serde_json::to_value(root_request()).unwrap();
        normalized["schema_version"] = json!(2);
        normalized["task_scope"]["resource_patterns"] = json!([""]);
        let error = decode_internal_exact::<NormalizedRootTaskCreatePayloadV2>(
            NORMALIZED_ROOT_SCHEMA,
            &normalized,
        )
        .unwrap_err();
        assert!(matches!(error, TaskCreationError::InternalContract(_)));
        assert!(error.public_error().is_none());
    }

    #[test]
    fn root_context_changes_only_idempotency_hash() {
        let request = root_request();
        let first = normalize_root_task_create(request.clone(), root_envelope()).unwrap();
        let mut envelope = root_envelope();
        envelope.context = Some(Map::from_iter([("trace".to_owned(), json!("changed"))]));
        let second = normalize_root_task_create(request, envelope).unwrap();
        assert_eq!(first.receipt.sha256, second.receipt.sha256);
        assert_ne!(first.idempotency.sha256, second.idempotency.sha256);
    }

    #[test]
    fn child_projection_is_independent_and_hash_binds_representative_fields() {
        let baseline = normalize_child_task_proposal(child_proposal()).unwrap();
        assert_eq!(
            serde_json::to_value(&baseline.proposal.value).unwrap()["schema_version"],
            1
        );
        assert_eq!(baseline.proposal.value.constraints, vec![" c ", " c "]);
        assert_eq!(
            baseline.proposal.value.task_scope.resource_patterns.len(),
            3
        );

        let mutations: [fn(&mut ChildTaskProposalV1); 3] = [
            |value| value.goal.push('!'),
            |value| value.constraints.push("new".to_owned()),
            |value| {
                value.origin.upstream_stable_id = None;
            },
        ];
        for mutate in mutations {
            let mut changed = child_proposal();
            mutate(&mut changed);
            let changed = normalize_child_task_proposal(changed).unwrap();
            assert_ne!(baseline.proposal.sha256, changed.proposal.sha256);
        }

        let mut equivalent = child_proposal();
        equivalent.origin.source_uri = Some("https://example.com/a/".to_owned());
        equivalent.task_scope.resource_patterns[0] = "https://example.com/a/**".to_owned();
        equivalent.task_scope.exclusions[0] = "https://example.com/a/secret/**".to_owned();
        equivalent.task_scope.expires_at = Some("2030-01-01T00:00:00Z".to_owned());
        let equivalent = normalize_child_task_proposal(equivalent).unwrap();
        assert_eq!(baseline.proposal.sha256, equivalent.proposal.sha256);
    }

    #[test]
    fn uri_errors_report_precise_kind_and_index() {
        let mut root = root_request();
        root.origin.source_uri = Some("https://example.com/*".to_owned());
        let error = normalize_root_task_create(root, root_envelope()).unwrap_err();
        assert!(matches!(error, TaskCreationError::InvalidOriginSourceUri));
        assert_eq!(
            error.public_error().unwrap().details,
            Some(json!({"input_kind":"origin_source_uri","index":null}))
        );

        let mut root = root_request();
        root.task_scope.resource_patterns[1] = "https://example.com/a/**/bad*".to_owned();
        assert!(matches!(
            normalize_root_task_create(root, root_envelope()).unwrap_err(),
            TaskCreationError::InvalidResourcePattern { index: 1 }
        ));

        let mut child = child_proposal();
        child.task_scope.exclusions[0] = "https://example.com/a/bad*".to_owned();
        assert!(matches!(
            normalize_child_task_proposal(child).unwrap_err(),
            TaskCreationError::InvalidExclusion { index: 0 }
        ));
    }

    #[test]
    fn raw_schema_is_rechecked_before_normalization() {
        let mut root = root_request();
        root.task_scope.expires_at = Some("2030-01-01T00:00:00.001Z".to_owned());
        let error = normalize_root_task_create(root, root_envelope()).unwrap_err();
        assert!(matches!(error, TaskCreationError::RawContract(_)));
        assert_eq!(error.public_error().unwrap().code, "invalid_request");
    }

    proptest! {
        #[test]
        fn ordinary_strings_are_never_trimmed(left in " +", right in " +") {
            let mut root = root_request();
            root.goal = format!("{left}goal{right}");
            let expected = root.goal.clone();
            let result = normalize_root_task_create(root, root_envelope()).unwrap();
            prop_assert_eq!(result.receipt.value.goal, expected);
        }
    }

    fn root_request() -> TaskCreateRequestV2 {
        TaskCreateRequestV2 {
            capability_hints: vec!["kernel.task".to_owned(), "kernel.task".to_owned()],
            constraints: vec![" c ".to_owned(), " c ".to_owned()],
            delegation_ref: None,
            goal: " keep spaces ".to_owned(),
            origin: origin(),
            proposer: NormalizedRootTaskCreatePayloadV2Proposer::User,
            risk_hint: Some(" low ".to_owned()),
            schema_version: TaskCreateRequestV2SchemaVersion,
            success_criteria: vec!["done".to_owned(), "done".to_owned()],
            task_scope: scope(),
        }
    }

    fn child_proposal() -> ChildTaskProposalV1 {
        ChildTaskProposalV1 {
            capability_hints: vec!["kernel.task".to_owned(), "kernel.task".to_owned()],
            constraints: vec![" c ".to_owned(), " c ".to_owned()],
            delegation_ref: None,
            goal: " keep spaces ".to_owned(),
            origin: origin(),
            proposer: NormalizedRootTaskCreatePayloadV2Proposer::Companion,
            risk_hint: Some(" low ".to_owned()),
            schema_version: ChildTaskProposalV1SchemaVersion,
            success_criteria: vec!["done".to_owned(), "done".to_owned()],
            task_scope: scope(),
        }
    }

    fn scope() -> InputTaskScopeV1 {
        InputTaskScopeV1 {
            allowed_capability_hints: vec!["fs.read".to_owned(), "fs.read".to_owned()],
            exclusions: vec!["HTTPS://EXAMPLE.COM:443/a/secret/**".to_owned()],
            expires_at: Some("2030-01-01T08:00:00.000+08:00".to_owned()),
            resource_patterns: vec![
                "HTTPS://EXAMPLE.COM:443/a/./**".to_owned(),
                "https://example.com/a/**".to_owned(),
                "https://example.com/b/*".to_owned(),
            ],
            schema_version: InputTaskScopeV1SchemaVersion,
        }
    }

    fn origin() -> InputContentOriginV1 {
        InputContentOriginV1 {
            kind: InputContentOriginV1Kind::DocumentContent,
            parent_origin_refs: vec![
                "11111111-1111-4111-8111-111111111111".to_owned(),
                "11111111-1111-4111-8111-111111111111".to_owned(),
            ],
            producer_ref: InputContentOriginV1ProducerRef {
                id: " producer ".to_owned(),
                kind: InputContentOriginV1ProducerRefKind::Extension,
            },
            schema_version: InputContentOriginV1SchemaVersion,
            source_uri: Some("HTTPS://EXAMPLE.COM:443/a/./".to_owned()),
            upstream_stable_id: Some(" upstream ".to_owned()),
        }
    }

    fn root_envelope() -> RootTaskProjectionInput {
        RootTaskProjectionInput {
            actor: Actor {
                authentication_level: ActorAuthenticationLevel::Unauthenticated,
                confidence: Some(0.125),
                id: "actor-1".to_owned(),
                kind: ActorKind::KnownUser,
                revision: 1,
                schema_version: ActorSchemaVersion,
                source: "actor-source://local/desktop".to_owned(),
            },
            entry_point: EntryPoint::LocalDesktop,
            context: Some(Map::from_iter([
                ("z".to_owned(), json!([" x ", " x "])),
                ("open".to_owned(), json!({"number":1.25,"null":null})),
            ])),
        }
    }
}
