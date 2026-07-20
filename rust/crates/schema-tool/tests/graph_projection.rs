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
    schema_id_in_id_base_namespace, validate_component_namespace, ResolvedSchemaRef,
};
use schema_tool::target::build_target_plan;
use schema_tool::{
    lower_and_render_rust, lower_target_contract_graph, project_rust, AliasResolution,
};
use serde_json::json;
use sha2::{Digest, Sha256};
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

fn schema_tool_bin() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_schema-tool") {
        return PathBuf::from(path);
    }
    repo_root().join("rust/target/debug/schema-tool")
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
    refresh_retained_source_hash(path);
}

fn refresh_retained_source_hash(source_path: &Path) {
    let Some(root) = source_path.ancestors().find(|ancestor| {
        ancestor.join("schemas/manifest.json").is_file()
            && ancestor
                .join("schemas/fixtures/manifest/retained_ids.v1.json")
                .is_file()
    }) else {
        return;
    };
    update_retained_source_hash(root, source_path);
}

fn update_retained_source_hash(root: &Path, source_path: &Path) {
    let Ok(relative) = source_path.strip_prefix(root) else {
        return;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    if !relative.starts_with("schemas/source/") {
        return;
    }
    let baseline_path = root.join("schemas/fixtures/manifest/retained_ids.v1.json");
    let mut baseline = read_json(&baseline_path);
    let Some(entry) = baseline["entries"]
        .as_array_mut()
        .expect("baseline entries")
        .iter_mut()
        .find(|entry| entry["source"].as_str() == Some(relative.as_str()))
    else {
        return;
    };
    let bytes = std::fs::read(source_path).expect("read changed retained source");
    entry["source_sha256"] = json!(hex::encode(Sha256::digest(bytes)));
    std::fs::write(
        baseline_path,
        serde_json::to_string_pretty(&baseline).unwrap() + "\n",
    )
    .expect("update retained baseline hash for coherent test fixture mutation");
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
fn alias_resolution_is_usable_from_schema_tool_root_api() {
    let alias_id = schema_tool::ContractTypeId::root("https://example/alias");
    let terminal_id = schema_tool::ContractTypeId::root("https://example/terminal");
    let fact = AliasResolution {
        alias_id: alias_id.clone(),
        terminal_id: terminal_id.clone(),
        chain: vec![alias_id],
    };
    assert_eq!(fact.terminal_id, terminal_id);
}

#[test]
fn production_graph_keeps_single_defs_node_and_two_policy_rule_clones() {
    let root = repo_root();
    let (graph, types, _catalog, _typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&root).expect("lower+render");
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
    let (_graph, types, catalog, typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&root).expect("lower+render");
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
    let (graph, _types, _catalog, typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&root).expect("lower+render");
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
fn refs_must_target_authoritative_schema_nodes_not_instance_values() {
    let temp = temporary_repo("ref-schema-node-index");
    let actor_path = temp.join("schemas/source/common/actor.v1.json");
    let actor_id = "https://schemas.shittim.local/v1/common/actor.json";
    let original = read_json(&actor_path);

    for (label, pointer, instance) in [
        ("const", "/const", serde_json::json!({"type": "string"})),
        ("default", "/default", serde_json::json!({"type": "string"})),
        (
            "examples",
            "/examples/0",
            serde_json::json!({"type": "string"}),
        ),
        ("enum", "/enum/0", serde_json::json!({"type": "string"})),
    ] {
        let mut actor = original.clone();
        actor[label] = if label == "examples" || label == "enum" {
            serde_json::json!([instance])
        } else {
            instance
        };
        actor["properties"]["display_name"] = serde_json::json!({"$ref": format!("#{pointer}")});
        write_json(&actor_path, &actor);
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(
            err.contains("not an authoritative SchemaNode") && err.contains(pointer),
            "{label}: {err}"
        );
    }

    let mut actor = original;
    actor["$defs"] = serde_json::json!({"named": {"type": "string"}});
    actor["properties"]["display_name"] = serde_json::json!({"$ref": "#/$defs/named"});
    actor["properties"]["schema_array"] = serde_json::json!({
        "type": "array",
        "items": {"type": "string"}
    });
    write_json(&actor_path, &actor);
    let registry = SchemaRegistry::load(&temp).expect("legal Schema-bearing pointers load");
    for reference in [
        "#/$defs/named",
        "#/properties/display_name",
        "#/properties/schema_array/items",
    ] {
        resolve_ref(&registry, actor_id, reference).expect(reference);
    }
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn external_fragment_graph_node_is_unique() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let plan =
        build_target_plan(schema_tool::ProductionRegistry::new(&registry).unwrap()).expect("plan");
    let graph = lower_target_contract_graph(&plan, GenerationTarget::Rust).expect("graph");

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
    let _schema_id = "https://schemas.shittim.local/kcp/bad_id/v1";
    let source = "schemas/source/kcp/bad_id.v1.json";
    // Write a document whose $id is relative — manifest id will also be set relative.
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "./bad_id.json",
            "title": "BadIdV1",
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": "./bad_id.json",
            "title": "BadIdV1",
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
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
    let schema_id = "https://schemas.shittim.local/kcp/nested_id_probe/v1";
    let source = "schemas/source/kcp/nested_id_probe.v1.json";
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "NestedIdProbeV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "nested"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "nested": {
                    "$id": "https://schemas.shittim.local/kcp/nested_id_probe_nested/v1",
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
            "title": "NestedIdProbeV1",
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }),
    );
    let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(err.contains("nested non-root $id"), "{err}");
    assert!(
        err.contains("/properties/nested") || err.contains("nested_id_probe"),
        "error must locate the nested $id: {err}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn registry_load_rejects_retained_source_hash_drift() {
    let temp = temporary_repo("retained-source-hash-drift");
    let actor_path = temp.join("schemas/source/common/actor.v1.json");
    let mut actor = read_json(&actor_path);
    actor["description"] = json!("valid schema bytes changed without ledger update");
    std::fs::write(
        &actor_path,
        serde_json::to_string_pretty(&actor).unwrap() + "\n",
    )
    .expect("tamper retained source without updating ledger");

    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(error.contains("source SHA-256 mismatch"), "{error}");
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn registry_rejects_invalid_manifest_source_paths() {
    for (label, source) in [
        ("absolute", "/tmp/actor.json"),
        ("traversal", "schemas/source/common/../actor.v1.json"),
        ("outside-source", "schemas/examples/actor.valid.json"),
        ("backslash", "schemas\\source\\common\\actor.v1.json"),
        ("empty-segment", "schemas/source//common/actor.v1.json"),
        ("dot-segment", "schemas/source/./common/actor.v1.json"),
        ("prefix-trick", "schemas/source-evil/common/actor.v1.json"),
    ] {
        let temp = temporary_repo(&format!("source-path-{label}"));
        let manifest_path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&manifest_path);
        manifest["schemas"][0]["source"] = json!(source);
        write_json(&manifest_path, &manifest);
        let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(error.contains("source path"), "{label}: {error}");
        std::fs::remove_dir_all(temp).ok();
    }
}

#[cfg(unix)]
#[test]
fn registry_rejects_schema_source_file_and_ancestor_symlinks() {
    use std::os::unix::fs::symlink;

    let file_temp = temporary_repo("source-file-symlink");
    let actor_path = file_temp.join("schemas/source/common/actor.v1.json");
    let real_actor = file_temp.join("schemas/source/common/actor.real.json");
    std::fs::rename(&actor_path, &real_actor).expect("move actor target");
    symlink(&real_actor, &actor_path).expect("source file symlink");
    let error = SchemaRegistry::load(&file_temp).unwrap_err().to_string();
    assert!(error.contains("symlink"), "{error}");
    std::fs::remove_dir_all(file_temp).ok();

    let ancestor_temp = temporary_repo("source-ancestor-symlink");
    let common_dir = ancestor_temp.join("schemas/source/common");
    let real_common_dir = ancestor_temp.join("schemas/source/common-real");
    std::fs::rename(&common_dir, &real_common_dir).expect("move common directory");
    symlink(&real_common_dir, &common_dir).expect("source ancestor symlink");
    let error = SchemaRegistry::load(&ancestor_temp)
        .unwrap_err()
        .to_string();
    assert!(error.contains("symlink"), "{error}");
    std::fs::remove_dir_all(ancestor_temp).ok();
}

#[test]
fn registry_and_cli_fail_closed_on_dynamic_ref() {
    let temp = temporary_repo("dynamic-ref-fail-closed");
    let actor_path = temp.join("schemas/source/common/actor.v1.json");
    let mut actor = read_json(&actor_path);
    actor["properties"]["id"] = json!({
        "$dynamicRef": "https://schemas.shittim.local/v1/task/task_spec.json"
    });
    std::fs::write(
        &actor_path,
        serde_json::to_string_pretty(&actor).unwrap() + "\n",
    )
    .expect("write dynamicRef probe");
    update_retained_source_hash(&temp, &actor_path);

    let load_error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(load_error.contains("$dynamicRef"), "{load_error}");

    let instance_path = temp.join("actor-instance.json");
    write_json(&instance_path, &json!({}));
    for command in ["check", "generate"] {
        let output = Command::new(schema_tool_bin())
            .arg("--repo-root")
            .arg(&temp)
            .arg(command)
            .output()
            .expect("run schema-tool");
        assert!(!output.status.success(), "{command} must fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("$dynamicRef"), "{command}: {stderr}");
    }
    let output = Command::new(schema_tool_bin())
        .arg("--repo-root")
        .arg(&temp)
        .args([
            "validate",
            "--schema",
            "https://schemas.shittim.local/v1/common/actor.json",
            "--instance",
        ])
        .arg(&instance_path)
        .output()
        .expect("run validate");
    assert!(!output.status.success(), "validate must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("$dynamicRef"), "{stderr}");
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn inline_oneof_branch_and_items_use_real_pointers() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let plan =
        build_target_plan(schema_tool::ProductionRegistry::new(&registry).unwrap()).expect("plan");
    let graph = lower_target_contract_graph(&plan, GenerationTarget::Rust).expect("graph");

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
    let self_id = "https://schemas.shittim.local/kcp/self_recursive/v1";
    let a_id = "https://schemas.shittim.local/kcp/mutual_a/v1";
    let b_id = "https://schemas.shittim.local/kcp/mutual_b/v1";
    let c_id = "https://schemas.shittim.local/kcp/scc_c/v1";
    let self_source = "schemas/source/kcp/self_recursive.v1.json";
    let a_source = "schemas/source/kcp/mutual_a.v1.json";
    let b_source = "schemas/source/kcp/mutual_b.v1.json";
    let c_source = "schemas/source/kcp/scc_c.v1.json";

    write_json(
        &temp.join(self_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": self_id,
            "title": "SelfRecursiveV1",
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
            "title": "MutualAV1",
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
            "title": "MutualBV1",
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
            "title": "SccCV1",
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
        (self_id, "SelfRecursiveV1", self_source),
        (a_id, "MutualAV1", a_source),
        (b_id, "MutualBV1", b_source),
        (c_id, "SccCV1", c_source),
    ] {
        schemas.push(json!({
            "id": id,
            "title": title,
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }));
    }
    write_json(&manifest_path, &manifest);

    let (graph, types, catalog, typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&temp)
            .expect("recursive lower+render");
    let projection = project_rust(&graph).expect("project");
    assert!(graph
        .nodes
        .contains_key(&schema_tool::ContractTypeId::root(self_id)));

    // Exact Option<Box<Self>> form — forbid Box<Option and Vec<Box solely for recursion.
    let next_ty = projection
        .field_type_expr("SelfRecursiveV1", "next")
        .expect("next field");
    assert_eq!(
        next_ty, "Option<Box<SelfRecursiveV1>>",
        "direct optional recursive field must be Option<Box<T>>, got {next_ty}"
    );
    assert!(
        !next_ty.contains("Box<Option"),
        "must not emit Box<Option<_>>: {next_ty}"
    );

    let children_ty = projection
        .field_type_expr("SelfRecursiveV1", "children")
        .expect("children field");
    assert_eq!(
        children_ty, "Option<Vec<SelfRecursiveV1>>",
        "array recursion must stay Vec without Box: {children_ty}"
    );
    assert!(
        !children_ty.contains("Box"),
        "array must not box items solely due to recursion: {children_ty}"
    );

    // Three-node direct SCC: each other edge is Option<Box<...>>.
    let a_other = projection.field_type_expr("MutualAV1", "other").unwrap();
    let b_other = projection.field_type_expr("MutualBV1", "other").unwrap();
    let c_other = projection.field_type_expr("SccCV1", "other").unwrap();
    assert_eq!(a_other, "Option<Box<MutualBV1>>");
    assert_eq!(b_other, "Option<Box<SccCV1>>");
    assert_eq!(c_other, "Option<Box<MutualAV1>>");

    // Secondary string oracle still forbids forbidden layouts.
    assert!(!types.contains("Box<Option"));
    assert!(!types.contains("Vec<Box<SelfRecursiveV1>>"));

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
    let schema_id = "https://schemas.shittim.local/kcp/unsupported_root/v1";
    let source = "schemas/source/kcp/unsupported_root.v1.json";
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "UnsupportedRootV1",
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
            "title": "UnsupportedRootV1",
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
            "generation_targets": ["rust"]
        }),
    );
    let registry = SchemaRegistry::load(&temp).expect("load");
    let plan =
        build_target_plan(schema_tool::SyntheticRegistry::new(&registry).unwrap()).expect("plan");
    let err = lower_target_contract_graph(&plan, GenerationTarget::Rust)
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
fn shared_goal_constraint_real_mutant_reloads_lowers_and_renders() {
    let temp = temporary_repo("shared-goal-real-mutant");
    let host_id = "https://schemas.shittim.local/task/normalized_root_task_create_payload/v2";
    let request_id = "https://schemas.shittim.local/kcp/task_create_request/v2";
    let child_id = "https://schemas.shittim.local/task/child_task_proposal/v1";
    let normalized_child_id =
        "https://schemas.shittim.local/task/normalized_child_task_proposal/v1";
    let host_path = temp.join("schemas/source/task/normalized_root_task_create_payload.v2.json");
    let mut host = read_json(&host_path);
    host["$defs"]["goal"]["minLength"] = json!(3);
    write_json(&host_path, &host);

    let registry = SchemaRegistry::load(&temp).expect("reload mutant");
    for (schema_id, version) in [
        (request_id, 2),
        (host_id, 2),
        (child_id, 1),
        (normalized_child_id, 1),
    ] {
        let mut short = business_v2_nine_fields(version, "ab");
        assert!(
            schema_tool::validate::validate_instance(&registry, schema_id, &short).is_err(),
            "mutated host must reject ab through {schema_id}"
        );
        short["goal"] = json!("abc");
        schema_tool::validate::validate_instance(&registry, schema_id, &short).unwrap_or_else(
            |error| panic!("mutated host must accept abc through {schema_id}: {error}"),
        );
    }

    let synthetic = schema_tool::SyntheticRegistry::new(&registry).expect("synthetic profile");
    let plan = build_target_plan(synthetic).expect("target plan");
    let graph = lower_target_contract_graph(&plan, GenerationTarget::Rust).expect("lower mutant");
    let projection = project_rust(&graph).expect("project mutant");
    let rendered =
        schema_tool::render_types_module_from_projection(&projection).expect("render mutant");
    assert!(rendered.contains("pub struct TaskCreateRequestV2"));
    let goal = schema_tool::ContractTypeId::root(host_id)
        .child("$defs")
        .child("goal");
    assert!(graph.nodes.contains_key(&goal));
    assert_eq!(
        graph
            .nodes
            .keys()
            .filter(|id| id.pointer.as_str() == "/$defs/goal")
            .count(),
        1
    );
    std::fs::remove_dir_all(temp).ok();
}

fn business_v2_nine_fields(schema_version: u32, goal: &str) -> serde_json::Value {
    json!({
        "schema_version": schema_version,
        "proposer": "user",
        "goal": goal,
        "constraints": [],
        "success_criteria": [],
        "risk_hint": null,
        "capability_hints": [],
        "task_scope": {
            "schema_version": 1,
            "resource_patterns": [],
            "exclusions": [],
            "allowed_capability_hints": [],
            "expires_at": null
        },
        "delegation_ref": null,
        "origin": {
            "schema_version": 1,
            "kind": "user_input",
            "source_uri": null,
            "upstream_stable_id": null,
            "producer_ref": {"kind": "actor", "id": "actor-1"},
            "parent_origin_refs": []
        }
    })
}

#[test]
fn canonical_task_create_proposer_fragment_has_one_shared_rust_declaration() {
    let root = repo_root();
    let (graph, types, _, _) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&root).expect("lower+render");
    let projection = project_rust(&graph).expect("project");
    let host = "https://schemas.shittim.local/task/normalized_root_task_create_payload/v2";
    let proposer = schema_tool::ContractTypeId::root(host)
        .child("$defs")
        .child("proposer");

    assert_eq!(projection.decls_for_canonical(&proposer).len(), 1);
    assert_eq!(projection.nominal_count_for(&proposer), 0);
    for root_name in [
        "NormalizedRootTaskCreatePayloadV2",
        "NormalizedChildTaskProposalV1",
        "TaskCreateRequestV2",
        "ChildTaskProposalV1",
    ] {
        assert_eq!(
            projection
                .field_type_expr(root_name, "proposer")
                .expect("proposer field"),
            "NormalizedRootTaskCreatePayloadV2Proposer"
        );
    }
    assert_eq!(
        types
            .matches("pub enum NormalizedRootTaskCreatePayloadV2Proposer")
            .count(),
        1
    );
    assert_eq!(
        types
            .matches("impl NormalizedRootTaskCreatePayloadV2Proposer")
            .count(),
        1
    );
    assert_eq!(
        types
            .matches("pub proposer: NormalizedRootTaskCreatePayloadV2Proposer")
            .count(),
        4
    );
}

#[test]
fn tagged_union_branch_fields_share_cross_root_canonical_fragment() {
    let temp = temporary_repo("tagged-union-shared-fragment");
    let host_id = "https://schemas.shittim.local/kcp/tagged_union_shared_host/v1";
    let branch_id = "https://schemas.shittim.local/kcp/tagged_union_shared_branch/v1";
    let first_id = "https://schemas.shittim.local/kcp/tagged_union_shared_first/v1";
    let second_id = "https://schemas.shittim.local/kcp/tagged_union_shared_second/v1";
    let host_source = "schemas/source/kcp/tagged_union_shared_host.v1.json";
    let branch_source = "schemas/source/kcp/tagged_union_shared_branch.v1.json";
    let first_source = "schemas/source/kcp/tagged_union_shared_first.v1.json";
    let second_source = "schemas/source/kcp/tagged_union_shared_second.v1.json";

    write_json(
        &temp.join(host_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": host_id,
            "title": "TaggedUnionSharedHostV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version"],
            "properties": {"schema_version": {"type": "integer", "const": 1}},
            "$defs": {
                "shared_payload": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["value"],
                    "properties": {"value": {"type": "string"}}
                }
            }
        }),
    );
    write_json(
        &temp.join(branch_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": branch_id,
            "title": "TaggedUnionSharedBranchV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "kind", "payload"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "kind": {"type": "string", "const": "shared"},
                "payload": {"$ref": format!("{host_id}#/$defs/shared_payload")}
            }
        }),
    );
    for (id, title, source) in [
        (first_id, "TaggedUnionSharedFirstV1", first_source),
        (second_id, "TaggedUnionSharedSecondV1", second_source),
    ] {
        write_json(
            &temp.join(source),
            &json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": id,
                "title": title,
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version", "choice"],
                "properties": {
                    "schema_version": {"type": "integer", "const": 1},
                    "choice": {
                        "type": "object",
                        "required": ["kind"],
                        "properties": {"kind": {"type": "string", "enum": ["shared"]}},
                        "oneOf": [{"$ref": branch_id}]
                    }
                }
            }),
        );
    }

    for (id, title, source) in [
        (host_id, "TaggedUnionSharedHostV1", host_source),
        (branch_id, "TaggedUnionSharedBranchV1", branch_source),
        (first_id, "TaggedUnionSharedFirstV1", first_source),
        (second_id, "TaggedUnionSharedSecondV1", second_source),
    ] {
        add_manifest_entry(
            &temp,
            json!({
                "id": id,
                "title": title,
                "version": 1,
                "source": source,
                "component": "kcp",
                "kind": "object",
                "compatibility": "new-contract",
                "generation_targets": ["rust"],
                "schema_version_field": "schema_version"
            }),
        );
    }

    let (graph, types, _catalog, _typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&temp).expect("lower+render");
    let projection = project_rust(&graph).expect("project");
    let shared = schema_tool::ContractTypeId::root(host_id)
        .child("$defs")
        .child("shared_payload");

    assert_eq!(projection.decls_for_canonical(&shared).len(), 1);
    assert_eq!(projection.nominal_count_for(&shared), 0);
    for union_name in [
        "TaggedUnionSharedFirstV1Choice",
        "TaggedUnionSharedSecondV1Choice",
    ] {
        assert_eq!(
            projection
                .tagged_union_field_type_expr(union_name, "shared", "payload")
                .expect("branch payload field"),
            "TaggedUnionSharedHostV1SharedPayload"
        );
    }
    assert_eq!(
        types
            .matches("pub struct TaggedUnionSharedHostV1SharedPayload {")
            .count(),
        1
    );
    assert!(
        types
            .matches("payload: TaggedUnionSharedHostV1SharedPayload")
            .count()
            >= 2
    );

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn cross_root_fragment_walk_is_cycle_safe_and_deduplicates_per_root() {
    let temp = temporary_repo("cross-root-fragment-cycle");
    let host_id = "https://schemas.shittim.local/kcp/cross_root_cycle_host/v1";
    let first_id = "https://schemas.shittim.local/kcp/cross_root_cycle_first/v1";
    let second_id = "https://schemas.shittim.local/kcp/cross_root_cycle_second/v1";
    let host_source = "schemas/source/kcp/cross_root_cycle_host.v1.json";
    let first_source = "schemas/source/kcp/cross_root_cycle_first.v1.json";
    let second_source = "schemas/source/kcp/cross_root_cycle_second.v1.json";

    write_json(
        &temp.join(host_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": host_id,
            "title": "CrossRootCycleHostV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version"],
            "properties": {"schema_version": {"type": "integer", "const": 1}},
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
    for (id, title, source) in [
        (first_id, "CrossRootCycleFirstV1", first_source),
        (second_id, "CrossRootCycleSecondV1", second_source),
    ] {
        write_json(
            &temp.join(source),
            &json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": id,
                "title": title,
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version", "left", "right"],
                "properties": {
                    "schema_version": {"type": "integer", "const": 1},
                    "left": {"$ref": format!("{host_id}#/$defs/node")},
                    "right": {"$ref": format!("{host_id}#/$defs/node")}
                }
            }),
        );
    }
    for (id, title, source) in [
        (host_id, "CrossRootCycleHostV1", host_source),
        (first_id, "CrossRootCycleFirstV1", first_source),
        (second_id, "CrossRootCycleSecondV1", second_source),
    ] {
        add_manifest_entry(
            &temp,
            json!({
                "id": id,
                "title": title,
                "version": 1,
                "source": source,
                "component": "kcp",
                "kind": "object",
                "compatibility": "new-contract",
                "generation_targets": ["rust"],
                "schema_version_field": "schema_version"
            }),
        );
    }

    let (graph, types, _catalog, _typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&temp).expect("lower+render");
    let projection = project_rust(&graph).expect("project recursive shared fragment");
    let node = schema_tool::ContractTypeId::root(host_id)
        .child("$defs")
        .child("node");
    assert_eq!(projection.decls_for_canonical(&node).len(), 1);
    assert_eq!(projection.nominal_count_for(&node), 0);
    assert_eq!(
        projection
            .field_type_expr("CrossRootCycleHostV1Node", "next")
            .expect("recursive next"),
        "Option<Box<CrossRootCycleHostV1Node>>"
    );
    assert_eq!(
        types
            .matches("pub struct CrossRootCycleHostV1Node {")
            .count(),
        1
    );

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn root_shared_refs_are_not_cloned_per_use_site() {
    let root = repo_root();
    let (_graph, types, _, _) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&root).expect("lower+render");
    let actor_decls = types.matches("pub struct Actor {").count();
    assert_eq!(actor_decls, 1, "Actor root must be shared");
}

