//! Method-version registry semantics and target-local authority compilation.
//!
//! Registry loading validates entry lifecycle, binding shape, active Envelope
//! claimants, and proves every binding has at least one complete common target.
//! TargetPlan construction owns closure validation and emits target-local facts;
//! lowering consumes those facts and never re-reads global bindings/authority.
//!
//! Pipeline: `SchemaRegistry -> ValidatedRegistry<Production|Synthetic> ->
//! TargetPlan/TargetSchemaSet -> target-scoped IR`.

use crate::compatibility::SchemaCompatibility;
use crate::error::SchemaToolError;
use crate::json_pointer::{select_json_value, JsonPointer};
use crate::manifest::{
    GenerationTarget, LoadedSchema, ManifestMethodVersionBinding, MethodFamily, SchemaRegistry,
};
use crate::manifest_identity::required_root_schema_version;
use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const COMMAND_ENVELOPE_TITLE: &str = "KcpCommandEnvelopeV2";
const QUERY_ENVELOPE_TITLE: &str = "KcpQueryEnvelopeV2";
const COMMAND_ENVELOPE_ID: &str = "https://schemas.shittim.local/kcp/command_envelope/v2";
const QUERY_ENVELOPE_ID: &str = "https://schemas.shittim.local/kcp/query_envelope/v2";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MethodVersionBindingFact {
    pub family: MethodFamily,
    pub method: String,
    pub active_request_versions: Vec<u32>,
    pub legacy_validation_versions: Vec<u32>,
    pub request_schema_id_by_version: BTreeMap<String, String>,
    pub response_schema_id_by_version: BTreeMap<String, String>,
}

