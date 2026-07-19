//! Library surface for `schema-tool`.
//!
//! Validation, JSON Pointer selection/mutation, canonicalization, and generation
//! are library-first APIs. The CLI is a thin adapter over these reusable entry
//! points; integration tests may also inspect target graph and Rust projection
//! facts without scraping generated source strings as their only oracle.

pub mod artifact_transaction;
pub mod canonicalize;
pub mod check;
pub mod codegen;
pub mod compatibility;
pub mod contract_model;
pub mod error;
pub mod generate;
pub mod json_pointer;
pub mod manifest;
pub mod manifest_identity;
pub mod method_bindings;
pub mod names;
/// Non-stable test-artifact contract shared by official fixture harnesses.
pub mod official_fixture;
pub mod paths;
pub mod production_stage;
pub mod resolve;
pub mod rust_codegen;
pub mod schema_walk;
pub mod target;
pub mod validate;

pub use canonicalize::{
    canonicalize_selected_json, canonicalize_value, CanonicalOutputMode, CanonicalizeRequest,
    CanonicalizeResult,
};
pub use compatibility::SchemaCompatibility;
pub use contract_model::{
    lower_target_contract_graph, AliasResolution, CatalogFacts, ConstJson, ContractTypeId,
    ContractTypeNode, EnvelopeWireBinding, IntegerConstraints, JsonInteger, Nullability,
    ObjectField, Presence, ScalarKind, SourceSchemaMetadata, SourceUseSite, TaggedUnionBranch,
    TargetContractGraph, TypeExpr, TypeShape, TypeUse, UnknownFieldPolicy,
};
pub use json_pointer::{
    apply_json_mutation, parse_array_index_token, pointer_from_decoded_fragment, select_json_value,
    select_json_value_at_pointer, JsonMutationOperation, JsonPointer,
};
pub use manifest::{
    GenerationTarget, Manifest, ManifestComponent, ManifestEntry, ManifestMethodVersionBinding,
    MethodFamily, SchemaRegistry, SchemaSourcePath,
};
pub use method_bindings::{
    discover_active_envelope_authority, validate_method_version_bindings, ActiveEnvelopeAuthority,
    MethodVersionBindingFact,
};
pub use production_stage::{
    validate_production_manifest_stage, ProductionRegistry, ProductionSchemaStage, RegistryProfile,
    SyntheticNonProduction, SyntheticRegistry, ValidatedRegistry,
};
pub use resolve::{
    require_canonical_id_base, require_canonical_schema_id, resolve_ref, schema_at,
    schema_id_in_id_base_namespace, schema_id_in_namespace, validate_component_namespace,
    ResolvedSchemaRef,
};
pub use rust_codegen::{
    project_rust, render_catalog_module, render_typed_module_from_projection,
    render_types_module_from_projection, RustProjection, GENERATED_MOD_RS, RUST_GENERATED_DIR,
};
pub use target::{build_target_plan, TargetPlan, TargetSchemaSet};
pub use validate::{
    render_success as render_validation_success, validate_selected_request,
    ValidateSelectedRequest, ValidateSelectedResult,
};

use anyhow::Result;
use std::path::Path;

/// Load the registry under an explicit profile, build the rust target graph, and render.
pub fn lower_and_render_rust<P: RegistryProfile>(
    repo_root: &Path,
) -> Result<(TargetContractGraph, String, String, String)> {
    let registry = SchemaRegistry::load(repo_root)?;
    lower_and_render_rust_from_registry(ValidatedRegistry::<P>::new(&registry)?)
}

/// Lower and render from an explicitly profiled registry.
pub fn lower_and_render_rust_from_registry<P: RegistryProfile>(
    validated: ValidatedRegistry<'_, P>,
) -> Result<(TargetContractGraph, String, String, String)> {
    let plan = target::build_target_plan(validated)?;
    let graph = lower_target_contract_graph(&plan, GenerationTarget::Rust)?;
    let projection = project_rust(&graph)?;
    let types = render_types_module_from_projection(&projection)?;
    let catalog = render_catalog_module(&graph)?;
    let typed = render_typed_module_from_projection(&projection, &graph)?;
    Ok((graph, types, catalog, typed))
}