#[test]
fn graph_source_schema_ids_match_rust_closure() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let plan =
        build_target_plan(schema_tool::ProductionRegistry::new(&registry).unwrap()).expect("plan");
    let graph = lower_target_contract_graph(&plan, GenerationTarget::Rust).expect("graph");
    let expected: BTreeSet<_> = plan
        .target(GenerationTarget::Rust)
        .expect("rust")
        .closure()
        .iter()
        .cloned()
        .collect();
    let actual: BTreeSet<_> = graph.source_schema_ids.iter().cloned().collect();
    assert_eq!(expected, actual);
}

#[test]
fn envelopes_reuse_projected_root_fields() {
    let root = repo_root();
    let (graph, _types, _catalog, typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&root).expect("lower+render");
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
    assert!(resolved.node.get("$id").is_some() || resolved.node.get("type").is_some());
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
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
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
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
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
// Manifest v2 root/component/retained-ID namespace migration
// ---------------------------------------------------------------------------

#[test]
fn component_namespace_rejects_spoofed_url_component_forms() {
    let root = require_canonical_id_base("https://schemas.shittim.local/").unwrap();
    assert!(
        validate_component_namespace(&root, "common", "https://schemas.shittim.local/common/")
            .is_ok()
    );
    for namespace in [
        "https://schemas.shittim.local/common_evil/",
        "https://schemas.shittim.local/common/./",
        "https://schemas.shittim.local/common//",
        "https://schemas.shittim.local/%63ommon/",
        "https://schemas.shittim.local:443/common/",
    ] {
        assert!(
            validate_component_namespace(&root, "common", namespace).is_err(),
            "component namespace must reject {namespace}"
        );
    }
}

#[test]
fn production_manifest_is_exactly_83_with_41_retained_and_42_component_native_schemas() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("production manifest v2 loads");
    assert_eq!(registry.manifest().schema_version, 2);
    assert_eq!(
        registry.manifest().id_base,
        "https://schemas.shittim.local/",
        "root namespace is fixed by the v2 contract"
    );
    let components = registry.manifest().components.clone();
    assert_eq!(
        registry.schema_count(),
        83,
        "production baseline is 41 retained + 42 component-native schemas"
    );

    let retained: BTreeSet<_> = components
        .iter()
        .flat_map(|component| component.retained_ids.iter())
        .collect();
    assert_eq!(retained.len(), 41, "all retained IDs must have one owner");
    let mut retained_count = 0usize;
    let mut native_count = 0usize;
    for (_id, loaded) in registry.loaded_schemas() {
        let entry = loaded.entry();
        let source_id = loaded
            .document()
            .get("$id")
            .and_then(serde_json::Value::as_str);
        assert_eq!(source_id, Some(entry.id.as_str()));
        if entry.id.starts_with("https://schemas.shittim.local/v1/") {
            assert!(retained.contains(&entry.id));
            retained_count += 1;
        } else {
            assert!(
                !retained.contains(&entry.id),
                "component-native id must not enter retained ledger: {}",
                entry.id
            );
            native_count += 1;
        }
    }
    assert_eq!(retained_count, 41);
    assert_eq!(native_count, 42);
}