impl From<&ManifestMethodVersionBinding> for MethodVersionBindingFact {
    fn from(binding: &ManifestMethodVersionBinding) -> Self {
        Self {
            family: binding.family,
            method: binding.method.clone(),
            active_request_versions: binding.active_request_versions.clone(),
            legacy_validation_versions: binding.legacy_validation_versions.clone(),
            request_schema_id_by_version: binding.request_schema_id_by_version.clone(),
            response_schema_id_by_version: binding.response_schema_id_by_version.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveEnvelopeAuthority {
    pub command_schema_id: Option<String>,
    pub query_schema_id: Option<String>,
    pub command_methods: Vec<String>,
    pub query_methods: Vec<String>,
}

impl ActiveEnvelopeAuthority {
    pub fn is_empty(&self) -> bool {
        self.command_schema_id.is_none() && self.query_schema_id.is_none()
    }

    pub fn schema_ids(&self) -> impl Iterator<Item = &str> {
        self.command_schema_id
            .iter()
            .chain(self.query_schema_id.iter())
            .map(String::as_str)
    }

    pub fn expected_bindings(&self) -> BTreeSet<(MethodFamily, String)> {
        self.command_methods
            .iter()
            .map(|method| (MethodFamily::Command, method.clone()))
            .chain(
                self.query_methods
                    .iter()
                    .map(|method| (MethodFamily::Query, method.clone())),
            )
            .collect()
    }

    pub fn all_methods_sorted(&self) -> Vec<String> {
        let mut methods = self.command_methods.clone();
        methods.extend(self.query_methods.iter().cloned());
        methods.sort();
        methods
    }
}

/// Profile-neutral registry validation. It deliberately does not build a plan.
pub fn validate_method_version_bindings(registry: &SchemaRegistry) -> Result<()> {
    let bindings = &registry.manifest().method_version_bindings;
    let authority = discover_active_envelope_authority(registry)?;
    if bindings.is_empty() {
        return Ok(());
    }
    if authority.is_empty() {
        bail!("non-empty method_version_bindings require active V2 Envelope authority");
    }
    validate_binding_array_shape(bindings)?;
    validate_binding_coverage(bindings, &authority)?;
    for binding in bindings {
        validate_one_binding(registry, binding)?;
        validate_binding_has_common_target(registry, binding, &authority)?;
    }
    Ok(())
}

/// Discover and validate active V2 Envelope authority with claimant semantics.
/// Any entry that reserves an authority ID/title, or presents the reserved V2
/// discriminator shape, is an audit claimant and must match every identity
/// field exactly. The third condition reserves that shape against partial
/// impostors; it does not recognize arbitrary future envelope support.
pub fn discover_active_envelope_authority(
    registry: &SchemaRegistry,
) -> Result<ActiveEnvelopeAuthority> {
    let command = discover_family_authority(registry, MethodFamily::Command)?;
    let query = discover_family_authority(registry, MethodFamily::Query)?;
    match (command, query) {
        (None, None) => Ok(ActiveEnvelopeAuthority::default()),
        (Some(command), Some(query)) => Ok(ActiveEnvelopeAuthority {
            command_schema_id: Some(command.id().to_owned()),
            query_schema_id: Some(query.id().to_owned()),
            command_methods: root_discriminator_enum(command, "command_type")?,
            query_methods: root_discriminator_enum(query, "query_type")?,
        }),
        _ => bail!("active KCP Envelope authority must contain both command and query V2 families"),
    }
}

fn discover_family_authority(
    registry: &SchemaRegistry,
    family: MethodFamily,
) -> Result<Option<&LoadedSchema>> {
    let identity = EnvelopeIdentity::for_family(family);
    let claimants: Vec<_> = registry
        .loaded_schemas()
        .filter_map(|(_, loaded)| is_envelope_claimant(loaded, &identity).then_some(loaded))
        .collect();
    for claimant in &claimants {
        validate_exact_envelope_claimant(claimant, &identity)?;
    }
    match claimants.len() {
        0 => Ok(None),
        1 => Ok(Some(claimants[0])),
        count => bail!(
            "active {:?} Envelope authority has {count} claimants; exactly one is required",
            family
        ),
    }
}

struct EnvelopeIdentity {
    id: &'static str,
    title: &'static str,
    discriminator: &'static str,
}

impl EnvelopeIdentity {
    fn for_family(family: MethodFamily) -> Self {
        match family {
            MethodFamily::Command => Self {
                id: COMMAND_ENVELOPE_ID,
                title: COMMAND_ENVELOPE_TITLE,
                discriminator: "command_type",
            },
            MethodFamily::Query => Self {
                id: QUERY_ENVELOPE_ID,
                title: QUERY_ENVELOPE_TITLE,
                discriminator: "query_type",
            },
        }
    }
}

fn is_envelope_claimant(loaded: &LoadedSchema, identity: &EnvelopeIdentity) -> bool {
    let entry = loaded.entry();
    entry.id == identity.id
        || entry.title == identity.title
        || (entry.component == "kcp"
            && entry.kind == "envelope"
            && entry.version == 2
            && select_json_value(
                loaded.document(),
                &JsonPointer::from_decoded_segments(["properties", identity.discriminator]),
            )
            .is_ok())
}

fn validate_exact_envelope_claimant(
    loaded: &LoadedSchema,
    identity: &EnvelopeIdentity,
) -> Result<()> {
    let entry = loaded.entry();
    if entry.id != identity.id
        || entry.title != identity.title
        || entry.component != "kcp"
        || entry.kind != "envelope"
        || entry.version != 2
        || entry.compatibility != SchemaCompatibility::BreakingReplacement
    {
        bail!(
            "partial active Envelope authority claimant {}: expected id={} title={} component=kcp kind=envelope version=2 compatibility=breaking-replacement",
            entry.id,
            identity.id,
            identity.title
        );
    }
    root_discriminator_enum(loaded, identity.discriminator)?;
    Ok(())
}

fn root_discriminator_enum(loaded: &LoadedSchema, property: &str) -> Result<Vec<String>> {
    let pointer = JsonPointer::from_decoded_segments(["properties", property]);
    let property_schema = select_json_value(loaded.document(), &pointer).map_err(|error| {
        SchemaToolError::msg(format!(
            "active envelope {} missing root discriminator property {property}: {error}",
            loaded.id()
        ))
    })?;
    let values = string_enum_values(property_schema)?;
    if values.is_empty() {
        bail!(
            "active envelope {} discriminator {property} enum must be non-empty",
            loaded.id()
        );
    }
    let unique: BTreeSet<&str> = values.iter().map(String::as_str).collect();
    if unique.len() != values.len() {
        bail!(
            "active envelope {} discriminator {property} enum must be unique",
            loaded.id()
        );
    }
    Ok(values)
}

fn string_enum_values(schema: &Value) -> Result<Vec<String>> {
    schema
        .get("enum")
        .and_then(Value::as_array)
        .ok_or_else(|| SchemaToolError::msg("discriminator property missing string enum array"))?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                SchemaToolError::msg("discriminator enum values must be strings").into()
            })
        })
        .collect()
}

