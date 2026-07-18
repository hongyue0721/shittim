//! Codegen facade and artifact planner.
//!
//! Pipeline:
//! `SchemaRegistry` -> `ValidatedRegistry<Production|Synthetic>` -> `TargetPlan` ->
//! target-scoped `TargetSchemaSet`/`TargetContractGraph` -> target renderer
//! -> ArtifactPlan. Artifacts are fully generated before any write; check compares the
//! exact recursive file set under each artifact root, allowing only planned path prefixes.

use crate::contract_model::{lower_target_contract_graph, TargetContractGraph};
use crate::error::SchemaToolError;
use crate::manifest::GenerationTarget;
use crate::production_stage::{RegistryProfile, ValidatedRegistry};
use crate::rust_codegen;
use crate::target;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// One generated artifact with target, kind, path, content, and contributing source IDs.
///
/// Fields are private. Construct via [`GeneratedArtifact::new`]; the final path set is
/// only accepted after [`ArtifactPlan::try_new`] validates roots/paths/components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifact {
    target: GenerationTarget,
    kind: ArtifactKind,
    /// Path relative to repository root, using `/` separators.
    relative_path: String,
    content: String,
    /// Source schema `$id`s that contributed to this artifact.
    source_ids: Vec<String>,
}

impl GeneratedArtifact {
    /// Build an artifact value. Path safety is enforced by [`ArtifactPlan::try_new`].
    pub fn new(
        target: GenerationTarget,
        kind: ArtifactKind,
        relative_path: impl Into<String>,
        content: impl Into<String>,
        source_ids: Vec<String>,
    ) -> Self {
        Self {
            target,
            kind,
            relative_path: relative_path.into(),
            content: content.into(),
            source_ids,
        }
    }

    pub fn target(&self) -> GenerationTarget {
        self.target
    }

