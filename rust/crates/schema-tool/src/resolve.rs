//! `$ref` resolution against the schema registry.
//!
//! Returns a real [`ContractTypeId`] (schema `$id` + canonical JSON Pointer) and a
//! borrowed document node. No language names, no property-path rewriting.
//!
//! URI handling uses `url` + strict percent-encoding validation:
//! local (`#...`), absolute (`https://...`), and relative refs are resolved with
//! `Url::join` against the base schema `$id`. Fragment is percent-decoded once
//! after validating `%HH` triplets, then parsed as a strict RFC 6901 pointer.

use crate::contract_model::ContractTypeId;
use crate::error::SchemaToolError;
use crate::json_pointer::{pointer_from_decoded_fragment, select_json_value, JsonPointer};
use crate::manifest::SchemaRegistry;
use crate::schema_walk::walk_schema_nodes;
use anyhow::Result;
use percent_encoding::percent_decode;
use serde_json::Value;
use std::collections::BTreeSet;
use url::Url;

/// Successful `$ref` resolution: canonical type identity + borrowed node.
///
/// Single owned-identity form used everywhere (no parallel `ResolvedRef` /
/// `OwnedResolvedRef` names).
#[derive(Debug, Clone)]
pub struct ResolvedSchemaRef<'a> {
    pub type_id: ContractTypeId,
    pub node: &'a Value,
}

/// Resolve a `$ref` string against a base schema `$id`.
///
/// - Local `#/pointer` stays on `base_id`.
/// - Absolute `https://...` (optional `#/pointer`) loads that manifest schema.
/// - Relative refs are joined with `Url::join` against the base `$id`.
/// - Fragment: strict `%HH` validation, one UTF-8 percent-decode, then RFC 6901.
/// - `$anchor` (non-pointer fragment) fails closed.
pub fn resolve_ref<'a>(
    registry: &'a SchemaRegistry,
    base_id: &str,
    reference: &str,
) -> Result<ResolvedSchemaRef<'a>> {
    let base_url = parse_schema_base_url(base_id)?;
    let joined = base_url.join(reference).map_err(|error| {
        SchemaToolError::msg(format!(
            "invalid $ref URI join of {reference:?} against base {base_id}: {error}"
        ))
    })?;

    let fragment_raw = joined.fragment();
    let pointer = match fragment_raw {
        None => JsonPointer::root(),
        Some(fragment) => {
            let decoded = decode_ref_fragment(fragment)?;
            pointer_from_decoded_fragment(&decoded)?
        }
    };

    // Drop fragment so the document id matches manifest `$id` (no fragment).
    let mut document_url = joined.clone();
    document_url.set_fragment(None);
    let schema_id = document_url.as_str().to_string();
    if schema_id.contains('#') {
        return Err(SchemaToolError::msg(format!(
            "resolved schema $id must not contain a fragment: {schema_id} (from {base_id} via {reference})"
        ))
        .into());
    }

    let loaded = registry.get(&schema_id)?;
    registry.component_allows_ref(base_id, &schema_id)?;
    if !registry.is_schema_node_pointer(&schema_id, &pointer)? {
        return Err(SchemaToolError::msg(format!(
            "$ref target is not an authoritative SchemaNode: {reference} from {base_id} -> {schema_id}#{}",
            pointer.as_str()
        ))
        .into());
    }
    let node = authoritative_schema_node(&schema_id, &loaded.document, &pointer)?;
    Ok(ResolvedSchemaRef {
        type_id: ContractTypeId { schema_id, pointer },
        node,
    })
}

/// Locate a node by [`ContractTypeId`] inside the registry.
pub fn schema_at<'a>(registry: &'a SchemaRegistry, type_id: &ContractTypeId) -> Result<&'a Value> {
    let loaded = registry.get(&type_id.schema_id)?;
    if !registry.is_schema_node_pointer(&type_id.schema_id, &type_id.pointer)? {
        return Err(SchemaToolError::msg(format!(
            "identity is not an authoritative SchemaNode: {}",
            type_id.display()
        ))
        .into());
    }
    authoritative_schema_node(&type_id.schema_id, &loaded.document, &type_id.pointer)
        .map_err(anyhow::Error::new)
}

/// Crate-private raw JSON lookup. This does not establish Schema identity; callers requiring a
/// Schema must first validate the pointer against `SchemaRegistry`'s authoritative index.
pub(crate) fn raw_json_at_pointer<'a>(
    document: &'a Value,
    pointer: &JsonPointer,
) -> Result<&'a Value, SchemaToolError> {
    select_json_value(document, pointer)
}

