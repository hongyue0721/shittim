use crate::error::SchemaToolError;
use crate::manifest::SchemaRegistry;
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeSet;

/// Resolve a `$ref` string against a base schema `$id`.
pub fn resolve_ref(
    registry: &SchemaRegistry,
    base_id: &str,
    reference: &str,
) -> Result<(String, Value)> {
    if let Some(fragment) = reference.strip_prefix('#') {
        let base = registry.get(base_id)?;
        let resolved = pointer_get(&base.document, fragment).ok_or_else(|| {
            SchemaToolError::msg(format!("unresolved local $ref {reference} in {base_id}"))
        })?;
        return Ok((base_id.to_string(), resolved.clone()));
    }

    let (id_part, fragment) = match reference.split_once('#') {
        Some((id, frag)) => (id.to_string(), Some(frag)),
        None => (reference.to_string(), None),
    };

    let absolute_id = if id_part.starts_with("https://") || id_part.starts_with("http://") {
        id_part
    } else {
        // Relative path refs are intentionally not used in this repo; require absolute $id.
        return Err(SchemaToolError::msg(format!(
            "relative $ref not supported (must use absolute $id): {reference} from {base_id}"
        ))
        .into());
    };

    let loaded = registry.get(&absolute_id)?;
    if let Some(fragment) = fragment {
        let resolved = pointer_get(&loaded.document, fragment).ok_or_else(|| {
            SchemaToolError::msg(format!(
                "unresolved $ref fragment #{fragment} in {absolute_id} (from {base_id})"
            ))
        })?;
        Ok((absolute_id, resolved.clone()))
    } else {
        Ok((absolute_id, loaded.document.clone()))
    }
}

fn pointer_get<'a>(root: &'a Value, fragment: &str) -> Option<&'a Value> {
    if fragment.is_empty() {
        return Some(root);
    }
    let pointer = if fragment.starts_with('/') {
        fragment.to_string()
    } else {
        format!("/{fragment}")
    };
    // JSON Pointer decoding of ~1 and ~0
    let mut current = root;
    for raw in pointer.split('/').skip(1) {
        let token = raw.replace("~1", "/").replace("~0", "~");
        current = match current {
            Value::Object(map) => map.get(&token)?,
            Value::Array(items) => {
                let index: usize = token.parse().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }
    Some(current)
}

/// Validate that every $ref in the registry can be resolved.
pub fn check_all_refs(registry: &SchemaRegistry) -> Result<()> {
    for (id, loaded) in &registry.by_id {
        let mut seen = BTreeSet::new();
        walk_refs(registry, id, &loaded.document, &mut seen)?;
    }
    Ok(())
}

fn walk_refs(
    registry: &SchemaRegistry,
    base_id: &str,
    node: &Value,
    seen: &mut BTreeSet<(String, String)>,
) -> Result<()> {
    match node {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref") {
                let key = (base_id.to_string(), reference.clone());
                if seen.insert(key) {
                    let (resolved_id, resolved) = resolve_ref(registry, base_id, reference)?;
                    walk_refs(registry, &resolved_id, &resolved, seen)?;
                }
            }
            for (key, value) in map {
                if key == "$ref" {
                    continue;
                }
                walk_refs(registry, base_id, value, seen)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_refs(registry, base_id, item, seen)?;
            }
        }
        _ => {}
    }
    Ok(())
}
