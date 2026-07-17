//! Integration tests for URI/$ref identity, neutral graph, and Rust projection layout.
//!
//! These tests assert through the library API. Generated source strings are only used
//! as secondary oracles (Box/Vec/symbol presence), never as the sole identity proof.

use schema_tool::codegen::{ArtifactKind, ArtifactPlan, GeneratedArtifact};
use schema_tool::contract_model::{TypeExpr, TypeShape};
use schema_tool::json_pointer::{
    parse_array_index_token, pointer_from_decoded_fragment, JsonPointer,
};
use schema_tool::manifest::{GenerationTarget, SchemaRegistry};
use schema_tool::resolve::{
    require_canonical_id_base, require_canonical_schema_id, resolve_ref, schema_at,
    schema_at_document, schema_id_in_id_base_namespace, ResolvedSchemaRef,
};
use schema_tool::target::build_target_plan;
use schema_tool::{lower_and_render_rust, lower_target_contract_graph, project_rust};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir.pop();
    dir.pop();
    dir
}

fn temporary_repo(label: &str) -> PathBuf {
    let root = repo_root();
    let temp = std::env::temp_dir().join(format!("shittim-graph-{}-{}", label, std::process::id()));
    if temp.exists() {
        std::fs::remove_dir_all(&temp).expect("clean old temp");
    }
    copy_tree(&root, &temp);
    // Drop heavy/unrelated trees that are not needed for schema-tool.
    let _ = std::fs::remove_dir_all(temp.join("node_modules"));
    let _ = std::fs::remove_dir_all(temp.join("rust/target"));
    temp
}

fn copy_tree(source: &Path, target: &Path) {
    for entry in walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry.path().strip_prefix(source).expect("strip");
        if rel.components().any(|c| {
            matches!(
                c.as_os_str().to_str(),
                Some("target" | "node_modules" | ".git")
            )
        }) {
            continue;
        }
        let dest = target.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest).expect("mkdir");
        } else if entry.file_type().is_file() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).expect("mkdir parent");
            }
            std::fs::copy(entry.path(), &dest).expect("copy");
        }
    }
}

fn write_json(path: &Path, value: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n").expect("write");
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json")
}

fn add_manifest_entry(temp: &Path, entry: serde_json::Value) {
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"]
        .as_array_mut()
        .expect("schemas")
        .push(entry);
    write_json(&manifest_path, &manifest);
}

// ---------------------------------------------------------------------------
// Production identity / HEAD bytes
// ---------------------------------------------------------------------------

#[test]
fn production_graph_keeps_single_defs_node_and_two_policy_rule_clones() {
    let root = repo_root();
    let (graph, types, _catalog, _typed) = lower_and_render_rust(&root).expect("lower+render");
    let projection = project_rust(&graph).expect("project");

    let policy_id = "https://schemas.shittim.local/v1/policy/policy_rule.json";
    let defs = schema_tool::ContractTypeId::root(policy_id)
        .child("$defs")
        .child("actorEntryPoint");
    assert!(graph.nodes.contains_key(&defs));

    let def_nodes: Vec<_> = graph
        .nodes
        .keys()
        .filter(|id| id.schema_id == policy_id && id.pointer.as_str() == "/$defs/actorEntryPoint")
        .collect();
    assert_eq!(def_nodes.len(), 1);

    let policy = graph
        .nodes
        .get(&schema_tool::ContractTypeId::root(policy_id))
        .expect("PolicyRule root");
    let TypeShape::Object { fields, .. } = &policy.shape else {
        panic!("PolicyRule must be object");
    };
    let created = fields
        .iter()
        .find(|f| f.json_name == "created_by")
        .expect("created_by");
    let updated = fields
        .iter()
        .find(|f| f.json_name == "updated_by")
        .expect("updated_by");
    match (&created.ty.expr, &updated.ty.expr) {
        (TypeExpr::Reference { id: a }, TypeExpr::Reference { id: b }) => {
            assert_eq!(a, &defs);
            assert_eq!(b, &defs);
        }
        other => panic!("expected refs to defs, got {other:?}"),
    }

    // Projection inspection: sibling non-recursive use-sites => two Nominal declarations.
    assert_eq!(projection.nominal_count_for(&defs), 2);
    let created_ty = projection
        .field_type_expr("PolicyRule", "created_by")
        .expect("created_by type");
    let updated_ty = projection
        .field_type_expr("PolicyRule", "updated_by")
        .expect("updated_by type");
    assert_eq!(created_ty, "PolicyRuleCreatedBy");
    assert_eq!(updated_ty, "PolicyRuleUpdatedBy");
    assert_ne!(created_ty, updated_ty);

    assert!(types.contains("pub struct PolicyRuleCreatedBy"));
    assert!(types.contains("pub struct PolicyRuleUpdatedBy"));
    assert!(types.contains("pub created_by: PolicyRuleCreatedBy"));
    assert!(types.contains("pub updated_by: PolicyRuleUpdatedBy"));
}

