//! Method-aware `serde_json::Value` preflight for the first-batch KCP catalog.
//!
//! Flow: correlatable request_id → family → protocol → auth → method →
//! payload.schema_version + `select_request_version` → V2 Envelope structure +
//! active method/version Schema + typed decode.

use crate::ports::{ResponseContractValidator, SchemaResponseContractValidator};
use crate::response::{validated_error_response_with_validator, SafeWireErrorKind};
use kernel_contracts::{
    decode_validated, validate_json, ContractError, ContractFailureClassification, EntryPoint,
    KcpMethodFamily, KcpResponseEnvelope, NullOnly, RequestVersionSelection, TaskCreateRequestV2,
    TypedKcpCommandEnvelope, TypedKcpQueryEnvelope, KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS,
    KCP_ENVELOPE_AUTHORITY_QUERY_METHODS, KCP_PROTOCOL_VERSION,
};
use serde_json::{Map, Value};
use uuid::Uuid;

const KCP_COMMAND_ENVELOPE_V2_SCHEMA_ID: &str =
    "https://schemas.shittim.local/kcp/command_envelope/v2";
const KCP_QUERY_ENVELOPE_V2_SCHEMA_ID: &str = "https://schemas.shittim.local/kcp/query_envelope/v2";

/// Result of validating and decoding one already parsed KCP value.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum PreflightResult {
    /// The request belongs to the generated first-batch catalog and is fully typed.
    Accepted(TypedCatalogRequest),
    /// A validated wire error response may be sent to the caller.
    Response(KcpResponseEnvelope),
    /// No wire response may be sent.
    LocalRejection(PreflightLocalRejection),
}

/// Fully typed first-batch catalog request with private construction.
///
/// Values of this type are created only by [`preflight_value`].
#[derive(Debug, Clone, PartialEq)]
pub struct TypedCatalogRequest(TypedCatalogRequestKind);

/// Selected method/version request after method-aware preflight.
///
/// This is intentionally not a generic `TypedKcpCommandEnvelopeV2` wrapper: active
/// `task.create` carries a dedicated root-only v2 request, while retained v1 methods keep
/// their existing typed envelopes.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TypedCatalogRequestKind {
    /// Retained active-v1 command methods (currently only `stop.activate` is in catalog).
    CommandV1(TypedKcpCommandEnvelope),
    /// Retained active-v1 query methods.
    QueryV1(TypedKcpQueryEnvelope),
    /// Active root-only `task.create` v2 request.
    TaskCreateV2(TaskCreateCommandRequestV2),
}

/// Fully decoded active root `task.create` v2 command request.
///
/// Built only by method-aware preflight after V2 Envelope + payload Schema validation.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCreateCommandRequestV2 {
    /// Actor snapshot from the Envelope.
    pub actor: kernel_contracts::Actor,
    /// Required null auth slot.
    pub auth: NullOnly,
    /// Required-nullable business context retained in the idempotency projection.
    pub context: Option<Value>,
    /// RFC 3339 UTC deadline.
    pub deadline: String,
    /// Entry point from the Envelope.
    pub entry_point: EntryPoint,
    /// Must be null for root-only create; handler enforces.
    pub expected_revision: Option<i64>,
    /// Non-empty idempotency key.
    pub idempotency_key: String,
    /// Caller request UUID text preserved verbatim.
    pub request_id: String,
    /// Must be null for root-only create; handler enforces.
    pub task_id: Option<String>,
    /// Fixed discriminator `task.create`.
    pub command_type: String,
    /// Active TaskCreateRequestV2 payload.
    pub payload: TaskCreateRequestV2,
}

/// KCP request family selected during preflight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedCatalogRequestFamily {
    /// Command envelope.
    Command,
    /// Query envelope.
    Query,
}

impl TypedCatalogRequest {
    /// Returns the selected request family.
    pub fn family(&self) -> TypedCatalogRequestFamily {
        match &self.0 {
            TypedCatalogRequestKind::CommandV1(_) | TypedCatalogRequestKind::TaskCreateV2(_) => {
                TypedCatalogRequestFamily::Command
            }
            TypedCatalogRequestKind::QueryV1(_) => TypedCatalogRequestFamily::Query,
        }
    }

