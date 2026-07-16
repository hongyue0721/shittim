use crate::error::SchemaToolError;
use crate::manifest::{LoadedSchema, SchemaRegistry};
use crate::paths;
use anyhow::{Context, Result};
use jsonschema::{Draft, Retrieve, Uri, Validator};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub fn run(repo_root: &Path, schema_selector: &str, instance_path: &Path) -> Result<()> {
    let registry = SchemaRegistry::load(repo_root)?;
    let loaded = registry.resolve_schema_selector(schema_selector)?;
    let instance = paths::read_json_file(instance_path)?;
    validate_instance(&registry, loaded, &instance).map_err(|e| anyhow::anyhow!(e))?;
    println!(
        "valid: instance {} against {}",
        instance_path.display(),
        loaded.entry.id
    );
    Ok(())
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
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter(|e| !e.path().components().any(|part| part.as_os_str() == "jcs"))
    {
        let path = entry.path();
        let value = paths::read_json_file(path)?;
        let (schema_id, instance) = if let Some(obj) = value.as_object() {
            if let (Some(Value::String(id)), Some(inst)) =
                (obj.get("$schema_id"), obj.get("instance"))
            {
                (id.clone(), inst.clone())
            } else if let Some(Value::String(id)) = obj.get("$schema_id") {
                let mut inst_obj = obj.clone();
                inst_obj.remove("$schema_id");
                (id.clone(), Value::Object(inst_obj))
            } else {
                anyhow::bail!(
                    "example {} must include $schema_id (and optional instance wrapper)",
                    path.display()
                );
            }
        } else {
            anyhow::bail!("example {} must be a JSON object", path.display());
        };

        let loaded = registry.get(&schema_id)?;
        validate_instance(registry, loaded, &instance).with_context(|| {
            format!(
                "example {} failed validation against {schema_id}",
                path.display()
            )
        })?;
    }
    Ok(())
}

pub fn validate_instance(
    registry: &SchemaRegistry,
    loaded: &LoadedSchema,
    instance: &Value,
) -> Result<(), SchemaToolError> {
    let validator = build_validator(registry, loaded)?;
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("{} at {}", e, e.instance_path))
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
    loaded: &LoadedSchema,
) -> Result<Validator, SchemaToolError> {
    let mut documents = BTreeMap::new();
    for (id, item) in &registry.by_id {
        documents.insert(id.clone(), item.document.clone());
    }

    let retriever = RegistryRetriever { documents };
    let resource = loaded.document.clone();

    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_retriever(retriever)
        .build(&resource)
        .map_err(|e| {
            SchemaToolError::msg(format!("failed to compile schema {}: {e}", loaded.entry.id))
        })
}

/// Public helper used by kernel-contracts tests via re-export path? kept for CLI.
#[allow(dead_code)]
pub fn compile_all(registry: &SchemaRegistry) -> Result<BTreeMap<String, Arc<Validator>>> {
    let documents = registry
        .by_id
        .iter()
        .map(|(id, loaded)| (id.clone(), loaded.document.clone()))
        .collect();
    let retriever = RegistryRetriever { documents };
    let mut out = BTreeMap::new();
    for (id, loaded) in &registry.by_id {
        let validator = Validator::options()
            .with_draft(Draft::Draft202012)
            .with_retriever(retriever.clone())
            .build(&loaded.document)
            .map_err(|error| {
                SchemaToolError::msg(format!("failed to compile schema {id}: {error}"))
            })?;
        out.insert(id.clone(), Arc::new(validator));
    }
    Ok(out)
}