#[test]
fn production_render_is_byte_identical_to_checked_in_generated() {
    let root = repo_root();
    let (_graph, types, catalog, typed) = lower_and_render_rust(&root).expect("lower+render");
    let expected_types =
        std::fs::read_to_string(root.join("rust/crates/kernel-contracts/src/generated/types.rs"))
            .expect("types.rs");
    let expected_catalog =
        std::fs::read_to_string(root.join("rust/crates/kernel-contracts/src/generated/catalog.rs"))
            .expect("catalog.rs");
    let expected_typed =
        std::fs::read_to_string(root.join("rust/crates/kernel-contracts/src/generated/typed.rs"))
            .expect("typed.rs");
    assert_eq!(
        schema_tool::codegen::ensure_trailing_newline(&types),
        schema_tool::codegen::ensure_trailing_newline(&expected_types)
    );
    assert_eq!(
        schema_tool::codegen::ensure_trailing_newline(&catalog),
        schema_tool::codegen::ensure_trailing_newline(&expected_catalog)
    );
    assert_eq!(
        schema_tool::codegen::ensure_trailing_newline(&typed),
        schema_tool::codegen::ensure_trailing_newline(&expected_typed)
    );
}

#[test]
fn response_envelope_has_no_typed_binding() {
    let root = repo_root();
    let (graph, _types, _catalog, typed) = lower_and_render_rust(&root).expect("lower+render");
    let response_id = "https://schemas.shittim.local/v1/kcp/response_envelope.json";
    assert!(graph
        .envelopes
        .iter()
        .all(|binding| binding.schema_id != response_id));
    assert!(!typed.contains("TypedKcpResponseEnvelope"));
    assert!(graph
        .nodes
        .contains_key(&schema_tool::ContractTypeId::root(response_id)));
}

// ---------------------------------------------------------------------------
// URI / pointer / $ref
// ---------------------------------------------------------------------------

#[test]
fn json_pointer_strict_rules_allow_literal_percent() {
    assert!(JsonPointer::parse("").unwrap().is_root());
    assert_eq!(
        JsonPointer::parse("/a~1b/c~0d").unwrap().as_str(),
        "/a~1b/c~0d"
    );
    assert!(JsonPointer::parse("/a~").is_err());
    assert!(JsonPointer::parse("/a~2").is_err());
    // Literal % is allowed in the pointer itself (URI decode happens earlier).
    let literal = JsonPointer::parse("/a%2Fb").unwrap();
    assert_eq!(literal.as_str(), "/a%2Fb");
    assert_eq!(literal.decoded_segments().unwrap(), vec!["a%2Fb"]);
    assert!(pointer_from_decoded_fragment("myAnchor").is_err());
    assert_eq!(
        pointer_from_decoded_fragment("/$defs/x").unwrap().as_str(),
        "/$defs/x"
    );
    assert!(parse_array_index_token("01").is_err());
    assert!(parse_array_index_token("-").is_err());
    assert_eq!(parse_array_index_token("0").unwrap(), 0);
}

#[test]
fn resolve_ref_local_absolute_relative_share_identity() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let base = "https://schemas.shittim.local/v1/policy/policy_rule.json";

    let local = resolve_ref(&registry, base, "#/$defs/actorEntryPoint").expect("local");
    let absolute = resolve_ref(
        &registry,
        base,
        "https://schemas.shittim.local/v1/policy/policy_rule.json#/$defs/actorEntryPoint",
    )
    .expect("absolute");
    // Relative to the policy directory.
    let relative = resolve_ref(&registry, base, "./policy_rule.json#/$defs/actorEntryPoint")
        .expect("relative");

    assert_eq!(local.type_id, absolute.type_id);
    assert_eq!(local.type_id, relative.type_id);
    assert_eq!(local.type_id.schema_id, base);
    assert_eq!(local.type_id.pointer.as_str(), "/$defs/actorEntryPoint");
    assert_eq!(local.node, absolute.node);
    assert_eq!(local.node, relative.node);
    let via_schema_at = schema_at(&registry, &local.type_id).expect("schema_at");
    assert_eq!(via_schema_at, local.node);
}

#[test]
fn external_fragment_graph_node_is_unique() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let plan = build_target_plan(&registry).expect("plan");
    let set = plan
        .targets
        .iter()
        .find(|s| s.target == GenerationTarget::Rust)
        .expect("rust");
    let graph = lower_target_contract_graph(&registry, set).expect("graph");

    // Actor is referenced from many schemas; root node must exist once.
    let actor =
        schema_tool::ContractTypeId::root("https://schemas.shittim.local/v1/common/actor.json");
    assert!(graph.nodes.contains_key(&actor));
    let actor_nodes: Vec<_> = graph
        .nodes
        .keys()
        .filter(|id| id.schema_id == actor.schema_id)
        .collect();
    // Only root pointer for actor schema (no extra clones of the root identity).
    assert!(actor_nodes.iter().any(|id| id.is_root()));
    assert_eq!(actor_nodes.iter().filter(|id| id.is_root()).count(), 1);
}

