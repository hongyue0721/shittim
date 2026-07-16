//! Runtime JSON Schema validation using Draft 2020-12.

use crate::error::ContractError;
use crate::generated::EMBEDDED_SCHEMA_DOCUMENTS;
use jsonschema::{Draft, Retrieve, Uri, Validator};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

#[derive(Clone)]
pub struct SchemaCatalog {
    validators: Arc<BTreeMap<String, Validator>>,
    documents: Arc<BTreeMap<String, Value>>,
}

impl SchemaCatalog {
    pub fn load_embedded() -> Result<Self, ContractError> {
        let mut documents = BTreeMap::new();
        for (id, raw) in EMBEDDED_SCHEMA_DOCUMENTS {
            let value: Value = serde_json::from_str(raw)
                .map_err(|error| ContractError::Catalog(format!("parse {id}: {error}")))?;
            documents.insert((*id).to_string(), value);
        }

        let retriever = CatalogRetriever {
            documents: documents.clone(),
        };
        let mut validators = BTreeMap::new();
        for (id, document) in &documents {
            let validator = Validator::options()
                .with_draft(Draft::Draft202012)
                .with_retriever(retriever.clone())
                .build(document)
                .map_err(|error| ContractError::Catalog(format!("compile {id}: {error}")))?;
            validators.insert(id.clone(), validator);
        }
        Ok(Self {
            validators: Arc::new(validators),
            documents: Arc::new(documents),
        })
    }

    pub fn validate(&self, schema_id: &str, instance: &Value) -> Result<(), ContractError> {
        let validator = self
            .validators
            .get(schema_id)
            .ok_or_else(|| ContractError::UnknownSchema(schema_id.to_string()))?;
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
