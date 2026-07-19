//! Target-scoped language-neutral contract IR.
//!
//! Pipeline stage:
//! `SchemaRegistry` -> `ValidatedRegistry<Production|Synthetic>` -> `TargetPlan`/`TargetSchemaSet` ->
//! target-scoped IR: `TargetContractGraph` (nodes keyed by [`ContractTypeId`]).
//!
//! Identity is always schema `$id` + strict RFC 6901 JSON Pointer (root uses empty
//! pointer). Fragment `$ref` targets keep their true definition pointer; inline
//! object/enum/const nodes use their real document pointer (`/properties/...`,
//! `/items`, `/oneOf/N`, `/$defs/...`).
//!
//! This module must not introduce language names, include paths, generated paths,
//! or target-specific symbol decisions. Rust symbol cloning for shared `$defs`
//! is a renderer projection concern (`ContractTypeId` ≠ `RustDeclarationId`).

use crate::conditional_envelope::EnvelopeConditionalBinding;
use crate::error::SchemaToolError;
use crate::json_pointer::{select_json_value, JsonPointer};
use crate::manifest::{GenerationTarget, LoadedSchema, SchemaRegistry};
use crate::production_stage::RegistryProfile;
use crate::resolve::resolve_ref;
use crate::target::{TargetPlan, TargetSchemaSet};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// ---------------------------------------------------------------------------
// Neutral identity
// ---------------------------------------------------------------------------

/// Stable identity of a contract type node: schema `$id` + canonical JSON Pointer.
///
/// Root schemas use an empty pointer (`""`). Fragment / inline nodes use a pointer
/// that starts with `/` (for example `/properties/status` or `/$defs/lease`).
///
/// This is **not** a language declaration id. A single [`ContractTypeId`] may be
/// projected to multiple Rust declarations (use-site lineage clones).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ContractTypeId {
    pub schema_id: String,
    pub pointer: JsonPointer,
}

impl ContractTypeId {
    pub fn root(schema_id: impl Into<String>) -> Self {
        Self {
            schema_id: schema_id.into(),
            pointer: JsonPointer::root(),
        }
    }

    pub fn new(schema_id: impl Into<String>, pointer: JsonPointer) -> Self {
        Self {
            schema_id: schema_id.into(),
            pointer,
        }
    }

    pub fn is_root(&self) -> bool {
        self.pointer.is_root()
    }

    pub fn child(&self, segment: &str) -> Self {
        Self {
            schema_id: self.schema_id.clone(),
            pointer: self.pointer.child(segment),
        }
    }

    pub fn index(&self, index: usize) -> Self {
        Self {
            schema_id: self.schema_id.clone(),
            pointer: self.pointer.index(index),
        }
    }

    pub fn display(&self) -> String {
        if self.pointer.is_root() {
            self.schema_id.clone()
        } else {
            format!("{}#{}", self.schema_id, self.pointer.as_str())
        }
    }
}

impl fmt::Display for ContractTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

// ---------------------------------------------------------------------------
// Source provenance (schema facts only)
// ---------------------------------------------------------------------------

/// Where a schema node lives in the source document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSchemaMetadata {
    pub schema_id: String,
    pub pointer: JsonPointer,
}

impl SourceSchemaMetadata {
    pub fn from_type_id(id: &ContractTypeId) -> Self {
        Self {
            schema_id: id.schema_id.clone(),
            pointer: id.pointer.clone(),
        }
    }
}

/// Use-site of a type expression (the schema location that produced the use).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceUseSite {
    pub schema_id: String,
    pub pointer: JsonPointer,
}

impl SourceUseSite {
    pub fn new(schema_id: impl Into<String>, pointer: JsonPointer) -> Self {
        Self {
            schema_id: schema_id.into(),
            pointer,
        }
    }

    pub fn display(&self) -> String {
        if self.pointer.is_root() {
            self.schema_id.clone()
        } else {
            format!("{}#{}", self.schema_id, self.pointer.as_str())
        }
    }
}

// ---------------------------------------------------------------------------
// Neutral type expression & definitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "sign", content = "value", rename_all = "snake_case")]
pub enum JsonInteger {
    Signed(i64),
    Unsigned(u64),
}

impl JsonInteger {
    pub(crate) fn as_i128(self) -> i128 {
        match self {
            Self::Signed(value) => i128::from(value),
            Self::Unsigned(value) => i128::from(value),
        }
    }
}

/// Language-neutral inclusive bounds proven by an integer Schema.
///
/// Bounds remain JSON integer facts; choosing `u32`, `i64`, or another language
/// type is exclusively a renderer decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegerConstraints {
    pub minimum: Option<JsonInteger>,
    pub maximum: Option<JsonInteger>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarKind {
    Integer { constraints: IntegerConstraints },
    Number,
    String,
    Boolean,
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nullability {
    NonNull,
    Nullable,
}

/// Language-neutral type expression. Never holds language type names.
///
/// Type-level null unions (`type: [T, "null"]` / `oneOf: [null, T]`) become
/// [`TypeExpr::Nullable`]. Field-level [`Nullability`] still records whether the
/// Schema accepts JSON null (used by renderers for serde omission policy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeExpr {
    Scalar {
        scalar: ScalarKind,
    },
    /// Arbitrary JSON value (Schema free-form object / true schema).
    AnyJson,
    Array {
        items: Box<TypeUse>,
    },
    /// Type-level nullability wrapper (not a language Option).
    Nullable {
        inner: Box<TypeUse>,
    },
    /// Reference to a graph node by canonical identity.
    Reference {
        id: ContractTypeId,
    },
}

/// A type expression at a concrete use-site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeUse {
    pub expr: TypeExpr,
    pub source: SourceUseSite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectField {
    pub json_name: String,
    pub ty: TypeUse,
    pub presence: Presence,
    pub nullability: Nullability,
    /// Canonical identity of the property schema node (`.../properties/<name>`).
    pub schema_location: ContractTypeId,
}

/// A branch of a source-profile tagged union. `object_type_id` is the canonical
/// identity of the closed branch object, never a renderer-specific declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaggedUnionBranch {
    pub tag: String,
    /// Canonical identity of the branch object. This is distinct from the arm
    /// source because a `$ref` branch lives at `/oneOf/N` but denotes another node.
    pub object_type_id: ContractTypeId,
    /// Concrete `oneOf` arm that declared this branch, retained for diagnostics
    /// and renderer collision reports.
    pub source: SourceUseSite,
}

/// Unknown-field policy carried by object and tagged-union graph nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownFieldPolicy {
    /// JSON object accepts no properties not declared in `properties`.
    Forbid,
    /// JSON object accepts declared properties and retains every additional
    /// property in a renderer-generated extension map during typed round-trip.
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConstJson {
    Null,
    Bool {
        value: bool,
    },
    /// Prefer signed when the JSON number fits in i64; otherwise store as u64.
    Integer {
        value: i64,
    },
    UnsignedInteger {
        value: u64,
    },
    String {
        value: String,
    },
}

