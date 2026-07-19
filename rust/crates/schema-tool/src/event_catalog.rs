//! Event catalog authority discovery and domain-specific fact projection.
//!
//! Event catalog facts are projected from the registry-cached, contract-neutral
//! conditional-envelope IR. Renderer names are deliberately absent.

use crate::compatibility::SchemaCompatibility;
use crate::conditional_envelope::EnvelopeConditionalBinding;
use crate::error::SchemaToolError;
use crate::manifest::{LoadedSchema, SchemaRegistry};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

const EVENT_ENVELOPE_V2_ID: &str = "https://schemas.shittim.local/event/event_envelope/v2";
const EVENT_ENVELOPE_V2_TITLE: &str = "EventEnvelopeV2";
const EVENT_ENVELOPE_V2_SOURCE: &str = "schemas/source/event/event_envelope.v2.json";
const RETAINED_EVENT_ENVELOPE_ID: &str =
    "https://schemas.shittim.local/v1/event/event_envelope.json";

const ACTIVE_EVENT_BINDINGS: &[(&str, &str, &str, u64)] = &[
    (
        "task.created",
        "task",
        "https://schemas.shittim.local/v1/event/task_created_payload.json",
        1,
    ),
    (
        "task.state_changed",
        "task",
        "https://schemas.shittim.local/v1/event/task_state_changed_payload.json",
        1,
    ),
    (
        "action.state_changed",
        "action",
        "https://schemas.shittim.local/event/action_state_changed_payload/v1",
        1,
    ),
    (
        "approval.state_changed",
        "approval_chain",
        "https://schemas.shittim.local/event/approval_state_changed_payload/v1",
        1,
    ),
    (
        "stop_fence.activated",
        "stop_fence",
        "https://schemas.shittim.local/v1/event/stop_fence_activated_payload.json",
        1,
    ),
];

const LEGACY_EVENT_BINDINGS: &[(&str, &str, &str, u64)] = &[
    (
        "task.created",
        "task",
        "https://schemas.shittim.local/v1/event/task_created_payload.json",
        1,
    ),
    (
        "task.state_changed",
        "task",
        "https://schemas.shittim.local/v1/event/task_state_changed_payload.json",
        1,
    ),
    (
        "stop_fence.activated",
        "stop_fence",
        "https://schemas.shittim.local/v1/event/stop_fence_activated_payload.json",
        1,
    ),
];

/// Neutral event type binding fact. Language field names are renderer concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTypeBindingFact {
    pub event_type: String,
    pub aggregate_type: String,
    pub payload_schema_id: String,
    pub payload_schema_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelopeCatalogAuthority {
    pub schema_id: String,
    pub bindings: Vec<EventTypeBindingFact>,
}

/// Active and retained Event authorities are lifecycle-orthogonal and explicit.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EventCatalogAuthorities {
    pub active: Option<EventEnvelopeCatalogAuthority>,
    pub legacy_v1: Option<EventEnvelopeCatalogAuthority>,
}

impl EventCatalogAuthorities {
    pub fn has_no_authority(&self) -> bool {
        self.active.is_none() && self.legacy_v1.is_none()
    }
}

/// Run the claimant-only gate before strict conditional-envelope analysis.
/// This preserves the IC ordering: broad candidates are collected first, then
/// exact metadata/root requirements reject impostors. Mapping facts are not
/// parsed or produced by this phase.
pub(crate) fn validate_event_catalog_claimant_gate(registry: &SchemaRegistry) -> Result<()> {
    let claimants = event_envelope_v2_claimants(registry)?;
    for claimant in &claimants {
        validate_exact_event_envelope_v2_claimant(claimant)?;
    }
    if claimants.len() > 1 {
        bail!(
            "active EventEnvelopeV2 authority has {} claimants; exactly one is required",
            claimants.len()
        );
    }
    Ok(())
}

fn event_envelope_v2_claimants(registry: &SchemaRegistry) -> Result<Vec<&LoadedSchema>> {
    let mut claimants = Vec::new();
    for (_, loaded) in registry.loaded_schemas() {
        if is_event_envelope_v2_claimant(registry, loaded)? {
            claimants.push(loaded);
        }
    }
    Ok(claimants)
}

/// Discover exact active and retained Event envelope authorities.
pub fn discover_event_catalog_authorities(
    registry: &SchemaRegistry,
) -> Result<EventCatalogAuthorities> {
    let claimants = event_envelope_v2_claimants(registry)?;

    for claimant in &claimants {
        validate_exact_event_envelope_v2_claimant(claimant)?;
    }

    let active = match claimants.len() {
        0 => None,
        1 => {
            let bindings = parse_and_validate_event_bindings(
                registry,
                claimants[0],
                ACTIVE_EVENT_BINDINGS,
                "active EventEnvelopeV2",
            )?;
            Some(EventEnvelopeCatalogAuthority {
                schema_id: claimants[0].id().to_owned(),
                bindings,
            })
        }
        count => {
            bail!("active EventEnvelopeV2 authority has {count} claimants; exactly one is required")
        }
    };

    let legacy_v1 = registry
        .loaded_schemas()
        .find_map(|(id, loaded)| (id == RETAINED_EVENT_ENVELOPE_ID).then_some(loaded))
        .map(|loaded| {
            parse_and_validate_event_bindings(
                registry,
                loaded,
                LEGACY_EVENT_BINDINGS,
                "retained EventEnvelope v1",
            )
            .map(|bindings| EventEnvelopeCatalogAuthority {
                schema_id: loaded.id().to_owned(),
                bindings,
            })
        })
        .transpose()?;

    Ok(EventCatalogAuthorities { active, legacy_v1 })
}