#[test]
fn percent_encoded_fragment_utf8_and_literal_percent_and_errors() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let base = "https://schemas.shittim.local/v1/policy/policy_rule.json";

    // Percent-encoded slash in pointer segment: /%24defs -> /$defs after decode?
    // $ is %24; encode "/$defs/actorEntryPoint" as "/%24defs/actorEntryPoint"
    let encoded = resolve_ref(&registry, base, "#/%24defs/actorEntryPoint").expect("percent $");
    assert_eq!(encoded.type_id.pointer.as_str(), "/$defs/actorEntryPoint");

    // UTF-8 percent encoding inside a non-existing path should fail at lookup, not at decode.
    let utf8_err = resolve_ref(&registry, base, "#/%E4%B8%AD");
    assert!(utf8_err.is_err(), "missing node after valid UTF-8 decode");

    // Malformed percent
    let malformed = resolve_ref(&registry, base, "#/a%zz");
    assert!(malformed.is_err());
    let truncated = resolve_ref(&registry, base, "#/a%2");
    assert!(truncated.is_err());

    // Non-UTF8 after decode
    let non_utf8 = resolve_ref(&registry, base, "#/%FF");
    assert!(non_utf8.is_err());

    // Anchor unsupported
    let anchor = resolve_ref(&registry, base, "#myAnchor");
    assert!(anchor.is_err());
    let err = anchor.unwrap_err().to_string();
    assert!(
        err.contains("anchor") || err.contains("non-pointer") || err.contains("not supported"),
        "{err}"
    );
}