/// Shape of a graph node. Supports the full restricted surface so fragment and
/// inline nodes share one representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeShape {
    Scalar {
        scalar: ScalarKind,
    },
    AnyJson,
    Array {
        items: TypeUse,
    },
    Nullable {
        inner: TypeUse,
    },
    Object {
        fields: Vec<ObjectField>,
        /// `additionalProperties` controls standalone object decoding. `true` and
        /// omission are both open under JSON Schema; only `false` is strict.
        unknown_field_policy: UnknownFieldPolicy,
    },
    /// Object `oneOf` that satisfies the closed discriminator source profile.
    /// Branch identities and wire facts remain language-neutral; renderers choose
    /// symbols and variant spellings locally.
    TaggedUnion {
        discriminator: String,
        branches: Vec<TaggedUnionBranch>,
        unknown_field_policy: UnknownFieldPolicy,
    },
    /// Closed string enum. Only wire values; no language variant names.
    StringEnum {
        values: Vec<String>,
    },
    Const {
        value: ConstJson,
    },
    /// Pure `$ref` definition alias (annotation siblings only). The alias node keeps
    /// its own canonical fragment identity while projection follows `target`.
    /// Used by shared `$defs` hosts that re-export whole-schema contracts.
    Alias {
        target: TypeUse,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractTypeNode {
    pub id: ContractTypeId,
    /// Schema `title` keyword when present on this node; never a language name.
    pub schema_title: Option<String>,
    pub source: SourceSchemaMetadata,
    pub shape: TypeShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadMapping {
    pub discriminator_value: String,
    pub payload_type: ContractTypeId,
    /// Source `allOf` branch order, retained for diagnostics and projections.
    pub source_order: usize,
}

/// Typed envelope wire binding discovered from conditional Schema allOf branches.
///
/// Wire fields are **not** re-lowered here: renderers read the envelope root
/// object node in the graph (`envelope_type`) and skip discriminator/payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeWireBinding {
    pub schema_id: String,
    /// Manifest / schema title fact (not a language symbol).
    pub schema_title: String,
    pub discriminator: String,
    /// Graph root object for this envelope schema.
    pub envelope_type: ContractTypeId,
    /// Ordered by the discriminator enum declaration order.
    pub mappings: Vec<PayloadMapping>,
}

/// Neutral catalog facts for a target. Source relative paths are manifest facts,
/// not language include paths.
///
/// Active KCP method catalogs come from V2 Envelope authority when present.
/// When production bindings are empty and V2 authority is absent, retained v1
/// Envelope discriminators are exposed only as explicit legacy facts — never as
/// active authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFacts {
    /// `(schema_id, source_relative_path)` for every schema in the target closure,
    /// ordered by schema_id.
    pub embedded_sources: Vec<(String, String)>,
    /// Active command methods from unique KcpCommandEnvelopeV2 authority, or empty.
    pub kcp_command_methods: Vec<String>,
    /// Active query methods from unique KcpQueryEnvelopeV2 authority, or empty.
    pub kcp_query_methods: Vec<String>,
    /// Canonical merge of active command+query methods (sorted).
    pub kcp_methods: Vec<String>,
    /// Explicit retained v1 command methods. This legacy catalog is independent
    /// of active V2 authority, so both may be populated at the same time.
    pub kcp_legacy_v1_command_methods: Vec<String>,
    /// Explicit retained v1 query methods. This legacy catalog is independent
    /// of active V2 authority, so both may be populated at the same time.
    pub kcp_legacy_v1_query_methods: Vec<String>,
    /// Explicit retained v1 merged methods. This legacy catalog is independent
    /// of active V2 authority, so both may be populated at the same time.
    pub kcp_legacy_v1_methods: Vec<String>,
    /// Active Event catalog bindings derived from exact EventEnvelopeV2 authority.
    pub event_active_bindings: Vec<crate::event_catalog::EventTypeBindingFact>,
    /// Legacy retained EventEnvelope v1 bindings. Lifecycle-orthogonal to active.
    pub event_legacy_v1_bindings: Vec<crate::event_catalog::EventTypeBindingFact>,
    pub method_version_bindings: Vec<crate::method_bindings::MethodVersionBindingFact>,
    pub kcp_protocol_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasResolution {
    /// Stable alias identity retained in the neutral graph.
    pub alias_id: ContractTypeId,
    /// First non-Alias graph node reached by the alias chain.
    pub terminal_id: ContractTypeId,
    /// Ordered aliases traversed from `alias_id` to the terminal target.
    pub chain: Vec<ContractTypeId>,
}

/// Target-scoped language-neutral contract graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetContractGraph {
    pub target: crate::manifest::GenerationTarget,
    /// Source schema ids in the target closure (sorted).
    pub source_schema_ids: Vec<String>,
    /// Nodes keyed by neutral identity. Ordering is BTreeMap order on ContractTypeId.
    pub nodes: BTreeMap<ContractTypeId, ContractTypeNode>,
    /// Single language-neutral Alias resolution fact table. Renderers consume
    /// this table rather than recursively rediscovering alias chains.
    pub alias_resolutions: BTreeMap<ContractTypeId, AliasResolution>,
    pub envelopes: Vec<EnvelopeWireBinding>,
    pub catalog: CatalogFacts,
}

// ---------------------------------------------------------------------------
// Lowering
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct LoweringState {
    nodes: BTreeMap<ContractTypeId, ContractTypeNode>,
    /// Identities currently being emitted; re-entry is idempotent (cycle safe).
    emitting: BTreeSet<ContractTypeId>,
}

/// Lower one target from a profile-validated plan into a neutral contract graph.
///
/// The plan keeps the registry/profile proof private, so external callers cannot
/// combine an arbitrary registry with a separately obtained target closure.
pub fn lower_target_contract_graph<P: RegistryProfile>(
    plan: &TargetPlan<'_, P>,
    target: GenerationTarget,
) -> Result<TargetContractGraph> {
    let schema_set = plan.target(target).ok_or_else(|| {
        SchemaToolError::msg(format!(
            "generation target {} is not present in the validated target plan",
            target.as_str()
        ))
    })?;
    lower_target_contract_graph_in_plan(plan.registry(), schema_set)
}

pub(crate) fn lower_target_contract_graph_in_plan(
    registry: &SchemaRegistry,
    schema_set: &TargetSchemaSet,
) -> Result<TargetContractGraph> {
    let mut state = LoweringState::default();

    for schema_id in schema_set.closure() {
        let loaded = registry.get(schema_id)?;
        audit_schema_tree(&loaded.document, schema_id)?;
        let root_id = ContractTypeId::root(schema_id);
        ensure_node_from_schema(
            registry,
            &mut state,
            &root_id,
            &loaded.document,
            loaded.entry.title.as_str(),
        )?;
    }

    let envelopes = discover_typed_envelopes(registry, schema_set, &state.nodes)?;
    // Envelope analysis is complete inside discover_typed_envelopes:
    // 0 whole-schema payload refs => untyped None (response-only success);
    // >=1 payload refs => every branch must be complete and bijective, else error.

    audit_graph_integrity(&state.nodes)?;
    let alias_resolutions = resolve_and_audit_aliases(&state.nodes)?;
    resolve_field_nullability(&mut state.nodes, &alias_resolutions)?;

    let catalog = build_catalog_facts(registry, schema_set)?;
    let mut source_schema_ids: Vec<String> = schema_set.closure().iter().cloned().collect();
    source_schema_ids.sort();

    Ok(TargetContractGraph {
        target: schema_set.target(),
        source_schema_ids,
        nodes: state.nodes,
        alias_resolutions,
        envelopes,
        catalog,
    })
}

fn build_catalog_facts(
    registry: &SchemaRegistry,
    schema_set: &TargetSchemaSet,
) -> Result<CatalogFacts> {
    let mut embedded_sources = Vec::new();
    for schema_id in schema_set.closure() {
        let loaded = registry.get(schema_id)?;
        embedded_sources.push((schema_id.clone(), loaded.source().as_str().to_owned()));
    }
    embedded_sources.sort_by(|left, right| left.0.cmp(&right.0));

    let authority = schema_set.active_envelope_authority();
    let (kcp_command_methods, kcp_query_methods, kcp_methods) = if authority.is_empty() {
        (Vec::new(), Vec::new(), Vec::new())
    } else {
        (
            authority.command_methods.clone(),
            authority.query_methods.clone(),
            authority.all_methods_sorted(),
        )
    };

    // Retained v1 catalogs are lifecycle-orthogonal to active V2 authority and
    // remain available for legacy preflight until runtime cutover.
    let commands = retained_envelope_discriminators_in_set(
        registry,
        schema_set,
        RETAINED_COMMAND_ENVELOPE_ID,
        "command_type",
    )?;
    let queries = retained_envelope_discriminators_in_set(
        registry,
        schema_set,
        RETAINED_QUERY_ENVELOPE_ID,
        "query_type",
    )?;
    let mut kcp_legacy_v1_methods = commands.clone();
    kcp_legacy_v1_methods.extend(queries.iter().cloned());
    kcp_legacy_v1_methods.sort();
    let kcp_legacy_v1_command_methods = commands;
    let kcp_legacy_v1_query_methods = queries;

    let event_authorities = schema_set.event_catalog_authorities();

    Ok(CatalogFacts {
        embedded_sources,
        kcp_command_methods,
        kcp_query_methods,
        kcp_methods,
        kcp_legacy_v1_command_methods,
        kcp_legacy_v1_query_methods,
        kcp_legacy_v1_methods,
        event_active_bindings: event_authorities
            .active
            .as_ref()
            .map(|authority| authority.bindings.clone())
            .unwrap_or_default(),
        event_legacy_v1_bindings: event_authorities
            .legacy_v1
            .as_ref()
            .map(|authority| authority.bindings.clone())
            .unwrap_or_default(),
        method_version_bindings: schema_set.method_version_bindings().to_vec(),
        kcp_protocol_version: "1.0".into(),
    })
}

const RETAINED_COMMAND_ENVELOPE_ID: &str =
    "https://schemas.shittim.local/v1/kcp/command_envelope.json";
const RETAINED_QUERY_ENVELOPE_ID: &str = "https://schemas.shittim.local/v1/kcp/query_envelope.json";

fn retained_envelope_discriminators_in_set(
    registry: &SchemaRegistry,
    schema_set: &TargetSchemaSet,
    exact_id: &str,
    property: &str,
) -> Result<Vec<String>> {
    let Some(loaded) = registry.loaded_schemas().find_map(|(id, loaded)| {
        if id == exact_id && schema_set.closure().contains(id) {
            Some(loaded)
        } else {
            None
        }
    }) else {
        return Ok(Vec::new());
    };
    let pointer = JsonPointer::from_decoded_segments(["properties", property]);
    string_enum_values(
        select_json_value(loaded.document(), &pointer).map_err(|error| {
            SchemaToolError::msg(format!("{} missing {property}: {error}", loaded.entry().id))
        })?,
    )
}

// ---------------------------------------------------------------------------
// Node emission
// ---------------------------------------------------------------------------