#[test]
fn manifest_v2_rejects_v1_alias_or_retained_ownership_breakage() {
    let temp = temporary_repo("manifest-v2-strict-retained");
    let manifest_path = temp.join("schemas/manifest.json");

    let mut legacy = read_json(&manifest_path);
    legacy["schema_version"] = json!(1);
    write_json(&manifest_path, &legacy);
    assert!(SchemaRegistry::load(&temp)
        .unwrap_err()
        .to_string()
        .contains("only manifest v2"));

    let mut orphan = read_json(&repo_root().join("schemas/manifest.json"));
    orphan["components"]
        .as_array_mut()
        .expect("components")
        .iter_mut()
        .find(|component| component["name"] == "common")
        .expect("common component")["retained_ids"]
        .as_array_mut()
        .expect("retained IDs")
        .remove(0);
    write_json(&manifest_path, &orphan);
    let orphan_error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        orphan_error.contains("retained ownership ledger mismatch"),
        "{orphan_error}"
    );

    let mut duplicate = read_json(&repo_root().join("schemas/manifest.json"));
    let retained_id = duplicate["components"][0]["retained_ids"][0].clone();
    duplicate["components"][1]["retained_ids"]
        .as_array_mut()
        .expect("retained IDs")
        .push(retained_id);
    write_json(&manifest_path, &duplicate);
    let duplicate_error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        duplicate_error.contains("strictly sorted and unique"),
        "{duplicate_error}"
    );

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn registry_load_rejects_component_ref_gate_before_public_use() {
    let temp = temporary_repo("component-ref-gate");
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["components"]
        .as_array_mut()
        .expect("components")
        .iter_mut()
        .find(|component| component["name"] == "kcp")
        .expect("kcp component")["allowed_refs"] = json!(["event", "task"]);
    write_json(&manifest_path, &manifest);

    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(error.contains("component ref gate error"), "{error}");
    assert!(error.contains("kcp") && error.contains("common"), "{error}");

    std::fs::remove_dir_all(temp).ok();
}

