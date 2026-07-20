//! Structured `serde_json::Value` preflight for the complete first-batch KCP catalog.

use crate::ports::{ResponseContractValidator, SchemaResponseContractValidator};
use crate::response::{validated_error_response_with_validator, SafeWireErrorKind};
use kernel_contracts::{
    validate_json, ContractError, ContractFailureClassification, KcpResponseEnvelope,
    TypedKcpCommandEnvelope, TypedKcpQueryEnvelope, KCP_COMMAND_ENVELOPE_SCHEMA_ID,
    KCP_LEGACY_V1_COMMAND_METHODS, KCP_LEGACY_V1_QUERY_METHODS, KCP_PROTOCOL_VERSION,
    KCP_QUERY_ENVELOPE_SCHEMA_ID,
};
use serde_json::{Map, Value};
use uuid::Uuid;

/// Result of validating and decoding one already parsed KCP value.
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TypedCatalogRequestKind {
    Command(TypedKcpCommandEnvelope),
    Query(TypedKcpQueryEnvelope),
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
            TypedCatalogRequestKind::Command(_) => TypedCatalogRequestFamily::Command,
            TypedCatalogRequestKind::Query(_) => TypedCatalogRequestFamily::Query,
        }
    }

    /// Returns the generated method discriminator.
    pub fn method(&self) -> &str {
        match &self.0 {
            TypedCatalogRequestKind::Command(envelope) => &envelope.command_type,
            TypedCatalogRequestKind::Query(envelope) => &envelope.query_type,
        }
    }

    pub(crate) fn into_kind(self) -> TypedCatalogRequestKind {
        self.0
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

    // Slice 3a activated production MethodVersionBindings + generated
    // select_request_version, but this preflight still consumes retained v1
    // envelopes / KCP_LEGACY_V1_* catalogs only. Runtime switch to method-aware
    // V2 Envelope structure + METHOD_VERSION_BINDINGS is slice 3b; authority
    // catalogs and non-empty bindings must not be treated as executable here.
    let method = match family {
        Family::Command => method_for_family(object, "command_type", KCP_LEGACY_V1_COMMAND_METHODS),
        Family::Query => method_for_family(object, "query_type", KCP_LEGACY_V1_QUERY_METHODS),
    };
    match method {
        MethodCheck::Known => {}
        MethodCheck::MissingOrWrongType => {
            return wire_error(&request_id, SafeWireErrorKind::InvalidRequest, validator)
        }
        MethodCheck::Unsupported => {
            return wire_error(&request_id, SafeWireErrorKind::UnsupportedMethod, validator)
        }
    }

    if let Err(kind) = check_root_schema_version(object) {
        return wire_error(&request_id, kind, validator);
    }

    let schema_id = match family {
        Family::Command => KCP_COMMAND_ENVELOPE_SCHEMA_ID,
        Family::Query => KCP_QUERY_ENVELOPE_SCHEMA_ID,
    };
    if let Err(error) = contract_validator.validate_envelope(schema_id, &value) {
        return match error.classification_for_preflight().classification {
            ContractFailureClassification::CallerInvalid => {
                wire_error(&request_id, SafeWireErrorKind::InvalidRequest, validator)
            }
            ContractFailureClassification::InternalContractFailure => contract_failure(),
        };
    }

    let decoded = match family {
        Family::Command => TypedKcpCommandEnvelope::decode_after_validation(value)
            .map(|envelope| TypedCatalogRequest(TypedCatalogRequestKind::Command(envelope))),
        Family::Query => TypedKcpQueryEnvelope::decode_after_validation(value)
            .map(|envelope| TypedCatalogRequest(TypedCatalogRequestKind::Query(envelope))),
    };
    match decoded {
        Ok(request) => PreflightResult::Accepted(request),
        Err(_) => contract_failure(),
    }
}

#[derive(Debug, Clone, Copy)]
enum Family {
    Command,
    Query,
}

trait PreflightContractValidator {
    fn validate_envelope(&self, schema_id: &str, value: &Value) -> Result<(), ContractError>;
}

#[derive(Debug, Clone, Copy)]
struct SchemaPreflightContractValidator;

impl PreflightContractValidator for SchemaPreflightContractValidator {
    fn validate_envelope(&self, schema_id: &str, value: &Value) -> Result<(), ContractError> {
        validate_json(schema_id, value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MethodCheck {
    Known,
    MissingOrWrongType,
    Unsupported,
}

fn method_for_family(object: &Map<String, Value>, field: &str, catalog: &[&str]) -> MethodCheck {
    match object.get(field) {
        Some(Value::String(method)) if catalog.contains(&method.as_str()) => MethodCheck::Known,
        Some(Value::String(_)) => MethodCheck::Unsupported,
        _ => MethodCheck::MissingOrWrongType,
    }
}

fn check_root_schema_version(object: &Map<String, Value>) -> Result<(), SafeWireErrorKind> {
    let payload = object
        .get("payload")
        .and_then(Value::as_object)
        .ok_or(SafeWireErrorKind::InvalidRequest)?;
    let number = payload
        .get("schema_version")
        .and_then(Value::as_number)
        .ok_or(SafeWireErrorKind::InvalidRequest)?;
    let positive = match (number.as_i64(), number.as_u64()) {
        (Some(value), _) if value > 0 => value == 1,
        (_, Some(value)) if value > 0 => value == 1,
        _ => return Err(SafeWireErrorKind::InvalidRequest),
    };
    if positive {
        Ok(())
    } else {
        Err(SafeWireErrorKind::UnsupportedSchemaVersion)
    }
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
        fn validate_envelope(&self, _schema_id: &str, _value: &Value) -> Result<(), ContractError> {
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