#[test]
fn root_id_must_be_canonical_absolute_without_fragment() {
    assert!(require_canonical_schema_id(
        "https://schemas.shittim.local/v1/common/actor.json",
        "ok"
    )
    .is_ok());
    assert!(require_canonical_schema_id("./relative.json", "rel").is_err());
    assert!(require_canonical_schema_id(
        "https://schemas.shittim.local/v1/common/actor.json#x",
        "frag"
    )
    .is_err());

    // Manifest load rejects non-canonical $id.
    let temp = temporary_repo("root-noncanonical-id");
    let _schema_id = "https://schemas.shittim.local/v1/kcp/bad_id.json";
    let source = "schemas/source/kcp/bad_id.v1.json";
    // Write a document whose $id is relative — manifest id will also be set relative.
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "./bad_id.json",
            "title": "BadId",
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": "./bad_id.json",
            "title": "BadId",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
            "generation_targets": ["rust"]
        }),
    );
    let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        err.contains("canonical")
            || err.contains("absolute")
            || err.contains("URI")
            || err.contains("$id"),
        "{err}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn nested_non_root_id_fails_with_real_location() {
    let temp = temporary_repo("nested-id-location");
    let schema_id = "https://schemas.shittim.local/v1/kcp/nested_id_probe.json";
    let source = "schemas/source/kcp/nested_id_probe.v1.json";
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "NestedIdProbe",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "nested"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "nested": {
                    "$id": "https://schemas.shittim.local/v1/kcp/nested_id_probe_nested.json",
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "flag": {"type": "boolean"}
                    }
                }
            }
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": schema_id,
            "title": "NestedIdProbe",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }),
    );
    let registry = SchemaRegistry::load(&temp).expect("registry loads (nested $id checked later)");
    let plan = build_target_plan(&registry).expect("plan");
    let set = plan
        .targets
        .iter()
        .find(|s| s.target == GenerationTarget::Rust)
        .expect("rust");
    let err = lower_target_contract_graph(&registry, set)
        .unwrap_err()
        .to_string();
    assert!(err.contains("nested non-root $id"), "{err}");
    assert!(
        err.contains("/properties/nested") || err.contains("nested_id_probe"),
        "error must locate the nested $id: {err}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn inline_oneof_branch_and_items_use_real_pointers() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let plan = build_target_plan(&registry).expect("plan");
    let set = plan
        .targets
        .iter()
        .find(|s| s.target == GenerationTarget::Rust)
        .expect("rust");
    let graph = lower_target_contract_graph(&registry, set).expect("graph");

    // Response envelope error is oneOf [null, $ref]; the null arm is not a named node,
    // but ActionRequest has $defs/lease referenced from properties/lease.
    let action = "https://schemas.shittim.local/v1/task/action_request.json";
    let lease_def = schema_tool::ContractTypeId::root(action)
        .child("$defs")
        .child("lease");
    assert!(graph.nodes.contains_key(&lease_def));

    // Find a Nullable TypeUse whose source pointer contains /oneOf/ — e.g. response error.
    let response = "https://schemas.shittim.local/v1/kcp/response_envelope.json";
    let response_root = graph
        .nodes
        .get(&schema_tool::ContractTypeId::root(response))
        .expect("response root");
    let TypeShape::Object { fields, .. } = &response_root.shape else {
        panic!("response must be object");
    };
    let error_field = fields
        .iter()
        .find(|f| f.json_name == "error")
        .expect("error field");
    match &error_field.ty.expr {
        TypeExpr::Nullable { inner } => {
            // Inner should be a reference to error schema root, and source of nullable
            // is the property pointer; arm source uses /oneOf/<index>.
            assert!(
                inner.source.pointer.as_str().contains("/oneOf/"),
                "nullable arm source must use real oneOf index pointer, got {}",
                inner.source.pointer.as_str()
            );
            assert!(matches!(inner.expr, TypeExpr::Reference { .. }));
        }
        other => panic!("error field expected Nullable, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Recursive projection + real cargo check
// ---------------------------------------------------------------------------

#[test]
fn scc_exact_option_box_self_mutual_and_array_indirect() {
    let temp = temporary_repo("recursive-layout");
    let self_id = "https://schemas.shittim.local/v1/kcp/self_recursive.json";
    let a_id = "https://schemas.shittim.local/v1/kcp/mutual_a.json";
    let b_id = "https://schemas.shittim.local/v1/kcp/mutual_b.json";
    let c_id = "https://schemas.shittim.local/v1/kcp/scc_c.json";
    let self_source = "schemas/source/kcp/self_recursive.v1.json";
    let a_source = "schemas/source/kcp/mutual_a.v1.json";
    let b_source = "schemas/source/kcp/mutual_b.v1.json";
    let c_source = "schemas/source/kcp/scc_c.v1.json";

    write_json(
        &temp.join(self_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": self_id,
            "title": "SelfRecursive",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "name"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "name": {"type": "string"},
                "next": {"$ref": self_id},
                "children": {
                    "type": "array",
                    "items": {"$ref": self_id}
                }
            }
        }),
    );
    // Three-node direct SCC: A->B->C->A
    write_json(
        &temp.join(a_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": a_id,
            "title": "MutualA",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "other": {"$ref": b_id}
            }
        }),
    );
    write_json(
        &temp.join(b_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": b_id,
            "title": "MutualB",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "other": {"$ref": c_id}
            }
        }),
    );
    write_json(
        &temp.join(c_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": c_id,
            "title": "SccC",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "other": {"$ref": a_id}
            }
        }),
    );

    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    for (id, title, source) in [
        (self_id, "SelfRecursive", self_source),
        (a_id, "MutualA", a_source),
        (b_id, "MutualB", b_source),
        (c_id, "SccC", c_source),
    ] {
        schemas.push(json!({
            "id": id,
            "title": title,
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }));
    }
    write_json(&manifest_path, &manifest);

    let (graph, types, catalog, typed) =
        lower_and_render_rust(&temp).expect("recursive lower+render");
    let projection = project_rust(&graph).expect("project");
    assert!(graph
        .nodes
        .contains_key(&schema_tool::ContractTypeId::root(self_id)));

    // Exact Option<Box<Self>> form — forbid Box<Option and Vec<Box solely for recursion.
    let next_ty = projection
        .field_type_expr("SelfRecursive", "next")
        .expect("next field");
    assert_eq!(
        next_ty, "Option<Box<SelfRecursive>>",
        "direct optional recursive field must be Option<Box<T>>, got {next_ty}"
    );
    assert!(
        !next_ty.contains("Box<Option"),
        "must not emit Box<Option<_>>: {next_ty}"
    );

    let children_ty = projection
        .field_type_expr("SelfRecursive", "children")
        .expect("children field");
    assert_eq!(
        children_ty, "Option<Vec<SelfRecursive>>",
        "array recursion must stay Vec without Box: {children_ty}"
    );
    assert!(
        !children_ty.contains("Box"),
        "array must not box items solely due to recursion: {children_ty}"
    );

    // Three-node direct SCC: each other edge is Option<Box<...>>.
    let a_other = projection.field_type_expr("MutualA", "other").unwrap();
    let b_other = projection.field_type_expr("MutualB", "other").unwrap();
    let c_other = projection.field_type_expr("SccC", "other").unwrap();
    assert_eq!(a_other, "Option<Box<MutualB>>");
    assert_eq!(b_other, "Option<Box<SccC>>");
    assert_eq!(c_other, "Option<Box<MutualA>>");

    // Secondary string oracle still forbids forbidden layouts.
    assert!(!types.contains("Box<Option"));
    assert!(!types.contains("Vec<Box<SelfRecursive>>"));

    // Nested cargo check retained (offline: use only cached registry deps).
    let gen_dir = temp.join("rust/crates/kernel-contracts/src/generated");
    std::fs::create_dir_all(&gen_dir).expect("gen dir");
    std::fs::write(
        gen_dir.join("types.rs"),
        schema_tool::codegen::ensure_trailing_newline(&types),
    )
    .unwrap();
    std::fs::write(
        gen_dir.join("catalog.rs"),
        schema_tool::codegen::ensure_trailing_newline(&catalog),
    )
    .unwrap();
    std::fs::write(
        gen_dir.join("typed.rs"),
        schema_tool::codegen::ensure_trailing_newline(&typed),
    )
    .unwrap();
    std::fs::write(
        gen_dir.join("mod.rs"),
        schema_tool::codegen::ensure_trailing_newline(schema_tool::GENERATED_MOD_RS),
    )
    .unwrap();

    let target_dir = temp.join("cargo-target-recursive");
    let output = Command::new("cargo")
        .args([
            "check",
            "--offline",
            "-p",
            "kernel-contracts",
            "--manifest-path",
        ])
        .arg(temp.join("rust/Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .expect("cargo check recursive");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "recursive kernel-contracts cargo check failed:\n{stdout}\n{stderr}"
    );

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn root_unsupported_shape_fails_closed() {
    let temp = temporary_repo("root-unsupported");
    let schema_id = "https://schemas.shittim.local/v1/kcp/unsupported_root.json";
    let source = "schemas/source/kcp/unsupported_root.v1.json";
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "UnsupportedRoot",
            "anyOf": [
                {"type": "string"},
                {"type": "integer"}
            ]
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": schema_id,
            "title": "UnsupportedRoot",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
            "generation_targets": ["rust"]
        }),
    );
    let registry = SchemaRegistry::load(&temp).expect("load");
    let plan = build_target_plan(&registry).expect("plan");
    let set = plan
        .targets
        .iter()
        .find(|s| s.target == GenerationTarget::Rust)
        .expect("rust");
    let err = lower_target_contract_graph(&registry, set)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("anyOf") || err.contains("unsupported") || err.contains("not supported"),
        "{err}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn artifact_plan_try_new_rejects_evil_absolute_traversal_duplicate_and_nested_ok() {
    let root = "rust/crates/kernel-contracts/src/generated";
    let mk = |path: &str| {
        GeneratedArtifact::new(
            GenerationTarget::Rust,
            ArtifactKind::Types,
            path,
            "x\n",
            vec![],
        )
    };

    // generated_evil string-prefix spoof
    let err = ArtifactPlan::try_new(
        vec![mk(
            "rust/crates/kernel-contracts/src/generated_evil/types.rs",
        )],
        [root.into()],
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("not under") || err.contains("unsafe") || err.contains("generated_evil"),
        "{err}"
    );

    // absolute path
    assert!(ArtifactPlan::try_new(vec![mk("/absolute/types.rs")], [root.into()]).is_err());

    // traversal
    assert!(ArtifactPlan::try_new(
        vec![mk(
            "rust/crates/kernel-contracts/src/generated/../secret.rs"
        )],
        [root.into()]
    )
    .is_err());

    // duplicate relative_path
    assert!(ArtifactPlan::try_new(
        vec![
            mk("rust/crates/kernel-contracts/src/generated/types.rs"),
            mk("rust/crates/kernel-contracts/src/generated/types.rs"),
        ],
        [root.into()]
    )
    .is_err());

    // nested under root is accepted; planned prefixes include nested parent
    let plan = ArtifactPlan::try_new(
        vec![mk(
            "rust/crates/kernel-contracts/src/generated/nested/types.rs",
        )],
        [root.into()],
    )
    .expect("nested ok");
    assert!(plan
        .planned_prefixes()
        .iter()
        .any(|p| p.ends_with("/generated/nested")));
    assert_eq!(plan.roots(), &[root.to_string()]);
    assert_eq!(
        plan.artifacts()[0].relative_path(),
        "rust/crates/kernel-contracts/src/generated/nested/types.rs"
    );

    // Filesystem check: minimal plan against production tree must report mismatch
    // (extra planned files missing from this synthetic plan).
    let production_plan = ArtifactPlan::try_new(
        vec![mk("rust/crates/kernel-contracts/src/generated/types.rs")],
        [root.into()],
    )
    .expect("production path ok");
    let err = schema_tool::codegen::check_artifact_file_set(&repo_root(), &production_plan)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("mismatch") || err.contains("extra") || err.contains("missing"),
        "{err}"
    );
}