    /// Returns the generated method discriminator.
    pub fn method(&self) -> &str {
        match &self.0 {
            TypedCatalogRequestKind::CommandV1(envelope) => &envelope.command_type,
            TypedCatalogRequestKind::QueryV1(envelope) => &envelope.query_type,
            TypedCatalogRequestKind::TaskCreateV2(request) => &request.command_type,
        }
    }

    pub(crate) fn into_kind(self) -> TypedCatalogRequestKind {
        self.0
    }

    /// Test-only constructor for crate-internal narrowing regression tests.
    ///
    /// Active method-aware preflight never produces a typed `task.create` v1 envelope;
    /// constructing one directly is the only way to prove the dispatcher's residual branch
    /// fails closed with an honest internal-contract violation.
    #[cfg(test)]
    pub(crate) fn from_kind_for_test(kind: TypedCatalogRequestKind) -> Self {
        Self(kind)
    }
}

/// Local preflight rejection that must not be serialized as a KCP response.
///
/// This type intentionally does not implement `serde::Serialize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightLocalRejection {
    /// Stable local rejection kind.
    pub kind: PreflightLocalRejectionKind,
    /// Stable safe summary.
    pub message: &'static str,
}

/// Stable local preflight rejection classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightLocalRejectionKind {
    /// No valid request ID was available for a response.
    UncorrelatableRequest,
    /// An internal Schema/generated-code/response contract failed.
    ContractFailure,
}

/// Validates and types one already parsed JSON value.
pub fn preflight_value(value: Value) -> PreflightResult {
    preflight_value_with_seams(
        value,
        &SchemaResponseContractValidator,
        &SchemaPreflightContractValidator,
    )
}

fn preflight_value_with_seams(
    value: Value,
    validator: &impl ResponseContractValidator,
    contract_validator: &impl PreflightContractValidator,
) -> PreflightResult {
    let Some(object) = value.as_object() else {
        return uncorrelatable();
    };
    let Some(request_id) = object.get("request_id").and_then(Value::as_str) else {
        return uncorrelatable();
    };
    if Uuid::parse_str(request_id).is_err() {
        return uncorrelatable();
    }
    let request_id = request_id.to_owned();

    let family = match object.get("message_kind").and_then(Value::as_str) {
        Some("command") => Family::Command,
        Some("query") => Family::Query,
        _ => return wire_error(&request_id, SafeWireErrorKind::InvalidRequest, validator),
    };
    match object.get("protocol_version") {
        Some(Value::String(version)) if version == KCP_PROTOCOL_VERSION => {}
        Some(Value::String(_)) => {
            return wire_error(
                &request_id,
                SafeWireErrorKind::UnsupportedProtocolVersion,
                validator,
            )
        }
        _ => return wire_error(&request_id, SafeWireErrorKind::InvalidRequest, validator),
    }
    match object.get("auth") {
        Some(Value::Null) => {}
        Some(_) => {
            return wire_error(
                &request_id,
                SafeWireErrorKind::UnsupportedAuthSchema,
                validator,
            )
        }
        None => return wire_error(&request_id, SafeWireErrorKind::InvalidRequest, validator),
    }

    let method_field = match family {
        Family::Command => "command_type",
        Family::Query => "query_type",
    };
    let catalog = match family {
        Family::Command => KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS,
        Family::Query => KCP_ENVELOPE_AUTHORITY_QUERY_METHODS,
    };
    let method = match method_for_family(object, method_field, catalog) {
        MethodCheck::Known(method) => method,
        MethodCheck::MissingOrWrongType => {
            return wire_error(&request_id, SafeWireErrorKind::InvalidRequest, validator)
        }
        MethodCheck::Unsupported => {
            return wire_error(&request_id, SafeWireErrorKind::UnsupportedMethod, validator)
        }
    };

    let schema_version = match root_schema_version(object) {
        Ok(version) => version,
        Err(kind) => return wire_error(&request_id, kind, validator),
    };

    let method_family = match family {
        Family::Command => KcpMethodFamily::Command,
        Family::Query => KcpMethodFamily::Query,
    };
    let selection = kernel_contracts::select_request_version(method_family, method, schema_version);
    let request_schema_id = match selection {
        RequestVersionSelection::Active {
            request_schema_id, ..
        } => request_schema_id,
        RequestVersionSelection::LegacyValidationOnly { .. }
        | RequestVersionSelection::Unsupported => {
            return wire_error(
                &request_id,
                SafeWireErrorKind::UnsupportedSchemaVersion,
                validator,
            )
        }
    };

    let envelope_schema_id = match family {
        Family::Command => KCP_COMMAND_ENVELOPE_V2_SCHEMA_ID,
        Family::Query => KCP_QUERY_ENVELOPE_V2_SCHEMA_ID,
    };
    if let Err(error) = contract_validator.validate_schema(envelope_schema_id, &value) {
        return map_contract_error(&request_id, error, validator);
    }

    let Some(payload) = object.get("payload") else {
        return wire_error(&request_id, SafeWireErrorKind::InvalidRequest, validator);
    };
    if let Err(error) = contract_validator.validate_schema(request_schema_id, payload) {
        return map_contract_error(&request_id, error, validator);
    }

    let decoded = match (family, method) {
        (Family::Command, "task.create") => decode_task_create_v2(object, payload)
            .map(|request| TypedCatalogRequest(TypedCatalogRequestKind::TaskCreateV2(request))),
        (Family::Command, _) => TypedKcpCommandEnvelope::decode_after_validation(value)
            .map(|envelope| TypedCatalogRequest(TypedCatalogRequestKind::CommandV1(envelope))),
        (Family::Query, _) => TypedKcpQueryEnvelope::decode_after_validation(value)
            .map(|envelope| TypedCatalogRequest(TypedCatalogRequestKind::QueryV1(envelope))),
    };
    match decoded {
        Ok(request) => PreflightResult::Accepted(request),
        Err(_) => contract_failure(),
    }
}

