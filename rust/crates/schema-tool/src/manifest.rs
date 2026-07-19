//! Manifest v2 loading, immutable retained-ID baseline verification, and component policy.

use crate::compatibility::SchemaCompatibility;
use crate::conditional_envelope::{
    analyze_registry_conditional_envelopes, EnvelopeConditionalBinding,
};
use crate::error::SchemaToolError;
use crate::event_catalog::discover_event_catalog_authorities;
use crate::manifest_identity::{
    validate_component_native_identity, validate_schema_version_field, validate_source_title,
};
use crate::method_bindings::validate_method_version_bindings;
use crate::paths;
use crate::resolve::{
    require_canonical_id_base, require_canonical_schema_id, schema_id_in_namespace,
    validate_component_namespace,
};
use crate::schema_walk::walk_schema_nodes;
use crate::target;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use url::Url;

const RETAINED_BASELINE_PATH: &str = "schemas/fixtures/manifest/retained_ids.v1.json";
const HISTORICAL_ID_PREFIX: &str = "https://schemas.shittim.local/v1/";
const SCHEMA_SOURCE_PREFIX: &str = "schemas/source/";
const UNSUPPORTED_IDENTITY_REF_KEYWORDS: &[&str] = &[
    "$anchor",
    "$dynamicAnchor",
    "$dynamicRef",
    "$recursiveAnchor",
    "$recursiveRef",
    "$vocabulary",
];

/// Supported code generation targets. Serde names are lowercase wire values.
///
/// There is intentionally no `ALL` constant: callers must not invent a closed
/// set of targets. Target discovery walks the manifest and collects declared values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GenerationTarget {
    Rust,
    Typescript,
}

impl GenerationTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Typescript => "typescript",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub draft: String,
    pub id_base: String,
    pub components: Vec<ManifestComponent>,
    pub method_version_bindings: Vec<ManifestMethodVersionBinding>,
    pub schemas: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestComponent {
    pub name: String,
    pub namespace: String,
    pub allowed_refs: Vec<String>,
    pub retained_ids: Vec<String>,
}

