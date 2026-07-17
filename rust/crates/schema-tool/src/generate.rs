use crate::codegen::{artifact_absolute_path, ensure_trailing_newline, plan_artifacts};
use crate::manifest::SchemaRegistry;
use crate::resolve;
use anyhow::{Context, Result};
use std::path::Path;

pub fn run(repo_root: &Path) -> Result<()> {
    let registry = SchemaRegistry::load(repo_root)?;
    resolve::check_all_refs(&registry)?;

    // Fully plan and render every artifact before writing anything. Unimplemented
    // targets (TypeScript) fail here with zero partial writes.
    let plan = plan_artifacts(&registry)?;
    for artifact in plan.artifacts() {
        let path = artifact_absolute_path(repo_root, artifact);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        write_if_changed(&path, artifact.content())?;
    }

    println!(
        "generated {} schemas into {} artifacts",
        registry.manifest.schemas.len(),
        plan.artifacts().len()
    );
    Ok(())
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    let normalized = ensure_trailing_newline(content);
    if path.is_file() {
        let existing = std::fs::read_to_string(path)
            .with_context(|| format!("read existing {}", path.display()))?;
        if existing == normalized {
            return Ok(());
        }
    }
    std::fs::write(path, normalized.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