fn ensure_node_from_schema(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    type_id: &ContractTypeId,
    schema: &Value,
    root_title_hint: &str,
) -> Result<()> {
    if state.nodes.contains_key(type_id) {
        return Ok(());
    }
    if !state.emitting.insert(type_id.clone()) {
        // Cycle: node slot is reserved by the outer emit.
        return Ok(());
    }

    let allows_unevaluated_properties = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .is_some_and(|variants| nullable_one_of_indices(variants).is_none());
    validate_supported_schema_node(schema, &type_id.display(), allows_unevaluated_properties)?;
    let schema_title = schema_title_of(schema).or_else(|| {
        if type_id.is_root() && !root_title_hint.is_empty() {
            Some(root_title_hint.to_string())
        } else {
            None
        }
    });

    let shape = lower_shape(registry, state, type_id, schema)?;
    state.emitting.remove(type_id);
    state.nodes.insert(
        type_id.clone(),
        ContractTypeNode {
            id: type_id.clone(),
            schema_title,
            source: SourceSchemaMetadata::from_type_id(type_id),
            shape,
        },
    );
    Ok(())
}

fn lower_shape(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    type_id: &ContractTypeId,
    schema: &Value,
) -> Result<TypeShape> {
    if schema == &Value::Bool(true) {
        return Ok(TypeShape::AnyJson);
    }
    if schema == &Value::Bool(false) {
        return Err(unsupported(
            "schema",
            &type_id.display(),
            "false schema has no inhabited type",
        ));
    }

    // Pure `$ref` definition/alias nodes keep their own ContractTypeId (fragment
    // identity) while projection follows the resolved target. Only annotation
    // siblings are allowed; validation/shape siblings remain unsupported.
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        ensure_ref_has_only_annotation_siblings(schema, &type_id.display())?;
        let resolved = resolve_ref(registry, &type_id.schema_id, reference)?;
        ensure_node_from_schema(registry, state, &resolved.type_id, resolved.node, "")?;
        return Ok(TypeShape::Alias {
            target: TypeUse {
                expr: TypeExpr::Reference {
                    id: resolved.type_id,
                },
                source: SourceUseSite::new(type_id.schema_id.clone(), type_id.pointer.clone()),
            },
        });
    }

    if is_string_enum(schema) {
        return Ok(TypeShape::StringEnum {
            values: string_enum_values(schema)?,
        });
    }
    if is_nullable_string_enum(schema) {
        // Represent as Nullable wrapping a synthetic? Restricted surface stores
        // string values only at named enum nodes; top-level nullable enum is rejected.
        if type_id.is_root() {
            return Err(unsupported(
                "enum",
                &type_id.display(),
                "top-level nullable enum requires an explicit wrapper schema",
            ));
        }
        let values = string_enum_values(schema)?;
        // Keep as StringEnum node; nullability is expressed at the use-site via
        // TypeExpr::Nullable when this schema is used as a property type.
        return Ok(TypeShape::StringEnum { values });
    }

    if let Some(value) = schema.get("const") {
        ensure_const_matches_type(schema, value, &type_id.display())?;
        return Ok(TypeShape::Const {
            value: const_json(value, type_id)?,
        });
    }

    if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
        return lower_one_of_shape(registry, state, type_id, schema, variants);
    }

    if is_object_schema(schema) {
        return Ok(TypeShape::Object {
            fields: lower_object_fields(registry, state, type_id, schema)?,
            unknown_field_policy: object_unknown_field_policy(schema, type_id)?,
        });
    }

    match schema.get("type") {
        Some(Value::String(kind)) => lower_primitive_shape(registry, state, type_id, schema, kind),
        Some(Value::Array(kinds)) => {
            let names: Vec<&str> = kinds.iter().filter_map(Value::as_str).collect();
            if names.len() != kinds.len() {
                return Err(unsupported(
                    "type",
                    &type_id.display(),
                    "type union contains a non-string member",
                ));
            }
            let non_null: Vec<&str> = names
                .iter()
                .copied()
                .filter(|name| *name != "null")
                .collect();
            if names.contains(&"null") && non_null.len() == 1 && names.len() == 2 {
                let inner_shape =
                    lower_primitive_shape(registry, state, type_id, schema, non_null[0])?;
                // Represent type-level null on a named node as Nullable shape only when
                // the non-null arm is not itself a named object/enum (those use use-site
                // Nullable). For scalar/any, store Nullable wrapping a scalar TypeUse.
                match inner_shape {
                    TypeShape::Scalar { scalar } => Ok(TypeShape::Nullable {
                        inner: TypeUse {
                            expr: TypeExpr::Scalar { scalar },
                            source: SourceUseSite::new(
                                type_id.schema_id.clone(),
                                type_id.pointer.clone(),
                            ),
                        },
                    }),
                    TypeShape::AnyJson => Ok(TypeShape::Nullable {
                        inner: TypeUse {
                            expr: TypeExpr::AnyJson,
                            source: SourceUseSite::new(
                                type_id.schema_id.clone(),
                                type_id.pointer.clone(),
                            ),
                        },
                    }),
                    other => Ok(other),
                }
            } else {
                Err(unsupported(
                    "type",
                    &type_id.display(),
                    "only a single non-null type unioned with null is supported",
                ))
            }
        }
        None if schema.get("properties").is_some() => Ok(TypeShape::Object {
            fields: lower_object_fields(registry, state, type_id, schema)?,
            unknown_field_policy: object_unknown_field_policy(schema, type_id)?,
        }),
        None => Err(unsupported(
            "type",
            &type_id.display(),
            "schema without type/$ref/enum/const is not a supported shape",
        )),
        Some(other) => Err(unsupported(
            "type",
            &type_id.display(),
            &format!("unsupported type form: {other}"),
        )),
    }
}

fn lower_one_of_shape(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    type_id: &ContractTypeId,
    schema: &Value,
    variants: &[Value],
) -> Result<TypeShape> {
    if nullable_one_of_indices(variants).is_some() {
        return Err(unsupported(
            "oneOf",
            &type_id.display(),
            "nullable oneOf is only valid at a type use, not as a standalone declaration",
        ));
    }
    lower_tagged_union(
        registry,
        state,
        &type_id.schema_id,
        type_id,
        schema,
        variants,
    )
}

/// The sole `oneOf` classifier. A union is either nullable, a proven tagged
/// union, or unsupported; object lowering never gets a chance to swallow it.
fn classify_one_of_use(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    base_id: &str,
    schema: &Value,
    variants: &[Value],
    preferred_id: &ContractTypeId,
    source: SourceUseSite,
) -> Result<TypeUse> {
    if nullable_one_of_indices(variants).is_some() {
        return nullable_one_of_use(registry, state, base_id, variants, preferred_id, source);
    }
    ensure_node_from_schema(registry, state, preferred_id, schema, "")?;
    let node = state.nodes.get(preferred_id).ok_or_else(|| {
        SchemaToolError::msg(format!(
            "missing tagged union node {}",
            preferred_id.display()
        ))
    })?;
    if !matches!(node.shape, TypeShape::TaggedUnion { .. }) {
        return Err(unsupported(
            "oneOf",
            &preferred_id.display(),
            "non-null oneOf is not a valid tagged union",
        ));
    }
    Ok(TypeUse {
        expr: TypeExpr::Reference {
            id: preferred_id.clone(),
        },
        source,
    })
}

fn nullable_one_of_indices(variants: &[Value]) -> Option<usize> {
    if variants.len() != 2 {
        return None;
    }
    let non_null: Vec<_> = variants
        .iter()
        .enumerate()
        .filter_map(|(index, variant)| (!is_null_type(variant)).then_some(index))
        .collect();
    (non_null.len() == 1 && variants.iter().any(is_null_type)).then_some(non_null[0])
}

