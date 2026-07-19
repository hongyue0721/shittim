//! Contract-neutral conditional-envelope analysis.
//!
//! Registry loading parses every manifest envelope exactly once into this
//! language-neutral IR. Target closure, typed-envelope projection, and
//! domain-specific catalogs only borrow the cached facts; none reparses raw JSON.

use crate::contract_model::ContractTypeId;
use crate::error::SchemaToolError;
use crate::json_pointer::{select_json_value, JsonPointer};
use crate::manifest::{LoadedSchema, SchemaRegistry};
use crate::resolve::resolve_ref;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One branch of a strict conditional envelope mapping, in source declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeConditionalMapping {
    pub discriminator_value: String,
    pub payload_type: ContractTypeId,
    /// Non-payload `then.properties` string constants, keyed by JSON field name.
    pub string_constants: BTreeMap<String, String>,
    pub source_order: usize,
}

/// Language-neutral IR for a closed discriminator → whole-root payload mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeConditionalBinding {
    pub schema_id: String,
    pub discriminator: String,
    /// Exact root enum declaration order.
    pub discriminator_values: Vec<String>,
    pub mappings: Vec<EnvelopeConditionalMapping>,
}

/// Analyze every manifest envelope once. The returned map is installed into the
/// registry before any catalog, target, or lowering projection can run.
pub(crate) fn analyze_registry_conditional_envelopes(
    registry: &SchemaRegistry,
) -> Result<BTreeMap<String, Option<EnvelopeConditionalBinding>>> {
    registry
        .loaded_schemas()
        .filter(|(_, loaded)| loaded.entry().kind == "envelope")
        .map(|(id, loaded)| {
            parse_envelope_conditional_binding(registry, loaded)
                .map(|binding| (id.to_owned(), binding))
        })
        .collect()
}

/// Parse one contract-neutral, restricted conditional payload mapping shared by
/// Event and KCP envelopes. Zero whole-root payload refs is intentionally
/// untyped; once any branch maps a payload, every `allOf` branch must have the
/// exact mapping shape and the mapping must be bijective.
fn parse_envelope_conditional_binding(
    registry: &SchemaRegistry,
    loaded: &LoadedSchema,
) -> Result<Option<EnvelopeConditionalBinding>> {
    let document = loaded.document();
    let properties = document
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| SchemaToolError::msg(format!("{} missing properties", loaded.id())))?;
    if !properties.contains_key("payload") {
        return Ok(None);
    }
    let Some(branches) = document.get("allOf").and_then(Value::as_array) else {
        return Ok(None);
    };
    if branches.is_empty() {
        return Ok(None);
    }

    let payload_ref_pointer = pointer(["then", "properties", "payload", "$ref"]);
    let mut whole_payload_refs = 0usize;
    let mut payload_ref_errors = Vec::new();
    for (index, branch) in branches.iter().enumerate() {
        let Ok(node) = select_json_value(branch, &payload_ref_pointer) else {
            continue;
        };
        let Some(reference) = node.as_str() else {
            payload_ref_errors.push(format!("allOf/{index}: payload $ref must be a string"));
            continue;
        };
        match resolve_ref(registry, loaded.id(), reference) {
            Ok(resolved) if resolved.type_id.is_root() => whole_payload_refs += 1,
            Ok(_) => payload_ref_errors.push(format!(
                "allOf/{index}: payload mapping must reference a whole manifest schema root"
            )),
            Err(error) => payload_ref_errors.push(format!(
                "allOf/{index}: payload $ref resolution failed: {error}"
            )),
        }
    }
    if whole_payload_refs == 0 {
        if payload_ref_errors.is_empty() {
            return Ok(None);
        }
        bail!(
            "{} has conditional payload mapping candidates but none is a valid whole-root reference: {:?}",
            loaded.id(),
            payload_ref_errors
        );
    }

    let mut discriminator: Option<String> = None;
    let mut by_value = BTreeMap::new();
    for (index, branch) in branches.iter().enumerate() {
        let location = format!("{}/allOf/{index}", loaded.id());
        let branch_object = exact_object(branch, &location, &["if", "then"])?;
        let if_object = exact_object(
            &branch_object["if"],
            &format!("{location}/if"),
            &["properties", "required"],
        )?;
        let if_properties = if_object["properties"].as_object().ok_or_else(|| {
            SchemaToolError::msg(format!("{location}/if/properties must be an object"))
        })?;
        if if_properties.len() != 1 {
            bail!("{location}: if.properties must contain exactly one discriminator");
        }
        let (branch_discriminator, discriminator_schema) =
            if_properties.iter().next().expect("one discriminator");
        let discriminator_schema = exact_object(
            discriminator_schema,
            &format!("{location}/if/properties/{branch_discriminator}"),
            &["const"],
        )?;
        let discriminator_value = discriminator_schema["const"]
            .as_str()
            .ok_or_else(|| {
                SchemaToolError::msg(format!("{location}: discriminator const must be a string"))
            })?
            .to_owned();
        let required =
            exact_string_array(&if_object["required"], &format!("{location}/if/required"))?;
        if required != [branch_discriminator.as_str()] {
            bail!("{location}: if.required must be exactly [{branch_discriminator:?}]");
        }
        match &discriminator {
            None => discriminator = Some(branch_discriminator.clone()),
            Some(expected) if expected == branch_discriminator => {}
            Some(expected) => bail!(
                "{location}: branch discriminator {branch_discriminator:?} differs from {expected:?}"
            ),
        }

        let then_object = exact_object(
            &branch_object["then"],
            &format!("{location}/then"),
            &["properties"],
        )?;
        let then_properties = then_object["properties"].as_object().ok_or_else(|| {
            SchemaToolError::msg(format!("{location}/then/properties must be an object"))
        })?;
        let payload_node = then_properties.get("payload").ok_or_else(|| {
            SchemaToolError::msg(format!("{location}: missing then.properties.payload"))
        })?;
        let payload_object = exact_object(
            payload_node,
            &format!("{location}/then/properties/payload"),
            &["$ref"],
        )?;
        let payload_ref = payload_object["$ref"].as_str().ok_or_else(|| {
            SchemaToolError::msg(format!("{location}: payload $ref must be a string"))
        })?;
        let resolved = resolve_ref(registry, loaded.id(), payload_ref).map_err(|error| {
            SchemaToolError::msg(format!(
                "{location}: payload $ref resolution failed: {error}"
            ))
        })?;
        if !resolved.type_id.is_root() {
            bail!("{location}: payload mapping must reference a whole manifest schema root");
        }

        let mut string_constants = BTreeMap::new();
        for (name, schema) in then_properties {
            if name == "payload" {
                continue;
            }
            let constant = exact_object(
                schema,
                &format!("{location}/then/properties/{name}"),
                &["const"],
            )?["const"]
                .as_str()
                .ok_or_else(|| {
                    SchemaToolError::msg(format!(
                        "{location}: then property {name:?} must be a string const"
                    ))
                })?
                .to_owned();
            string_constants.insert(name.clone(), constant);
        }
        if by_value.contains_key(&discriminator_value) {
            bail!(
                "{location}: duplicate payload mapping for discriminator {discriminator_value:?}"
            );
        }
        by_value.insert(
            discriminator_value.clone(),
            EnvelopeConditionalMapping {
                discriminator_value,
                payload_type: resolved.type_id,
                string_constants,
                source_order: index,
            },
        );
    }

    let discriminator = discriminator.expect("non-empty branches");
    let discriminator_schema = properties.get(&discriminator).ok_or_else(|| {
        SchemaToolError::msg(format!(
            "{} missing discriminator property {discriminator}",
            loaded.id()
        ))
    })?;
    let discriminator_values =
        closed_string_enum(discriminator_schema, loaded.id(), &discriminator)?;
    let enum_set: BTreeSet<_> = discriminator_values.iter().cloned().collect();
    if enum_set.len() != discriminator_values.len() {
        bail!("{} discriminator enum contains duplicates", loaded.id());
    }
    let mapping_set: BTreeSet<_> = by_value.keys().cloned().collect();
    if enum_set != mapping_set {
        let missing: Vec<_> = enum_set.difference(&mapping_set).cloned().collect();
        let extra: Vec<_> = mapping_set.difference(&enum_set).cloned().collect();
        bail!(
            "{} discriminator enum/mapping mismatch; missing={missing:?}, extra={extra:?}",
            loaded.id()
        );
    }
    let required = required_set(document);
    if !required.contains(&discriminator) || !required.contains("payload") {
        bail!(
            "{} typed envelope requires discriminator and payload fields",
            loaded.id()
        );
    }
    let mappings = discriminator_values
        .iter()
        .map(|value| {
            by_value.remove(value).ok_or_else(|| {
                SchemaToolError::msg(format!("{} internal mapping order failure", loaded.id()))
                    .into()
            })
        })
        .collect::<Result<_>>()?;

    Ok(Some(EnvelopeConditionalBinding {
        schema_id: loaded.id().to_owned(),
        discriminator,
        discriminator_values,
        mappings,
    }))
}

