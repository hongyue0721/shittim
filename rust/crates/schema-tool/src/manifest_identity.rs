//! Component-native exact identity, source-title, and root schema-version hard gates.

use crate::error::SchemaToolError;
use crate::manifest::ManifestEntry;
use crate::names::to_snake_case;
use anyhow::{bail, Result};
use serde_json::Value;
use url::Url;

const ID_AUTHORITY: &str = "schemas.shittim.local";
const ID_SCHEME: &str = "https";

/// Derive the component-native snake_case stem for a non-retained entry.
///
/// General rule: strip the exact trailing `V{version}` from `title`, then apply
/// project canonical snake_case. KCP envelope titles use fixed stems that drop
/// the domain prefix already encoded by `component=kcp`.
pub fn component_native_stem(entry: &ManifestEntry) -> Result<String> {
    if entry.component == "kcp" && entry.kind == "envelope" {
        match (entry.title.as_str(), entry.version) {
            ("KcpCommandEnvelopeV2", 2) => return Ok("command_envelope".to_owned()),
            ("KcpQueryEnvelopeV2", 2) => return Ok("query_envelope".to_owned()),
            _ => {}
        }
    }

    let version_suffix = format!("V{}", entry.version);
    let Some(base_title) = entry.title.strip_suffix(&version_suffix) else {
        bail!(
            "component-native entry {} title {:?} must end with exact version suffix {version_suffix}",
            entry.id,
            entry.title
        );
    };
    if base_title.is_empty() {
        bail!(
            "component-native entry {} title {:?} has empty base before version suffix",
            entry.id,
            entry.title
        );
    }
    let stem = to_snake_case(base_title);
    validate_snake_case_stem(&stem, &entry.id)?;
    Ok(stem)
}

pub fn expected_component_native_id(component: &str, stem: &str, version: u32) -> String {
    format!("https://{ID_AUTHORITY}/{component}/{stem}/v{version}")
}

pub fn expected_component_native_source(component: &str, stem: &str, version: u32) -> String {
    format!("schemas/source/{component}/{stem}.v{version}.json")
}

/// Exact component-native hard gate for ID, source, title-derived stem, and URL shape.
pub fn validate_component_native_identity(entry: &ManifestEntry) -> Result<()> {
    let stem = component_native_stem(entry)?;
    let expected_id = expected_component_native_id(&entry.component, &stem, entry.version);
    let expected_source = expected_component_native_source(&entry.component, &stem, entry.version);

    if entry.id != expected_id {
        bail!(
            "component-native entry id mismatch for title {:?}: expected {expected_id}, got {}",
            entry.title,
            entry.id
        );
    }
    if entry.source != expected_source {
        bail!(
            "component-native entry source mismatch for {}: expected {expected_source}, got {}",
            entry.id,
            entry.source
        );
    }
    validate_component_native_url_shape(&entry.id, &entry.component, &stem, entry.version)?;
    Ok(())
}

/// Validate the source root title as the sole human-readable identity authority.
pub fn validate_source_title(entry: &ManifestEntry, document: &Value) -> Result<()> {
    let source_title = document
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            SchemaToolError::msg(format!(
                "schema source {} must declare a non-empty root title",
                entry.source
            ))
        })?;
    if source_title.is_empty() {
        bail!(
            "schema source {} root title must be non-empty",
            entry.source
        );
    }
    if entry.title.is_empty() || source_title != entry.title {
        bail!(
            "manifest title mismatch for {}: manifest={:?}, source={:?}",
            entry.id,
            entry.title,
            source_title
        );
    }
    Ok(())
}