/// Target-local Event authority facts. Active envelope and all active payload
/// roots form one indivisible target unit; either direction of partial presence
/// fails closed. Retained v1 remains independently legal.
pub fn compile_target_event_catalog_facts(
    registry: &SchemaRegistry,
    target_name: &str,
    closure: &BTreeSet<String>,
) -> Result<EventCatalogAuthorities> {
    let global = discover_event_catalog_authorities(registry)?;
    let legacy_v1 = global
        .legacy_v1
        .filter(|authority| closure.contains(&authority.schema_id));

    let Some(active) = global.active else {
        return Ok(EventCatalogAuthorities {
            active: None,
            legacy_v1,
        });
    };

    let envelope_present = closure.contains(&active.schema_id);
    let present_payloads: Vec<_> = active
        .bindings
        .iter()
        .filter(|binding| closure.contains(&binding.payload_schema_id))
        .map(|binding| binding.payload_schema_id.as_str())
        .collect();

    if !envelope_present && !present_payloads.is_empty() {
        bail!(
            "target {target_name} contains active Event payload root(s) {present_payloads:?} but is missing authority envelope {}",
            active.schema_id
        );
    }
    if envelope_present {
        let missing: Vec<_> = active
            .bindings
            .iter()
            .filter(|binding| !closure.contains(&binding.payload_schema_id))
            .map(|binding| binding.payload_schema_id.as_str())
            .collect();
        if !missing.is_empty() {
            bail!(
                "target {target_name} contains active EventEnvelopeV2 but is missing payload root(s) {missing:?}"
            );
        }
    }

    Ok(EventCatalogAuthorities {
        active: envelope_present.then_some(active),
        legacy_v1,
    })
}

fn is_event_envelope_v2_claimant(
    _registry: &SchemaRegistry,
    loaded: &LoadedSchema,
) -> Result<bool> {
    let entry = loaded.entry();
    if entry.id == EVENT_ENVELOPE_V2_ID
        || entry.title == EVENT_ENVELOPE_V2_TITLE
        || entry.source == EVENT_ENVELOPE_V2_SOURCE
    {
        return Ok(true);
    }
    Ok(looks_like_event_envelope_v2_structure(loaded))
}

/// Recognize the deliberately broad IC §5.6 structural claimant gate.
///
/// This predicate must not call the strict conditional-envelope parser or
/// derive mapping facts. It only observes enough raw SchemaNode structure to
/// ensure near-miss EventEnvelopeV2 impostors enter the claimant set and are
/// rejected by the exact gate below.
pub(crate) fn looks_like_event_envelope_v2_structure(loaded: &LoadedSchema) -> bool {
    let entry = loaded.entry();
    if entry.component != "event" || entry.kind != "envelope" {
        return false;
    }
    let document = loaded.document();
    let Some(properties) = document.get("properties").and_then(Value::as_object) else {
        return false;
    };
    if properties
        .get("schema_version")
        .and_then(Value::as_object)
        .and_then(|schema| schema.get("const"))
        .and_then(Value::as_u64)
        != Some(2)
    {
        return false;
    }
    let Some(type_schema) = properties.get("type").and_then(Value::as_object) else {
        return false;
    };
    if type_schema.get("type").and_then(Value::as_str) != Some("string")
        || type_schema
            .get("enum")
            .and_then(Value::as_array)
            .is_none_or(|values| values.is_empty() || values.iter().any(|value| !value.is_string()))
    {
        return false;
    }
    document
        .get("allOf")
        .and_then(Value::as_array)
        .is_some_and(|branches| branches.iter().any(looks_like_event_mapping_branch))
}

fn looks_like_event_mapping_branch(branch: &Value) -> bool {
    let discriminator = branch
        .get("if")
        .and_then(|value| value.get("properties"))
        .and_then(|value| value.get("type"))
        .and_then(|value| value.get("const"))
        .and_then(Value::as_str);
    let then_properties = branch
        .get("then")
        .and_then(|value| value.get("properties"))
        .and_then(Value::as_object);
    discriminator.is_some()
        && then_properties.is_some_and(|properties| {
            properties.contains_key("aggregate_type") && properties.contains_key("payload")
        })
}