fn validate_binding_array_shape(bindings: &[ManifestMethodVersionBinding]) -> Result<()> {
    for window in bindings.windows(2) {
        let left_key = (window[0].family, window[0].method.as_str());
        let right_key = (window[1].family, window[1].method.as_str());
        if left_key >= right_key {
            bail!("method_version_bindings must be uniquely sorted by family then method");
        }
    }
    for binding in bindings {
        if binding.method.is_empty() {
            bail!("method_version_bindings method must be non-empty");
        }
    }
    Ok(())
}

fn validate_binding_coverage(
    bindings: &[ManifestMethodVersionBinding],
    authority: &ActiveEnvelopeAuthority,
) -> Result<()> {
    let expected = authority.expected_bindings();
    let actual = bindings
        .iter()
        .map(|binding| (binding.family, binding.method.clone()))
        .collect::<BTreeSet<_>>();
    if actual != expected {
        bail!(
            "method_version_bindings must exactly cover active Envelope methods; missing={:?}, extra={:?}",
            expected.difference(&actual).collect::<Vec<_>>(),
            actual.difference(&expected).collect::<Vec<_>>()
        );
    }
    Ok(())
}

fn validate_one_binding(
    registry: &SchemaRegistry,
    binding: &ManifestMethodVersionBinding,
) -> Result<()> {
    let active = validate_version_list(
        &binding.active_request_versions,
        "active_request_versions",
        binding,
        false,
    )?;
    let legacy = validate_version_list(
        &binding.legacy_validation_versions,
        "legacy_validation_versions",
        binding,
        true,
    )?;
    if let Some(shared) = active.intersection(&legacy).next() {
        bail!(
            "binding {:?}/{} active and legacy sets share {shared}",
            binding.family,
            binding.method
        );
    }
    let request_versions = active.union(&legacy).copied().collect();
    validate_version_map_keys(
        &binding.request_schema_id_by_version,
        &request_versions,
        "request_schema_id_by_version",
        binding,
    )?;
    validate_version_map_keys(
        &binding.response_schema_id_by_version,
        &active,
        "response_schema_id_by_version",
        binding,
    )?;
    for version in active {
        validate_bound_entry(
            registry,
            binding,
            version,
            &binding.request_schema_id_by_version[&canonical_version_key(version)],
            BoundRole::ActiveRequest,
        )?;
        validate_bound_entry(
            registry,
            binding,
            version,
            &binding.response_schema_id_by_version[&canonical_version_key(version)],
            BoundRole::ActiveResponse,
        )?;
    }
    for version in legacy {
        validate_bound_entry(
            registry,
            binding,
            version,
            &binding.request_schema_id_by_version[&canonical_version_key(version)],
            BoundRole::LegacyRequest,
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum BoundRole {
    ActiveRequest,
    ActiveResponse,
    LegacyRequest,
}

fn validate_bound_entry(
    registry: &SchemaRegistry,
    binding: &ManifestMethodVersionBinding,
    version: u32,
    schema_id: &str,
    role: BoundRole,
) -> Result<()> {
    let loaded = registry.get(schema_id).map_err(|_| {
        SchemaToolError::msg(format!(
            "binding {:?}/{} version {version} references unknown schema {schema_id}",
            binding.family, binding.method
        ))
    })?;
    let expected_kind = match role {
        BoundRole::ActiveRequest | BoundRole::LegacyRequest => "kcp_request",
        BoundRole::ActiveResponse => "kcp_response",
    };
    if loaded.entry().kind != expected_kind {
        bail!("binding schema {schema_id} kind must be {expected_kind}");
    }
    if loaded.entry().version != version
        || required_root_schema_version(loaded.entry(), loaded.document())? != version
    {
        bail!("binding schema {schema_id} root/entry version must equal {version}");
    }
    match role {
        BoundRole::ActiveRequest | BoundRole::ActiveResponse
            if loaded.entry().compatibility.is_legacy() =>
        {
            bail!("active binding schema {schema_id} must not have legacy compatibility");
        }
        BoundRole::LegacyRequest
            if loaded.entry().compatibility != SchemaCompatibility::LegacyValidationOnly =>
        {
            bail!("legacy binding request {schema_id} must be legacy-validation-only");
        }
        _ => {}
    }
    Ok(())
}

fn validate_binding_has_common_target(
    registry: &SchemaRegistry,
    binding: &ManifestMethodVersionBinding,
    authority: &ActiveEnvelopeAuthority,
) -> Result<()> {
    let mut ids = bound_schema_ids(binding);
    ids.extend(authority.schema_ids());
    let mut common: Option<BTreeSet<GenerationTarget>> = None;
    for id in ids {
        let targets = registry
            .get(id)?
            .entry()
            .generation_targets
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        common = Some(match common {
            None => targets,
            Some(current) => current.intersection(&targets).copied().collect(),
        });
    }
    if common.is_none_or(|targets| targets.is_empty()) {
        bail!(
            "binding {:?}/{} has no generation target shared by request, response, legacy request, and both Envelope authorities",
            binding.family,
            binding.method
        );
    }
    Ok(())
}

/// Compile target-local binding and authority facts after closure construction.
pub fn compile_target_method_facts(
    registry: &SchemaRegistry,
    target: GenerationTarget,
    closure: &BTreeSet<String>,
) -> Result<(ActiveEnvelopeAuthority, Vec<MethodVersionBindingFact>)> {
    let global = discover_active_envelope_authority(registry)?;
    let authority_ids = global.schema_ids().collect::<Vec<_>>();
    let authority_members = authority_ids
        .iter()
        .filter(|id| closure.contains(**id))
        .count();
    if authority_members != 0 && authority_members != authority_ids.len() {
        bail!(
            "target {} contains partial active Envelope authority; both families must be in the same target",
            target.as_str()
        );
    }
    let authority = if authority_members == authority_ids.len() && !authority_ids.is_empty() {
        global
    } else {
        ActiveEnvelopeAuthority::default()
    };

    let mut facts = Vec::new();
    for binding in &registry.manifest().method_version_bindings {
        let ids = bound_schema_ids(binding);
        let members = ids.iter().filter(|id| closure.contains(**id)).count();
        if members == 0 {
            continue;
        }
        if members != ids.len() || authority.is_empty() {
            bail!(
                "binding {:?}/{} is only partially present in target {} closure",
                binding.family,
                binding.method,
                target.as_str()
            );
        }
        for id in ids {
            let loaded = registry.get(id)?;
            if !loaded.entry().generation_targets.contains(&target) {
                bail!(
                    "binding {:?}/{} schema {id} does not declare target {}",
                    binding.family,
                    binding.method,
                    target.as_str()
                );
            }
        }
        facts.push(MethodVersionBindingFact::from(binding));
    }
    if !facts.is_empty() && facts.len() != registry.manifest().method_version_bindings.len() {
        bail!(
            "target {} must contain the complete active method binding catalog, not a subset",
            target.as_str()
        );
    }
    Ok((authority, facts))
}

fn validate_version_list(
    versions: &[u32],
    field: &str,
    binding: &ManifestMethodVersionBinding,
    allow_empty: bool,
) -> Result<BTreeSet<u32>> {
    if versions.is_empty() && !allow_empty {
        bail!(
            "binding {:?}/{} {field} must be non-empty",
            binding.family,
            binding.method
        );
    }
    let mut set = BTreeSet::new();
    let mut prior = None;
    for version in versions {
        if *version == 0 || prior.is_some_and(|value| value >= *version) {
            bail!(
                "binding {:?}/{} {field} must be strictly ascending positive integers",
                binding.family,
                binding.method
            );
        }
        prior = Some(*version);
        set.insert(*version);
    }
    Ok(set)
}

fn validate_version_map_keys(
    map: &BTreeMap<String, String>,
    expected: &BTreeSet<u32>,
    field: &str,
    binding: &ManifestMethodVersionBinding,
) -> Result<()> {
    let actual = map
        .keys()
        .map(|key| {
            parse_canonical_version_key(key).ok_or_else(|| {
                SchemaToolError::msg(format!(
                    "binding {:?}/{} {field} key {key:?} is not canonical positive decimal",
                    binding.family, binding.method
                ))
                .into()
            })
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if &actual != expected {
        bail!(
            "binding {:?}/{} {field} keys must equal {expected:?}, got {actual:?}",
            binding.family,
            binding.method
        );
    }
    Ok(())
}

fn parse_canonical_version_key(key: &str) -> Option<u32> {
    let value: u32 = key.parse().ok()?;
    (value > 0 && value.to_string() == key).then_some(value)
}

pub fn canonical_version_key(version: u32) -> String {
    version.to_string()
}

fn bound_schema_ids(binding: &ManifestMethodVersionBinding) -> BTreeSet<&str> {
    binding
        .request_schema_id_by_version
        .values()
        .chain(binding.response_schema_id_by_version.values())
        .map(String::as_str)
        .collect()
}