fn authoritative_schema_node<'a>(
    schema_id: &str,
    document: &'a Value,
    pointer: &JsonPointer,
) -> Result<&'a Value, SchemaToolError> {
    raw_json_at_pointer(document, pointer).map_err(|source| SchemaToolError::InternalInvariant {
        schema_id: schema_id.to_owned(),
        pointer: pointer.as_str().to_owned(),
        source: Box::new(source),
    })
}

/// Validate every `$ref` in the registry before it is exposed to public callers.
///
/// The registry is constructed only after this walk succeeds; no deferred public
/// reference-validation phase exists.
pub(crate) fn validate_registry_references(registry: &SchemaRegistry) -> Result<()> {
    for (id, loaded) in registry.loaded_schemas() {
        let mut seen = BTreeSet::new();
        walk_reachable_refs(registry, id, &loaded.document, &mut seen)?;
    }
    Ok(())
}

fn walk_reachable_refs(
    registry: &SchemaRegistry,
    base_id: &str,
    schema: &Value,
    seen: &mut BTreeSet<ContractTypeId>,
) -> Result<()> {
    walk_schema_nodes(schema, |pointer, _, node| {
        let Some(object) = node.as_object() else {
            return Ok(());
        };
        let Some(reference_value) = object.get("$ref") else {
            return Ok(());
        };
        let reference = reference_value.as_str().ok_or_else(|| {
            SchemaToolError::msg(format!(
                "$ref must be a string at {base_id}#{}",
                pointer.as_str()
            ))
        })?;
        let resolved = resolve_ref(registry, base_id, reference)?;
        if seen.insert(resolved.type_id.clone()) {
            walk_reachable_refs(registry, &resolved.type_id.schema_id, resolved.node, seen)?;
        }
        Ok(())
    })
}

/// Parse a schema `$id` as an absolute URL that may serve as a join base.
pub fn parse_schema_base_url(schema_id: &str) -> Result<Url> {
    let url = Url::parse(schema_id).map_err(|error| {
        SchemaToolError::msg(format!(
            "schema $id is not a valid URI: {schema_id}: {error}"
        ))
    })?;
    if url.cannot_be_a_base() {
        return Err(SchemaToolError::msg(format!(
            "schema $id cannot be used as a URI base: {schema_id}"
        ))
        .into());
    }
    if url.fragment().is_some() {
        return Err(SchemaToolError::msg(format!(
            "schema $id must not contain a fragment: {schema_id}"
        ))
        .into());
    }
    Ok(url)
}

/// Require a canonical absolute schema `$id` (absolute http(s) URL, no fragment).
///
/// Canonical means `Url::parse(id).as_str() == id` exactly (serialization equality).
pub fn require_canonical_schema_id(schema_id: &str, location: &str) -> Result<()> {
    let url = parse_schema_base_url(schema_id).map_err(|error| {
        SchemaToolError::msg(format!(
            "non-canonical schema $id at {location}: {schema_id}: {error}"
        ))
    })?;
    // Re-serialize and require exact string equality so relative/non-normalized
    // forms fail closed (e.g. missing scheme, unnormalized path).
    if url.as_str() != schema_id {
        return Err(SchemaToolError::msg(format!(
            "schema $id is not in canonical absolute form at {location}: declared {schema_id:?}, canonical {:?}",
            url.as_str()
        ))
        .into());
    }
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(SchemaToolError::msg(format!(
                "schema $id must be an absolute http(s) URI at {location}: {schema_id} (scheme {other})"
            ))
            .into());
        }
    }
    Ok(())
}

/// Require a canonical absolute `id_base` (absolute http(s) URL, no fragment, trailing `/`).
///
/// `id_base` is the authoritative URL path namespace for every manifest entry `$id`.
pub fn require_canonical_id_base(id_base: &str) -> Result<Url> {
    let url = parse_schema_base_url(id_base).map_err(|error| {
        SchemaToolError::msg(format!(
            "manifest id_base is not a valid absolute URI: {id_base}: {error}"
        ))
    })?;
    if url.as_str() != id_base {
        return Err(SchemaToolError::msg(format!(
            "manifest id_base is not in canonical absolute form: declared {id_base:?}, canonical {:?}",
            url.as_str()
        ))
        .into());
    }
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(SchemaToolError::msg(format!(
                "manifest id_base must be an absolute http(s) URI: {id_base} (scheme {other})"
            ))
            .into());
        }
    }
    if !id_base.ends_with('/') {
        return Err(
            SchemaToolError::msg(format!("manifest id_base must end with '/': {id_base}")).into(),
        );
    }
    Ok(url)
}

