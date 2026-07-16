use crate::error::SchemaToolError;
use crate::paths;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub draft: String,
    pub id_base: String,
    pub schemas: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub title: String,
    pub version: u32,
    pub source: String,
    pub domain: String,
    pub kind: String,
    pub compatibility: String,
    pub generation_targets: Vec<String>,
    #[serde(default)]
    pub schema_version_field: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LoadedSchema {
    pub entry: ManifestEntry,
    pub source_path: PathBuf,
    pub document: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct SchemaRegistry {
    pub repo_root: PathBuf,
    pub manifest: Manifest,
    pub by_id: BTreeMap<String, LoadedSchema>,
}

impl SchemaRegistry {
    pub fn load(repo_root: &Path) -> Result<Self> {
        let manifest_path = paths::manifest_path(repo_root);
        paths::require_file(&manifest_path)?;
        let manifest: Manifest = serde_json::from_str(
            &std::fs::read_to_string(&manifest_path)
                .with_context(|| format!("read {}", manifest_path.display()))?,
        )
        .with_context(|| format!("parse manifest {}", manifest_path.display()))?;

        if manifest.schema_version != 1 {
            bail!(
                "unsupported manifest schema_version {}",
                manifest.schema_version
            );
        }
        if manifest.draft != "https://json-schema.org/draft/2020-12/schema" {
            bail!(
                "manifest draft must be JSON Schema 2020-12, got {}",
                manifest.draft
            );
        }
        const ALLOWED_KINDS: &[&str] = &[
            "enum",
            "object",
            "domain_object",
            "envelope",
            "event_payload",
            "kcp_request",
            "kcp_response",
        ];

        let mut by_id = BTreeMap::new();
        let mut seen_ids = BTreeSet::new();
        let mut seen_sources = BTreeSet::new();

        for entry in &manifest.schemas {
            if !ALLOWED_KINDS.contains(&entry.kind.as_str()) {
                return Err(SchemaToolError::msg(format!(
                    "manifest entry {} has unsupported kind {}",
                    entry.id, entry.kind
                ))
                .into());
            }
            if entry.generation_targets != ["rust"] {
                return Err(SchemaToolError::msg(format!(
                    "manifest entry {} must currently target only rust",
                    entry.id
                ))
                .into());
            }
            if !seen_ids.insert(entry.id.clone()) {
                return Err(SchemaToolError::msg(format!(
                    "duplicate $id in manifest: {}",
                    entry.id
                ))
                .into());
            }
            if !seen_sources.insert(entry.source.clone()) {
                return Err(SchemaToolError::msg(format!(
                    "duplicate source path in manifest: {}",
                    entry.source
                ))
                .into());
            }

            let source_path = repo_root.join(&entry.source);
            paths::require_file(&source_path)?;
            let document = paths::read_json_file(&source_path)?;

            let schema_keyword = document
                .get("$schema")
                .and_then(|v| v.as_str())
                .ok_or_else(|| SchemaToolError::msg(format!("{} missing $schema", entry.source)))?;
            if schema_keyword != "https://json-schema.org/draft/2020-12/schema" {
                return Err(SchemaToolError::msg(format!(
                    "{} declares non-2020-12 $schema: {schema_keyword}",
                    entry.source
                ))
                .into());
            }

            let id = document
                .get("$id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| SchemaToolError::msg(format!("{} missing $id", entry.source)))?;
            if id != entry.id {
                return Err(SchemaToolError::msg(format!(
                    "manifest id {} does not match source $id {id}",
                    entry.id
                ))
                .into());
            }
            if let Some(field) = &entry.schema_version_field {
                let has_field = document
                    .get("properties")
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|properties| properties.contains_key(field));
                if !has_field {
                    return Err(SchemaToolError::msg(format!(
                        "manifest entry {} declares missing schema_version_field {}",
                        entry.id, field
                    ))
                    .into());
                }
            }

            by_id.insert(
                entry.id.clone(),
                LoadedSchema {
                    entry: entry.clone(),
                    source_path,
                    document,
                },
            );
        }

        // Every source file must be listed in the manifest.
        let source_root = paths::source_dir(repo_root);
        for entry in walkdir::WalkDir::new(&source_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        {
            let rel = entry
                .path()
                .strip_prefix(repo_root)
                .with_context(|| format!("strip prefix for {}", entry.path().display()))?
                .to_string_lossy()
                .replace('\\', "/");
            if !seen_sources.contains(&rel) {
                return Err(SchemaToolError::msg(format!(
                    "source file not listed in manifest: {rel}"
                ))
                .into());
            }
        }

        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            manifest,
            by_id,
        })
    }

    pub fn get(&self, id: &str) -> Result<&LoadedSchema> {
        self.by_id
            .get(id)
            .ok_or_else(|| SchemaToolError::msg(format!("unknown schema $id: {id}")).into())
    }

    pub fn resolve_schema_selector(&self, selector: &str) -> Result<&LoadedSchema> {
        if let Some(loaded) = self.by_id.get(selector) {
            return Ok(loaded);
        }

        let as_path = if selector.starts_with("schemas/") {
            self.repo_root.join(selector)
        } else {
            paths::source_dir(&self.repo_root).join(selector)
        };

        for loaded in self.by_id.values() {
            if loaded.source_path == as_path {
                return Ok(loaded);
            }
        }

        Err(SchemaToolError::msg(format!("schema selector not found: {selector}")).into())
    }
}