#[test]
fn root_shared_refs_are_not_cloned_per_use_site() {
    let root = repo_root();
    let (_graph, types, _, _) = lower_and_render_rust(&root).expect("lower+render");
    let actor_decls = types.matches("pub struct Actor {").count();
    assert_eq!(actor_decls, 1, "Actor root must be shared");
}

#[test]
fn graph_source_schema_ids_match_rust_closure() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let plan = build_target_plan(&registry).expect("plan");
    let set = plan
        .targets
        .iter()
        .find(|s| s.target == GenerationTarget::Rust)
        .expect("rust");
    let graph = lower_target_contract_graph(&registry, set).expect("graph");
    let expected: BTreeSet<_> = set.closure.iter().cloned().collect();
    let actual: BTreeSet<_> = graph.source_schema_ids.iter().cloned().collect();
    assert_eq!(expected, actual);
}

#[test]
fn envelopes_reuse_projected_root_fields() {
    let root = repo_root();
    let (graph, _types, _catalog, typed) = lower_and_render_rust(&root).expect("lower+render");
    assert!(!graph.envelopes.is_empty());
    for binding in &graph.envelopes {
        let node = graph
            .nodes
            .get(&binding.envelope_type)
            .unwrap_or_else(|| panic!("missing envelope root {}", binding.schema_id));
        assert!(matches!(node.shape, TypeShape::Object { .. }));
    }
    assert!(typed.contains("KcpCommandEnvelopeProtocolVersion"));
    // Response-only remains untyped by the unique envelope analysis.
    assert!(!typed.contains("TypedKcpResponseEnvelope"));
}

