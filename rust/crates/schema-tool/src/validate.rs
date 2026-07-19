use crate::error::SchemaToolError;
use crate::json_pointer::{select_json_value, JsonPointer};
use crate::manifest::SchemaRegistry;
use crate::paths;
use anyhow::{Context, Result};
use jsonschema::{Retrieve, Uri, Validator};
use kernel_contracts::validator::contract_validator_options;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Library request for validating a selected value from a JSON document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateSelectedRequest {
    pub schema_selector: String,
    pub instance_path: PathBuf,
    pub pointer: JsonPointer,
}

impl ValidateSelectedRequest {
    pub fn new(
        schema_selector: impl Into<String>,
        instance_path: impl Into<PathBuf>,
        pointer: JsonPointer,
    ) -> Self {
        Self {
            schema_selector: schema_selector.into(),
            instance_path: instance_path.into(),
            pointer,
        }
    }
}

/// Successful selected-value validation facts for a thin CLI or another tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateSelectedResult {
    pub schema_id: String,
    pub instance_path: PathBuf,
    pub pointer: JsonPointer,
}

/// Load a registry and validate one selected value from a JSON file.
pub fn validate_selected_request(
    repo_root: &Path,
    request: &ValidateSelectedRequest,
) -> Result<ValidateSelectedResult> {
    let registry = SchemaRegistry::load(repo_root)?;
    let loaded = registry.resolve_schema_selector(&request.schema_selector)?;
    let document = paths::read_json_file(&request.instance_path)?;
    let selected = select_json_value(&document, &request.pointer)?;
    validate_instance(&registry, &request.schema_selector, selected).map_err(anyhow::Error::new)?;
    Ok(ValidateSelectedResult {
        schema_id: loaded.entry.id.clone(),
        instance_path: request.instance_path.clone(),
        pointer: request.pointer.clone(),
    })
}

/// Compatibility wrapper for root-document validation.
pub fn run(repo_root: &Path, schema_selector: &str, instance_path: &Path) -> Result<()> {
    let request = ValidateSelectedRequest::new(schema_selector, instance_path, JsonPointer::root());
    let result = validate_selected_request(repo_root, &request)?;
    println!("{}", render_success(&result));
    Ok(())
}

/// Render the CLI success line. Root validation preserves the historical text;
/// selected validation appends the canonical pointer.
pub fn render_success(result: &ValidateSelectedResult) -> String {
    let base = format!(
        "valid: instance {} against {}",
        result.instance_path.display(),
        result.schema_id
    );
    if result.pointer.is_root() {
        base
    } else {
        format!("{base} at pointer {:?}", result.pointer.as_str())
    }
}