/// Require a canonical component namespace under root `id_base`.
///
/// Components are direct, unencoded path segments under the root. This deliberately
/// rejects paths that URL serialization alone could preserve (`//`, `%xx`) because
/// those forms make component ownership ambiguous.
pub fn validate_component_namespace(
    id_base: &Url,
    component_name: &str,
    namespace: &str,
) -> Result<Url> {
    let url = require_canonical_id_base(namespace)?;
    if url.scheme() != id_base.scheme()
        || url.host_str() != id_base.host_str()
        || url.port_or_known_default() != id_base.port_or_known_default()
    {
        return Err(namespace_error(
            id_base,
            namespace,
            "component authority mismatch",
        ));
    }
    let path = url.path();
    if !path.starts_with(id_base.path()) {
        return Err(namespace_error(
            id_base,
            namespace,
            "component path is outside root namespace",
        ));
    }
    let remainder = &path[id_base.path().len()..];
    let Some(segment) = remainder.strip_suffix('/') else {
        return Err(SchemaToolError::msg(format!(
            "component namespace must end in '/': {namespace}"
        ))
        .into());
    };
    if segment != component_name
        || segment.is_empty()
        || segment.contains('/')
        || segment == "."
        || segment == ".."
        || segment.contains('%')
    {
        return Err(SchemaToolError::msg(format!(
            "component namespace must be exactly root/<component>/ with an unencoded direct component segment: component={component_name}, namespace={namespace}"
        )).into());
    }
    Ok(url)
}

/// True when `entry_id` lies under a component namespace.
///
/// This is distinct from root `id_base` membership and intentionally has no
/// retained-ID exception; callers decide that exception explicitly.
pub fn schema_id_in_namespace(namespace: &Url, entry_id: &str) -> Result<()> {
    schema_id_in_id_base_namespace(namespace, entry_id)
}

/// True when `entry_id` is under the `id_base` URL path namespace.
///
/// Comparison uses scheme/host/port/path components, not bare string prefix matching.
/// This rejects spoofing such as `https://schemas.shittim.local/v1_evil/...` against
/// base `https://schemas.shittim.local/v1/`.
pub fn schema_id_in_id_base_namespace(id_base: &Url, entry_id: &str) -> Result<()> {
    let entry = Url::parse(entry_id).map_err(|error| {
        SchemaToolError::msg(format!(
            "schema $id is not a valid URI under id_base: {entry_id}: {error}"
        ))
    })?;
    if entry.scheme() != id_base.scheme() {
        return Err(namespace_error(id_base, entry_id, "scheme mismatch"));
    }
    if entry.host_str() != id_base.host_str() {
        return Err(namespace_error(id_base, entry_id, "host mismatch"));
    }
    if entry.port_or_known_default() != id_base.port_or_known_default() {
        return Err(namespace_error(id_base, entry_id, "port mismatch"));
    }
    let base_path = id_base.path();
    let entry_path = entry.path();
    // Base path is required to end with '/'; entry path must equal base or extend it
    // with additional path segments (component boundary after the base path).
    if entry_path == base_path {
        return Ok(());
    }
    if entry_path.starts_with(base_path) {
        // Because base_path ends with '/', the next character starts a new segment.
        // Reject empty remainder only if equal (handled above); any extension is fine.
        return Ok(());
    }
    Err(namespace_error(
        id_base,
        entry_id,
        "path is outside id_base namespace",
    ))
}

fn namespace_error(id_base: &Url, entry_id: &str, detail: &str) -> anyhow::Error {
    SchemaToolError::msg(format!(
        "manifest entry $id is not under id_base namespace: entry={entry_id}, id_base={}, detail={detail}",
        id_base.as_str()
    ))
    .into()
}

/// Strict percent-decode of a `$ref` fragment.
///
/// 1. Validate every `%` is followed by two hex digits.
/// 2. Percent-decode once.
/// 3. Require the result is valid UTF-8.
fn decode_ref_fragment(fragment: &str) -> Result<String> {
    validate_percent_triplets(fragment)?;
    percent_decode(fragment.as_bytes())
        .decode_utf8()
        .map(|cow| cow.into_owned())
        .map_err(|error| {
            SchemaToolError::msg(format!(
                "percent-decoded $ref fragment is not valid UTF-8: #{fragment}: {error}"
            ))
            .into()
        })
}