#[test]
fn resolved_schema_ref_is_the_single_resolution_type() {
    // Type-level smoke: resolve_ref returns ResolvedSchemaRef and root pointer is valid.
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let resolved: ResolvedSchemaRef<'_> = resolve_ref(
        &registry,
        "https://schemas.shittim.local/v1/common/actor.json",
        "https://schemas.shittim.local/v1/common/actor.json",
    )
    .expect("resolve");
    assert!(resolved.type_id.is_root());
    let node = schema_at_document(resolved.node, &JsonPointer::root())
        .expect("root pointer on resolved node");
    assert!(node.get("$id").is_some() || node.get("type").is_some());
}

// ---------------------------------------------------------------------------
// id_base namespace authority
// ---------------------------------------------------------------------------

#[test]
fn id_base_must_be_canonical_with_trailing_slash() {
    assert!(require_canonical_id_base("https://schemas.shittim.local/v1/").is_ok());
    assert!(require_canonical_id_base("https://schemas.shittim.local/v1").is_err());
    assert!(require_canonical_id_base("https://schemas.shittim.local/v1/#x").is_err());
    assert!(require_canonical_id_base("./v1/").is_err());

    let temp = temporary_repo("id-base-no-slash");
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    // Drop trailing slash — still absolute but non-conforming id_base.
    manifest["id_base"] = json!("https://schemas.shittim.local/v1");
    write_json(&manifest_path, &manifest);
    let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        err.contains("id_base") && (err.contains("/") || err.contains("canonical")),
        "{err}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn id_base_rejects_entry_outside_and_prefix_spoof() {
    let base = require_canonical_id_base("https://schemas.shittim.local/v1/").unwrap();
    assert!(schema_id_in_id_base_namespace(
        &base,
        "https://schemas.shittim.local/v1/common/actor.json"
    )
    .is_ok());
    assert!(schema_id_in_id_base_namespace(
        &base,
        "https://schemas.shittim.local/v1_evil/common/actor.json"
    )
    .is_err());
    assert!(
        schema_id_in_id_base_namespace(&base, "https://other.example/v1/common/actor.json")
            .is_err()
    );

    // Manifest load: entry outside id_base path namespace fails.
    let temp = temporary_repo("id-base-outside");
    let schema_id = "https://schemas.shittim.local/v2/outside.json";
    let source = "schemas/source/kcp/outside.v1.json";
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "Outside",
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": schema_id,
            "title": "Outside",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
            "generation_targets": ["rust"]
        }),
    );
    let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        err.contains("id_base") || err.contains("namespace"),
        "{err}"
    );
    std::fs::remove_dir_all(temp).ok();

    // Prefix spoof path under load.
    let temp = temporary_repo("id-base-spoof");
    let schema_id = "https://schemas.shittim.local/v1_evil/spoof.json";
    let source = "schemas/source/kcp/spoof.v1.json";
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "Spoof",
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": schema_id,
            "title": "Spoof",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
            "generation_targets": ["rust"]
        }),
    );
    let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        err.contains("id_base") || err.contains("namespace"),
        "{err}"
    );
    std::fs::remove_dir_all(temp).ok();
}

// ---------------------------------------------------------------------------
// Projection sibling / diamond + single projection instance
// ---------------------------------------------------------------------------