fn lower_tagged_union(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    base_id: &str,
    union_id: &ContractTypeId,
    union_schema: &Value,
    variants: &[Value],
) -> Result<TypeShape> {
    if variants.is_empty() {
        return Err(unsupported(
            "oneOf",
            &union_id.display(),
            "tagged union needs at least one branch",
        ));
    }
    let union_properties = union_schema.get("properties").and_then(Value::as_object);
    let enum_candidates: Vec<(String, Vec<String>)> = union_properties
        .into_iter()
        .flat_map(|properties| properties.iter())
        .filter(|(_, property)| is_string_enum(property))
        .map(|(name, property)| string_enum_values(property).map(|values| (name.clone(), values)))
        .collect::<Result<_>>()?;
    if enum_candidates.len() != 1 {
        return Err(unsupported(
            "oneOf",
            &union_id.display(),
            "tagged union requires exactly one union-level string enum discriminator",
        ));
    }
    let (discriminator, enum_values) = enum_candidates.into_iter().next().expect("one candidate");
    if enum_values.is_empty()
        || BTreeSet::<_>::from_iter(enum_values.iter()).len() != enum_values.len()
    {
        return Err(unsupported(
            "oneOf",
            &union_id.display(),
            "discriminator enum must be non-empty and unique",
        ));
    }

    let union_required = required_set(union_schema);
    if !union_required.contains(&discriminator) {
        return Err(unsupported(
            "required",
            &union_id.display(),
            "tagged union discriminator must be required at union level",
        ));
    }

    let union_unevaluated_closed = match union_schema.get("unevaluatedProperties") {
        None => false,
        Some(Value::Bool(false)) => true,
        Some(_) => {
            return Err(unsupported(
                "unevaluatedProperties",
                &union_id.display(),
                "tagged unions only support unevaluatedProperties: false",
            ));
        }
    };
    let mut branches = Vec::new();
    let mut tags = BTreeSet::new();
    for (index, branch_schema) in variants.iter().enumerate() {
        let branch_id = union_id.child("oneOf").index(index);
        let branch_source =
            SourceUseSite::new(union_id.schema_id.clone(), branch_id.pointer.clone());
        let (object_id, object_schema) =
            resolve_union_branch(registry, base_id, branch_schema, &branch_id)?;
        let object = object_schema.as_object().ok_or_else(|| {
            unsupported(
                "oneOf",
                &branch_id.display(),
                "branch must resolve to an object schema",
            )
        })?;
        if !is_object_schema(object_schema) {
            return Err(unsupported(
                "oneOf",
                &branch_id.display(),
                "branch must resolve to an object schema",
            ));
        }
        match object.get("additionalProperties") {
            Some(Value::Bool(false)) => {}
            Some(_) => {
                return Err(unsupported(
                    "additionalProperties",
                    &branch_source.display(),
                    "tagged union branch must not override union unevaluatedProperties:false with a non-false additionalProperties policy",
                ));
            }
            None if union_unevaluated_closed => {}
            None => {
                return Err(unsupported(
                    "additionalProperties",
                    &branch_source.display(),
                    "tagged union branches must be closed by branch additionalProperties:false or union unevaluatedProperties:false",
                ));
            }
        }
        let required = required_set(object_schema);
        if !required.contains(&discriminator) {
            return Err(unsupported(
                "oneOf",
                &branch_id.display(),
                "branch discriminator must be required",
            ));
        }
        let discriminator_pointer =
            JsonPointer::from_decoded_segments(["properties", discriminator.as_str()]);
        let discriminator_schema = select_json_value(object_schema, &discriminator_pointer)
            .map_err(|_| {
                unsupported(
                    "oneOf",
                    &branch_id.display(),
                    "branch discriminator property is missing",
                )
            })?;
        let tag = discriminator_schema
            .get("const")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                unsupported(
                    "oneOf",
                    &branch_id.display(),
                    "branch discriminator must be a single string const",
                )
            })?
            .to_owned();
        if discriminator_schema.get("type").and_then(Value::as_str) != Some("string") {
            return Err(unsupported(
                "oneOf",
                &branch_id.display(),
                "branch discriminator const must declare type string",
            ));
        }
        if !tags.insert(tag.clone()) {
            return Err(unsupported(
                "oneOf",
                &branch_id.display(),
                "duplicate tagged-union discriminator const",
            ));
        }
        ensure_node_from_schema(registry, state, &object_id, object_schema, "")?;
        branches.push(TaggedUnionBranch {
            tag,
            object_type_id: object_id,
            source: branch_source,
        });
    }
    let tag_values: BTreeSet<_> = branches.iter().map(|branch| branch.tag.clone()).collect();
    let enum_set: BTreeSet<_> = enum_values.into_iter().collect();
    if tag_values != enum_set {
        return Err(unsupported(
            "oneOf",
            &union_id.display(),
            "discriminator enum and branch const tags must be bijective",
        ));
    }
    Ok(TypeShape::TaggedUnion {
        discriminator,
        branches,
        unknown_field_policy: UnknownFieldPolicy::Forbid,
    })
}

fn resolve_union_branch<'a>(
    registry: &'a SchemaRegistry,
    base_id: &str,
    branch: &'a Value,
    branch_id: &ContractTypeId,
) -> Result<(ContractTypeId, &'a Value)> {
    if let Some(reference) = branch.get("$ref").and_then(Value::as_str) {
        ensure_ref_has_only_annotation_siblings(branch, &branch_id.display())?;
        let resolved = resolve_ref(registry, base_id, reference)?;
        return Ok((resolved.type_id, resolved.node));
    }
    Ok((branch_id.clone(), branch))
}

fn lower_primitive_shape(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    type_id: &ContractTypeId,
    schema: &Value,
    type_name: &str,
) -> Result<TypeShape> {
    match type_name {
        "null" => Ok(TypeShape::Scalar {
            scalar: ScalarKind::Null,
        }),
        "boolean" => Ok(TypeShape::Scalar {
            scalar: ScalarKind::Boolean,
        }),
        "integer" => Ok(TypeShape::Scalar {
            scalar: ScalarKind::Integer {
                constraints: integer_constraints(schema, &type_id.display())?,
            },
        }),
        "number" => Ok(TypeShape::Scalar {
            scalar: ScalarKind::Number,
        }),
        "string" => Ok(TypeShape::Scalar {
            scalar: ScalarKind::String,
        }),
        "array" => {
            let items = schema.get("items").ok_or_else(|| {
                SchemaToolError::msg(format!("array without items in {}", type_id.display()))
            })?;
            let items_id = type_id.child("items");
            let items_use = schema_to_type_use(
                registry,
                state,
                &type_id.schema_id,
                items,
                &items_id,
                SourceUseSite::new(type_id.schema_id.clone(), items_id.pointer.clone()),
            )?;
            Ok(TypeShape::Array { items: items_use })
        }
        "object" if schema.get("properties").is_some() => Ok(TypeShape::Object {
            fields: lower_object_fields(registry, state, type_id, schema)?,
            unknown_field_policy: object_unknown_field_policy(schema, type_id)?,
        }),
        "object" if schema.get("additionalProperties") == Some(&Value::Bool(true)) => {
            Ok(TypeShape::AnyJson)
        }
        "object" => Err(unsupported(
            "additionalProperties",
            &type_id.display(),
            "free-form object requires explicit additionalProperties: true",
        )),
        other => Err(unsupported(
            "type",
            &type_id.display(),
            &format!("unsupported type `{other}`"),
        )),
    }
}

fn object_unknown_field_policy(
    schema: &Value,
    type_id: &ContractTypeId,
) -> Result<UnknownFieldPolicy> {
    match schema.get("additionalProperties") {
        Some(Value::Bool(false)) => Ok(UnknownFieldPolicy::Forbid),
        Some(Value::Bool(true)) | None => Ok(UnknownFieldPolicy::Allow),
        Some(_) => Err(unsupported(
            "additionalProperties",
            &type_id.display(),
            "schema-valued additionalProperties is not supported",
        )),
    }
}

fn lower_object_fields(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    type_id: &ContractTypeId,
    schema: &Value,
) -> Result<Vec<ObjectField>> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = required_set(schema);
    let mut names: Vec<_> = properties.keys().cloned().collect();
    names.sort();
    let mut fields = Vec::new();
    for json_name in names {
        let property = properties
            .get(&json_name)
            .ok_or_else(|| SchemaToolError::msg(format!("missing property {json_name}")))?;
        let field_location = type_id.child("properties").child(&json_name);
        let ty = schema_to_type_use(
            registry,
            state,
            &type_id.schema_id,
            property,
            &field_location,
            SourceUseSite::new(type_id.schema_id.clone(), field_location.pointer.clone()),
        )?;
        let nullability = immediate_type_use_nullability(&ty);
        fields.push(ObjectField {
            json_name: json_name.clone(),
            ty,
            presence: if required.contains(&json_name) {
                Presence::Required
            } else {
                Presence::Optional
            },
            nullability,
            schema_location: field_location,
        });
    }
    Ok(fields)
}

