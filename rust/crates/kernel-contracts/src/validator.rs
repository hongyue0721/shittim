//! Runtime JSON Schema validation using Draft 2020-12.

use crate::error::{ContractError, DecodeStage};
use crate::generated::EMBEDDED_SCHEMA_DOCUMENTS;
use jsonschema::{Draft, Retrieve, Uri, ValidationOptions, Validator};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

/// Build the single validator configuration used by both runtime contracts and
/// schema-tool instance validation.
///
/// Draft 2020-12 treats `format` as an annotation unless the implementation is
/// explicitly configured to assert it. Contract validation always asserts known
/// formats and fails closed on unknown formats so CLI and runtime cannot drift.
pub fn contract_validator_options(retriever: impl Retrieve + 'static) -> ValidationOptions {
    let mut options = Validator::options();
    options
        .with_draft(Draft::Draft202012)
        .with_retriever(retriever)
        .should_validate_formats(true)
        .should_ignore_unknown_formats(false);
    options
}

#[derive(Clone)]
pub struct SchemaCatalog {
    validators: Arc<BTreeMap<String, Validator>>,
    documents: Arc<BTreeMap<String, Value>>,
}

impl SchemaCatalog {
    pub fn load_embedded() -> Result<Self, ContractError> {
        let mut documents = BTreeMap::new();
        for (id, raw) in EMBEDDED_SCHEMA_DOCUMENTS {
            let value: Value = serde_json::from_str(raw).map_err(|error| {
                ContractError::Catalog(format!("parse embedded schema {id}: {error}"))
            })?;
            documents.insert((*id).to_string(), value);
        }

        let retriever = CatalogRetriever {
            documents: documents.clone(),
        };
        let mut validators = BTreeMap::new();
        for (id, document) in &documents {
            let validator = contract_validator_options(retriever.clone())
                .build(document)
                .map_err(|error| {
                    ContractError::Catalog(format!("compile embedded schema {id}: {error}"))
                })?;
            validators.insert(id.clone(), validator);
        }
        Ok(Self {
            validators: Arc::new(validators),
            documents: Arc::new(documents),
        })
    }

    pub fn validate(&self, schema_id: &str, instance: &Value) -> Result<(), ContractError> {
        let validator =
            self.validators
                .get(schema_id)
                .ok_or_else(|| ContractError::UnknownSchema {
                    schema_id: schema_id.to_string(),
                })?;
        let errors: Vec<String> = validator
            .iter_errors(instance)
            .map(|error| format!("{} at {}", error, error.instance_path))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ContractError::SchemaValidation {
                schema_id: schema_id.to_string(),
                detail: errors.join("; "),
            })
        }
    }

    pub fn schema_ids(&self) -> Vec<String> {
        self.documents.keys().cloned().collect()
    }

    pub fn document(&self, schema_id: &str) -> Option<&Value> {
        self.documents.get(schema_id)
    }
}

#[derive(Clone)]
struct CatalogRetriever {
    documents: BTreeMap<String, Value>,
}

impl Retrieve for CatalogRetriever {
    fn retrieve(&self, uri: &Uri<&str>) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let key = uri.as_str().trim_end_matches('#').to_string();
        self.documents
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("schema not found: {key}").into())
    }
}

static GLOBAL_CATALOG: OnceLock<Result<SchemaCatalog, String>> = OnceLock::new();

fn global_catalog() -> Result<&'static SchemaCatalog, ContractError> {
    match GLOBAL_CATALOG
        .get_or_init(|| SchemaCatalog::load_embedded().map_err(|error| error.to_string()))
    {
        Ok(catalog) => Ok(catalog),
        Err(message) => Err(ContractError::Catalog(message.clone())),
    }
}

pub fn validate_json(schema_id: &str, instance: &Value) -> Result<(), ContractError> {
    global_catalog()?.validate(schema_id, instance)
}

/// Validate external JSON against the selected embedded Schema before decoding
/// it into a generated or caller-supplied Rust type.
///
/// This is the official generic decode boundary. Direct `serde_json::from_value`
/// remains useful for trusted/internal values, but it does not enforce validation-
/// only facts such as `minLength`, `format`, numeric bounds, or `$ref` composition.
pub fn decode_validated<T: DeserializeOwned>(
    schema_id: &str,
    instance: &Value,
) -> Result<T, ContractError> {
    validate_json(schema_id, instance)?;
    serde_json::from_value(instance.clone()).map_err(|error| ContractError::DecodeAfterSchema {
        schema_id: schema_id.to_string(),
        stage: DecodeStage::TypedDeserialize,
        detail: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct EmptyRetriever;

    impl Retrieve for EmptyRetriever {
        fn retrieve(
            &self,
            uri: &Uri<&str>,
        ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
            Err(format!("unexpected retrieval: {uri}").into())
        }
    }

    #[test]
    fn contract_options_assert_known_formats_and_fail_closed_on_unknown() {
        let known = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "string",
            "format": "uuid"
        });
        let validator = contract_validator_options(EmptyRetriever)
            .build(&known)
            .expect("known format compiles");
        assert!(validator.is_valid(&serde_json::json!("11111111-1111-4111-8111-111111111111")));
        assert!(!validator.is_valid(&serde_json::json!("not-a-uuid")));

        let unknown = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "string",
            "format": "shittim-unknown-format"
        });
        let error = contract_validator_options(EmptyRetriever)
            .build(&unknown)
            .expect_err("unknown format must fail at Schema compilation")
            .to_string();
        assert!(error.contains("shittim-unknown-format"), "{error}");
    }
}
