//! Tagged-union source-profile integration tests.
//!
//! The test uses raw JSON text deliberately: `serde_json::Value` cannot retain
//! duplicate object keys, while serde must reject duplicate tags and fields.

use schema_tool::contract_model::{TypeExpr, TypeShape};
use schema_tool::{lower_and_render_rust, ContractTypeId};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

fn temporary_repo() -> PathBuf {
    let root = repo_root();
    let target = std::env::temp_dir().join(format!(
        "shittim-tagged-union-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    for entry in walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
    {
        let rel = entry.path().strip_prefix(&root).expect("relative path");
        if rel.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some(".git" | "target" | "node_modules")
            )
        }) {
            continue;
        }
        let destination = target.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(destination).expect("create directory");
        } else if entry.file_type().is_file() {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::copy(entry.path(), destination).expect("copy file");
        }
    }
    target
}

fn write_json(path: &Path, value: serde_json::Value) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(path, serde_json::to_string_pretty(&value).unwrap() + "\n").expect("write json");
}

fn add_manifest_entry(root: &Path, entry: serde_json::Value) {
    let path = root.join("schemas/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("manifest")).unwrap();
    manifest["schemas"].as_array_mut().unwrap().push(entry);
    write_json(&path, manifest);
}

fn manifest_entry(id: &str, title: &str, source: &str) -> serde_json::Value {
    json!({
        "id": id,
        "title": title,
        "version": 1,
        "source": source,
        "domain": "kcp",
        "kind": "object",
        "compatibility": "test-only",
        "generation_targets": ["rust"],
        "schema_version_field": "schema_version"
    })
}

type SchemaMutator = Box<dyn Fn(&mut serde_json::Value)>;

fn generated_artifact_bytes(root: &Path) -> Vec<(String, Vec<u8>)> {
    ["types.rs", "catalog.rs", "typed.rs", "mod.rs"]
        .into_iter()
        .map(|name| {
            let path = root
                .join("rust/crates/kernel-contracts/src/generated")
                .join(name);
            (
                name.to_owned(),
                std::fs::read(path).expect("read generated artifact"),
            )
        })
        .collect()
}

fn schema_tool_bin() -> PathBuf {
    std::env::var("CARGO_BIN_EXE_schema-tool")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("rust/target/debug/schema-tool"))
}

fn cli_generate(root: &Path) -> std::process::Output {
    Command::new(schema_tool_bin())
        .args(["generate", "--repo-root"])
        .arg(root)
        .output()
        .expect("run schema-tool generate")
}

fn add_valid_tagged_union_probe(root: &Path, id: &str, targets: serde_json::Value) {
    write_json(
        &root.join("schemas/source/kcp/tagged_union_cli_probe.v1.json"),
        union_probe_schema(id),
    );
    let mut entry = manifest_entry(
        id,
        "TaggedUnionCliProbe",
        "schemas/source/kcp/tagged_union_cli_probe.v1.json",
    );
    entry["generation_targets"] = targets;
    add_manifest_entry(root, entry);
}

fn union_probe_schema(id: &str) -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": id,
        "title": "TaggedUnionInvalidProbe",
        "type": "object",
        "additionalProperties": false,
        "required": ["schema_version", "value"],
        "properties": {
            "schema_version": {"type": "integer", "const": 1},
            "value": {
                "type": "object",
                "required": ["kind"],
                "properties": {"kind": {"type": "string", "enum": ["a"]}},
                "oneOf": [{
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["kind"],
                    "properties": {"kind": {"type": "string", "const": "a"}}
                }]
            }
        }
    })
}

fn lower_union_probe(mutator: impl FnOnce(&mut serde_json::Value)) -> String {
    let temp = temporary_repo();
    let id = "https://schemas.shittim.local/v1/kcp/tagged_union_invalid_probe.json";
    let mut schema = union_probe_schema(id);
    mutator(&mut schema);
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_invalid_probe.v1.json"),
        schema,
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            id,
            "TaggedUnionInvalidProbe",
            "schemas/source/kcp/tagged_union_invalid_probe.v1.json",
        ),
    );
    let error = lower_and_render_rust(&temp).unwrap_err().to_string();
    std::fs::remove_dir_all(temp).ok();
    error
}