/// Lower a schema value into a [`TypeUse`] at `preferred_id` (document pointer of this schema).
fn schema_to_type_use(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    base_id: &str,
    schema: &Value,
    preferred_id: &ContractTypeId,
    source: SourceUseSite,
) -> Result<TypeUse> {
    if schema == &Value::Bool(true) {
        return Ok(TypeUse {
            expr: TypeExpr::AnyJson,
            source,
        });
    }
    if schema == &Value::Bool(false) {
        return Err(unsupported(
            "schema",
            &preferred_id.display(),
            "false schema has no inhabited type",
        ));
    }
    let allows_unevaluated_properties = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .is_some_and(|variants| nullable_one_of_indices(variants).is_none());
    validate_supported_schema_node(
        schema,
        &preferred_id.display(),
        allows_unevaluated_properties,
    )?;

    if schema.get("$ref").is_some() {
        ensure_ref_has_only_annotation_siblings(schema, &preferred_id.display())?;
    }

    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let resolved = resolve_ref(registry, base_id, reference)?;
        // Ensure the canonical target node exists exactly once.
        ensure_node_from_schema(registry, state, &resolved.type_id, resolved.node, "")?;
        return Ok(TypeUse {
            expr: TypeExpr::Reference {
                id: resolved.type_id,
            },
            source,
        });
    }

    if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
        return classify_one_of_use(
            registry,
            state,
            base_id,
            schema,
            variants,
            preferred_id,
            source,
        );
    }

    if schema.get("properties").is_some() {
        ensure_node_from_schema(registry, state, preferred_id, schema, "")?;
        let named = TypeUse {
            expr: TypeExpr::Reference {
                id: preferred_id.clone(),
            },
            source: source.clone(),
        };
        return Ok(
            if schema
                .get("type")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|value| value == "null"))
            {
                TypeUse {
                    expr: TypeExpr::Nullable {
                        inner: Box::new(named),
                    },
                    source,
                }
            } else {
                named
            },
        );
    }

    if let Some(value) = schema.get("const") {
        ensure_const_matches_type(schema, value, &preferred_id.display())?;
        ensure_node_from_schema(registry, state, preferred_id, schema, "")?;
        return Ok(TypeUse {
            expr: TypeExpr::Reference {
                id: preferred_id.clone(),
            },
            source,
        });
    }

    if is_nullable_string_enum(schema) {
        ensure_node_from_schema(registry, state, preferred_id, schema, "")?;
        return Ok(TypeUse {
            expr: TypeExpr::Nullable {
                inner: Box::new(TypeUse {
                    expr: TypeExpr::Reference {
                        id: preferred_id.clone(),
                    },
                    source: source.clone(),
                }),
            },
            source,
        });
    }

    if is_string_enum(schema) {
        ensure_node_from_schema(registry, state, preferred_id, schema, "")?;
        return Ok(TypeUse {
            expr: TypeExpr::Reference {
                id: preferred_id.clone(),
            },
            source,
        });
    }

    match schema.get("type") {
        Some(Value::String(kind)) => {
            primitive_or_container_use(registry, state, base_id, schema, kind, preferred_id, source)
        }
        Some(Value::Array(kinds)) => {
            let names: Vec<&str> = kinds.iter().filter_map(Value::as_str).collect();
            if names.len() != kinds.len() {
                return Err(unsupported(
                    "type",
                    &preferred_id.display(),
                    "type union contains a non-string member",
                ));
            }
            let non_null: Vec<&str> = names
                .iter()
                .copied()
                .filter(|name| *name != "null")
                .collect();
            if names.contains(&"null") && non_null.len() == 1 && names.len() == 2 {
                let inner = primitive_or_container_use(
                    registry,
                    state,
                    base_id,
                    schema,
                    non_null[0],
                    preferred_id,
                    source.clone(),
                )?;
                Ok(TypeUse {
                    expr: TypeExpr::Nullable {
                        inner: Box::new(inner),
                    },
                    source,
                })
            } else {
                Err(unsupported(
                    "type",
                    &preferred_id.display(),
                    "only a single non-null type unioned with null is supported",
                ))
            }
        }
        None if schema.get("properties").is_some() => {
            ensure_node_from_schema(registry, state, preferred_id, schema, "")?;
            Ok(TypeUse {
                expr: TypeExpr::Reference {
                    id: preferred_id.clone(),
                },
                source,
            })
        }
        None => Err(unsupported(
            "type",
            &preferred_id.display(),
            "schema without type/$ref/enum/const is not a supported shape",
        )),
        Some(other) => Err(unsupported(
            "type",
            &preferred_id.display(),
            &format!("unsupported type form: {other}"),
        )),
    }
}

fn primitive_or_container_use(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    base_id: &str,
    schema: &Value,
    type_name: &str,
    preferred_id: &ContractTypeId,
    source: SourceUseSite,
) -> Result<TypeUse> {
    match type_name {
        "null" => Ok(TypeUse {
            expr: TypeExpr::Scalar {
                scalar: ScalarKind::Null,
            },
            source,
        }),
        "boolean" => Ok(TypeUse {
            expr: TypeExpr::Scalar {
                scalar: ScalarKind::Boolean,
            },
            source,
        }),
        "integer" => Ok(TypeUse {
            expr: TypeExpr::Scalar {
                scalar: ScalarKind::Integer {
                    constraints: integer_constraints(schema, &preferred_id.display())?,
                },
            },
            source,
        }),
        "number" => Ok(TypeUse {
            expr: TypeExpr::Scalar {
                scalar: ScalarKind::Number,
            },
            source,
        }),
        "string" => Ok(TypeUse {
            expr: TypeExpr::Scalar {
                scalar: ScalarKind::String,
            },
            source,
        }),
        "array" => {
            let items = schema.get("items").ok_or_else(|| {
                SchemaToolError::msg(format!("array without items in {}", preferred_id.display()))
            })?;
            let item_id = preferred_id.child("items");
            let items_use = schema_to_type_use(
                registry,
                state,
                base_id,
                items,
                &item_id,
                SourceUseSite::new(preferred_id.schema_id.clone(), item_id.pointer.clone()),
            )?;
            Ok(TypeUse {
                expr: TypeExpr::Array {
                    items: Box::new(items_use),
                },
                source,
            })
        }
        "object" if schema.get("properties").is_some() => {
            ensure_node_from_schema(registry, state, preferred_id, schema, "")?;
            Ok(TypeUse {
                expr: TypeExpr::Reference {
                    id: preferred_id.clone(),
                },
                source,
            })
        }
        "object" if schema.get("additionalProperties") == Some(&Value::Bool(true)) => Ok(TypeUse {
            expr: TypeExpr::AnyJson,
            source,
        }),
        "object" => Err(unsupported(
            "additionalProperties",
            &preferred_id.display(),
            "free-form object requires explicit additionalProperties: true",
        )),
        other => Err(unsupported(
            "type",
            &preferred_id.display(),
            &format!("unsupported type `{other}`"),
        )),
    }
}

fn nullable_one_of_use(
    registry: &SchemaRegistry,
    state: &mut LoweringState,
    base_id: &str,
    variants: &[Value],
    preferred_id: &ContractTypeId,
    source: SourceUseSite,
) -> Result<TypeUse> {
    if variants.len() != 2 {
        return Err(unsupported(
            "oneOf",
            &preferred_id.display(),
            "only nullable oneOf with exactly [null, T] is supported",
        ));
    }
    let mut non_null_index = None;
    let mut saw_null = false;
    for (index, variant) in variants.iter().enumerate() {
        if is_null_type(variant) {
            saw_null = true;
        } else if non_null_index.is_some() {
            return Err(unsupported(
                "oneOf",
                &preferred_id.display(),
                "ambiguous oneOf requires an explicit generated discriminator strategy",
            ));
        } else {
            non_null_index = Some(index);
        }
    }
    let Some(index) = non_null_index else {
        return Err(unsupported(
            "oneOf",
            &preferred_id.display(),
            "ambiguous oneOf requires an explicit generated discriminator strategy",
        ));
    };
    if !saw_null {
        return Err(unsupported(
            "oneOf",
            &preferred_id.display(),
            "ambiguous oneOf requires an explicit generated discriminator strategy",
        ));
    }
    // Non-null arm identity uses the real oneOf index pointer.
    let arm_id = preferred_id.child("oneOf").index(index);
    let arm_source = SourceUseSite::new(preferred_id.schema_id.clone(), arm_id.pointer.clone());
    let inner = schema_to_type_use(
        registry,
        state,
        base_id,
        &variants[index],
        &arm_id,
        arm_source,
    )?;
    Ok(TypeUse {
        expr: TypeExpr::Nullable {
            inner: Box::new(inner),
        },
        source,
    })
}

fn const_json(value: &Value, type_id: &ContractTypeId) -> Result<ConstJson> {
    match value {
        Value::String(expected) => Ok(ConstJson::String {
            value: expected.clone(),
        }),
        Value::Number(number) if number.is_i64() => Ok(ConstJson::Integer {
            value: number.as_i64().ok_or_else(|| {
                SchemaToolError::msg(format!(
                    "const i64 conversion failed at {}",
                    type_id.display()
                ))
            })?,
        }),
        Value::Number(number) if number.is_u64() => Ok(ConstJson::UnsignedInteger {
            value: number.as_u64().ok_or_else(|| {
                SchemaToolError::msg(format!(
                    "const u64 conversion failed at {}",
                    type_id.display()
                ))
            })?,
        }),
        Value::Bool(expected) => Ok(ConstJson::Bool { value: *expected }),
        Value::Null => Ok(ConstJson::Null),
        _ => Err(unsupported(
            "const",
            &type_id.display(),
            "only string, integer, boolean, and null const values are supported",
        )),
    }
}

// ---------------------------------------------------------------------------
// Typed envelope discovery (reuses graph root objects)
// ---------------------------------------------------------------------------