fn exact_object<'a>(
    value: &'a Value,
    location: &str,
    expected_keys: &[&str],
) -> Result<&'a Map<String, Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| SchemaToolError::msg(format!("{location} must be an object")))?;
    let actual: BTreeSet<_> = object.keys().map(String::as_str).collect();
    let expected: BTreeSet<_> = expected_keys.iter().copied().collect();
    if actual != expected {
        bail!("{location} must have exact keys {expected:?}, got {actual:?}");
    }
    Ok(object)
}

fn exact_string_array<'a>(value: &'a Value, location: &str) -> Result<Vec<&'a str>> {
    value
        .as_array()
        .ok_or_else(|| SchemaToolError::msg(format!("{location} must be an array")))?
        .iter()
        .map(|item| {
            item.as_str().ok_or_else(|| {
                SchemaToolError::msg(format!("{location} values must be strings")).into()
            })
        })
        .collect()
}

fn closed_string_enum(schema: &Value, schema_id: &str, field: &str) -> Result<Vec<String>> {
    let object = schema.as_object().ok_or_else(|| {
        SchemaToolError::msg(format!(
            "{schema_id} discriminator {field} must be an object"
        ))
    })?;
    if object.get("type").and_then(Value::as_str) != Some("string") {
        bail!("{schema_id} discriminator {field} must declare type=string");
    }
    let values = object
        .get("enum")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            SchemaToolError::msg(format!("{schema_id} discriminator {field} needs enum"))
        })?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                SchemaToolError::msg(format!(
                    "{schema_id} discriminator enum values must be strings"
                ))
                .into()
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if values.is_empty() {
        bail!("{schema_id} discriminator enum must be non-empty");
    }
    Ok(values)
}

fn required_set(document: &Value) -> BTreeSet<String> {
    document
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn pointer(segments: impl IntoIterator<Item = &'static str>) -> JsonPointer {
    JsonPointer::from_decoded_segments(segments)
}