// ---------------------------------------------------------------------------
// Manifest v2 root/component/retained-ID namespace migration
// ---------------------------------------------------------------------------

#[test]
fn production_manifest_loads_with_empty_bindings_and_lifecycle_labels() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("production manifest v2 loads");
    assert!(
        registry.manifest().method_version_bindings.is_empty(),
        "production bindings remain empty in the schema/tool stage"
    );
    let mut legacy_validation = 0usize;
    let mut legacy_read = 0usize;
    let mut stable = 0usize;
    let mut new_contract = 0usize;
    let mut breaking = 0usize;
    for entry in &registry.manifest().schemas {
        match entry.compatibility {
            schema_tool::SchemaCompatibility::LegacyValidationOnly => legacy_validation += 1,
            schema_tool::SchemaCompatibility::LegacyReadOnly => legacy_read += 1,
            schema_tool::SchemaCompatibility::V1Stable => stable += 1,
            schema_tool::SchemaCompatibility::NewContract => new_contract += 1,
            schema_tool::SchemaCompatibility::BreakingReplacement => breaking += 1,
        }
    }
    assert_eq!(legacy_validation, 3);
    assert_eq!(legacy_read, 1);
    assert_eq!(stable, 37);
    assert_eq!(new_contract, 30);
    assert_eq!(breaking, 12);
    assert_eq!(registry.schema_count(), 83);
    schema_tool::validate_production_manifest_stage(&registry)
        .expect("production stage gate accepts empty bindings");
}