fn decode_task_create_v2(
    object: &Map<String, Value>,
    payload: &Value,
) -> Result<TaskCreateCommandRequestV2, ContractError> {
    let request: TaskCreateRequestV2 = decode_validated(
        "https://schemas.shittim.local/kcp/task_create_request/v2",
        payload,
    )?;
    let actor = serde_json::from_value(
        object
            .get("actor")
            .cloned()
            .ok_or_else(|| contract_decode_failure("actor"))?,
    )
    .map_err(|_| contract_decode_failure("actor"))?;
    let entry_point = serde_json::from_value(
        object
            .get("entry_point")
            .cloned()
            .ok_or_else(|| contract_decode_failure("entry_point"))?,
    )
    .map_err(|_| contract_decode_failure("entry_point"))?;
    let deadline = object
        .get("deadline")
        .and_then(Value::as_str)
        .ok_or_else(|| contract_decode_failure("deadline"))?
        .to_owned();
    let request_id = object
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(|| contract_decode_failure("request_id"))?
        .to_owned();
    let idempotency_key = object
        .get("idempotency_key")
        .and_then(Value::as_str)
        .ok_or_else(|| contract_decode_failure("idempotency_key"))?
        .to_owned();
    let context = match object.get("context") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.clone()),
    };
    let task_id = match object.get("task_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => return Err(contract_decode_failure("task_id")),
    };
    let expected_revision = match object.get("expected_revision") {
        None | Some(Value::Null) => None,
        Some(Value::Number(number)) => number
            .as_i64()
            .ok_or_else(|| contract_decode_failure("expected_revision"))
            .map(Some)?,
        Some(_) => return Err(contract_decode_failure("expected_revision")),
    };
    Ok(TaskCreateCommandRequestV2 {
        actor,
        auth: NullOnly,
        context,
        deadline,
        entry_point,
        expected_revision,
        idempotency_key,
        request_id,
        task_id,
        command_type: "task.create".into(),
        payload: request,
    })
}

fn contract_decode_failure(field: &str) -> ContractError {
    ContractError::GeneratedDiscriminatorMapping {
        schema_id: KCP_COMMAND_ENVELOPE_V2_SCHEMA_ID.to_owned(),
        discriminator: field.to_owned(),
    }
}

fn map_contract_error(
    request_id: &str,
    error: ContractError,
    validator: &impl ResponseContractValidator,
) -> PreflightResult {
    match error.classification_for_preflight().classification {
        ContractFailureClassification::CallerInvalid => {
            wire_error(request_id, SafeWireErrorKind::InvalidRequest, validator)
        }
        ContractFailureClassification::InternalContractFailure => contract_failure(),
    }
}

#[derive(Debug, Clone, Copy)]
enum Family {
    Command,
    Query,
}

trait PreflightContractValidator {
    fn validate_schema(&self, schema_id: &str, value: &Value) -> Result<(), ContractError>;
}