fn validate_exact_event_envelope_v2_claimant(loaded: &LoadedSchema) -> Result<()> {
    let entry = loaded.entry();
    if entry.id != EVENT_ENVELOPE_V2_ID
        || entry.title != EVENT_ENVELOPE_V2_TITLE
        || entry.source != EVENT_ENVELOPE_V2_SOURCE
        || entry.component != "event"
        || entry.kind != "envelope"
        || entry.version != 2
        || entry.compatibility != SchemaCompatibility::BreakingReplacement
        || entry.schema_version_field.as_deref() != Some("schema_version")
    {
        bail!(
            "partial active EventEnvelopeV2 claimant {}: expected id={} title={} source={} component=event kind=envelope version=2 compatibility=breaking-replacement schema_version_field=schema_version",
            entry.id,
            EVENT_ENVELOPE_V2_ID,
            EVENT_ENVELOPE_V2_TITLE,
            EVENT_ENVELOPE_V2_SOURCE
        );
    }

    let document = loaded.document();
    if document.get("type").and_then(Value::as_str) != Some("object") {
        bail!("active EventEnvelopeV2 exact root must declare type=object");
    }
    if document
        .get("additionalProperties")
        .and_then(Value::as_bool)
        != Some(false)
    {
        bail!("active EventEnvelopeV2 exact root must declare additionalProperties=false");
    }
    let required = required_set(document);
    let missing: Vec<_> = ["type", "schema_version", "aggregate_type", "payload"]
        .into_iter()
        .filter(|field| !required.contains(*field))
        .collect();
    if !missing.is_empty() {
        bail!("active EventEnvelopeV2 exact root is missing required fields {missing:?}");
    }
    Ok(())
}

fn parse_and_validate_event_bindings(
    registry: &SchemaRegistry,
    loaded: &LoadedSchema,
    expected: &[(&str, &str, &str, u64)],
    authority_name: &str,
) -> Result<Vec<EventTypeBindingFact>> {
    let binding = registry
        .conditional_envelope_binding(loaded.id())?
        .ok_or_else(|| {
            SchemaToolError::msg(format!(
                "{authority_name} has no conditional payload mapping"
            ))
        })?;
    validate_exact_event_envelope_root_mapping(loaded, binding, authority_name)?;
    if binding.discriminator != "type" {
        bail!("{authority_name} discriminator must be exactly `type`");
    }
    let actual = event_facts_from_binding(registry, binding)?;
    let exact = expected_event_facts(expected);
    if actual != exact {
        bail!(
            "{authority_name} closed binding contract mismatch; expected={exact:?}, actual={actual:?}"
        );
    }
    if binding
        .mappings
        .iter()
        .find(|mapping| mapping.discriminator_value == "stop_fence.activated")
        .and_then(|mapping| mapping.string_constants.get("aggregate_id"))
        .map(String::as_str)
        != Some("global")
    {
        bail!("{authority_name} stop_fence.activated must constrain aggregate_id const=\"global\"");
    }
    Ok(actual)
}

fn validate_exact_event_envelope_root_mapping(
    loaded: &LoadedSchema,
    binding: &EnvelopeConditionalBinding,
    authority_name: &str,
) -> Result<()> {
    let document = loaded.document();
    if document.get("type").and_then(Value::as_str) != Some("object") {
        bail!("{authority_name} exact root must declare type=object");
    }
    if document
        .get("additionalProperties")
        .and_then(Value::as_bool)
        != Some(false)
    {
        bail!("{authority_name} exact root must declare additionalProperties=false");
    }
    let required = required_set(document);
    let missing: Vec<_> = [
        binding.discriminator.as_str(),
        "schema_version",
        "aggregate_type",
        "payload",
    ]
    .into_iter()
    .filter(|field| !required.contains(*field))
    .collect();
    if !missing.is_empty() {
        bail!("{authority_name} exact root is missing required fields {missing:?}");
    }
    Ok(())
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

fn expected_event_facts(expected: &[(&str, &str, &str, u64)]) -> Vec<EventTypeBindingFact> {
    expected
        .iter()
        .map(
            |(event_type, aggregate_type, payload_schema_id, payload_schema_version)| {
                EventTypeBindingFact {
                    event_type: (*event_type).to_owned(),
                    aggregate_type: (*aggregate_type).to_owned(),
                    payload_schema_id: (*payload_schema_id).to_owned(),
                    payload_schema_version: *payload_schema_version,
                }
            },
        )
        .collect()
}

fn event_facts_from_binding(
    registry: &SchemaRegistry,
    binding: &EnvelopeConditionalBinding,
) -> Result<Vec<EventTypeBindingFact>> {
    binding
        .mappings
        .iter()
        .map(|mapping| {
            let aggregate_type = mapping
                .string_constants
                .get("aggregate_type")
                .cloned()
                .ok_or_else(|| {
                    SchemaToolError::msg(format!(
                        "{}/allOf/{}: Event mapping requires aggregate_type string const",
                        binding.schema_id, mapping.source_order
                    ))
                })?;
            let payload = registry.get(&mapping.payload_type.schema_id)?;
            Ok(EventTypeBindingFact {
                event_type: mapping.discriminator_value.clone(),
                aggregate_type,
                payload_schema_id: mapping.payload_type.schema_id.clone(),
                payload_schema_version: u64::from(payload.entry().version),
            })
        })
        .collect()
}
