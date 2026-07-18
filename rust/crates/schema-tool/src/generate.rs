use crate::artifact_transaction::ArtifactTransaction;
use crate::codegen::plan_artifacts;
use crate::manifest::SchemaRegistry;
use anyhow::Result;
use std::path::Path;

pub fn run(repo_root: &Path) -> Result<()> {
    // Hold one exclusive lock from crash recovery through registry load, render, and commit. This
    // prevents another generator from creating a new journal between recovery and this plan.
    let mut transaction = ArtifactTransaction::begin(repo_root)?;
    let registry = SchemaRegistry::load(repo_root)?;

    // Fully plan and render every artifact before the durable transaction stages any bytes.
    let plan = plan_artifacts(&registry)?;
    transaction.install(&plan)?;

    println!(
        "generated {} schemas into {} artifacts",
        registry.manifest().schemas.len(),
        plan.artifacts().len()
    );
    Ok(())
}