#[derive(Debug, Clone, Copy)]
struct SchemaPreflightContractValidator;

impl PreflightContractValidator for SchemaPreflightContractValidator {
    fn validate_schema(&self, schema_id: &str, value: &Value) -> Result<(), ContractError> {
        validate_json(schema_id, value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MethodCheck {
    Known(&'static str),
    MissingOrWrongType,
    Unsupported,
}

fn method_for_family(
    object: &Map<String, Value>,
    field: &str,
    catalog: &[&'static str],
) -> MethodCheck {
    match object.get(field) {
        Some(Value::String(method)) => match catalog.iter().copied().find(|item| *item == method) {
            Some(known) => MethodCheck::Known(known),
            None => MethodCheck::Unsupported,
        },
        _ => MethodCheck::MissingOrWrongType,
    }
}

fn root_schema_version(object: &Map<String, Value>) -> Result<u32, SafeWireErrorKind> {
    let payload = object
        .get("payload")
        .and_then(Value::as_object)
        .ok_or(SafeWireErrorKind::InvalidRequest)?;
    let number = payload
        .get("schema_version")
        .and_then(Value::as_number)
        .ok_or(SafeWireErrorKind::InvalidRequest)?;
    let positive = match (number.as_i64(), number.as_u64()) {
        (Some(value), _) if value > 0 => u32::try_from(value).ok(),
        (_, Some(value)) if value > 0 => u32::try_from(value).ok(),
        _ => return Err(SafeWireErrorKind::InvalidRequest),
    };
    positive.ok_or(SafeWireErrorKind::InvalidRequest)
}

fn wire_error(
    request_id: &str,
    kind: SafeWireErrorKind,
    validator: &impl ResponseContractValidator,
) -> PreflightResult {
    match validated_error_response_with_validator(request_id, kind, validator) {
        Ok(response) => PreflightResult::Response(response),
        Err(_) => contract_failure(),
    }
}

fn uncorrelatable() -> PreflightResult {
    PreflightResult::LocalRejection(PreflightLocalRejection {
        kind: PreflightLocalRejectionKind::UncorrelatableRequest,
        message: "request cannot be correlated",
    })
}

fn contract_failure() -> PreflightResult {
    PreflightResult::LocalRejection(PreflightLocalRejection {
        kind: PreflightLocalRejectionKind::ContractFailure,
        message: "preflight contract failure",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{ResponseValidationError, SchemaResponseContractValidator};

    struct RejectResponse;

    impl ResponseContractValidator for RejectResponse {
        fn validate_method_payload(
            &self,
            schema_id: &str,
            value: &Value,
        ) -> Result<(), ResponseValidationError> {
            SchemaResponseContractValidator.validate_method_payload(schema_id, value)
        }

        fn validate_response_envelope(
            &self,
            _value: &Value,
        ) -> Result<(), ResponseValidationError> {
            Err(ResponseValidationError)
        }
    }

    struct UnknownSchema;

    impl PreflightContractValidator for UnknownSchema {
        fn validate_schema(&self, _schema_id: &str, _value: &Value) -> Result<(), ContractError> {
            Err(ContractError::UnknownSchema {
                schema_id: "https://schemas.shittim.local/v1/missing.json".into(),
            })
        }
    }

    #[test]
    fn private_unknown_schema_fault_seam_fails_closed() {
        let value = serde_json::json!({
            "protocol_version":"1.0",
            "message_kind":"query",
            "request_id":"11111111-1111-4111-8111-111111111111",
            "actor":{"schema_version":1,"revision":1,"id":"actor","kind":"known_user","source":"actor-source://local/desktop","authentication_level":"platform_verified","confidence":0.9},
            "entry_point":"local_desktop",
            "auth":null,
            "task_id":null,
            "deadline":"2026-07-18T12:00:10Z",
            "query_type":"system.ping",
            "payload":{"schema_version":1,"echo":null}
        });
        assert_eq!(
            preflight_value_with_seams(value, &SchemaResponseContractValidator, &UnknownSchema,),
            contract_failure()
        );
    }

    #[test]
    fn private_response_fault_seam_fails_closed() {
        let value = serde_json::json!({
            "request_id": "11111111-1111-4111-8111-111111111111",
            "message_kind": "response"
        });
        assert_eq!(
            preflight_value_with_seams(value, &RejectResponse, &SchemaPreflightContractValidator),
            contract_failure()
        );
    }
}