    pub fn kind(&self) -> ArtifactKind {
        self.kind
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn source_ids(&self) -> &[String] {
        &self.source_ids
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactKind {
    Types,
    Catalog,
    Typed,
    Mod,
}

/// Fully planned artifact set for all targets in a TargetPlan.
///
/// Construction is only valid through [`ArtifactPlan::try_new`], which validates
/// roots/paths/duplicates/component-safety and computes planned directory prefixes.
/// Fields are private; no mutation after construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPlan {
    artifacts: Vec<GeneratedArtifact>,
    /// Relative roots that must match the generated file set exactly (recursive).
    artifact_roots: Vec<String>,
    /// Relative directory prefixes that may exist because planned files live under them.
    /// Includes each artifact root and every parent directory of planned files under that root.
    planned_directory_prefixes: Vec<String>,
}

impl ArtifactPlan {
    /// Validate and encapsulate a planned artifact set.
    ///
    /// Rejects empty plans, unsafe roots/paths, duplicates, paths outside roots
    /// (including string-prefix spoofs such as `generated_evil`), and absolute/traversal
    /// components. Computes planned directory prefixes from the validated inputs.
    pub fn try_new(
        artifacts: Vec<GeneratedArtifact>,
        artifact_roots: impl IntoIterator<Item = String>,
    ) -> Result<Self> {
        if artifacts.is_empty() {
            return Err(SchemaToolError::msg(
                "no generation targets requested by any manifest schema",
            )
            .into());
        }
        let roots: BTreeSet<String> = artifact_roots.into_iter().collect();
        if roots.len() != 1 {
            bail!(
                "artifact transaction currently supports exactly one artifact root; got {} ({roots:?})",
                roots.len()
            );
        }
        validate_artifact_paths(&artifacts, &roots)?;
        let planned_directory_prefixes = compute_planned_directory_prefixes(&artifacts, &roots);
        Ok(Self {
            artifacts,
            artifact_roots: roots.into_iter().collect(),
            planned_directory_prefixes,
        })
    }

    pub fn artifacts(&self) -> &[GeneratedArtifact] {
        &self.artifacts
    }

    pub fn roots(&self) -> &[String] {
        &self.artifact_roots
    }

    pub fn planned_prefixes(&self) -> &[String] {
        &self.planned_directory_prefixes
    }
}

/// Build the full artifact plan. Fails closed before any write when a declared target
/// has no renderer (currently TypeScript).
pub fn plan_artifacts<P: RegistryProfile>(
    validated: ValidatedRegistry<'_, P>,
) -> Result<ArtifactPlan> {
    let plan = target::build_target_plan(validated)?;
    for set in plan.targets() {
        match set.target() {
            GenerationTarget::Rust => {}
            GenerationTarget::Typescript => {
                return Err(SchemaToolError::msg(
                    "typescript generation target is declared but TypeScript codegen is not implemented yet",
                )
                .into());
            }
        }
    }

    let mut artifacts = Vec::new();
    let mut artifact_roots = BTreeSet::new();

    for set in plan.targets() {
        match set.target() {
            GenerationTarget::Rust => {
                let graph = lower_target_contract_graph(&plan, GenerationTarget::Rust)?;
                let rust_artifacts = render_rust_artifacts(&graph)?;
                artifact_roots.insert(rust_codegen::RUST_GENERATED_DIR.to_string());
                artifacts.extend(rust_artifacts);
            }
            GenerationTarget::Typescript => {
                return Err(SchemaToolError::msg(
                    "typescript generation target is declared but TypeScript codegen is not implemented yet",
                )
                .into());
            }
        }
    }

    ArtifactPlan::try_new(artifacts, artifact_roots)
}

fn render_rust_artifacts(graph: &TargetContractGraph) -> Result<Vec<GeneratedArtifact>> {
    // Project once; types and typed modules consume the same RustProjection instance.
    let projection = rust_codegen::project_rust(graph)?;
    let types = rust_codegen::render_types_module_from_projection(&projection)?;
    let catalog = rust_codegen::render_catalog_module(graph)?;
    let typed = rust_codegen::render_typed_module_from_projection(&projection, graph)?;
    let source_ids = graph.source_schema_ids.clone();
    let base = rust_codegen::RUST_GENERATED_DIR;
    Ok(vec![
        GeneratedArtifact::new(
            GenerationTarget::Rust,
            ArtifactKind::Types,
            format!("{base}/types.rs"),
            types,
            source_ids.clone(),
        ),
        GeneratedArtifact::new(
            GenerationTarget::Rust,
            ArtifactKind::Catalog,
            format!("{base}/catalog.rs"),
            catalog,
            source_ids.clone(),
        ),
        GeneratedArtifact::new(
            GenerationTarget::Rust,
            ArtifactKind::Typed,
            format!("{base}/typed.rs"),
            typed,
            source_ids.clone(),
        ),
        GeneratedArtifact::new(
            GenerationTarget::Rust,
            ArtifactKind::Mod,
            format!("{base}/mod.rs"),
            rust_codegen::GENERATED_MOD_RS.to_string(),
            source_ids,
        ),
    ])
}

/// Absolute path for an artifact under `repo_root`.
pub fn artifact_absolute_path(repo_root: &Path, artifact: &GeneratedArtifact) -> PathBuf {
    repo_root.join(normalize_rel(artifact.relative_path()))
}

/// Ensure trailing newline normalization used by generate/check.
pub fn ensure_trailing_newline(content: &str) -> String {
    format!("{}\n", content.trim_end_matches(['\r', '\n']))
}

/// Expected exact set of generated relative paths from an artifact plan.
pub fn expected_paths_from_plan(plan: &ArtifactPlan) -> BTreeSet<String> {
    plan.artifacts()
        .iter()
        .map(|artifact| artifact.relative_path().to_string())
        .collect()
}

fn normalize_rel(path: &str) -> PathBuf {
    PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn validate_artifact_paths(
    artifacts: &[GeneratedArtifact],
    roots: &BTreeSet<String>,
) -> Result<()> {
    for root in roots {
        validate_component_safe_rel_path(root, "artifact root")?;
    }
    let mut seen = BTreeSet::new();
    for artifact in artifacts {
        let rel = artifact.relative_path();
        validate_component_safe_rel_path(rel, "artifact path")?;
        if !seen.insert(rel.to_string()) {
            bail!("duplicate artifact relative_path: {rel}");
        }
        let under_root = roots.iter().any(|root| path_is_under_root(rel, root));
        if !under_root {
            bail!("artifact path {rel} is not under any artifact root {roots:?}");
        }
    }
    Ok(())
}

/// Component-safe relative path: non-empty, `/`-separated, no empty/`.`/`..` components,
/// no backslashes, no absolute/prefix forms. Rejects prefix tricks such as
/// `generated_evil` matching a `generated` root via naive string prefix.
fn validate_component_safe_rel_path(path: &str, label: &str) -> Result<()> {
    if path.is_empty() {
        bail!("{label} must not be empty");
    }
    if path.starts_with('/') || path.ends_with('/') {
        bail!("{label} must not be absolute or end with '/': {path:?}");
    }
    if path.contains('\\') {
        bail!("{label} must use '/' separators only: {path:?}");
    }
    if Path::new(path).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) | Component::CurDir
        )
    }) {
        bail!("{label} has unsafe path components: {path}");
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            bail!("{label} has empty or relative component: {path:?}");
        }
    }
    Ok(())
}