/// Validate the single root schema-version fact used by loading and bindings.
///
/// A non-null manifest declaration is intentionally not an alias mechanism: the
/// only supported field is `schema_version`, it must be required at the root,
/// and its positive integer const must equal `entry.version`. A null declaration
/// means the root has no schema-version obligation (for example KCP envelopes).
pub fn validate_schema_version_field(entry: &ManifestEntry, document: &Value) -> Result<()> {
    let Some(field) = entry.schema_version_field.as_deref() else {
        return Ok(());
    };
    if field != "schema_version" {
        bail!(
            "manifest entry {} schema_version_field must be exactly schema_version, got {field:?}",
            entry.id
        );
    }
    let required = document
        .get("required")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            SchemaToolError::msg(format!(
                "manifest entry {} schema_version root field must be required",
                entry.id
            ))
        })?;
    if !required.iter().any(|name| name.as_str() == Some(field)) {
        bail!(
            "manifest entry {} root required must contain schema_version",
            entry.id
        );
    }
    let const_version = root_positive_integer_const(document, field).ok_or_else(|| {
        SchemaToolError::msg(format!(
            "manifest entry {} root schema_version must declare a unique positive integer const",
            entry.id
        ))
    })?;
    if const_version != entry.version {
        bail!(
            "manifest entry {} root schema_version const {const_version} must equal entry.version {}",
            entry.id,
            entry.version
        );
    }
    Ok(())
}

/// Return the already-validated root schema version for a versioned entry.
pub fn required_root_schema_version(entry: &ManifestEntry, document: &Value) -> Result<u32> {
    validate_schema_version_field(entry, document)?;
    if entry.schema_version_field.as_deref() != Some("schema_version") {
        bail!(
            "binding schema {} must declare schema_version_field=\"schema_version\"",
            entry.id
        );
    }
    root_positive_integer_const(document, "schema_version").ok_or_else(|| {
        SchemaToolError::msg(format!(
            "binding schema {} is missing validated root schema_version const",
            entry.id
        ))
        .into()
    })
}

fn positive_integer_const(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => {
            if let Some(u) = number.as_u64() {
                if u >= 1 && u <= u64::from(u32::MAX) {
                    return Some(u as u32);
                }
            }
            if let Some(i) = number.as_i64() {
                if i >= 1 && i <= i64::from(u32::MAX) {
                    return Some(i as u32);
                }
            }
            None
        }
        _ => None,
    }
}

fn validate_snake_case_stem(stem: &str, entry_id: &str) -> Result<()> {
    if stem.is_empty() {
        bail!("component-native stem for {entry_id} must not be empty");
    }
    let mut segments = 0usize;
    for segment in stem.split('_') {
        segments += 1;
        if segment.is_empty() {
            bail!("component-native stem for {entry_id} contains empty underscore segment: {stem}");
        }
        if !segment
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        {
            bail!(
                "component-native stem for {entry_id} must be lowercase ASCII alnum segments joined by '_': {stem}"
            );
        }
    }
    if segments == 0 {
        bail!("component-native stem for {entry_id} has no segments");
    }
    Ok(())
}