fn union_value(schema: &mut serde_json::Value) -> &mut serde_json::Value {
    &mut schema["properties"]["value"]
}

fn union_branch(schema: &mut serde_json::Value, index: usize) -> &mut serde_json::Value {
    &mut schema["properties"]["value"]["oneOf"][index]
}

#[test]
fn tagged_unions_are_neutral_strict_and_serde_faithful() {
    let temp = temporary_repo();
    let probe_id = "https://schemas.shittim.local/v1/kcp/tagged_union_probe.json";
    let branch_id = "https://schemas.shittim.local/v1/kcp/tagged_union_ref_branch.json";
    let nested_id = "https://schemas.shittim.local/v1/kcp/tagged_union_nested_probe.json";
    let recursive_id = "https://schemas.shittim.local/v1/kcp/tagged_union_recursive_probe.json";

    write_json(
        &temp.join("schemas/source/kcp/tagged_union_ref_branch.v1.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": branch_id,
            "title": "TaggedUnionRefBranch",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "kind", "beta_name"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "kind": {"type": "string", "const": "beta"},
                "beta_name": {"type": "string"}
            }
        }),
    );
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_probe.v1.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": probe_id,
            "title": "TaggedUnionProbe",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "choice"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "choice": {
                    "type": "object",
                    "required": ["kind"],
                    "properties": {"kind": {"type": "string", "enum": ["alpha", "beta"]}},
                    "oneOf": [
                        {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["schema_version", "kind", "alpha_name"],
                            "properties": {
                                "schema_version": {"type": "integer", "const": 1},
                                "kind": {"type": "string", "const": "alpha"},
                                "alpha_name": {"type": "string"}
                            }
                        },
                        {"$ref": branch_id}
                    ]
                }
            }
        }),
    );
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_nested_probe.v1.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": nested_id,
            "title": "TaggedUnionNestedProbe",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "record"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "record": {
                    "type": "object",
                    "required": ["record_kind"],
                    "properties": {"record_kind": {"type": "string", "enum": ["request"]}},
                    "oneOf": [{
                        "type": "object", "additionalProperties": false,
                        "required": ["record_kind", "subject"],
                        "properties": {
                            "record_kind": {"type": "string", "const": "request"},
                            "subject": {
                                "type": "object", "required": ["subject_kind"],
                                "properties": {"subject_kind": {"type": "string", "enum": ["operation"]}},
                                "oneOf": [{
                                    "type": "object", "additionalProperties": false,
                                    "required": ["subject_kind", "operation"],
                                    "properties": {
                                        "subject_kind": {"type": "string", "const": "operation"},
                                        "operation": {"type": "string"}
                                    }
                                }]
                            }
                        }
                    }]
                }
            }
        }),
    );
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_recursive_probe.v1.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": recursive_id,
            "title": "TaggedUnionRecursiveProbe",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "choice"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "choice": {
                    "type": "object",
                    "required": ["kind"],
                    "properties": {"kind": {"type": "string", "enum": ["loop"]}},
                    "oneOf": [{
                        "type": "object", "additionalProperties": false,
                        "required": ["kind"],
                        "properties": {
                            "kind": {"type": "string", "const": "loop"},
                            "next": {"$ref": recursive_id},
                            "children": {"type": "array", "items": {"$ref": recursive_id}}
                        }
                    }]
                }
            }
        }),
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            recursive_id,
            "TaggedUnionRecursiveProbe",
            "schemas/source/kcp/tagged_union_recursive_probe.v1.json",
        ),
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            branch_id,
            "TaggedUnionRefBranch",
            "schemas/source/kcp/tagged_union_ref_branch.v1.json",
        ),
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            probe_id,
            "TaggedUnionProbe",
            "schemas/source/kcp/tagged_union_probe.v1.json",
        ),
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            nested_id,
            "TaggedUnionNestedProbe",
            "schemas/source/kcp/tagged_union_nested_probe.v1.json",
        ),
    );

    let (graph, mut types, catalog, typed) =
        lower_and_render_rust(&temp).expect("lower tagged unions");
    let choice = ContractTypeId::root(probe_id)
        .child("properties")
        .child("choice");
    let TypeShape::TaggedUnion {
        discriminator,
        branches,
        ..
    } = &graph.nodes[&choice].shape
    else {
        panic!("oneOf must not be lowered as an object");
    };
    assert_eq!(discriminator, "kind");
    assert_eq!(branches.len(), 2);
    let ref_branch = branches
        .iter()
        .find(|branch| branch.object_type_id == ContractTypeId::root(branch_id))
        .expect("ref branch");
    assert_eq!(
        ref_branch.source.pointer.as_str(),
        "/properties/choice/oneOf/1"
    );
    assert_eq!(ref_branch.source.schema_id, probe_id);
    let inline_branch = branches
        .iter()
        .find(|branch| branch.tag == "alpha")
        .expect("inline branch");
    assert_eq!(
        inline_branch.object_type_id,
        choice.child("oneOf").index(0),
        "inline branch keeps its canonical object identity"
    );
    assert_eq!(
        inline_branch.source.pointer.as_str(),
        "/properties/choice/oneOf/0"
    );
    assert!(
        graph
            .nodes
            .values()
            .filter(|node| matches!(node.shape, TypeShape::TaggedUnion { .. }))
            .count()
            >= 3
    );
    assert!(types.contains("#[serde(tag = \"kind\", deny_unknown_fields)]"));
    assert!(types.contains("pub enum TaggedUnionProbeChoice"));
    assert!(!types.contains("serde(flatten)"));
    assert!(!types.contains("pub struct TaggedUnionProbeChoice {"));
    assert!(types.contains("pub choice: Box<TaggedUnionRecursiveProbeChoice>"));
    assert!(types.contains("next: Option<Box<TaggedUnionRecursiveProbe>>"));
    assert!(types.contains("children: Option<Vec<TaggedUnionRecursiveProbe>>"));
    assert!(!types.contains("Box<Option"));
    assert!(!types.contains("Vec<Box<TaggedUnionRecursiveProbe>>"));

    types.push_str(r##"
#[cfg(test)]
mod tagged_union_raw_json_contracts {
    use super::*;
    #[test]
    fn rejects_missing_unknown_and_duplicate_tags_and_fields_from_raw_json() {
        let alpha = r#"{"kind":"alpha","schema_version":1,"alpha_name":"ok"}"#;
        assert!(serde_json::from_str::<TaggedUnionProbeChoice>(alpha).is_ok());
        assert!(serde_json::from_str::<TaggedUnionProbeChoice>(r#"{"alpha_name":"ok"}"#).is_err());
        assert!(serde_json::from_str::<TaggedUnionProbeChoice>(r#"{"kind":"unknown","alpha_name":"ok"}"#).is_err());
        assert!(serde_json::from_str::<TaggedUnionProbeChoice>(r#"{"kind":"alpha","kind":"beta","alpha_name":"ok"}"#).is_err());
        for raw in [
            alpha,
            r#"{"kind":"beta","schema_version":1,"beta_name":"ok"}"#,
        ] {
            let decoded: TaggedUnionProbeChoice = serde_json::from_str(raw).unwrap();
            let encoded = serde_json::to_string(&decoded).unwrap();
            assert_eq!(encoded.matches("\"kind\"").count(), 1, "tag serializes once: {encoded}");
            let roundtrip: TaggedUnionProbeChoice = serde_json::from_str(&encoded).unwrap();
            assert_eq!(roundtrip, decoded);
        }
        let nested: TaggedUnionNestedProbe = serde_json::from_str(
            r#"{"schema_version":1,"record":{"record_kind":"request","subject":{"subject_kind":"operation","operation":"run"}}}"#,
        ).unwrap();
        let nested_encoded = serde_json::to_string(&nested).unwrap();
        assert_eq!(nested_encoded.matches("\"record_kind\"").count(), 1);
        assert_eq!(nested_encoded.matches("\"subject_kind\"").count(), 1);
        assert_eq!(
            serde_json::from_str::<TaggedUnionNestedProbe>(&nested_encoded).unwrap(),
            nested,
        );
        assert!(serde_json::from_str::<TaggedUnionProbeChoice>(r#"{"kind":"alpha","alpha_name":"ok","extra":true}"#).is_err());
        assert!(serde_json::from_str::<TaggedUnionProbe>(r#"{"schema_version":1,"choice":{"kind":"alpha","schema_version":1,"alpha_name":"ok"},"choice":{"kind":"alpha","schema_version":1,"alpha_name":"again"}}"#).is_err());
    }
}
"##);
    let generated = temp.join("rust/crates/kernel-contracts/src/generated");
    std::fs::write(
        generated.join("types.rs"),
        schema_tool::codegen::ensure_trailing_newline(&types),
    )
    .unwrap();
    std::fs::write(
        generated.join("catalog.rs"),
        schema_tool::codegen::ensure_trailing_newline(&catalog),
    )
    .unwrap();
    std::fs::write(
        generated.join("typed.rs"),
        schema_tool::codegen::ensure_trailing_newline(&typed),
    )
    .unwrap();

    let output = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "-p",
            "kernel-contracts",
            "--manifest-path",
        ])
        .arg(temp.join("rust/Cargo.toml"))
        .env("CARGO_TARGET_DIR", temp.join("cargo-target-tagged-union"))
        .output()
        .expect("run generated serde contract");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(temp).ok();
}

fn assert_nullable_and_non_discriminated_oneof_follow_distinct_contract_paths() {
    let temp = temporary_repo();
    let nullable_id = "https://schemas.shittim.local/v1/kcp/nullable_oneof_probe.json";
    let mut nullable = union_probe_schema(nullable_id);
    nullable["properties"]["value"] = json!({
        "oneOf": [
            {"type": "null"},
            {"type": "string"}
        ]
    });
    write_json(
        &temp.join("schemas/source/kcp/nullable_oneof_probe.v1.json"),
        nullable,
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            nullable_id,
            "NullableOneOfProbe",
            "schemas/source/kcp/nullable_oneof_probe.v1.json",
        ),
    );
    let (graph, _, _, _) = lower_and_render_rust(&temp).expect("nullable oneOf lowers");
    let nullable_value = ContractTypeId::root(nullable_id)
        .child("properties")
        .child("value");
    let TypeShape::Object { fields, .. } = &graph.nodes[&ContractTypeId::root(nullable_id)].shape
    else {
        panic!("nullable probe root must remain object");
    };
    let value = fields
        .iter()
        .find(|field| field.json_name == "value")
        .expect("value field");
    assert!(matches!(value.ty.expr, TypeExpr::Nullable { .. }));
    assert!(
        !graph.nodes.values().any(|node| {
            node.id == nullable_value && matches!(node.shape, TypeShape::TaggedUnion { .. })
        }),
        "nullable oneOf must not be classified as TaggedUnion"
    );
    std::fs::remove_dir_all(temp).ok();

    let error = lower_union_probe(|schema| {
        union_value(schema)["properties"] = json!({});
        union_value(schema)["required"] = json!([]);
        union_value(schema)["oneOf"] = json!([
            {"type": "string"},
            {"type": "integer"}
        ]);
    });
    assert!(
        error.contains("tagged union requires exactly one union-level string enum discriminator"),
        "non-discriminated oneOf must be unsupported: {error}"
    );
}

fn assert_tagged_union_rejects_unresolved_and_cross_target_ref_branches() {
    let unresolved_error = lower_union_probe(|schema| {
        union_value(schema)["oneOf"] = json!([{"$ref": "./missing_tagged_branch.json"}]);
    });
    assert!(
        unresolved_error.contains("missing_tagged_branch")
            || unresolved_error.contains("not found"),
        "unresolved tagged branch must fail: {unresolved_error}"
    );

    let temp = temporary_repo();
    let id = "https://schemas.shittim.local/v1/kcp/tagged_union_cross_target_probe.json";
    let branch_id = "https://schemas.shittim.local/v1/kcp/tagged_union_cross_target_branch.json";
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_cross_target_branch.v1.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema", "$id": branch_id,
            "title": "TaggedUnionCrossTargetBranch", "type": "object", "additionalProperties": false,
            "required": ["schema_version", "kind"], "properties": {"schema_version": {"type": "integer", "const": 1}, "kind": {"type": "string", "const": "ref"}}
        }),
    );
    let mut root = union_probe_schema(id);
    root["properties"]["value"]["oneOf"] = json!([{"$ref": branch_id}]);
    root["properties"]["value"]["properties"]["kind"]["enum"] = json!(["ref"]);
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_cross_target_probe.v1.json"),
        root,
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            branch_id,
            "TaggedUnionCrossTargetBranch",
            "schemas/source/kcp/tagged_union_cross_target_branch.v1.json",
        ),
    );
    let mut entry = manifest_entry(
        id,
        "TaggedUnionCrossTargetProbe",
        "schemas/source/kcp/tagged_union_cross_target_probe.v1.json",
    );
    entry["generation_targets"] = json!(["typescript"]);
    add_manifest_entry(&temp, entry);
    let error = lower_and_render_rust(&temp).unwrap_err().to_string();
    assert!(
        error.contains("generation target closure error") && error.contains(branch_id),
        "cross-target tagged branch must fail target closure: {error}"
    );
    std::fs::remove_dir_all(temp).ok();
}

fn assert_tagged_union_does_not_change_envelope_bindings() {
    let baseline = lower_and_render_rust(&repo_root())
        .expect("baseline graph")
        .0
        .envelopes;
    let temp = temporary_repo();
    add_valid_tagged_union_probe(
        &temp,
        "https://schemas.shittim.local/v1/kcp/tagged_union_binding_probe.json",
        json!(["rust"]),
    );
    let after = lower_and_render_rust(&temp)
        .expect("tagged union graph")
        .0
        .envelopes;
    assert_eq!(
        after, baseline,
        "TaggedUnion must not alter Envelope bindings"
    );
    std::fs::remove_dir_all(temp).ok();
}

fn assert_cli_generation_fails_without_partial_artifacts_for_tagged_union_target_or_profile_errors()
{
    let ts_temp = temporary_repo();
    add_valid_tagged_union_probe(
        &ts_temp,
        "https://schemas.shittim.local/v1/kcp/tagged_union_ts_probe.json",
        json!(["rust", "typescript"]),
    );
    let before = generated_artifact_bytes(&ts_temp);
    let output = cli_generate(&ts_temp);
    assert!(!output.status.success(), "tagged TS target must fail");
    assert!(String::from_utf8_lossy(&output.stderr)
        .to_ascii_lowercase()
        .contains("typescript"),);
    assert_eq!(
        generated_artifact_bytes(&ts_temp),
        before,
        "TS tagged source failure must leave all Rust artifacts byte-identical"
    );
    std::fs::remove_dir_all(ts_temp).ok();

    let invalid_temp = temporary_repo();
    add_valid_tagged_union_probe(
        &invalid_temp,
        "https://schemas.shittim.local/v1/kcp/tagged_union_invalid_cli_probe.json",
        json!(["rust"]),
    );
    let schema_path = invalid_temp.join("schemas/source/kcp/tagged_union_cli_probe.v1.json");
    let mut schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema_path).expect("read probe schema"))
            .expect("parse probe schema");
    schema["properties"]["value"]["properties"]["kind"]["enum"] = json!(["a", "b"]);
    write_json(&schema_path, schema);
    let before = generated_artifact_bytes(&invalid_temp);
    let output = cli_generate(&invalid_temp);
    assert!(!output.status.success(), "invalid tagged profile must fail");
    assert_eq!(
        generated_artifact_bytes(&invalid_temp),
        before,
        "invalid tagged profile must leave all artifacts byte-identical"
    );
    std::fs::remove_dir_all(invalid_temp).ok();
}
#[test]
fn tagged_union_rejects_source_profile_violations() {
    assert_nullable_and_non_discriminated_oneof_follow_distinct_contract_paths();
    assert_tagged_union_rejects_unresolved_and_cross_target_ref_branches();
    assert_tagged_union_does_not_change_envelope_bindings();
    assert_cli_generation_fails_without_partial_artifacts_for_tagged_union_target_or_profile_errors(
    );

    let cases: Vec<(&str, SchemaMutator)> = vec![
        (
            "union discriminator must be required",
            Box::new(|schema| {
                union_value(schema)["required"] = json!([]);
            }),
        ),
        (
            "branch discriminator must be required",
            Box::new(|schema| {
                union_branch(schema, 0)["required"] = json!([]);
            }),
        ),
        (
            "branch discriminator must be a single string const",
            Box::new(|schema| {
                union_branch(schema, 0)["properties"]["kind"] = json!({"type": "string"});
            }),
        ),
        (
            "branch discriminator const must declare type string",
            Box::new(|schema| {
                union_branch(schema, 0)["properties"]["kind"] = json!({"const": "a"});
            }),
        ),
        (
            "tagged union needs at least one branch",
            Box::new(|schema| {
                union_value(schema)["oneOf"] = json!([]);
            }),
        ),
        (
            "discriminator enum and branch const tags must be bijective",
            Box::new(|schema| {
                union_value(schema)["properties"]["kind"]["enum"] = json!(["a", "b"]);
            }),
        ),
        (
            "discriminator enum must be non-empty and unique",
            Box::new(|schema| {
                union_value(schema)["properties"]["kind"]["enum"] = json!(["a", "a"]);
            }),
        ),
        (
            "duplicate tagged-union discriminator const",
            Box::new(|schema| {
                let branch = union_branch(schema, 0).clone();
                union_value(schema)["oneOf"] = json!([branch.clone(), branch]);
                union_value(schema)["properties"]["kind"]["enum"] = json!(["a"]);
            }),
        ),
        (
            "Rust tagged-union variant collision",
            Box::new(|schema| {
                union_value(schema)["oneOf"] = json!([
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind"],
                        "properties": {"kind": {"type": "string", "const": "a-b"}}
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind"],
                        "properties": {"kind": {"type": "string", "const": "a_b"}}
                    }
                ]);
                union_value(schema)["properties"]["kind"]["enum"] = json!(["a-b", "a_b"]);
            }),
        ),
        (
            "Rust tagged-union field collision",
            Box::new(|schema| {
                union_branch(schema, 0)["properties"]["a-b"] = json!({"type": "string"});
                union_branch(schema, 0)["properties"]["a_b"] = json!({"type": "string"});
            }),
        ),
        (
            "tagged union branches must be closed",
            Box::new(|schema| {
                union_branch(schema, 0)
                    .as_object_mut()
                    .unwrap()
                    .remove("additionalProperties");
            }),
        ),
        (
            "tagged union branch must not override",
            Box::new(|schema| {
                union_value(schema)["unevaluatedProperties"] = json!(false);
                union_branch(schema, 0)["additionalProperties"] = json!(true);
            }),
        ),
        (
            "schema-valued additionalProperties",
            Box::new(|schema| {
                union_value(schema)["unevaluatedProperties"] = json!(false);
                union_branch(schema, 0)["additionalProperties"] = json!({"type": "string"});
            }),
        ),
        (
            "patternProperties",
            Box::new(|schema| {
                union_branch(schema, 0)["patternProperties"] = json!({"^x": {"type": "string"}});
            }),
        ),
        (
            "tagged unions only support unevaluatedProperties: false",
            Box::new(|schema| {
                union_value(schema)["unevaluatedProperties"] = json!(true);
            }),
        ),
        (
            "tagged unions only support unevaluatedProperties: false",
            Box::new(|schema| {
                union_value(schema)["unevaluatedProperties"] = json!({"type": "string"});
            }),
        ),
    ];

    for (expected, mutate) in cases {
        let error = lower_union_probe(mutate);
        assert!(
            error.contains(expected),
            "expected {expected:?}, got: {error}"
        );
    }
}