#[test]
fn generic_loader_rejects_incomplete_nonempty_bindings_without_v2_authority() {
    let temp = temporary_repo("manifest-v2-nonempty-method-binding");
    // Production now includes V2 envelopes. Strip them so this case still proves the
    // no-authority branch rather than the incomplete-coverage branch.
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    schemas.retain(|entry| {
        !matches!(
            entry["id"].as_str(),
            Some(
                "https://schemas.shittim.local/kcp/command_envelope/v2"
                    | "https://schemas.shittim.local/kcp/query_envelope/v2",
            )
        )
    });
    // Source files that remain without manifest entries fail closed as orphans.
    let _ = std::fs::remove_file(temp.join("schemas/source/kcp/command_envelope.v2.json"));
    let _ = std::fs::remove_file(temp.join("schemas/source/kcp/query_envelope.v2.json"));
    manifest["method_version_bindings"] = json!([{
        "family": "command",
        "method": "task.create",
        "active_request_versions": [1],
        "legacy_validation_versions": [],
        "request_schema_id_by_version": {
            "1": "https://schemas.shittim.local/v1/kcp/task_create_request.json"
        },
        "response_schema_id_by_version": {
            "1": "https://schemas.shittim.local/v1/kcp/task_create_response.json"
        }
    }]);
    write_json(&manifest_path, &manifest);

    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        error.contains("non-empty method_version_bindings require active V2 Envelope authority")
            || error.contains("active KCP Envelope authority"),
        "{error}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn public_validation_resolves_schema_only_from_its_registry() {
    let registry = SchemaRegistry::load(&repo_root()).expect("production registry loads");
    let schema_id = "https://schemas.shittim.local/v1/common/actor.json";
    let valid = json!({
        "schema_version": 1,
        "revision": 1,
        "id": "actor-1",
        "kind": "known_user",
        "source": "test",
        "authentication_level": "asserted",
        "confidence": 1.0
    });
    schema_tool::validate::validate_instance(&registry, schema_id, &valid)
        .expect("schema ID is resolved by the supplied registry");

    let error = schema_tool::validate::validate_instance(
        &registry,
        "https://schemas.shittim.local/v1/not-present.json",
        &valid,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("resolve schema"), "{error}");
    assert!(error.contains("schema selector not found"), "{error}");
}