pub fn validate_examples(
    _repo_root: &Path,
    registry: &SchemaRegistry,
    examples_dir: &Path,
) -> Result<()> {
    // Convention: schemas/examples/<domain>/<name>.json may include
    // { "$schema_id": "...", "instance": { ... } }
    // or be a bare instance if filename maps to a known schema via sidecar .schema_id file.
    for entry in walkdir::WalkDir::new(examples_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("json")
        })
        .filter(|entry| {
            !entry
                .path()
                .components()
                .any(|part| part.as_os_str() == "jcs")
        })
    {
        let path = entry.path();
        let value = paths::read_json_file(path)?;
        let (schema_id, instance) = if let Some(object) = value.as_object() {
            if let (Some(Value::String(id)), Some(instance)) =
                (object.get("$schema_id"), object.get("instance"))
            {
                (id.clone(), instance.clone())
            } else if let Some(Value::String(id)) = object.get("$schema_id") {
                let mut instance_object = object.clone();
                instance_object.remove("$schema_id");
                (id.clone(), Value::Object(instance_object))
            } else {
                anyhow::bail!(
                    "example {} must include $schema_id (and optional instance wrapper)",
                    path.display()
                );
            }
        } else {
            anyhow::bail!("example {} must be a JSON object", path.display());
        };

        validate_instance(registry, &schema_id, &instance).with_context(|| {
            format!(
                "example {} failed validation against {schema_id}",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Validate an instance against a schema resolved exclusively from `registry`.
///
/// The public API accepts a schema ID or source-path selector rather than a
/// `LoadedSchema`, so a caller cannot pair a schema borrowed from one registry
/// with another registry's reference set.
pub fn validate_instance(
    registry: &SchemaRegistry,
    schema_selector: &str,
    instance: &Value,
) -> Result<(), SchemaToolError> {
    let loaded = registry
        .resolve_schema_selector(schema_selector)
        .map_err(|error| {
            SchemaToolError::msg(format!("resolve schema {schema_selector}: {error}"))
        })?;
    let validator = build_validator(registry, loaded.id())?;
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|error| format!("{} at {}", error, error.instance_path))
        .collect();
    if !errors.is_empty() {
        return Err(SchemaToolError::ValidationFailed {
            schema_id: loaded.entry.id.clone(),
            detail: errors.join("; "),
        });
    }
    Ok(())
}

#[derive(Clone)]
struct RegistryRetriever {
    documents: BTreeMap<String, Value>,
}

impl Retrieve for RegistryRetriever {
    fn retrieve(&self, uri: &Uri<&str>) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        // jsonschema may append empty fragment
        let key = uri.as_str().trim_end_matches('#').to_string();
        self.documents
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("schema not found for retrieve: {key}").into())
    }
}

fn build_validator(
    registry: &SchemaRegistry,
    schema_id: &str,
) -> Result<Validator, SchemaToolError> {
    let loaded = registry
        .get(schema_id)
        .map_err(|error| SchemaToolError::msg(format!("resolve schema {schema_id}: {error}")))?;
    build_validator_for_loaded_schema(registry, loaded)
}

fn build_validator_for_loaded_schema(
    registry: &SchemaRegistry,
    loaded: &crate::manifest::LoadedSchema,
) -> Result<Validator, SchemaToolError> {
    let mut documents = BTreeMap::new();
    for (id, item) in registry.loaded_schemas() {
        documents.insert(id.to_owned(), item.document.clone());
    }

    let retriever = RegistryRetriever { documents };
    let resource = loaded.document.clone();

    contract_validator_options(retriever)
        .build(&resource)
        .map_err(|error| {
            SchemaToolError::msg(format!(
                "failed to compile schema {}: {error}",
                loaded.entry.id
            ))
        })
}

/// Public helper used by CLI/check tests.
#[allow(dead_code)]
pub fn compile_all(registry: &SchemaRegistry) -> Result<BTreeMap<String, Arc<Validator>>> {
    let documents = registry
        .loaded_schemas()
        .map(|(id, loaded)| (id.to_owned(), loaded.document.clone()))
        .collect();
    let retriever = RegistryRetriever { documents };
    let mut out = BTreeMap::new();
    for (id, loaded) in registry.loaded_schemas() {
        let validator = contract_validator_options(retriever.clone())
            .build(&loaded.document)
            .map_err(|error| {
                SchemaToolError::msg(format!("failed to compile schema {id}: {error}"))
            })?;
        out.insert(id.to_owned(), Arc::new(validator));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_success_text_remains_compatible_and_nested_text_names_pointer() {
        let root = ValidateSelectedResult {
            schema_id: "https://example.test/schema".into(),
            instance_path: PathBuf::from("instance.json"),
            pointer: JsonPointer::root(),
        };
        assert_eq!(
            render_success(&root),
            "valid: instance instance.json against https://example.test/schema"
        );

        let nested = ValidateSelectedResult {
            pointer: JsonPointer::parse("/payload").unwrap(),
            ..root
        };
        assert_eq!(
            render_success(&nested),
            "valid: instance instance.json against https://example.test/schema at pointer \"/payload\""
        );
    }
}