fn discover_typed_envelopes(
    registry: &SchemaRegistry,
    schema_set: &TargetSchemaSet,
    nodes: &BTreeMap<ContractTypeId, ContractTypeNode>,
) -> Result<Vec<EnvelopeWireBinding>> {
    let mut bindings = Vec::new();
    let event_authorities = schema_set.event_catalog_authorities();
    let event_binding_ids: BTreeSet<&str> = event_authorities
        .active
        .iter()
        .chain(event_authorities.legacy_v1.iter())
        .map(|authority| authority.schema_id.as_str())
        .collect();
    for schema_id in schema_set.closure() {
        let loaded = registry.get(schema_id)?;
        if loaded.entry.kind != "envelope" {
            continue;
        }
        if loaded.entry.component == "event" && !event_binding_ids.contains(schema_id.as_str()) {
            continue;
        }
        let cached_binding = registry.conditional_envelope_binding(schema_id)?;
        let projected = project_envelope_binding(loaded, cached_binding, nodes)?;
        if loaded.entry.component == "event" && projected.is_none() {
            return Err(mapping_error(
                schema_id,
                "validated Event authority lost its conditional mapping",
            ));
        }
        if let Some(binding) = projected {
            for mapping in &binding.mappings {
                if !schema_set
                    .closure()
                    .contains(&mapping.payload_type.schema_id)
                {
                    return Err(SchemaToolError::msg(format!(
                        "generation target closure error: envelope {} payload {} is not in target {}",
                        schema_id,
                        mapping.payload_type.schema_id,
                        schema_set.target().as_str()
                    ))
                    .into());
                }
            }
            bindings.push(binding);
        }
    }
    bindings.sort_by(|left, right| left.schema_id.cmp(&right.schema_id));
    Ok(bindings)
}

fn project_envelope_binding(
    loaded: &LoadedSchema,
    binding: Option<&EnvelopeConditionalBinding>,
    nodes: &BTreeMap<ContractTypeId, ContractTypeNode>,
) -> Result<Option<EnvelopeWireBinding>> {
    let Some(binding) = binding else {
        return Ok(None);
    };
    Ok(Some(project_envelope_wire_binding(loaded, nodes, binding)?))
}

fn project_envelope_wire_binding(
    loaded: &LoadedSchema,
    nodes: &BTreeMap<ContractTypeId, ContractTypeNode>,
    binding: &EnvelopeConditionalBinding,
) -> Result<EnvelopeWireBinding> {
    let envelope_type = ContractTypeId::root(&loaded.entry.id);
    let node = nodes.get(&envelope_type).ok_or_else(|| {
        mapping_error(
            &loaded.entry.id,
            "envelope root object missing from contract graph",
        )
    })?;
    if !matches!(node.shape, TypeShape::Object { .. }) {
        return Err(mapping_error(
            &loaded.entry.id,
            "typed envelope root must lower to an object node",
        ));
    }

    Ok(EnvelopeWireBinding {
        schema_id: loaded.entry.id.clone(),
        schema_title: loaded.entry.title.clone(),
        discriminator: binding.discriminator.clone(),
        envelope_type,
        mappings: binding
            .mappings
            .iter()
            .map(|mapping| PayloadMapping {
                discriminator_value: mapping.discriminator_value.clone(),
                payload_type: mapping.payload_type.clone(),
                source_order: mapping.source_order,
            })
            .collect(),
    })
}

fn mapping_error(location: &str, detail: &str) -> anyhow::Error {
    SchemaToolError::msg(format!(
        "conditional payload mapping error in {location}: {detail}"
    ))
    .into()
}

// ---------------------------------------------------------------------------
// Graph integrity
// ---------------------------------------------------------------------------

fn audit_graph_integrity(nodes: &BTreeMap<ContractTypeId, ContractTypeNode>) -> Result<()> {
    for (id, node) in nodes {
        if &node.id != id {
            return Err(SchemaToolError::msg(format!(
                "graph integrity: node key {} does not match node.id {}",
                id.display(),
                node.id.display()
            ))
            .into());
        }
        if node.source.schema_id != id.schema_id || node.source.pointer != id.pointer {
            return Err(SchemaToolError::msg(format!(
                "graph integrity: source metadata mismatch for {}",
                id.display()
            ))
            .into());
        }
        audit_shape_refs(nodes, id, &node.shape)?;
    }
    Ok(())
}

fn audit_shape_refs(
    nodes: &BTreeMap<ContractTypeId, ContractTypeNode>,
    owner: &ContractTypeId,
    shape: &TypeShape,
) -> Result<()> {
    match shape {
        TypeShape::Scalar { .. }
        | TypeShape::AnyJson
        | TypeShape::StringEnum { .. }
        | TypeShape::Const { .. } => Ok(()),
        TypeShape::Array { items } => audit_type_use_refs(nodes, owner, items),
        TypeShape::Nullable { inner } => audit_type_use_refs(nodes, owner, inner),
        TypeShape::Alias { target } => audit_type_use_refs(nodes, owner, target),
        TypeShape::Object { fields, .. } => {
            for field in fields {
                if field.schema_location.schema_id != owner.schema_id {
                    return Err(SchemaToolError::msg(format!(
                        "graph integrity: field {}.{} schema_location leaves owner schema",
                        owner.display(),
                        field.json_name
                    ))
                    .into());
                }
                audit_type_use_refs(nodes, owner, &field.ty)?;
            }
            Ok(())
        }
        TypeShape::TaggedUnion { branches, .. } => {
            for branch in branches {
                if !nodes.contains_key(&branch.object_type_id) {
                    return Err(SchemaToolError::msg(format!(
                        "graph integrity: {} tagged-union branch {:?} at {} references missing node {}",
                        owner.display(),
                        branch.tag,
                        branch.source.display(),
                        branch.object_type_id.display()
                    ))
                    .into());
                }
            }
            Ok(())
        }
    }
}

fn audit_type_use_refs(
    nodes: &BTreeMap<ContractTypeId, ContractTypeNode>,
    owner: &ContractTypeId,
    ty: &TypeUse,
) -> Result<()> {
    match &ty.expr {
        TypeExpr::Scalar { .. } | TypeExpr::AnyJson => Ok(()),
        TypeExpr::Array { items } => audit_type_use_refs(nodes, owner, items),
        TypeExpr::Nullable { inner } => audit_type_use_refs(nodes, owner, inner),
        TypeExpr::Reference { id } => {
            if !nodes.contains_key(id) {
                return Err(SchemaToolError::msg(format!(
                    "graph integrity: {} references missing node {}",
                    owner.display(),
                    id.display()
                ))
                .into());
            }
            Ok(())
        }
    }
}

fn resolve_and_audit_aliases(
    nodes: &BTreeMap<ContractTypeId, ContractTypeNode>,
) -> Result<BTreeMap<ContractTypeId, AliasResolution>> {
    let aliases: Vec<_> = nodes
        .iter()
        .filter_map(|(id, node)| {
            matches!(node.shape, TypeShape::Alias { .. }).then_some(id.clone())
        })
        .collect();
    let mut resolutions = BTreeMap::new();
    for alias_id in aliases {
        if resolutions.contains_key(&alias_id) {
            continue;
        }
        resolve_alias_path(nodes, &alias_id, &mut resolutions)?;
    }
    Ok(resolutions)
}

fn resolve_alias_path(
    nodes: &BTreeMap<ContractTypeId, ContractTypeNode>,
    start: &ContractTypeId,
    resolutions: &mut BTreeMap<ContractTypeId, AliasResolution>,
) -> Result<()> {
    let mut path = Vec::new();
    let mut positions = BTreeMap::new();
    let mut current = start.clone();
    let terminal_id = loop {
        if let Some(resolved) = resolutions.get(&current) {
            break resolved.terminal_id.clone();
        }
        let node = nodes.get(&current).ok_or_else(|| {
            SchemaToolError::msg(format!(
                "alias resolution error: missing node {}",
                current.display()
            ))
        })?;
        match &node.shape {
            TypeShape::Alias { target } => {
                if let Some(cycle_start) = positions.insert(current.clone(), path.len()) {
                    let mut cycle = path[cycle_start..].to_vec();
                    cycle.push(current);
                    return Err(SchemaToolError::msg(format!(
                        "alias resolution error: pure Alias cycle has no terminal declaration: {}",
                        cycle
                            .iter()
                            .map(ContractTypeId::display)
                            .collect::<Vec<_>>()
                            .join(" -> ")
                    ))
                    .into());
                }
                path.push(current.clone());
                let TypeExpr::Reference { id } = &target.expr else {
                    return Err(SchemaToolError::msg(format!(
                        "alias resolution error: {} target is not a graph reference",
                        current.display()
                    ))
                    .into());
                };
                current = id.clone();
            }
            _ => break current,
        }
    };

    for index in (0..path.len()).rev() {
        let alias_id = path[index].clone();
        let chain = path[index..].to_vec();
        resolutions.insert(
            alias_id.clone(),
            AliasResolution {
                alias_id,
                terminal_id: terminal_id.clone(),
                chain,
            },
        );
    }
    Ok(())
}

fn immediate_type_use_nullability(ty: &TypeUse) -> Nullability {
    match &ty.expr {
        TypeExpr::Nullable { .. }
        | TypeExpr::AnyJson
        | TypeExpr::Scalar {
            scalar: ScalarKind::Null,
        } => Nullability::Nullable,
        TypeExpr::Scalar { .. } | TypeExpr::Array { .. } | TypeExpr::Reference { .. } => {
            Nullability::NonNull
        }
    }
}

