use crate::codegen::{
    artifact_absolute_path, check_artifact_file_set, ensure_trailing_newline, plan_artifacts,
};
use crate::manifest::SchemaRegistry;
use crate::resolve;
use crate::validate;
use anyhow::{bail, Context, Result};
use std::path::Path;

pub fn run(repo_root: &Path) -> Result<()> {
    let registry = SchemaRegistry::load(repo_root)?;
    resolve::check_all_refs(&registry)?;
    validate_schema_documents(&registry)?;

    let plan = plan_artifacts(&registry)?;
    for artifact in plan.artifacts() {
        let path = artifact_absolute_path(repo_root, artifact);
        if !path.is_file() {
            bail!(
                "generated file missing {}; run `schema-tool generate`",
                path.display()
            );
        }
        let actual =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let expected = ensure_trailing_newline(artifact.content());
        if actual != expected {
            bail!("generation drift in {}", path.display());
        }
    }

    check_artifact_file_set(repo_root, &plan)?;

    // Validate examples if present.
    let examples = crate::paths::examples_dir(repo_root);
    if examples.is_dir() {
        validate::validate_examples(repo_root, &registry, &examples)?;
    }

    println!(
        "schema-tool check ok: {} schemas, Draft 2020-12 meta-validation, refs, catalog and generated types stable",
        registry.manifest.schemas.len()
    );
    Ok(())
}

fn validate_schema_documents(registry: &SchemaRegistry) -> Result<()> {
    for (id, loaded) in &registry.by_id {
        jsonschema::draft202012::meta::validate(&loaded.document).map_err(|error| {
            anyhow::anyhow!("schema {id} fails Draft 2020-12 meta-schema: {error}")
        })?;
    }
    validate::compile_all(registry)
        .map_err(|error| anyhow::anyhow!("Draft 2020-12 schema compilation failed: {error}"))?;
    Ok(())
}
