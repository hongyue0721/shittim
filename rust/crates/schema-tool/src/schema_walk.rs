//! Authoritative traversal of Schema-bearing locations in the restricted Draft 2020-12 profile.
//!
//! The walker deliberately follows only keywords whose values are themselves Schemas. It never
//! recursively enters instance-valued keywords such as `const`, `default`, `examples`, or `enum`.

use crate::error::SchemaToolError;
use crate::json_pointer::JsonPointer;
use anyhow::Result;
use serde_json::{Map, Value};

const MAP_SCHEMA_KEYWORDS: &[&str] = &[
    "properties",
    "patternProperties",
    "dependentSchemas",
    "$defs",
    "definitions",
];

const SINGLE_SCHEMA_KEYWORDS: &[&str] = &[
    "additionalProperties",
    "unevaluatedProperties",
    "propertyNames",
    "items",
    "contains",
    "unevaluatedItems",
    "contentSchema",
    "not",
    "if",
    "then",
    "else",
];

const ARRAY_SCHEMA_KEYWORDS: &[&str] = &["prefixItems", "allOf", "anyOf", "oneOf"];

/// Visit every Schema node in pre-order.
///
/// The callback receives the canonical JSON Pointer, whether the node is the document root, and
/// the Schema value. Every visited value is guaranteed to be an object or boolean. Present
/// Schema-bearing containers are type-checked instead of being silently skipped.
pub fn walk_schema_nodes(
    root: &Value,
    mut visitor: impl FnMut(&JsonPointer, bool, &Value) -> Result<()>,
) -> Result<()> {
    walk_node(root, &JsonPointer::root(), true, &mut visitor)
}

fn walk_node(
    node: &Value,
    pointer: &JsonPointer,
    is_root: bool,
    visitor: &mut impl FnMut(&JsonPointer, bool, &Value) -> Result<()>,
) -> Result<()> {
    if !node.is_object() && !node.is_boolean() {
        return Err(SchemaToolError::msg(format!(
            "schema node must be an object or boolean at #{}",
            pointer.as_str()
        ))
        .into());
    }

    visitor(pointer, is_root, node)?;
    let Some(object) = node.as_object() else {
        return Ok(());
    };

    for keyword in MAP_SCHEMA_KEYWORDS {
        let Some(value) = object.get(*keyword) else {
            continue;
        };
        let children = require_object_container(value, keyword, pointer)?;
        let container_pointer = pointer.child(keyword);
        for (name, child) in children {
            walk_node(child, &container_pointer.child(name), false, visitor)?;
        }
    }

    for keyword in SINGLE_SCHEMA_KEYWORDS {
        if let Some(child) = object.get(*keyword) {
            walk_node(child, &pointer.child(keyword), false, visitor)?;
        }
    }

    for keyword in ARRAY_SCHEMA_KEYWORDS {
        let Some(value) = object.get(*keyword) else {
            continue;
        };
        let children = value.as_array().ok_or_else(|| {
            SchemaToolError::msg(format!(
                "schema-bearing keyword {keyword} must be an array at #{}",
                pointer.as_str()
            ))
        })?;
        let container_pointer = pointer.child(keyword);
        for (index, child) in children.iter().enumerate() {
            walk_node(
                child,
                &container_pointer.child(&index.to_string()),
                false,
                visitor,
            )?;
        }
    }

    Ok(())
}

fn require_object_container<'a>(
    value: &'a Value,
    keyword: &str,
    pointer: &JsonPointer,
) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| {
        SchemaToolError::msg(format!(
            "schema-bearing keyword {keyword} must be an object at #{}",
            pointer.as_str()
        ))
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn visits_every_restricted_schema_bearing_location() {
        let map_keywords = [
            "properties",
            "patternProperties",
            "dependentSchemas",
            "$defs",
            "definitions",
        ];
        let single_keywords = [
            "additionalProperties",
            "unevaluatedProperties",
            "propertyNames",
            "items",
            "contains",
            "unevaluatedItems",
            "contentSchema",
            "not",
            "if",
            "then",
            "else",
        ];
        let array_keywords = ["prefixItems", "allOf", "anyOf", "oneOf"];

        let mut root = serde_json::Map::new();
        for keyword in map_keywords {
            root.insert(
                keyword.to_string(),
                json!({"named": {"$dynamicRef": "#bad"}}),
            );
        }
        for keyword in single_keywords {
            root.insert(keyword.to_string(), json!({"$dynamicRef": "#bad"}));
        }
        for keyword in array_keywords {
            root.insert(keyword.to_string(), json!([{"$dynamicRef": "#bad"}]));
        }

        let mut visited = BTreeSet::new();
        walk_schema_nodes(&Value::Object(root), |pointer, is_root, node| {
            assert_eq!(pointer.is_root(), is_root);
            visited.insert(pointer.as_str().to_string());
            if !is_root {
                assert!(node.get("$dynamicRef").is_some());
            }
            Ok(())
        })
        .unwrap();

        let expected_count = 1 + map_keywords.len() + single_keywords.len() + array_keywords.len();
        assert_eq!(visited.len(), expected_count);
        assert!(visited.contains("/unevaluatedItems"));
        assert!(visited.contains("/properties/named"));
    }

    #[test]
    fn reserved_map_keys_are_names_and_instance_data_is_not_traversed() {
        let schema = json!({
            "properties": {
                "$ref": {"$ref": "#/real"},
                "$id": {"type": "string"},
                "$schema": {"type": "boolean"},
                "$dynamicRef": false
            },
            "const": {"$ref": "https://instance.invalid/const"},
            "default": {"$id": "instance"},
            "examples": [{"$dynamicRef": "instance"}],
            "enum": [{"$ref": "https://instance.invalid/enum"}]
        });
        let mut refs = Vec::new();
        walk_schema_nodes(&schema, |pointer, _, node| {
            if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
                refs.push((pointer.as_str().to_string(), reference.to_string()));
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(refs, vec![("/properties/$ref".into(), "#/real".into())]);
    }

    #[test]
    fn boolean_roots_and_children_are_schema_nodes() {
        let mut visited = Vec::new();
        walk_schema_nodes(&json!(true), |pointer, is_root, node| {
            visited.push((pointer.as_str().to_string(), is_root, node.clone()));
            Ok(())
        })
        .unwrap();
        assert_eq!(visited, vec![(String::new(), true, json!(true))]);

        let mut child_seen = false;
        walk_schema_nodes(&json!({"items": false}), |pointer, _, node| {
            if pointer.as_str() == "/items" {
                child_seen = node == &json!(false);
            }
            Ok(())
        })
        .unwrap();
        assert!(child_seen);
    }

    #[test]
    fn malformed_schema_bearing_values_fail_loudly() {
        for schema in [
            json!({"properties": []}),
            json!({"allOf": {}}),
            json!({"items": 1}),
            json!({"properties": {"x": null}}),
        ] {
            assert!(walk_schema_nodes(&schema, |_, _, _| Ok(())).is_err());
        }
    }
}