fn resolve_field_nullability(
    nodes: &mut BTreeMap<ContractTypeId, ContractTypeNode>,
    alias_resolutions: &BTreeMap<ContractTypeId, AliasResolution>,
) -> Result<()> {
    let snapshot = nodes.clone();
    for node in nodes.values_mut() {
        let TypeShape::Object { fields, .. } = &mut node.shape else {
            continue;
        };
        for field in fields {
            field.nullability =
                if resolved_type_use_allows_null(&snapshot, alias_resolutions, &field.ty)? {
                    Nullability::Nullable
                } else {
                    Nullability::NonNull
                };
        }
    }
    Ok(())
}

fn resolved_type_use_allows_null(
    nodes: &BTreeMap<ContractTypeId, ContractTypeNode>,
    alias_resolutions: &BTreeMap<ContractTypeId, AliasResolution>,
    ty: &TypeUse,
) -> Result<bool> {
    match &ty.expr {
        TypeExpr::Nullable { .. } => Ok(true),
        TypeExpr::Scalar { scalar } => Ok(*scalar == ScalarKind::Null),
        TypeExpr::AnyJson => Ok(true),
        TypeExpr::Array { .. } => Ok(false),
        TypeExpr::Reference { id } => {
            let terminal = alias_resolutions
                .get(id)
                .map(|resolution| &resolution.terminal_id)
                .unwrap_or(id);
            let node = nodes.get(terminal).ok_or_else(|| {
                SchemaToolError::msg(format!(
                    "nullability resolution error: missing terminal node {}",
                    terminal.display()
                ))
            })?;
            Ok(matches!(
                node.shape,
                TypeShape::Nullable { .. }
                    | TypeShape::AnyJson
                    | TypeShape::Scalar {
                        scalar: ScalarKind::Null
                    }
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Schema audit / support surface
// ---------------------------------------------------------------------------

fn audit_schema_tree(schema: &Value, schema_id: &str) -> Result<()> {
    crate::schema_walk::walk_schema_nodes(schema, |pointer, _, node| {
        let location = if pointer.is_root() {
            schema_id.to_string()
        } else {
            format!("{schema_id}#{}", pointer.as_str())
        };
        let Some(object) = node.as_object() else {
            // The support profile historically accepts boolean Schema nodes as terminal controls
            // (for example additionalProperties/unevaluatedProperties). The authoritative walker
            // still visits them so there is no second traversal, but there is no object keyword
            // surface to audit.
            return Ok(());
        };
        let allows_unevaluated_properties = object
            .get("oneOf")
            .and_then(Value::as_array)
            .is_some_and(|variants| nullable_one_of_indices(variants).is_none());
        validate_supported_schema_node(node, &location, allows_unevaluated_properties)
    })
}

fn validate_supported_schema_node(
    schema: &Value,
    location: &str,
    allows_unevaluated_properties: bool,
) -> Result<()> {
    let Some(object) = schema.as_object() else {
        return Err(unsupported(
            "schema",
            location,
            "boolean schemas are not supported by codegen",
        ));
    };
    const KNOWN_KEYWORDS: &[&str] = &[
        "$schema",
        "$id",
        "$ref",
        "$defs",
        "title",
        "description",
        "type",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "enum",
        "const",
        "oneOf",
        "allOf",
        "unevaluatedProperties",
        "if",
        "then",
        "else",
        "format",
        "minimum",
        "maximum",
        "minLength",
        "pattern",
        "minItems",
        "uniqueItems",
    ];
    for keyword in object.keys() {
        if !KNOWN_KEYWORDS.contains(&keyword.as_str()) {
            return Err(unsupported(
                keyword,
                location,
                "unknown schema keyword is not classified by the restricted codegen",
            ));
        }
    }
    const SHAPE_UNSUPPORTED: &[&str] = &[
        "anyOf",
        "not",
        "patternProperties",
        "dependentSchemas",
        "dependentRequired",
        "prefixItems",
        "contains",
        "propertyNames",
        "unevaluatedItems",
        "contentSchema",
    ];
    for keyword in SHAPE_UNSUPPORTED {
        if object.contains_key(*keyword) {
            return Err(unsupported(
                keyword,
                location,
                "shape keyword is not supported by restricted codegen",
            ));
        }
    }
    if let Some(unevaluated) = object.get("unevaluatedProperties") {
        if !allows_unevaluated_properties {
            return Err(unsupported(
                "unevaluatedProperties",
                location,
                "unevaluatedProperties is only supported on a non-null tagged-union classifier",
            ));
        }
        if unevaluated != &Value::Bool(false) {
            return Err(unsupported(
                "unevaluatedProperties",
                location,
                "tagged unions only support unevaluatedProperties: false",
            ));
        }
    }
    if let Some(additional) = object.get("additionalProperties") {
        if !additional.is_boolean() {
            return Err(unsupported(
                "additionalProperties",
                location,
                "schema-valued additionalProperties is not supported",
            ));
        }
    }
    Ok(())
}

fn integer_constraints(schema: &Value, location: &str) -> Result<IntegerConstraints> {
    fn bound(schema: &Value, keyword: &str, location: &str) -> Result<Option<JsonInteger>> {
        let Some(value) = schema.get(keyword) else {
            return Ok(None);
        };
        let Some(number) = value.as_number() else {
            return Err(unsupported(
                keyword,
                location,
                "integer bound must be a JSON number",
            ));
        };
        if let Some(value) = number.as_i64() {
            return Ok(Some(JsonInteger::Signed(value)));
        }
        if let Some(value) = number.as_u64() {
            return Ok(Some(JsonInteger::Unsigned(value)));
        }
        Err(unsupported(
            keyword,
            location,
            "integer bound must itself be an exact JSON integer representable by serde_json",
        ))
    }

    let constraints = IntegerConstraints {
        minimum: bound(schema, "minimum", location)?,
        maximum: bound(schema, "maximum", location)?,
    };
    if let (Some(minimum), Some(maximum)) = (constraints.minimum, constraints.maximum) {
        if minimum.as_i128() > maximum.as_i128() {
            return Err(unsupported(
                "minimum",
                location,
                "integer minimum must not exceed maximum",
            ));
        }
    }
    Ok(constraints)
}

fn ensure_const_matches_type(schema: &Value, value: &Value, location: &str) -> Result<()> {
    let valid = match schema.get("type").and_then(Value::as_str) {
        Some("string") => value.is_string(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("boolean") => value.is_boolean(),
        Some("null") => value.is_null(),
        Some(_) | None => true,
    };
    if valid {
        Ok(())
    } else {
        Err(unsupported(
            "const",
            location,
            "const value does not match declared type",
        ))
    }
}

fn unsupported(keyword: &str, location: &str, detail: &str) -> anyhow::Error {
    SchemaToolError::UnsupportedKeyword {
        keyword: keyword.into(),
        location: location.into(),
        detail: detail.into(),
    }
    .into()
}

// ---------------------------------------------------------------------------
// Schema predicates / helpers
// ---------------------------------------------------------------------------

fn is_string_enum(schema: &Value) -> bool {
    schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|values| !values.is_empty() && values.iter().all(Value::is_string))
}

fn is_nullable_string_enum(schema: &Value) -> bool {
    schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.iter().any(Value::is_null)
                && values
                    .iter()
                    .all(|value| value.is_null() || value.is_string())
        })
}

pub fn string_enum_values(schema: &Value) -> Result<Vec<String>> {
    schema
        .get("enum")
        .and_then(Value::as_array)
        .ok_or_else(|| SchemaToolError::msg("enum schema missing enum array"))?
        .iter()
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::Error::from(SchemaToolError::msg("non-string enum value")))
        })
        .collect()
}

fn is_object_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("object")
        || schema.get("properties").is_some()
}

fn is_null_type(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("null")
}