fn path_is_under_root(path: &str, root: &str) -> bool {
    if path == root {
        return true;
    }
    path.strip_prefix(root)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn compute_planned_directory_prefixes(
    artifacts: &[GeneratedArtifact],
    roots: &BTreeSet<String>,
) -> Vec<String> {
    let mut prefixes = BTreeSet::new();
    for root in roots {
        prefixes.insert(root.clone());
    }
    for artifact in artifacts {
        let mut current = String::new();
        let parts: Vec<&str> = artifact.relative_path().split('/').collect();
        // All directory components (exclude the final file name).
        for (index, part) in parts.iter().enumerate() {
            if index + 1 == parts.len() {
                break;
            }
            if current.is_empty() {
                current = (*part).to_string();
            } else {
                current = format!("{current}/{part}");
            }
            prefixes.insert(current.clone());
        }
    }
    prefixes.into_iter().collect()
}

/// Recursively list files under each artifact root.
///
/// - Planned directory prefixes may exist (including nested planned paths).
/// - Unplanned directories (empty or non-empty) fail closed.
/// - Symlinks fail closed.
/// - Missing roots are treated as empty (caller compares exact file set).
pub fn list_artifact_root_files(repo_root: &Path, plan: &ArtifactPlan) -> Result<BTreeSet<String>> {
    let planned_dirs: BTreeSet<&str> = plan.planned_prefixes().iter().map(String::as_str).collect();
    let mut files = BTreeSet::new();
    for root_rel in plan.roots() {
        let root_abs = repo_root.join(normalize_rel(root_rel));
        if !root_abs.exists() {
            continue;
        }
        if is_symlink(&root_abs)? {
            bail!(
                "artifact root must not be a symlink: {}",
                root_abs.display()
            );
        }
        if !root_abs.is_dir() {
            bail!("artifact root is not a directory: {}", root_abs.display());
        }
        collect_files_recursive(repo_root, &root_abs, &planned_dirs, &mut files)?;
    }
    Ok(files)
}

fn is_symlink(path: &Path) -> Result<bool> {
    let meta = std::fs::symlink_metadata(path)
        .with_context(|| format!("symlink_metadata {}", path.display()))?;
    Ok(meta.file_type().is_symlink())
}

fn collect_files_recursive(
    repo_root: &Path,
    dir: &Path,
    planned_dirs: &BTreeSet<&str>,
    files: &mut BTreeSet<String>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if is_symlink(&path)? {
            bail!("unexpected symlink in artifact root: {}", path.display());
        }
        let meta = entry
            .metadata()
            .with_context(|| format!("metadata {}", path.display()))?;
        let rel = path
            .strip_prefix(repo_root)
            .with_context(|| format!("strip prefix for {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");

        if meta.is_dir() {
            if !planned_dirs.contains(rel.as_str()) {
                bail!(
                    "unexpected directory under artifact root (not a planned path prefix): {}",
                    path.display()
                );
            }
            collect_files_recursive(repo_root, &path, planned_dirs, files)?;
        } else if meta.is_file() {
            files.insert(rel);
        } else {
            bail!(
                "unexpected non-file entry under artifact root: {}",
                path.display()
            );
        }
    }
    Ok(())
}

/// Compare actual artifact roots against the planned exact file set.
pub fn check_artifact_file_set(repo_root: &Path, plan: &ArtifactPlan) -> Result<()> {
    let expected = expected_paths_from_plan(plan);
    let actual = list_artifact_root_files(repo_root, plan)?;
    if actual != expected {
        let missing: Vec<_> = expected.difference(&actual).cloned().collect();
        let extra: Vec<_> = actual.difference(&expected).cloned().collect();
        bail!(
            "generated artifact file set mismatch under {:?}; missing={missing:?}, extra={extra:?}",
            plan.roots()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(path: &str) -> GeneratedArtifact {
        GeneratedArtifact::new(
            GenerationTarget::Rust,
            ArtifactKind::Types,
            path,
            "x\n",
            vec![],
        )
    }

    #[test]
    fn ensure_trailing_newline_is_stable() {
        assert_eq!(ensure_trailing_newline("a\n"), "a\n");
        assert_eq!(ensure_trailing_newline("a\r\n"), "a\n");
        assert_eq!(ensure_trailing_newline("a"), "a\n");
    }

    #[test]
    fn try_new_computes_planned_prefixes_for_nested_parents() {
        let plan = ArtifactPlan::try_new(
            vec![artifact(
                "rust/crates/kernel-contracts/src/generated/nested/types.rs",
            )],
            ["rust/crates/kernel-contracts/src/generated".into()],
        )
        .expect("nested under root is valid");
        assert!(plan
            .planned_prefixes()
            .iter()
            .any(|p| p.ends_with("/generated")));
        assert!(plan
            .planned_prefixes()
            .iter()
            .any(|p| p.ends_with("/generated/nested")));
    }

    #[test]
    fn try_new_rejects_unsafe_paths_and_non_single_root_plans() {
        assert!(ArtifactPlan::try_new(vec![artifact("out/../secret.rs")], ["out".into()]).is_err());

        assert!(ArtifactPlan::try_new(
            vec![artifact("out/a.rs"), artifact("out/a.rs")],
            ["out".into()]
        )
        .is_err());

        // Absolute path rejected by component-safe rules.
        assert!(
            ArtifactPlan::try_new(vec![artifact("/absolute/types.rs")], ["absolute".into()])
                .is_err()
        );

        // generated_evil must not be accepted under a generated root via string prefix.
        let root = "rust/crates/kernel-contracts/src/generated";
        assert!(ArtifactPlan::try_new(
            vec![artifact(
                "rust/crates/kernel-contracts/src/generated_evil/types.rs"
            )],
            [root.into()]
        )
        .is_err());

        assert!(ArtifactPlan::try_new(
            vec![artifact(
                "rust/crates/kernel-contracts/src/generated/nested/types.rs"
            )],
            [root.into()]
        )
        .is_ok());

        assert!(ArtifactPlan::try_new(
            vec![artifact("rust/crates/kernel-contracts/src/other/types.rs")],
            [root.into()]
        )
        .is_err());

        assert!(ArtifactPlan::try_new(vec![artifact("out/a.rs")], []).is_err());
        assert!(ArtifactPlan::try_new(
            vec![artifact("out/a.rs"), artifact("other/b.rs")],
            ["out".into(), "other".into()]
        )
        .is_err());
    }
}