#[test]
fn sibling_and_diamond_nominal_projection_and_shared_root() {
    let temp = temporary_repo("sibling-diamond");
    let shared_id = "https://schemas.shittim.local/v1/kcp/shared_leaf.json";
    let parent_id = "https://schemas.shittim.local/v1/kcp/diamond_parent.json";
    let shared_source = "schemas/source/kcp/shared_leaf.v1.json";
    let parent_source = "schemas/source/kcp/diamond_parent.v1.json";

    write_json(
        &temp.join(shared_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": shared_id,
            "title": "SharedLeaf",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "label"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "label": {"type": "string"}
            }
        }),
    );
    // Sibling use of one $defs shape (non-recursive) + diamond of whole-schema root refs.
    write_json(
        &temp.join(parent_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": parent_id,
            "title": "DiamondParent",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "left", "right", "shared_a", "shared_b"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "left": {"$ref": "#/$defs/point"},
                "right": {"$ref": "#/$defs/point"},
                "shared_a": {"$ref": shared_id},
                "shared_b": {"$ref": shared_id}
            },
            "$defs": {
                "point": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["x"],
                    "properties": {
                        "x": {"type": "integer"}
                    }
                }
            }
        }),
    );

    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    for (id, title, source) in [
        (shared_id, "SharedLeaf", shared_source),
        (parent_id, "DiamondParent", parent_source),
    ] {
        schemas.push(json!({
            "id": id,
            "title": title,
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }));
    }
    write_json(&manifest_path, &manifest);

    let (graph, types, _catalog, _typed) = lower_and_render_rust(&temp).expect("lower+render");
    let projection = project_rust(&graph).expect("project");

    let point = schema_tool::ContractTypeId::root(parent_id)
        .child("$defs")
        .child("point");
    assert_eq!(
        projection.nominal_count_for(&point),
        2,
        "sibling non-recursive use-sites must project two Nominal declarations"
    );
    let left = projection.field_type_expr("DiamondParent", "left").unwrap();
    let right = projection
        .field_type_expr("DiamondParent", "right")
        .unwrap();
    assert_eq!(left, "DiamondParentLeft");
    assert_eq!(right, "DiamondParentRight");
    assert_ne!(left, right);

    // Diamond whole-schema roots stay SharedRoot (one declaration).
    assert_eq!(types.matches("pub struct SharedLeaf {").count(), 1);
    let shared_a = projection
        .field_type_expr("DiamondParent", "shared_a")
        .unwrap();
    let shared_b = projection
        .field_type_expr("DiamondParent", "shared_b")
        .unwrap();
    assert_eq!(shared_a, "SharedLeaf");
    assert_eq!(shared_b, "SharedLeaf");
    assert_eq!(projection.root_name(shared_id), Some("SharedLeaf"));

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn recursive_backedge_reuses_active_declaration_not_extra_nominal() {
    let temp = temporary_repo("backedge-reuse");
    let self_id = "https://schemas.shittim.local/v1/kcp/backedge_self.json";
    let source = "schemas/source/kcp/backedge_self.v1.json";
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": self_id,
            "title": "BackedgeSelf",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "child": {"$ref": "#/$defs/node"}
            },
            "$defs": {
                "node": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["label"],
                    "properties": {
                        "label": {"type": "string"},
                        "next": {"$ref": "#/$defs/node"}
                    }
                }
            }
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": self_id,
            "title": "BackedgeSelf",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }),
    );

    let (graph, _types, _catalog, _typed) = lower_and_render_rust(&temp).expect("lower+render");
    let projection = project_rust(&graph).expect("project");
    let node = schema_tool::ContractTypeId::root(self_id)
        .child("$defs")
        .child("node");
    // Active backedge reuses the single Nominal for the recursive $defs node.
    assert_eq!(
        projection.nominal_count_for(&node),
        1,
        "recursive backedge must reuse active declaration"
    );
    let next = projection
        .field_type_expr("BackedgeSelfChild", "next")
        .expect("next on projected node");
    assert_eq!(next, "Option<Box<BackedgeSelfChild>>");

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn types_and_typed_share_single_projection_instance_api() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let plan = build_target_plan(&registry).expect("plan");
    let set = plan
        .targets
        .iter()
        .find(|s| s.target == GenerationTarget::Rust)
        .expect("rust");
    let graph = lower_target_contract_graph(&registry, set).expect("graph");
    let projection = project_rust(&graph).expect("once");
    let types = schema_tool::render_types_module_from_projection(&projection).expect("types");
    let typed =
        schema_tool::render_typed_module_from_projection(&projection, &graph).expect("typed");
    // Catalog may render directly from graph without projection.
    let catalog = schema_tool::render_catalog_module(&graph).expect("catalog");
    assert!(types.contains("pub struct Actor"));
    assert!(typed.contains("TypedKcpCommandEnvelope") || typed.contains("decode"));
    assert!(catalog.contains("EMBEDDED_SCHEMA_DOCUMENTS"));
    // Root lookup is available from the same projection.
    assert_eq!(
        projection.root_name("https://schemas.shittim.local/v1/common/actor.json"),
        Some("Actor")
    );
}