#[test]
fn registry_load_rejects_coordinated_retained_rebind_and_real_orphan() {
    let temp = temporary_repo("manifest-v2-retained-ledger");
    let manifest_path = temp.join("schemas/manifest.json");

    let mut coordinated_rebind = read_json(&manifest_path);
    let moved = coordinated_rebind["components"]
        .as_array_mut()
        .expect("components")
        .iter_mut()
        .find(|component| component["name"] == "common")
        .expect("common component")["retained_ids"]
        .as_array_mut()
        .expect("common retained IDs")
        .remove(0);
    coordinated_rebind["components"]
        .as_array_mut()
        .expect("components")
        .iter_mut()
        .find(|component| component["name"] == "kcp")
        .expect("kcp component")["retained_ids"]
        .as_array_mut()
        .expect("kcp retained IDs")
        .push(moved.clone());
    coordinated_rebind["components"]
        .as_array_mut()
        .expect("components")
        .iter_mut()
        .find(|component| component["name"] == "kcp")
        .expect("kcp component")["retained_ids"]
        .as_array_mut()
        .expect("kcp retained IDs")
        .sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    coordinated_rebind["schemas"]
        .as_array_mut()
        .expect("schemas")
        .iter_mut()
        .find(|entry| entry["id"] == moved)
        .expect("moved entry")["component"] = json!("kcp");
    write_json(&manifest_path, &coordinated_rebind);
    let rebind_error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        rebind_error.contains("retained baseline mismatch"),
        "{rebind_error}"
    );

    let mut orphan = read_json(&repo_root().join("schemas/manifest.json"));
    orphan["schemas"]
        .as_array_mut()
        .expect("schemas")
        .retain(|entry| entry["id"] != "https://schemas.shittim.local/v1/common/actor.json");
    write_json(&manifest_path, &orphan);
    let orphan_error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        orphan_error.contains("retained baseline ID is orphaned"),
        "{orphan_error}"
    );

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn retained_and_component_native_namespaces_are_mutually_exclusive() {
    let temp = temporary_repo("retained-namespace-overlap");
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["components"]
        .as_array_mut()
        .expect("components")
        .push(json!({
            "name": "v1",
            "namespace": "https://schemas.shittim.local/v1/",
            "allowed_refs": [],
            "retained_ids": []
        }));
    write_json(&manifest_path, &manifest);

    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        error.contains("retained and component-native identity classes are mutually exclusive"),
        "{error}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn registry_loads_component_native_entry_and_all_ref_forms() {
    let temp = temporary_repo("component-native-positive");
    let native_id = "https://schemas.shittim.local/kcp/future_native_request/v2";
    let native_source = "schemas/source/kcp/future_native_request.v2.json";
    write_json(
        &temp.join(native_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": native_id,
            "title": "FutureNativeRequestV2",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "actor", "local", "relative", "absolute"],
            "properties": {
                "schema_version": {"type": "integer", "const": 2},
                "actor": {"$ref": "https://schemas.shittim.local/v1/common/actor.json"},
                "local": {"$ref": "#/$defs/local"},
                "relative": {"$ref": "../../../v1/common/actor.json"},
                "absolute": {"$ref": "https://schemas.shittim.local/v1/common/actor.json"}
            },
            "$defs": {
                "local": {"type": "string"}
            }
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": native_id,
            "title": "FutureNativeRequestV2",
            "version": 2,
            "source": native_source,
            "component": "kcp",
            "kind": "kcp_request",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }),
    );

    let registry = SchemaRegistry::load(&temp).expect("native-to-retained refs are allowed");
    assert!(registry.get(native_id).is_ok());
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn component_gate_can_allow_ref_while_target_closure_still_fails() {
    let temp = temporary_repo("component-gate-target-closure");
    let native_id = "https://schemas.shittim.local/kcp/future_closure_request/v1";
    let native_source = "schemas/source/kcp/future_closure_request.v1.json";
    write_json(
        &temp.join(native_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": native_id,
            "title": "FutureClosureRequestV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "actor"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "actor": {"$ref": "https://schemas.shittim.local/v1/common/actor.json"}
            }
        }),
    );
    add_manifest_entry(
        &temp,
        json!({
            "id": native_id,
            "title": "FutureClosureRequestV1",
            "version": 1,
            "source": native_source,
            "component": "kcp",
            "kind": "kcp_request",
            "compatibility": "new-contract",
            "generation_targets": ["typescript"],
            "schema_version_field": "schema_version"
        }),
    );

    let registry = SchemaRegistry::load(&temp).expect("kcp allowed_refs permits common");
    let error = build_target_plan(schema_tool::SyntheticRegistry::new(&registry).unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("generation target closure error"), "{error}");
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn cli_validate_rejects_unauthorized_component_ref_during_registry_load() {
    let temp = temporary_repo("cli-validate-component-gate");
    let source_path = temp.join("schemas/source/common/actor.v1.json");
    let mut source = read_json(&source_path);
    source["properties"]["unauthorized_event"] = json!({
        "$ref": "https://schemas.shittim.local/v1/event/event_envelope.json"
    });
    write_json(&source_path, &source);
    let instance_path = temp.join("instance.json");
    write_json(&instance_path, &json!({}));

    let (code, _stdout, stderr) = Command::new(schema_tool_bin())
        .args([
            "validate",
            "--schema",
            "schemas/source/common/actor.v1.json",
            "--instance",
            instance_path.to_str().expect("UTF-8 path"),
            "--repo-root",
            temp.to_str().expect("UTF-8 path"),
        ])
        .output()
        .map(|output| {
            (
                output.status.code().unwrap_or(1),
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )
        })
        .expect("run schema-tool validate");
    assert_ne!(code, 0, "unauthorized ref must reject CLI validate");
    assert!(stderr.contains("component ref gate error"), "{stderr}");
    std::fs::remove_dir_all(temp).ok();
}

