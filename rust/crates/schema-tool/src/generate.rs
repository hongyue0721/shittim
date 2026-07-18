use crate::artifact_transaction::ArtifactTransaction;
use crate::codegen::plan_artifacts;
use crate::manifest::SchemaRegistry;
use crate::production_stage::ProductionRegistry;
use anyhow::Result;
use std::path::Path;

pub fn run(repo_root: &Path) -> Result<()> {
    // Invalid production stage must fail before lock acquisition or recovery.
    let registry = SchemaRegistry::load(repo_root)?;
    let production = ProductionRegistry::new(&registry)?;
    let plan = plan_artifacts(production)?;

    // Only a completely validated and rendered plan may acquire the durable lock.
    let mut transaction = ArtifactTransaction::begin(repo_root)?;
    transaction.install(&plan)?;

    println!(
        "generated {} schemas into {} artifacts",
        registry.manifest().schemas.len(),
        plan.artifacts().len()
    );
    Ok(())
}
