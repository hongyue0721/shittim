use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};

pub fn discover_repo_root() -> Result<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd);
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.to_path_buf());
        }
    }

    for start in candidates {
        if let Some(root) = find_root_upwards(&start) {
            return Ok(root);
        }
    }

    bail!("could not discover repository root containing schemas/manifest.json; pass --repo-root")
}

fn find_root_upwards(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join("schemas/manifest.json").is_file() && dir.join("AGENT.md").is_file() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

pub fn schemas_dir(repo_root: &Path) -> PathBuf {
    repo_root.join("schemas")
}

pub fn source_dir(repo_root: &Path) -> PathBuf {
    schemas_dir(repo_root).join("source")
}

pub fn manifest_path(repo_root: &Path) -> PathBuf {
    schemas_dir(repo_root).join("manifest.json")
}

pub fn examples_dir(repo_root: &Path) -> PathBuf {
    schemas_dir(repo_root).join("examples")
}

pub fn require_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("required file missing: {}", path.display());
    }
    Ok(())
}

pub fn read_json_file(path: &Path) -> Result<serde_json::Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value =
        serde_json::from_str(&text).with_context(|| format!("parse JSON {}", path.display()))?;
    Ok(value)
}