#[test]
fn unevaluated_properties_is_tagged_union_only_and_preserves_canonical_ref_object_policy() {
    let ordinary_error = lower_union_probe(|schema| {
        union_value(schema).as_object_mut().unwrap().remove("oneOf");
        union_value(schema)["unevaluatedProperties"] = json!(false);
    });
    assert!(ordinary_error.contains("only supported on a non-null tagged-union classifier"));

    let nullable_error = lower_union_probe(|schema| {
        union_value(schema)["oneOf"] = json!([{"type": "null"}, {"type": "string"}]);
        union_value(schema)["unevaluatedProperties"] = json!(false);
    });
    assert!(nullable_error.contains("only supported on a non-null tagged-union classifier"));

    let temp = temporary_repo();
    let id = "https://schemas.shittim.local/v1/kcp/tagged_union_uep_ref_probe.json";
    let branch_id = "https://schemas.shittim.local/v1/kcp/tagged_union_uep_ref_branch.json";
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_uep_ref_branch.v1.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema", "$id": branch_id,
            "title": "TaggedUnionUepRefBranch", "type": "object",
            "required": ["schema_version", "kind"],
            "properties": {"schema_version": {"type": "integer", "const": 1}, "kind": {"type": "string", "const": "ref"}, "note": {"type": "string"}}
        }),
    );
    let mut root = union_probe_schema(id);
    root["required"] = json!(["schema_version", "value", "branch"]);
    root["properties"]["branch"] = json!({"$ref": branch_id});
    root["properties"]["value"]["unevaluatedProperties"] = json!(false);
    root["properties"]["value"]["oneOf"] = json!([{"$ref": branch_id}]);
    root["properties"]["value"]["properties"]["kind"]["enum"] = json!(["ref"]);
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_uep_ref_probe.v1.json"),
        root,
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            branch_id,
            "TaggedUnionUepRefBranch",
            "schemas/source/kcp/tagged_union_uep_ref_branch.v1.json",
        ),
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            id,
            "TaggedUnionUepRefProbe",
            "schemas/source/kcp/tagged_union_uep_ref_probe.v1.json",
        ),
    );
    let (graph, mut types, catalog, typed) =
        lower_and_render_rust(&temp).expect("UEP closes union only");
    let branch = graph
        .nodes
        .get(&ContractTypeId::root(branch_id))
        .expect("canonical ref branch");
    assert!(matches!(
        branch.shape,
        TypeShape::Object {
            unknown_field_policy: schema_tool::UnknownFieldPolicy::Allow,
            ..
        }
    ));
    assert!(types.contains("pub struct TaggedUnionUepRefBranch"));
    assert!(types.contains("#[serde(tag = \"kind\", deny_unknown_fields)]"));
    types.push_str(
        r##"
#[cfg(test)]
mod uep_ref_branch_policy_contracts {
    use super::*;

    #[test]
    fn canonical_open_ref_stays_open_while_uep_union_variant_is_strict() {
        assert!(serde_json::from_str::<TaggedUnionUepRefBranch>(
            r#"{"schema_version":1,"kind":"ref","note":"ok","extra":true}"#,
        ).is_ok());
        assert!(serde_json::from_str::<TaggedUnionInvalidProbeValue>(
            r#"{"kind":"ref","schema_version":1,"note":"ok","extra":true}"#,
        ).is_err());
    }
}
"##,
    );
    let generated = temp.join("rust/crates/kernel-contracts/src/generated");
    std::fs::write(
        generated.join("types.rs"),
        schema_tool::codegen::ensure_trailing_newline(&types),
    )
    .unwrap();
    std::fs::write(
        generated.join("catalog.rs"),
        schema_tool::codegen::ensure_trailing_newline(&catalog),
    )
    .unwrap();
    std::fs::write(
        generated.join("typed.rs"),
        schema_tool::codegen::ensure_trailing_newline(&typed),
    )
    .unwrap();
    let output = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "-p",
            "kernel-contracts",
            "--manifest-path",
        ])
        .arg(temp.join("rust/Cargo.toml"))
        .env("CARGO_TARGET_DIR", temp.join("cargo-target-uep-policy"))
        .output()
        .expect("run canonical and union policy contracts");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn tagged_union_rejects_explicit_open_ref_branch_even_when_uep_closes_union() {
    let temp = temporary_repo();
    let id = "https://schemas.shittim.local/v1/kcp/tagged_union_uep_explicit_open_probe.json";
    let branch_id =
        "https://schemas.shittim.local/v1/kcp/tagged_union_uep_explicit_open_branch.json";
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_uep_explicit_open_branch.v1.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema", "$id": branch_id,
            "title": "TaggedUnionUepExplicitOpenBranch", "type": "object", "additionalProperties": true,
            "required": ["schema_version", "kind"],
            "properties": {"schema_version": {"type": "integer", "const": 1}, "kind": {"type": "string", "const": "ref"}}
        }),
    );
    let mut root = union_probe_schema(id);
    root["properties"]["value"]["unevaluatedProperties"] = json!(false);
    root["properties"]["value"]["oneOf"] = json!([{"$ref": branch_id}]);
    root["properties"]["value"]["properties"]["kind"]["enum"] = json!(["ref"]);
    write_json(
        &temp.join("schemas/source/kcp/tagged_union_uep_explicit_open_probe.v1.json"),
        root,
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            branch_id,
            "TaggedUnionUepExplicitOpenBranch",
            "schemas/source/kcp/tagged_union_uep_explicit_open_branch.v1.json",
        ),
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            id,
            "TaggedUnionUepExplicitOpenProbe",
            "schemas/source/kcp/tagged_union_uep_explicit_open_probe.v1.json",
        ),
    );
    let error = lower_and_render_rust(&temp).unwrap_err().to_string();
    assert!(
        error.contains("tagged union branch must not override"),
        "{error}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn tagged_union_rejects_open_branches_and_non_bijective_tags() {
    let temp = temporary_repo();
    let id = "https://schemas.shittim.local/v1/kcp/invalid_tagged_union.json";
    write_json(
        &temp.join("schemas/source/kcp/invalid_tagged_union.v1.json"),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema", "$id": id,
            "title": "InvalidTaggedUnion", "type": "object", "additionalProperties": false,
            "required": ["schema_version"],
            "properties": {"schema_version": {"type": "integer", "const": 1}, "value": {
                "type": "object", "required": ["kind"],
                "properties": {"kind": {"type":"string", "enum":["a", "b"]}},
                "oneOf": [{"type":"object", "additionalProperties":false, "required":["schema_version", "kind"], "properties":{"schema_version":{"type":"integer", "const":1}, "kind":{"type":"string", "const":"a"}}}]
            }}
        }),
    );
    add_manifest_entry(
        &temp,
        manifest_entry(
            id,
            "InvalidTaggedUnion",
            "schemas/source/kcp/invalid_tagged_union.v1.json",
        ),
    );
    let error = lower_and_render_rust(&temp).unwrap_err().to_string();
    assert!(error.contains("bijective"), "{error}");
    std::fs::remove_dir_all(temp).ok();
}