/// Manifest-derived method lifecycle binding. Validated fully by
/// [`crate::method_bindings::validate_method_version_bindings`]; empty arrays are
/// allowed by the generic loader, while production emptiness is a separate stage gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestMethodVersionBinding {
    pub family: MethodFamily,
    pub method: String,
    pub active_request_versions: Vec<u32>,
    pub legacy_validation_versions: Vec<u32>,
    pub request_schema_id_by_version: BTreeMap<String, String>,
    pub response_schema_id_by_version: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MethodFamily {
    Command,
    Query,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub id: String,
    pub title: String,
    pub version: u32,
    pub source: String,
    pub component: String,
    pub kind: String,
    pub compatibility: SchemaCompatibility,
    pub generation_targets: Vec<GenerationTarget>,
    #[serde(default)]
    pub schema_version_field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SchemaSourcePath(String);

impl SchemaSourcePath {
    pub fn parse(source: &str) -> Result<Self> {
        if source.is_empty() {
            bail!("manifest schema source path must not be empty");
        }
        if source.contains('\\') {
            bail!("manifest schema source path must use POSIX separators: {source}");
        }
        if !source.starts_with(SCHEMA_SOURCE_PREFIX) {
            bail!("manifest schema source path must start with {SCHEMA_SOURCE_PREFIX}: {source}");
        }
        let path = Path::new(source);
        if path.is_absolute() {
            bail!("manifest schema source path must be repository-relative: {source}");
        }
        for component in path.components() {
            match component {
                Component::Normal(segment) if !segment.is_empty() => {}
                Component::CurDir => {
                    bail!("manifest schema source path must not contain '.': {source}")
                }
                Component::ParentDir => {
                    bail!("manifest schema source path must not contain '..': {source}")
                }
                _ => bail!("manifest schema source path is not lexically normalized: {source}"),
            }
        }
        if source.split('/').any(str::is_empty) {
            bail!("manifest schema source path contains an empty segment: {source}");
        }
        let normalized = path
            .components()
            .map(|component| component.as_os_str().to_str().expect("source is UTF-8"))
            .collect::<Vec<_>>()
            .join("/");
        if normalized != source {
            bail!(
                "manifest schema source path must be exact lexical normalized POSIX form: {source}"
            );
        }
        Ok(Self(source.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    fn verify_regular_file(&self, repo_root: &Path) -> Result<PathBuf> {
        let canonical_repo_root = repo_root
            .canonicalize()
            .with_context(|| format!("canonicalize repository root {}", repo_root.display()))?;
        let source_root = repo_root.join("schemas/source");
        reject_symlink_path(repo_root, &source_root)?;
        let canonical_source_root = source_root.canonicalize().with_context(|| {
            format!("canonicalize schema source root {}", source_root.display())
        })?;
        if !canonical_source_root.starts_with(&canonical_repo_root) {
            bail!(
                "schema source root escapes repository root: {}",
                source_root.display()
            );
        }

        let source_path = repo_root.join(self.as_path());
        reject_symlink_path(repo_root, &source_path)?;
        let metadata = std::fs::symlink_metadata(&source_path)
            .with_context(|| format!("symlink_metadata {}", source_path.display()))?;
        if !metadata.file_type().is_file() {
            bail!(
                "schema source must be a regular file: {}",
                source_path.display()
            );
        }
        let canonical_source_path = source_path
            .canonicalize()
            .with_context(|| format!("canonicalize schema source {}", source_path.display()))?;
        if !canonical_source_path.starts_with(&canonical_source_root) {
            bail!(
                "schema source escapes schemas/source after canonicalization: {}",
                source_path.display()
            );
        }
        Ok(source_path)
    }
}

impl std::fmt::Display for SchemaSourcePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn reject_symlink_path(repo_root: &Path, path: &Path) -> Result<()> {
    let relative = path.strip_prefix(repo_root).with_context(|| {
        format!(
            "schema source path {} is not below repository root {}",
            path.display(),
            repo_root.display()
        )
    })?;
    let mut current = repo_root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata = std::fs::symlink_metadata(&current)
            .with_context(|| format!("symlink_metadata {}", current.display()))?;
        if metadata.file_type().is_symlink() {
            bail!(
                "schema source path must not contain symlinks: {}",
                current.display()
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct LoadedSchema {
    pub(crate) entry: ManifestEntry,
    pub(crate) source: SchemaSourcePath,
    pub(crate) source_path: PathBuf,
    pub(crate) document: serde_json::Value,
}

impl LoadedSchema {
    pub fn entry(&self) -> &ManifestEntry {
        &self.entry
    }

    pub fn id(&self) -> &str {
        &self.entry.id
    }

    pub fn document(&self) -> &serde_json::Value {
        &self.document
    }

    pub fn source(&self) -> &SchemaSourcePath {
        &self.source
    }

    pub fn source_path(&self) -> &Path {
        &self.source_path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityClass {
    Retained { component_index: usize },
    ComponentNative { component_index: usize },
}

impl IdentityClass {
    fn component_index(self) -> usize {
        match self {
            Self::Retained { component_index } | Self::ComponentNative { component_index } => {
                component_index
            }
        }
    }
}

/// Fully validated registry. Construction is private to `load`, which verifies
/// the migration ledger and every `$ref` before a registry can escape.
#[derive(Debug, Clone)]
pub struct SchemaRegistry {
    repo_root: PathBuf,
    manifest: Manifest,
    by_id: BTreeMap<String, LoadedSchema>,
    identity_classes_by_id: BTreeMap<String, IdentityClass>,
    /// Authoritative identity set for Schema nodes in each loaded document.
    /// Built once by `schema_walk` during load and never inferred from raw JSON shape.
    schema_node_pointers_by_id: BTreeMap<String, BTreeSet<crate::json_pointer::JsonPointer>>,
    /// Strict conditional-envelope facts parsed once per manifest envelope.
    conditional_envelope_bindings_by_id: BTreeMap<String, Option<EnvelopeConditionalBinding>>,
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

        validate_manifest_header(&manifest)?;
        let id_base_url = require_canonical_id_base(&manifest.id_base)?;
        let component_indexes_by_name = validate_components(&manifest.components, &id_base_url)?;
        let retained_baseline = RetainedBaseline::load(repo_root)?;
        let retained_by_id = retained_baseline.by_id()?;

        let mut by_id = BTreeMap::new();
        let mut seen_ids = BTreeSet::new();
        let mut seen_sources = BTreeSet::new();
        let mut identity_classes_by_id = BTreeMap::new();
        let mut schema_node_pointers_by_id = BTreeMap::new();

        for entry in &manifest.schemas {
            validate_manifest_entry(
                repo_root,
                entry,
                &manifest.components,
                &component_indexes_by_name,
                &retained_by_id,
                &mut seen_ids,
                &mut seen_sources,
                &mut identity_classes_by_id,
                &mut schema_node_pointers_by_id,
                &mut by_id,
            )?;
        }

        validate_retained_baseline(
            &manifest,
            &retained_baseline,
            &retained_by_id,
            &by_id,
            &component_indexes_by_name,
        )?;
        validate_all_source_files_listed(repo_root, &seen_sources)?;

        let mut registry = Self {
            repo_root: repo_root.to_path_buf(),
            manifest,
            by_id,
            identity_classes_by_id,
            schema_node_pointers_by_id,
            conditional_envelope_bindings_by_id: BTreeMap::new(),
        };
        crate::resolve::validate_registry_references(&registry)?;
        crate::event_catalog::validate_event_catalog_claimant_gate(&registry)?;
        registry.conditional_envelope_bindings_by_id =
            analyze_registry_conditional_envelopes(&registry)?;
        // Full MethodVersionBinding validation is stage-independent: empty or
        // complete legal non-empty registries are accepted. Production emptiness
        // is enforced later by validate_production_manifest_stage.
        validate_method_version_bindings(&registry)?;
        // Event catalog authority is independent of KCP MethodVersionBinding.
        // Reserved-identity/structural claimants and whole-schema mapping bijection
        // are fail-closed at registry load so partial impostors never lower.
        let _event_authorities = discover_event_catalog_authorities(&registry)?;
        Ok(registry)
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn schema_count(&self) -> usize {
        self.by_id.len()
    }

    pub fn loaded_schemas(&self) -> impl Iterator<Item = (&str, &LoadedSchema)> {
        self.by_id.iter().map(|(id, loaded)| (id.as_str(), loaded))
    }

    pub fn get(&self, id: &str) -> Result<&LoadedSchema> {
        self.by_id
            .get(id)
            .ok_or_else(|| SchemaToolError::msg(format!("unknown schema $id: {id}")).into())
    }

    pub(crate) fn conditional_envelope_binding(
        &self,
        id: &str,
    ) -> Result<Option<&EnvelopeConditionalBinding>> {
        let loaded = self.get(id)?;
        if loaded.entry().kind != "envelope" {
            return Ok(None);
        }
        self.conditional_envelope_bindings_by_id
            .get(id)
            .map(Option::as_ref)
            .ok_or_else(|| {
                SchemaToolError::msg(format!(
                    "envelope {id} is missing its registry conditional analysis"
                ))
                .into()
            })
    }

    pub(crate) fn is_schema_node_pointer(
        &self,
        schema_id: &str,
        pointer: &crate::json_pointer::JsonPointer,
    ) -> Result<bool> {
        self.get(schema_id)?;
        let pointers = self
            .schema_node_pointers_by_id
            .get(schema_id)
            .ok_or_else(|| {
                SchemaToolError::msg(format!(
                    "schema {schema_id} is missing its authoritative SchemaNode identity index"
                ))
            })?;
        Ok(pointers.contains(pointer))
    }

    pub fn component_allows_ref(&self, from_id: &str, to_id: &str) -> Result<()> {
        let source = self.get(from_id)?;
        let target = self.get(to_id)?;
        let source_class = self.identity_classes_by_id.get(from_id).ok_or_else(|| {
            SchemaToolError::msg(format!(
                "schema {from_id} is missing its identity classification"
            ))
        })?;
        let target_class = self.identity_classes_by_id.get(to_id).ok_or_else(|| {
            SchemaToolError::msg(format!(
                "schema {to_id} is missing its identity classification"
            ))
        })?;
        let source_component_index = source_class.component_index();
        let target_component_index = target_class.component_index();
        if source_component_index == target_component_index {
            return Ok(());
        }
        let component = &self.manifest.components[source_component_index];
        if component
            .allowed_refs
            .iter()
            .any(|name| name == &target.entry.component)
        {
            return Ok(());
        }
        Err(SchemaToolError::msg(format!(
            "component ref gate error: schema {from_id} in component {} may not reference schema {to_id} in component {}; declare {} in {}.allowed_refs",
            source.entry.component,
            target.entry.component,
            target.entry.component,
            source.entry.component,
        ))
        .into())
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
        self.by_id
            .values()
            .find(|loaded| loaded.source_path() == as_path)
            .ok_or_else(|| {
                SchemaToolError::msg(format!("schema selector not found: {selector}")).into()
            })
    }
}

fn validate_manifest_header(manifest: &Manifest) -> Result<()> {
    if manifest.schema_version != 2 {
        bail!(
            "unsupported manifest schema_version {}; only manifest v2 is supported",
            manifest.schema_version
        );
    }
    if manifest.draft != "https://json-schema.org/draft/2020-12/schema" {
        bail!(
            "manifest draft must be JSON Schema 2020-12, got {}",
            manifest.draft
        );
    }
    let id_base_url = require_canonical_id_base(&manifest.id_base)?;
    if id_base_url.as_str() != "https://schemas.shittim.local/" {
        bail!(
            "manifest v2 id_base must be https://schemas.shittim.local/, got {}",
            id_base_url
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_manifest_entry(
    repo_root: &Path,
    entry: &ManifestEntry,
    components: &[ManifestComponent],
    component_indexes_by_name: &BTreeMap<String, usize>,
    retained_by_id: &BTreeMap<String, &RetainedBaselineEntry>,
    seen_ids: &mut BTreeSet<String>,
    seen_sources: &mut BTreeSet<String>,
    identity_classes_by_id: &mut BTreeMap<String, IdentityClass>,
    schema_node_pointers_by_id: &mut BTreeMap<String, BTreeSet<crate::json_pointer::JsonPointer>>,
    by_id: &mut BTreeMap<String, LoadedSchema>,
) -> Result<()> {
    const ALLOWED_KINDS: &[&str] = &[
        "enum",
        "object",
        "domain_object",
        "envelope",
        "event_payload",
        "kcp_request",
        "kcp_response",
    ];
    if !ALLOWED_KINDS.contains(&entry.kind.as_str()) {
        bail!(
            "manifest entry {} has unsupported kind {}",
            entry.id,
            entry.kind
        );
    }
    target::validate_generation_targets(&entry.id, &entry.generation_targets)?;
    crate::compatibility::validate_kind_compatibility(entry)?;
    let component_index = *component_indexes_by_name
        .get(&entry.component)
        .ok_or_else(|| {
            SchemaToolError::msg(format!(
                "manifest entry {} declares unknown component {}",
                entry.id, entry.component
            ))
        })?;
    if !seen_ids.insert(entry.id.clone()) {
        bail!("duplicate $id in manifest: {}", entry.id);
    }
    let source = SchemaSourcePath::parse(&entry.source)?;
    if !seen_sources.insert(source.as_str().to_owned()) {
        bail!("duplicate source path in manifest: {source}");
    }

    let source_path = source.verify_regular_file(repo_root)?;
    let source_bytes =
        std::fs::read(&source_path).with_context(|| format!("read {}", source_path.display()))?;
    let document: serde_json::Value = serde_json::from_slice(&source_bytes)
        .with_context(|| format!("parse JSON {}", source_path.display()))?;
    let schema_node_pointers = audit_schema_identity_keywords(&document, &entry.id)?;
    let schema_keyword = document
        .get("$schema")
        .and_then(|value| value.as_str())
        .ok_or_else(|| SchemaToolError::msg(format!("{} missing $schema", entry.source)))?;
    if schema_keyword != "https://json-schema.org/draft/2020-12/schema" {
        bail!(
            "{} declares non-2020-12 $schema: {schema_keyword}",
            entry.source
        );
    }
    let source_id = document
        .get("$id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| SchemaToolError::msg(format!("{} missing $id", entry.source)))?;
    if source_id != entry.id {
        bail!(
            "manifest id {} does not match source $id {source_id}",
            entry.id
        );
    }
    require_canonical_schema_id(source_id, &entry.source)?;
    require_canonical_schema_id(&entry.id, "manifest entry id")?;
    validate_source_title(entry, &document)?;

    let identity_class = match retained_by_id.get(&entry.id) {
        Some(baseline) => {
            if baseline.component != entry.component || baseline.source != source.as_str() {
                bail!(
                    "retained baseline mismatch for {}: expected component={} source={}, got component={} source={}",
                    entry.id,
                    baseline.component,
                    baseline.source,
                    entry.component,
                    source
                );
            }
            let actual_sha256 = sha256_bytes(&source_bytes);
            if actual_sha256 != baseline.source_sha256 {
                bail!(
                    "retained baseline source SHA-256 mismatch for {} at {}: expected {}, got {}",
                    entry.id,
                    source,
                    baseline.source_sha256,
                    actual_sha256
                );
            }
            IdentityClass::Retained { component_index }
        }
        None => {
            if entry.id.starts_with(HISTORICAL_ID_PREFIX) {
                bail!(
                    "historical /v1/ schema ID is not present in immutable retained baseline: {}",
                    entry.id
                );
            }
            let namespace = Url::parse(&components[component_index].namespace)
                .expect("component namespace was validated before entry loading");
            schema_id_in_namespace(&namespace, &entry.id).map_err(|_| {
                SchemaToolError::msg(format!(
                    "manifest entry {} must lie under component {} namespace {}",
                    entry.id, entry.component, components[component_index].namespace
                ))
            })?;
            // Exact component-native hard gate: ID/source/title stem/version/URL shape.
            validate_component_native_identity(entry)?;
            IdentityClass::ComponentNative { component_index }
        }
    };
    // Retained and component-native entries share the same schema_version_field rule.
    validate_schema_version_field(entry, &document)?;

    identity_classes_by_id.insert(entry.id.clone(), identity_class);
    schema_node_pointers_by_id.insert(entry.id.clone(), schema_node_pointers);
    by_id.insert(
        entry.id.clone(),
        LoadedSchema {
            entry: entry.clone(),
            source,
            source_path,
            document,
        },
    );
    Ok(())
}

fn audit_schema_identity_keywords(
    document: &serde_json::Value,
    schema_id: &str,
) -> Result<BTreeSet<crate::json_pointer::JsonPointer>> {
    let mut schema_node_pointers = BTreeSet::new();
    walk_schema_nodes(document, |pointer, is_root, node| {
        schema_node_pointers.insert(pointer.clone());
        let Some(object) = node.as_object() else {
            return Ok(());
        };
        if !is_root && object.contains_key("$id") {
            bail!(
                "nested non-root $id is not supported at {schema_id}#{}",
                pointer.as_str()
            );
        }
        if !is_root && object.contains_key("$schema") {
            bail!(
                "nested non-root $schema is not supported at {schema_id}#{}",
                pointer.as_str()
            );
        }
        for keyword in UNSUPPORTED_IDENTITY_REF_KEYWORDS {
            if object.contains_key(*keyword) {
                bail!(
                    "unsupported JSON Schema identity/ref keyword {keyword} at {schema_id}#{}",
                    pointer.as_str()
                );
            }
        }
        Ok(())
    })?;
    Ok(schema_node_pointers)
}

fn validate_components(
    components: &[ManifestComponent],
    id_base: &Url,
) -> Result<BTreeMap<String, usize>> {
    if components.is_empty() {
        bail!("manifest v2 components must be non-empty");
    }
    let mut indexes_by_name = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        if component.name.is_empty() {
            bail!("manifest component name must be non-empty");
        }
        if indexes_by_name
            .insert(component.name.clone(), index)
            .is_some()
        {
            bail!("duplicate manifest component name: {}", component.name);
        }
        validate_component_namespace(id_base, &component.name, &component.namespace)?;
        require_strictly_sorted_unique(
            &component.allowed_refs,
            &format!("component {} allowed_refs", component.name),
        )?;
        require_strictly_sorted_unique(
            &component.retained_ids,
            &format!("component {} retained_ids", component.name),
        )?;
    }
    for component in components {
        for allowed in &component.allowed_refs {
            if allowed == &component.name {
                bail!(
                    "component {} may not declare itself in allowed_refs",
                    component.name
                );
            }
            if !indexes_by_name.contains_key(allowed) {
                bail!(
                    "component {} allowed_refs contains unknown component {allowed}",
                    component.name
                );
            }
        }
    }
    Ok(indexes_by_name)
}

fn validate_retained_baseline(
    manifest: &Manifest,
    baseline: &RetainedBaseline,
    retained_by_id: &BTreeMap<String, &RetainedBaselineEntry>,
    by_id: &BTreeMap<String, LoadedSchema>,
    component_indexes_by_name: &BTreeMap<String, usize>,
) -> Result<()> {
    for retained in &baseline.entries {
        for component in &manifest.components {
            let namespace = Url::parse(&component.namespace)
                .expect("component namespace was validated before baseline verification");
            if schema_id_in_namespace(&namespace, &retained.id).is_ok() {
                bail!(
                    "retained ID {} overlaps component namespace {}; retained and component-native identity classes are mutually exclusive",
                    retained.id,
                    component.name
                );
            }
        }
        let entry = by_id.get(&retained.id).ok_or_else(|| {
            SchemaToolError::msg(format!(
                "retained baseline ID is orphaned from manifest: {}",
                retained.id
            ))
        })?;
        if entry.entry.component != retained.component || entry.entry.source != retained.source {
            bail!(
                "retained baseline mismatch for {}: expected component={} source={}",
                retained.id,
                retained.component,
                retained.source
            );
        }
    }

    for component in &manifest.components {
        let expected: Vec<&str> = baseline
            .entries
            .iter()
            .filter(|entry| entry.component == component.name)
            .map(|entry| entry.id.as_str())
            .collect();
        let actual: Vec<&str> = component.retained_ids.iter().map(String::as_str).collect();
        if actual != expected {
            bail!(
                "retained ownership ledger mismatch for component {}: retained_ids must exactly match immutable baseline",
                component.name
            );
        }
    }

    for (id, loaded) in by_id {
        let Some(classification) = retained_by_id.get(id) else {
            continue;
        };
        let component_index = component_indexes_by_name
            .get(&loaded.entry.component)
            .expect("entry component was validated");
        let manifest_component = &manifest.components[*component_index];
        if classification.component != manifest_component.name
            || manifest_component.retained_ids.binary_search(id).is_err()
        {
            bail!("retained ID ownership mismatch for {id}");
        }
    }
    Ok(())
}

fn validate_all_source_files_listed(
    repo_root: &Path,
    seen_sources: &BTreeSet<String>,
) -> Result<()> {
    let source_root = paths::source_dir(repo_root);
    reject_symlink_path(repo_root, &source_root)?;
    for entry in walkdir::WalkDir::new(&source_root)
        .follow_links(false)
        .into_iter()
    {
        let entry =
            entry.with_context(|| format!("walk schema source root {}", source_root.display()))?;
        if entry.file_type().is_symlink() {
            bail!(
                "schema source tree must not contain symlinks: {}",
                entry.path().display()
            );
        }
        if !entry.file_type().is_file()
            || entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("json")
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(repo_root)
            .with_context(|| format!("strip prefix for {}", entry.path().display()))?
            .to_str()
            .ok_or_else(|| {
                SchemaToolError::msg(format!(
                    "schema source path is not valid UTF-8: {}",
                    entry.path().display()
                ))
            })?
            .replace('\\', "/");
        if !seen_sources.contains(&relative) {
            bail!("source file not listed in manifest: {relative}");
        }
    }
    Ok(())
}

fn require_strictly_sorted_unique(values: &[String], location: &str) -> Result<()> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        bail!("{location} must be strictly sorted and unique");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedBaseline {
    fixture_version: u32,
    entries: Vec<RetainedBaselineEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedBaselineEntry {
    id: String,
    component: String,
    source: String,
    source_sha256: String,
}

impl RetainedBaseline {
    fn load(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(RETAINED_BASELINE_PATH);
        paths::require_file(&path)?;
        let baseline: Self = serde_json::from_str(
            &std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse retained baseline {}", path.display()))?;
        if baseline.fixture_version != 1 {
            bail!(
                "unsupported retained baseline fixture_version {}; expected 1",
                baseline.fixture_version
            );
        }
        if baseline.entries.len() != 41 {
            bail!(
                "retained baseline must contain exactly 41 historical entries, got {}",
                baseline.entries.len()
            );
        }
        let mut prior_id = None;
        let mut sources = BTreeSet::new();
        for entry in &baseline.entries {
            require_canonical_schema_id(&entry.id, "retained baseline id")?;
            let source = SchemaSourcePath::parse(&entry.source).map_err(|error| {
                SchemaToolError::msg(format!(
                    "invalid retained baseline source path for {}: {error:#}",
                    entry.id
                ))
            })?;
            if !entry.id.starts_with(HISTORICAL_ID_PREFIX)
                || entry.component.is_empty()
                || !is_sha256_hex(&entry.source_sha256)
            {
                bail!("invalid retained baseline entry for {}", entry.id);
            }
            if prior_id
                .as_deref()
                .is_some_and(|prior| prior >= entry.id.as_str())
            {
                bail!("retained baseline entries must be strictly sorted by id");
            }
            prior_id = Some(entry.id.clone());
            if !sources.insert(source.as_str().to_owned()) {
                bail!("retained baseline source paths must be unique");
            }
        }
        Ok(baseline)
    }

    fn by_id(&self) -> Result<BTreeMap<String, &RetainedBaselineEntry>> {
        let mut entries = BTreeMap::new();
        for entry in &self.entries {
            if entries.insert(entry.id.clone(), entry).is_some() {
                bail!("retained baseline IDs must be unique");
            }
        }
        Ok(entries)
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Hash source bytes exactly as loaded. Generated artifacts are intentionally
/// checked by the normal generation/check Git-drift gate, not this migration ledger.
#[cfg(test)]
fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_baseline_fixture_matches_current_sources() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_path_buf();
        let baseline = RetainedBaseline::load(&root).expect("load retained baseline");
        for entry in baseline.entries {
            assert_eq!(
                sha256_file(&root.join(&entry.source)).expect("hash source"),
                entry.source_sha256,
                "historical source bytes changed: {}",
                entry.source
            );
        }
    }
}