fn validate_component_native_url_shape(
    entry_id: &str,
    component: &str,
    stem: &str,
    version: u32,
) -> Result<()> {
    if entry_id.contains('%') {
        bail!("component-native id must not contain percent-encoding: {entry_id}");
    }
    if entry_id.contains('?') {
        bail!("component-native id must not contain a query: {entry_id}");
    }
    if entry_id.contains('#') {
        bail!("component-native id must not contain a fragment: {entry_id}");
    }
    if entry_id.ends_with(".json") || entry_id.contains(".json") {
        bail!("component-native id must not use a .json suffix: {entry_id}");
    }

    let url = Url::parse(entry_id).map_err(|error| {
        SchemaToolError::msg(format!(
            "component-native id is not a valid URL: {entry_id}: {error}"
        ))
    })?;
    if url.scheme() != ID_SCHEME {
        bail!("component-native id scheme must be https: {entry_id}");
    }
    if url.host_str() != Some(ID_AUTHORITY) {
        bail!("component-native id host must be {ID_AUTHORITY}: {entry_id}");
    }
    if url.port().is_some() {
        bail!("component-native id must not include an explicit port: {entry_id}");
    }
    if url.query().is_some() {
        bail!("component-native id must not contain a query: {entry_id}");
    }
    if url.fragment().is_some() {
        bail!("component-native id must not contain a fragment: {entry_id}");
    }
    if url.as_str() != entry_id {
        bail!(
            "component-native id is not canonical absolute form: declared {entry_id:?}, canonical {:?}",
            url.as_str()
        );
    }

    let path = url.path();
    if !path.starts_with('/') || path.ends_with('/') {
        bail!("component-native id path must be /<component>/<stem>/vN: {entry_id}");
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments.len() != 3 {
        bail!(
            "component-native id must have exactly three path segments <component>/<stem>/vN: {entry_id}"
        );
    }
    if segments.iter().any(|segment| segment.is_empty()) {
        bail!("component-native id path contains an empty segment: {entry_id}");
    }
    if segments.iter().any(|segment| segment.contains('.')) {
        bail!("component-native id path segments must not contain '.': {entry_id}");
    }
    if segments[0] != component {
        bail!("component-native id first segment must equal component {component}: {entry_id}");
    }
    if segments[1] != stem {
        bail!("component-native id second segment must equal stem {stem}: {entry_id}");
    }
    let expected_version = format!("v{version}");
    if segments[2] != expected_version {
        bail!("component-native id third segment must be {expected_version}: {entry_id}");
    }
    Ok(())
}

/// Read a root property's unique positive integer const, if present.
pub fn root_positive_integer_const(document: &Value, field: &str) -> Option<u32> {
    let field_schema = document
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(field))?;
    field_schema.get("const").and_then(positive_integer_const)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::SchemaCompatibility;
    use crate::manifest::GenerationTarget;

    fn entry(
        title: &str,
        version: u32,
        component: &str,
        kind: &str,
        id: &str,
        source: &str,
    ) -> ManifestEntry {
        ManifestEntry {
            id: id.to_owned(),
            title: title.to_owned(),
            version,
            source: source.to_owned(),
            component: component.to_owned(),
            kind: kind.to_owned(),
            compatibility: SchemaCompatibility::NewContract,
            generation_targets: vec![GenerationTarget::Rust],
            schema_version_field: Some("schema_version".into()),
        }
    }

    #[test]
    fn kcp_envelope_stems_drop_domain_prefix() {
        let command = entry(
            "KcpCommandEnvelopeV2",
            2,
            "kcp",
            "envelope",
            "https://schemas.shittim.local/kcp/command_envelope/v2",
            "schemas/source/kcp/command_envelope.v2.json",
        );
        assert_eq!(component_native_stem(&command).unwrap(), "command_envelope");
        validate_component_native_identity(&command).unwrap();

        let query = entry(
            "KcpQueryEnvelopeV2",
            2,
            "kcp",
            "envelope",
            "https://schemas.shittim.local/kcp/query_envelope/v2",
            "schemas/source/kcp/query_envelope.v2.json",
        );
        assert_eq!(component_native_stem(&query).unwrap(), "query_envelope");
        validate_component_native_identity(&query).unwrap();
    }

    #[test]
    fn general_title_stem_and_negative_url_shapes() {
        let ok = entry(
            "TaskCreateRequestV2",
            2,
            "kcp",
            "kcp_request",
            "https://schemas.shittim.local/kcp/task_create_request/v2",
            "schemas/source/kcp/task_create_request.v2.json",
        );
        validate_component_native_identity(&ok).unwrap();

        let mut bad = ok.clone();
        bad.id = "https://schemas.shittim.local/kcp/task_create_request/v2.json".into();
        assert!(validate_component_native_identity(&bad).is_err());
        bad.id = "https://schemas.shittim.local/kcp/task_create_request/v2?x=1".into();
        assert!(validate_component_native_identity(&bad).is_err());
        bad.id = "https://schemas.shittim.local/kcp/task.create/v2".into();
        bad.source = "schemas/source/kcp/task.create.v2.json".into();
        assert!(validate_component_native_identity(&bad).is_err());
    }
}