// ---------------------------------------------------------------------------
// Projection sibling / diamond + single projection instance
// ---------------------------------------------------------------------------

#[test]
fn sibling_and_diamond_nominal_projection_and_shared_root() {
    let temp = temporary_repo("sibling-diamond");
    let shared_id = "https://schemas.shittim.local/kcp/shared_leaf/v1";
    let parent_id = "https://schemas.shittim.local/kcp/diamond_parent/v1";
    let shared_source = "schemas/source/kcp/shared_leaf.v1.json";
    let parent_source = "schemas/source/kcp/diamond_parent.v1.json";

    write_json(
        &temp.join(shared_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": shared_id,
            "title": "SharedLeafV1",
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
            "title": "DiamondParentV1",
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
        (shared_id, "SharedLeafV1", shared_source),
        (parent_id, "DiamondParentV1", parent_source),
    ] {
        schemas.push(json!({
            "id": id,
            "title": title,
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }));
    }
    write_json(&manifest_path, &manifest);

    let (graph, types, _catalog, _typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&temp).expect("lower+render");
    let projection = project_rust(&graph).expect("project");

    let point = schema_tool::ContractTypeId::root(parent_id)
        .child("$defs")
        .child("point");
    assert_eq!(
        projection.nominal_count_for(&point),
        2,
        "sibling non-recursive use-sites must project two Nominal declarations"
    );
    let left = projection
        .field_type_expr("DiamondParentV1", "left")
        .unwrap();
    let right = projection
        .field_type_expr("DiamondParentV1", "right")
        .unwrap();
    assert_eq!(left, "DiamondParentV1Left");
    assert_eq!(right, "DiamondParentV1Right");
    assert_ne!(left, right);

    // Diamond whole-schema roots stay SharedRoot (one declaration).
    assert_eq!(types.matches("pub struct SharedLeafV1 {").count(), 1);
    let shared_a = projection
        .field_type_expr("DiamondParentV1", "shared_a")
        .unwrap();
    let shared_b = projection
        .field_type_expr("DiamondParentV1", "shared_b")
        .unwrap();
    assert_eq!(shared_a, "SharedLeafV1");
    assert_eq!(shared_b, "SharedLeafV1");
    assert_eq!(projection.root_name(shared_id), Some("SharedLeafV1"));

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn recursive_backedge_reuses_active_declaration_not_extra_nominal() {
    let temp = temporary_repo("backedge-reuse");
    let self_id = "https://schemas.shittim.local/kcp/backedge_self/v1";
    let source = "schemas/source/kcp/backedge_self.v1.json";
    write_json(
        &temp.join(source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": self_id,
            "title": "BackedgeSelfV1",
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
            "title": "BackedgeSelfV1",
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }),
    );

    let (graph, _types, _catalog, _typed) =
        lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&temp).expect("lower+render");
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
        .field_type_expr("BackedgeSelfV1Child", "next")
        .expect("next on projected node");
    assert_eq!(next, "Option<Box<BackedgeSelfV1Child>>");

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn types_and_typed_share_single_projection_instance_api() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("registry");
    let plan =
        build_target_plan(schema_tool::ProductionRegistry::new(&registry).unwrap()).expect("plan");
    let graph = lower_target_contract_graph(&plan, GenerationTarget::Rust).expect("graph");
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
    let orphan_id = "https://schemas.shittim.local/kcp/ts_only_orphan/v1";
    let orphan_source = "schemas/source/kcp/ts_only_orphan.v1.json";
    write_json(
        &temp.join(orphan_source),
        &json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": orphan_id,
            "title": "TsOnlyOrphanV1",
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
            "title": "TsOnlyOrphanV1",
            "version": 1,
            "source": orphan_source,
            "component": "kcp",
            "kind": "kcp_request",
            "compatibility": "new-contract",
            "generation_targets": ["typescript"],
            "schema_version_field": "schema_version"
        }),
    );

    let registry = SchemaRegistry::load(&temp).expect("load mixed-target registry");
    let plan = build_target_plan(schema_tool::SyntheticRegistry::new(&registry).unwrap())
        .expect("TargetPlan builds without rendering TS");

    let rust_set = plan
        .targets()
        .iter()
        .find(|set| set.target() == GenerationTarget::Rust)
        .expect("rust TargetSchemaSet");
    let ts_set = plan
        .targets()
        .iter()
        .find(|set| set.target() == GenerationTarget::Typescript)
        .expect("typescript TargetSchemaSet");

    assert_eq!(rust_set.target(), GenerationTarget::Rust);
    // Production now includes active V2 Envelope authority; this mixed-target
    // fixture inherits those roots for rust and keeps production-empty bindings.
    assert!(!rust_set.active_envelope_authority().is_empty());
    assert!(rust_set.method_version_bindings().is_empty());

    assert!(
        !rust_set.roots().contains(orphan_id),
        "TS-only orphan must not be a rust root"
    );
    assert!(
        !rust_set.closure().contains(orphan_id),
        "TS-only orphan must not enter rust closure"
    );
    assert!(
        ts_set.roots().contains(orphan_id),
        "TS-only orphan must be a typescript root"
    );
    assert!(
        ts_set.closure().contains(orphan_id),
        "TS-only orphan must be in typescript closure"
    );

    let graph = lower_target_contract_graph(&plan, GenerationTarget::Rust).expect("rust graph");
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
    let plan_err = schema_tool::codegen::plan_artifacts(
        schema_tool::SyntheticRegistry::new(&registry).unwrap(),
    )
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

    let load_err = SchemaRegistry::load(&temp).unwrap_err().to_string();
    let lower = load_err.to_ascii_lowercase();
    assert!(
        lower.contains("mapping")
            || lower.contains("bijective")
            || lower.contains("payload")
            || lower.contains("exact keys"),
        "mixed envelope must fail during registry conditional IR analysis: {load_err}"
    );

    // generate path must also fail closed with the same class of error.
    let graph_err = lower_and_render_rust::<schema_tool::SyntheticNonProduction>(&temp)
        .unwrap_err()
        .to_string();
    let lower = graph_err.to_ascii_lowercase();
    assert!(
        lower.contains("mapping") || lower.contains("bijective") || lower.contains("payload"),
        "{graph_err}"
    );

    std::fs::remove_dir_all(temp).ok();
}