fn schema_title_of(schema: &Value) -> Option<String> {
    schema
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn ensure_ref_has_only_annotation_siblings(schema: &Value, location: &str) -> Result<()> {
    let object = schema.as_object().ok_or_else(|| {
        unsupported(
            "$ref",
            location,
            "$ref node must be an object in restricted codegen",
        )
    })?;
    const ALLOWED_REF_KEYS: &[&str] = &["$ref", "title", "description"];
    if let Some(keyword) = object
        .keys()
        .find(|keyword| !ALLOWED_REF_KEYS.contains(&keyword.as_str()))
    {
        return Err(unsupported(
            keyword,
            location,
            "$ref siblings with validation or shape semantics are not supported; compose them in an explicit source Schema instead",
        ));
    }
    Ok(())
}

// Nullability is derived from the already-lowered graph by
// `resolved_type_use_allows_null`; do not add a second raw-Schema `$ref` traversal here.

fn required_set(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn contract_type_id_root_and_child() {
        let root = ContractTypeId::root("https://example/schema.json");
        assert!(root.is_root());
        assert_eq!(root.display(), "https://example/schema.json");
        let child = root.child("properties").child("status");
        assert_eq!(
            child.display(),
            "https://example/schema.json#/properties/status"
        );
        assert_eq!(child.pointer.as_str(), "/properties/status");
    }

    #[test]
    fn string_enum_values_drop_null() {
        let schema = json!({"enum": ["a", null, "b"]});
        assert_eq!(
            string_enum_values(&schema).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn neutral_ir_types_have_no_language_fields() {
        let node = ContractTypeNode {
            id: ContractTypeId::root("https://example/a.json"),
            schema_title: Some("A".into()),
            source: SourceSchemaMetadata::from_type_id(&ContractTypeId::root(
                "https://example/a.json",
            )),
            shape: TypeShape::Object {
                fields: vec![ObjectField {
                    json_name: "x".into(),
                    ty: TypeUse {
                        expr: TypeExpr::Scalar {
                            scalar: ScalarKind::String,
                        },
                        source: SourceUseSite::new(
                            "https://example/a.json",
                            JsonPointer::root().child("properties").child("x"),
                        ),
                    },
                    presence: Presence::Required,
                    nullability: Nullability::NonNull,
                    schema_location: ContractTypeId::root("https://example/a.json")
                        .child("properties")
                        .child("x"),
                }],
                unknown_field_policy: UnknownFieldPolicy::Forbid,
            },
        };
        let text = serde_json::to_string(&node).expect("serialize");
        for banned in [
            "rust_name",
            "typescript_name",
            "logical_title",
            "hint",
            "pascal",
            "include_str",
            "I64",
            "F64",
            "Vec",
            "Option",
            "Box",
            "Named",
            "u32",
            "i64",
            "variant_name",
            "raw_name",
            "typed_name",
            "schema_const_name",
        ] {
            assert!(
                !text.contains(banned),
                "neutral IR serialization must not contain {banned}: {text}"
            );
        }
    }

    #[test]
    fn type_expr_is_neutral_scalars_only() {
        let samples = [
            TypeExpr::Scalar {
                scalar: ScalarKind::Integer {
                    constraints: IntegerConstraints::default(),
                },
            },
            TypeExpr::Scalar {
                scalar: ScalarKind::Number,
            },
            TypeExpr::AnyJson,
            TypeExpr::Reference {
                id: ContractTypeId::root("https://x"),
            },
        ];
        for sample in samples {
            let text = serde_json::to_string(&sample).unwrap();
            assert!(!text.contains("i64") && !text.contains("f64") && !text.contains("String"));
        }
    }

    #[test]
    fn alias_resolution_records_chain_and_rejects_pure_cycle() {
        fn alias_node(id: &ContractTypeId, target: &ContractTypeId) -> ContractTypeNode {
            ContractTypeNode {
                id: id.clone(),
                schema_title: None,
                source: SourceSchemaMetadata::from_type_id(id),
                shape: TypeShape::Alias {
                    target: TypeUse {
                        expr: TypeExpr::Reference { id: target.clone() },
                        source: SourceUseSite::new(id.schema_id.clone(), id.pointer.clone()),
                    },
                },
            }
        }

        let a = ContractTypeId::root("https://example/a");
        let b = ContractTypeId::root("https://example/b");
        let c = ContractTypeId::root("https://example/c");
        let mut chain_nodes = BTreeMap::new();
        chain_nodes.insert(a.clone(), alias_node(&a, &b));
        chain_nodes.insert(b.clone(), alias_node(&b, &c));
        chain_nodes.insert(
            c.clone(),
            ContractTypeNode {
                id: c.clone(),
                schema_title: None,
                source: SourceSchemaMetadata::from_type_id(&c),
                shape: TypeShape::Scalar {
                    scalar: ScalarKind::String,
                },
            },
        );
        let resolutions = resolve_and_audit_aliases(&chain_nodes).expect("alias chain");
        assert_eq!(resolutions[&a].terminal_id, c);
        assert_eq!(resolutions[&a].chain, vec![a.clone(), b.clone()]);

        let mut cycle_nodes = BTreeMap::new();
        cycle_nodes.insert(a.clone(), alias_node(&a, &b));
        cycle_nodes.insert(b.clone(), alias_node(&b, &a));
        let error = resolve_and_audit_aliases(&cycle_nodes)
            .expect_err("pure Alias cycle must fail closed")
            .to_string();
        assert!(error.contains("pure Alias cycle"), "{error}");
        assert!(error.contains("https://example/a"), "{error}");
        assert!(error.contains("https://example/b"), "{error}");
    }

    #[test]
    fn alias_nullability_uses_terminal_fact_and_recursive_object_is_not_pure_cycle() {
        let nullable = ContractTypeId::root("https://example/nullable");
        let alias = ContractTypeId::root("https://example/nullable-alias");
        let recursive = ContractTypeId::root("https://example/recursive");
        let recursive_alias = recursive.child("$defs").child("self");
        let mut nodes = BTreeMap::new();
        nodes.insert(
            nullable.clone(),
            ContractTypeNode {
                id: nullable.clone(),
                schema_title: None,
                source: SourceSchemaMetadata::from_type_id(&nullable),
                shape: TypeShape::Nullable {
                    inner: TypeUse {
                        expr: TypeExpr::Scalar {
                            scalar: ScalarKind::String,
                        },
                        source: SourceUseSite::new(
                            nullable.schema_id.clone(),
                            nullable.pointer.clone(),
                        ),
                    },
                },
            },
        );
        nodes.insert(alias.clone(), alias_node_for_test(&alias, &nullable));
        nodes.insert(
            recursive.clone(),
            ContractTypeNode {
                id: recursive.clone(),
                schema_title: None,
                source: SourceSchemaMetadata::from_type_id(&recursive),
                shape: TypeShape::Object {
                    fields: vec![ObjectField {
                        json_name: "next".into(),
                        ty: TypeUse {
                            expr: TypeExpr::Reference {
                                id: recursive_alias.clone(),
                            },
                            source: SourceUseSite::new(
                                recursive.schema_id.clone(),
                                recursive.child("properties").child("next").pointer,
                            ),
                        },
                        presence: Presence::Optional,
                        nullability: Nullability::NonNull,
                        schema_location: recursive.child("properties").child("next"),
                    }],
                    unknown_field_policy: UnknownFieldPolicy::Forbid,
                },
            },
        );
        nodes.insert(
            recursive_alias.clone(),
            alias_node_for_test(&recursive_alias, &recursive),
        );

        let resolutions = resolve_and_audit_aliases(&nodes).expect("aliases with terminals");
        assert!(resolved_type_use_allows_null(
            &nodes,
            &resolutions,
            &TypeUse {
                expr: TypeExpr::Reference { id: alias },
                source: SourceUseSite::new("https://example/use", JsonPointer::root()),
            }
        )
        .expect("nullable alias"));
        assert_eq!(resolutions[&recursive_alias].terminal_id, recursive);
    }

    fn alias_node_for_test(id: &ContractTypeId, target: &ContractTypeId) -> ContractTypeNode {
        ContractTypeNode {
            id: id.clone(),
            schema_title: None,
            source: SourceSchemaMetadata::from_type_id(id),
            shape: TypeShape::Alias {
                target: TypeUse {
                    expr: TypeExpr::Reference { id: target.clone() },
                    source: SourceUseSite::new(id.schema_id.clone(), id.pointer.clone()),
                },
            },
        }
    }

    #[test]
    fn contract_type_id_deserialization_cannot_bypass_pointer_validation() {
        for invalid in ["a", "/a~", "/a~2"] {
            let encoded = serde_json::json!({
                "schema_id": "https://example.test/schema",
                "pointer": invalid
            });
            assert!(serde_json::from_value::<ContractTypeId>(encoded).is_err());
        }

        let type_id = ContractTypeId::new(
            "https://example.test/schema",
            JsonPointer::from_decoded_segments(["properties", "a/b"]),
        );
        let encoded = serde_json::to_value(&type_id).unwrap();
        assert_eq!(encoded["pointer"], "/properties/a~1b");
        assert_eq!(
            serde_json::from_value::<ContractTypeId>(encoded).unwrap(),
            type_id
        );
    }

    #[test]
    fn decoded_property_lookup_escapes_slash_and_tilde_names() {
        let document = json!({
            "properties": {
                "kind/value~tag": {"type": "string", "enum": ["alpha"]}
            }
        });
        let pointer = JsonPointer::from_decoded_segments(["properties", "kind/value~tag"]);
        assert_eq!(pointer.as_str(), "/properties/kind~1value~0tag");
        assert_eq!(
            select_json_value(&document, &pointer)
                .unwrap()
                .get("enum")
                .and_then(Value::as_array)
                .and_then(|values| values.first()),
            Some(&json!("alpha"))
        );
    }

    #[test]
    fn raw_json_pointer_roundtrip_for_internal_shape_helper() {
        use crate::resolve::raw_json_at_pointer;
        let doc = json!({"$defs": {"a": {"type": "string", "enum": ["x"]}}});
        let id = ContractTypeId::root("https://x").child("$defs").child("a");
        let node = raw_json_at_pointer(&doc, &id.pointer).unwrap();
        assert!(is_string_enum(node));
    }
}