#[test]
fn target_plan_and_rust_graph_exclude_typescript_only_orphan() {
    // Synthetic mixed-target registry: production rust schemas stay rust-only; a brand-new
    // orphan is typescript-only and is not $ref'd from any rust root. TargetPlan must still
    // surface both targets, but the Rust set/graph must not pull the orphan in. We never
    // call the unimplemented TypeScript renderer — only plan + lower the Rust graph.
    let temp = temporary_repo("ts-only-orphan-plan");
    let orphan_id = "https://schemas.shittim.local/v1/kcp/ts_only_orphan.json";
    let orphan_source = "schemas/source/kcp/ts_only_orphan.v1.json";
    write_json(
        &temp.join(orphan_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": orphan_id,
            "title": "TsOnlyOrphan",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "note"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "note": {"type": "string"}
            }
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": orphan_id,
            "title": "TsOnlyOrphan",
            "version": 1,
            "source": orphan_source,
            "domain": "kcp",
            "kind": "kcp_request",
            "compatibility": "test-only",
            "generation_targets": ["typescript"],
            "schema_version_field": "schema_version"
        }),
    );

    let registry = SchemaRegistry::load(&temp).expect("load mixed-target registry");
    let plan = build_target_plan(&registry).expect("TargetPlan builds without rendering TS");

    let rust_set = plan
        .targets
        .iter()
        .find(|set| set.target == GenerationTarget::Rust)
        .expect("rust TargetSchemaSet");
    let ts_set = plan
        .targets
        .iter()
        .find(|set| set.target == GenerationTarget::Typescript)
        .expect("typescript TargetSchemaSet");

    assert!(
        !rust_set.roots.contains(orphan_id),
        "TS-only orphan must not be a rust root"
    );
    assert!(
        !rust_set.closure.contains(orphan_id),
        "TS-only orphan must not enter rust closure"
    );
    assert!(
        ts_set.roots.contains(orphan_id),
        "TS-only orphan must be a typescript root"
    );
    assert!(
        ts_set.closure.contains(orphan_id),
        "TS-only orphan must be in typescript closure"
    );

    let graph = lower_target_contract_graph(&registry, rust_set).expect("rust graph");
    assert!(
        !graph.source_schema_ids.iter().any(|id| id == orphan_id),
        "rust TargetContractGraph must exclude TS-only orphan"
    );
    assert!(
        !graph
            .nodes
            .contains_key(&schema_tool::ContractTypeId::root(orphan_id)),
        "rust graph nodes must exclude TS-only orphan root"
    );

    // Catalog rendered from the rust graph must also exclude the orphan identity.
    let catalog = schema_tool::render_catalog_module(&graph).expect("catalog");
    assert!(
        !catalog.contains("ts_only_orphan") && !catalog.contains(orphan_id),
        "rust catalog must exclude TS-only orphan"
    );

    // Declaring typescript still fails closed at ArtifactPlan before any write.
    let plan_err = schema_tool::codegen::plan_artifacts(&registry)
        .unwrap_err()
        .to_string();
    assert!(
        plan_err.contains("typescript") && plan_err.contains("not implemented"),
        "plan_artifacts must fail on declared TS without rendering: {plan_err}"
    );

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn mixed_envelope_payload_ref_and_missing_branch_fails_bijective_mapping() {
    let temp = temporary_repo("mixed-envelope-branch");
    let envelope_path = temp.join("schemas/source/kcp/command_envelope.v1.json");
    let mut envelope = read_json(&envelope_path);
    // Keep one real whole-schema payload $ref branch; replace another with inline object
    // (no $ref) so analysis sees >=1 whole-schema refs but incomplete bijective mapping.
    let all_of = envelope["allOf"].as_array_mut().expect("allOf");
    assert!(
        all_of.len() >= 2,
        "command envelope needs multiple branches"
    );
    all_of[1]["then"]["properties"]["payload"] = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "schema_version": {"type": "integer", "const": 1}
        }
    });
    write_json(&envelope_path, &envelope);

    let registry = SchemaRegistry::load(&temp).expect("load");
    let plan = build_target_plan(&registry).expect("plan");
    let set = plan
        .targets
        .iter()
        .find(|s| s.target == GenerationTarget::Rust)
        .expect("rust");
    let err = lower_target_contract_graph(&registry, set)
        .unwrap_err()
        .to_string();
    let lower = err.to_ascii_lowercase();
    assert!(
        lower.contains("mapping") || lower.contains("bijective") || lower.contains("payload"),
        "mixed envelope must fail mapping/bijective path: {err}"
    );

    // generate path must also fail closed with the same class of error.
    let graph_err = lower_and_render_rust(&temp).unwrap_err().to_string();
    let lower = graph_err.to_ascii_lowercase();
    assert!(
        lower.contains("mapping") || lower.contains("bijective") || lower.contains("payload"),
        "{graph_err}"
    );

    std::fs::remove_dir_all(temp).ok();
}