fn validate_percent_triplets(input: &str) -> Result<()> {
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(SchemaToolError::msg(format!(
                    "truncated percent-encoding in $ref fragment: {input:?}"
                ))
                .into());
            }
            let hi = bytes[index + 1];
            let lo = bytes[index + 2];
            if !hi.is_ascii_hexdigit() || !lo.is_ascii_hexdigit() {
                return Err(SchemaToolError::msg(format!(
                    "malformed percent-encoding in $ref fragment: {input:?}"
                ))
                .into());
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn raw_json_at_pointer_respects_escaping_and_rejects_bad_index() {
        let doc = json!({
            "properties": {
                "a/b": {"type": "string"},
                "c~d": {"type": "integer"}
            },
            "oneOf": [{"type": "null"}, {"type": "string"}]
        });
        let p = JsonPointer::root().child("properties").child("a/b");
        assert_eq!(
            raw_json_at_pointer(&doc, &p)
                .ok()
                .and_then(|value| value.get("type")),
            Some(&json!("string"))
        );
        let p2 = JsonPointer::root().child("properties").child("c~d");
        assert_eq!(
            raw_json_at_pointer(&doc, &p2)
                .ok()
                .and_then(|value| value.get("type")),
            Some(&json!("integer"))
        );
        let bad = JsonPointer::parse("/oneOf/01").unwrap();
        assert!(raw_json_at_pointer(&doc, &bad).is_err());
        let dash = JsonPointer::parse("/oneOf/-").unwrap();
        assert!(raw_json_at_pointer(&doc, &dash).is_err());
    }

    #[test]
    fn authoritative_selection_failure_preserves_pointer_source() {
        let document = json!({"properties": {}});
        let pointer = JsonPointer::from_decoded_segments(["properties", "missing"]);
        let error = authoritative_schema_node("https://example.test/schema", &document, &pointer)
            .unwrap_err();
        match error {
            SchemaToolError::InternalInvariant {
                schema_id,
                pointer,
                source,
            } => {
                assert_eq!(schema_id, "https://example.test/schema");
                assert_eq!(pointer, "/properties/missing");
                assert!(matches!(
                    *source,
                    SchemaToolError::PointerEvaluation { token_index: 2, .. }
                ));
            }
            other => panic!("expected invariant error, got {other:?}"),
        }
    }

    #[test]
    fn decode_ref_fragment_rules() {
        assert_eq!(decode_ref_fragment("/$defs/x").unwrap(), "/$defs/x");
        assert_eq!(decode_ref_fragment("/a%2Fb").unwrap(), "/a/b");
        assert_eq!(decode_ref_fragment("/%E4%B8%AD").unwrap(), "/中");
        assert!(decode_ref_fragment("/a%2").is_err());
        assert!(decode_ref_fragment("/a%zz").is_err());
        // Invalid UTF-8 after percent-decode: %FF is not valid UTF-8 alone.
        assert!(decode_ref_fragment("/%FF").is_err());
    }

    #[test]
    fn pointer_from_decoded_fragment_rejects_anchor() {
        assert!(pointer_from_decoded_fragment("myAnchor").is_err());
    }

    #[test]
    fn require_canonical_schema_id_rejects_relative_and_fragment() {
        assert!(require_canonical_schema_id(
            "https://schemas.shittim.local/v1/common/actor.json",
            "test"
        )
        .is_ok());
        assert!(require_canonical_schema_id("./actor.json", "test").is_err());
        assert!(require_canonical_schema_id(
            "https://schemas.shittim.local/v1/common/actor.json#frag",
            "test"
        )
        .is_err());
        assert!(require_canonical_schema_id("ftp://example.com/x.json", "test").is_err());
    }

    #[test]
    fn id_base_requires_trailing_slash_and_canonical_form() {
        let ok = require_canonical_id_base("https://schemas.shittim.local/v1/").unwrap();
        assert_eq!(ok.as_str(), "https://schemas.shittim.local/v1/");
        assert!(require_canonical_id_base("https://schemas.shittim.local/v1").is_err());
        assert!(require_canonical_id_base("https://schemas.shittim.local/v1/#frag").is_err());
        assert!(require_canonical_id_base("./v1/").is_err());
    }

    #[test]
    fn id_base_namespace_uses_url_components_not_string_prefix() {
        let base = require_canonical_id_base("https://schemas.shittim.local/v1/").unwrap();
        assert!(schema_id_in_id_base_namespace(
            &base,
            "https://schemas.shittim.local/v1/common/actor.json"
        )
        .is_ok());
        // Prefix spoof: path segment boundary must not accept v1_evil.
        let spoof = schema_id_in_id_base_namespace(
            &base,
            "https://schemas.shittim.local/v1_evil/common/actor.json",
        );
        assert!(spoof.is_err(), "{spoof:?}");
        // Host spoof
        assert!(schema_id_in_id_base_namespace(
            &base,
            "https://evil.shittim.local/v1/common/actor.json"
        )
        .is_err());
        // Scheme spoof
        assert!(schema_id_in_id_base_namespace(
            &base,
            "http://schemas.shittim.local/v1/common/actor.json"
        )
        .is_err());
    }

    #[test]
    fn id_base_canonical_rejects_default_port_and_dot_segment_forms() {
        // url crate drops explicit default ports; serialization equality fails.
        assert!(
            require_canonical_id_base("https://schemas.shittim.local:443/v1/").is_err(),
            "https default port must not be accepted as canonical id_base"
        );
        assert!(
            require_canonical_id_base("http://schemas.shittim.local:80/v1/").is_err(),
            "http default port must not be accepted as canonical id_base"
        );
        // Non-default port is part of the canonical serialization and may be accepted.
        assert!(
            require_canonical_id_base("https://schemas.shittim.local:8443/v1/").is_ok(),
            "non-default port is a distinct authority and remains canonical"
        );

        // Dot segments are normalized by Url::parse; declared form is non-canonical.
        assert!(require_canonical_id_base("https://schemas.shittim.local/v1/./").is_err());
        assert!(require_canonical_id_base("https://schemas.shittim.local/v1/foo/../").is_err());
    }

    #[test]
    fn id_base_double_slash_and_percent_path_follow_url_serialization() {
        // Double slash is preserved by the url crate and can be string-canonical.
        let double = require_canonical_id_base("https://schemas.shittim.local/v1//").unwrap();
        assert_eq!(double.path(), "/v1//");
        // Entries under the ordinary /v1/ path are outside a /v1// base.
        assert!(schema_id_in_id_base_namespace(
            &double,
            "https://schemas.shittim.local/v1/common/actor.json"
        )
        .is_err());
        assert!(schema_id_in_id_base_namespace(
            &double,
            "https://schemas.shittim.local/v1//common/actor.json"
        )
        .is_ok());

        // Percent-encoded path segments stay encoded; namespace is path-string based.
        let percent_base =
            require_canonical_id_base("https://schemas.shittim.local/v1/%63ommon/").unwrap();
        assert_eq!(percent_base.path(), "/v1/%63ommon/");
        assert!(schema_id_in_id_base_namespace(
            &percent_base,
            "https://schemas.shittim.local/v1/%63ommon/actor.json"
        )
        .is_ok());
        // Decoded literal "common" is a different path string, not under %63ommon.
        assert!(schema_id_in_id_base_namespace(
            &percent_base,
            "https://schemas.shittim.local/v1/common/actor.json"
        )
        .is_err());

        // Ordinary /v1/ base accepts percent-encoded extensions that still start with /v1/.
        let base = require_canonical_id_base("https://schemas.shittim.local/v1/").unwrap();
        assert!(schema_id_in_id_base_namespace(
            &base,
            "https://schemas.shittim.local/v1/%63ommon/actor.json"
        )
        .is_ok());
    }

    #[test]
    fn id_base_namespace_port_uses_known_default_equivalence() {
        let base = require_canonical_id_base("https://schemas.shittim.local/v1/").unwrap();
        // Explicit default port is non-canonical for id_base itself, but namespace comparison
        // uses port_or_known_default so a default-port entry still matches the bare base.
        assert!(schema_id_in_id_base_namespace(
            &base,
            "https://schemas.shittim.local:443/v1/common/actor.json"
        )
        .is_ok());
        // Distinct non-default port is a different authority.
        assert!(schema_id_in_id_base_namespace(
            &base,
            "https://schemas.shittim.local:8443/v1/common/actor.json"
        )
        .is_err());
    }
}
