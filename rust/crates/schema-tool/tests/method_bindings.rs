//! MethodVersionBinding validator, component-native identity, production stage gate,
//! and synthetic 8-method catalog render coverage.

use schema_tool::codegen::plan_artifacts;
use schema_tool::manifest::{GenerationTarget, SchemaRegistry};
use schema_tool::target::build_target_plan;
use schema_tool::{
    lower_and_render_rust_from_registry, lower_target_contract_graph,
    validate_production_manifest_stage,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
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
    let temp = PathBuf::from(
        std::env::var("TMPDIR").unwrap_or_else(|_| "/mnt/data/shittim-build-tmp/tmp".into()),
    )
    .join(format!("shittim-bindings-{}-{}", label, std::process::id()));
    if temp.exists() {
        std::fs::remove_dir_all(&temp).expect("clean old temp");
    }
    copy_tree(&root, &temp);
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

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json")
}

fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n").expect("write");
}

fn sha256_file(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("read");
    hex::encode(Sha256::digest(bytes))
}

const COMMAND_METHODS: &[&str] = &["stop.activate", "task.create"];
const QUERY_METHODS: &[&str] = &[
    "event.poll",
    "event.subscribe",
    "stop.status",
    "system.ping",
    "task.get",
    "task.list",
];

fn retained_request_id(method: &str) -> String {
    let stem = method.replace('.', "_");
    format!("https://schemas.shittim.local/v1/kcp/{stem}_request.json")
}

fn retained_response_id(method: &str) -> String {
    let stem = method.replace('.', "_");
    format!("https://schemas.shittim.local/v1/kcp/{stem}_response.json")
}

struct ComponentNativeSpec<'a> {
    component: &'a str,
    stem: &'a str,
    version: u32,
    title: &'a str,
    kind: &'a str,
    compatibility: &'a str,
    document: Value,
    generation_targets: &'a [&'a str],
    schema_version_field: Option<&'a str>,
}

fn write_component_native_schema(root: &Path, spec: ComponentNativeSpec<'_>) {
    let id = format!(
        "https://schemas.shittim.local/{}/{}/v{}",
        spec.component, spec.stem, spec.version
    );
    let source = format!(
        "schemas/source/{}/{}.v{}.json",
        spec.component, spec.stem, spec.version
    );
    let mut doc = spec.document;
    doc["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    doc["$id"] = json!(id);
    doc["title"] = json!(spec.title);
    write_json(&root.join(&source), &doc);

    let manifest_path = root.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let mut entry = json!({
        "id": id,
        "title": spec.title,
        "version": spec.version,
        "source": source,
        "component": spec.component,
        "kind": spec.kind,
        "compatibility": spec.compatibility,
        "generation_targets": spec.generation_targets,
    });
    entry["schema_version_field"] = match spec.schema_version_field {
        Some(field) => json!(field),
        None => Value::Null,
    };
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    // Production now ships the first-batch business-v2 component-native IDs.
    // Synthetic fixtures must upsert by id instead of always appending, or load
    // fails closed on duplicate $id while still allowing document mutation.
    if let Some(existing) = schemas.iter_mut().find(|item| item["id"] == id) {
        *existing = entry;
    } else {
        schemas.push(entry);
    }
    schemas.sort_by(|left, right| {
        left["id"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["id"].as_str().unwrap_or_default())
    });
    write_json(&manifest_path, &manifest);
}

fn envelope_document(family: &str, methods: &[&str]) -> Value {
    let property = if family == "command" {
        "command_type"
    } else {
        "query_type"
    };
    let message_kind = family;
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["protocol_version", "message_kind", property, "request_id", "payload"],
        "properties": {
            "protocol_version": {"type": "string", "const": "1.0"},
            "message_kind": {"type": "string", "const": message_kind},
            property: {"type": "string", "enum": methods},
            "request_id": {"type": "string"},
            "payload": {"type": "object", "additionalProperties": true}
        }
    })
}

fn install_v2_envelopes_both_targets(
    root: &Path,
    command_methods: &[&str],
    query_methods: &[&str],
) {
    install_v2_envelopes(root, command_methods, query_methods);
    let manifest_path = root.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let schemas = manifest["schemas"].as_array_mut().unwrap();
    for entry in schemas.iter_mut() {
        let id = entry["id"].as_str().unwrap_or_default();
        if id == "https://schemas.shittim.local/kcp/command_envelope/v2"
            || id == "https://schemas.shittim.local/kcp/query_envelope/v2"
        {
            entry["generation_targets"] = json!(["rust", "typescript"]);
        }
    }
    write_json(&manifest_path, &manifest);
}

fn install_v2_envelopes(root: &Path, command_methods: &[&str], query_methods: &[&str]) {
    write_component_native_schema(
        root,
        ComponentNativeSpec {
            component: "kcp",
            stem: "command_envelope",
            version: 2,
            title: "KcpCommandEnvelopeV2",
            kind: "envelope",
            compatibility: "breaking-replacement",
            document: envelope_document("command", command_methods),
            generation_targets: &["rust"],
            schema_version_field: None,
        },
    );
    write_component_native_schema(
        root,
        ComponentNativeSpec {
            component: "kcp",
            stem: "query_envelope",
            version: 2,
            title: "KcpQueryEnvelopeV2",
            kind: "envelope",
            compatibility: "breaking-replacement",
            document: envelope_document("query", query_methods),
            generation_targets: &["rust"],
            schema_version_field: None,
        },
    );
}

fn install_task_create_v2_pair(root: &Path) {
    // Minimal active request/response pair for synthetic task.create v2 binding.
    write_component_native_schema(
        root,
        ComponentNativeSpec {
            component: "kcp",
            stem: "task_create_request",
            version: 2,
            title: "TaskCreateRequestV2",
            kind: "kcp_request",
            compatibility: "breaking-replacement",
            document: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version", "goal"],
                "properties": {
                    "schema_version": {"type": "integer", "const": 2},
                    "goal": {"type": "string", "minLength": 1}
                }
            }),
            generation_targets: &["rust"],
            schema_version_field: Some("schema_version"),
        },
    );
    write_component_native_schema(
        root,
        ComponentNativeSpec {
            component: "kcp",
            stem: "task_create_response",
            version: 2,
            title: "TaskCreateResponseV2",
            kind: "kcp_response",
            compatibility: "breaking-replacement",
            document: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version", "task_id"],
                "properties": {
                    "schema_version": {"type": "integer", "const": 2},
                    "task_id": {"type": "string", "minLength": 1}
                }
            }),
            generation_targets: &["rust"],
            schema_version_field: Some("schema_version"),
        },
    );
}

fn eight_method_bindings() -> Value {
    let mut bindings = Vec::new();
    // command methods sorted by method UTF-8: stop.activate, task.create
    for method in COMMAND_METHODS {
        if *method == "task.create" {
            bindings.push(json!({
                "family": "command",
                "method": method,
                "active_request_versions": [2],
                "legacy_validation_versions": [1],
                "request_schema_id_by_version": {
                    "1": retained_request_id(method),
                    "2": "https://schemas.shittim.local/kcp/task_create_request/v2"
                },
                "response_schema_id_by_version": {
                    "2": "https://schemas.shittim.local/kcp/task_create_response/v2"
                }
            }));
        } else {
            bindings.push(simple_v1_binding("command", method));
        }
    }
    for method in QUERY_METHODS {
        bindings.push(simple_v1_binding("query", method));
    }
    Value::Array(bindings)
}

fn simple_v1_binding(family: &str, method: &str) -> Value {
    json!({
        "family": family,
        "method": method,
        "active_request_versions": [1],
        "legacy_validation_versions": [],
        "request_schema_id_by_version": {
            "1": retained_request_id(method)
        },
        "response_schema_id_by_version": {
            "1": retained_response_id(method)
        }
    })
}

fn set_bindings(root: &Path, bindings: Value) {
    let path = root.join("schemas/manifest.json");
    let mut manifest = read_json(&path);
    manifest["method_version_bindings"] = bindings;
    write_json(&path, &manifest);
}

fn remove_component_native_entries(root: &Path, ids: &[&str]) {
    let path = root.join("schemas/manifest.json");
    let mut manifest = read_json(&path);
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    schemas.retain(|entry| !ids.iter().any(|id| entry["id"].as_str() == Some(*id)));
    write_json(&path, &manifest);
    for id in ids {
        // Best-effort source removal keeps temp trees tidy; load only needs manifest absence.
        if let Some(rest) = id.strip_prefix("https://schemas.shittim.local/") {
            let parts: Vec<_> = rest.split('/').collect();
            if parts.len() == 3 {
                let source = root.join(format!(
                    "schemas/source/{}/{}.{}.json",
                    parts[0], parts[1], parts[2]
                ));
                let _ = std::fs::remove_file(source);
            }
        }
    }
}

fn install_complete_synthetic_registry(root: &Path) {
    install_v2_envelopes(root, COMMAND_METHODS, QUERY_METHODS);
    install_task_create_v2_pair(root);
    // Active request schemas for retained methods are already present.
    // task.create v1 remains legacy-validation-only in production manifest labels.
    // For active task.create response v1 is legacy-read-only and must not be active.
    // Active stop.activate etc. use retained v1 request/response with v1-stable.
    set_bindings(root, eight_method_bindings());
}

#[test]
fn production_load_empty_bindings_and_retained_lifecycle_labels() {
    let registry = SchemaRegistry::load(&repo_root()).expect("production load");
    assert!(registry.manifest().method_version_bindings.is_empty());
    let mut counts = BTreeMap::new();
    for entry in &registry.manifest().schemas {
        *counts.entry(entry.compatibility.as_str()).or_insert(0usize) += 1;
    }
    assert_eq!(counts.get("legacy-validation-only"), Some(&3));
    assert_eq!(counts.get("legacy-read-only"), Some(&1));
    assert_eq!(counts.get("v1-stable"), Some(&37));
    assert_eq!(counts.get("new-contract"), Some(&22));
    assert_eq!(counts.get("breaking-replacement"), Some(&12));
    assert_eq!(registry.schema_count(), 75);
    validate_production_manifest_stage(&registry).expect("stage gate");

    // retained ledger/source bytes unchanged relative to fixture.
    let baseline = read_json(&repo_root().join("schemas/fixtures/manifest/retained_ids.v1.json"));
    for entry in baseline["entries"].as_array().expect("entries") {
        let source = entry["source"].as_str().unwrap();
        let expected = entry["source_sha256"].as_str().unwrap();
        assert_eq!(sha256_file(&repo_root().join(source)), expected, "{source}");
    }
}

#[test]
fn compatibility_rejects_unknown_and_test_only_values() {
    for bad in ["test-only", "future-test-only", "internal", "active"] {
        let temp = temporary_repo(&format!("compat-{bad}"));
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        manifest["schemas"][0]["compatibility"] = json!(bad);
        write_json(&path, &manifest);
        let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(
            error.contains("compatibility") || error.contains(bad) || error.contains("unknown"),
            "bad={bad} error={error}"
        );
        std::fs::remove_dir_all(temp).ok();
    }
}

#[test]
fn component_native_positive_general_and_kcp_envelopes() {
    let temp = temporary_repo("native-positive");
    write_component_native_schema(
        &temp,
        ComponentNativeSpec {
            component: "task",
            stem: "input_task_scope",
            version: 1,
            title: "InputTaskScopeV1",
            kind: "object",
            compatibility: "new-contract",
            document: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version", "label"],
                "properties": {
                    "schema_version": {"type": "integer", "const": 1},
                    "label": {"type": "string", "minLength": 1}
                }
            }),
            generation_targets: &["rust"],
            schema_version_field: Some("schema_version"),
        },
    );
    install_v2_envelopes(&temp, &["task.create"], &["system.ping"]);
    SchemaRegistry::load(&temp).expect("component-native positives load");
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn component_native_negative_url_and_version_shapes() {
    let cases = [
        (
            "dot-stem",
            "https://schemas.shittim.local/kcp/task.create/v1",
            "schemas/source/kcp/task.create.v1.json",
            "TaskCreateV1",
        ),
        (
            "json-suffix",
            "https://schemas.shittim.local/kcp/task_create_request/v1.json",
            "schemas/source/kcp/task_create_request.v1.json",
            "TaskCreateRequestV1",
        ),
        (
            "query",
            "https://schemas.shittim.local/kcp/task_create_request/v1?x=1",
            "schemas/source/kcp/task_create_request.v1.json",
            "TaskCreateRequestV1",
        ),
        (
            "fragment",
            "https://schemas.shittim.local/kcp/task_create_request/v1#frag",
            "schemas/source/kcp/task_create_request.v1.json",
            "TaskCreateRequestV1",
        ),
        (
            "extra-segment",
            "https://schemas.shittim.local/kcp/task/create_request/v1",
            "schemas/source/kcp/task_create_request.v1.json",
            "TaskCreateRequestV1",
        ),
        (
            "encoded",
            "https://schemas.shittim.local/kcp/task%5Fcreate_request/v1",
            "schemas/source/kcp/task_create_request.v1.json",
            "TaskCreateRequestV1",
        ),
    ];
    for (label, id, source, title) in cases {
        let temp = temporary_repo(label);
        write_json(
            &temp.join(source),
            &json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": id,
                "title": title,
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version"],
                "properties": {"schema_version": {"type": "integer", "const": 1}}
            }),
        );
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        manifest["schemas"].as_array_mut().unwrap().push(json!({
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
        write_json(&path, &manifest);
        let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(
            error.contains("component-native")
                || error.contains("canonical")
                || error.contains("percent")
                || error.contains("query")
                || error.contains("fragment")
                || error.contains("segment")
                || error.contains(".json")
                || error.contains("source"),
            "label={label} error={error}"
        );
        std::fs::remove_dir_all(temp).ok();
    }
}

#[test]
fn synthetic_eight_method_bindings_load_lower_render_stable() {
    let temp = temporary_repo("synthetic-8");
    install_complete_synthetic_registry(&temp);

    let registry = SchemaRegistry::load(&temp).expect("synthetic load");
    assert_eq!(registry.manifest().method_version_bindings.len(), 8);

    // production stage gate rejects non-empty synthetic registry
    let stage_err = validate_production_manifest_stage(&registry)
        .unwrap_err()
        .to_string();
    assert!(
        stage_err.contains("production manifest stage gate"),
        "{stage_err}"
    );

    // generic library path succeeds
    let (graph, _types, catalog, _typed) = lower_and_render_rust_from_registry(
        schema_tool::SyntheticRegistry::new(&registry).unwrap(),
    )
    .expect("lower/render");
    assert_eq!(graph.catalog.kcp_command_methods, COMMAND_METHODS);
    assert_eq!(graph.catalog.kcp_query_methods, QUERY_METHODS);
    assert_eq!(graph.catalog.method_version_bindings.len(), 8);
    assert!(catalog.contains("METHOD_VERSION_BINDINGS"));
    assert!(catalog.contains("KCP_ENVELOPE_AUTHORITY_METHODS"));
    assert!(catalog.contains("task.create"));
    assert!(catalog.contains("legacy_validation_versions"));
    assert!(
        catalog.contains("select_request_version"),
        "typed selector must bind active request and response to one request version"
    );

    let second = lower_and_render_rust_from_registry(
        schema_tool::SyntheticRegistry::new(&registry).unwrap(),
    )
    .expect("second render")
    .2;
    assert_eq!(catalog, second, "render twice must be stable");

    // plan_artifacts also works without production gate
    let synthetic_profile = schema_tool::SyntheticRegistry::new(&registry).unwrap();
    plan_artifacts(synthetic_profile).expect("plan synthetic artifacts");
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn binding_negative_cases_fail_closed() {
    // empty active
    {
        let temp = temporary_repo("empty-active");
        install_complete_synthetic_registry(&temp);
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        manifest["method_version_bindings"][0]["active_request_versions"] = json!([]);
        write_json(&path, &manifest);
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(err.contains("active_request_versions"), "{err}");
        std::fs::remove_dir_all(temp).ok();
    }
    // unsorted methods
    {
        let temp = temporary_repo("unsorted");
        install_complete_synthetic_registry(&temp);
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        let arr = manifest["method_version_bindings"].as_array_mut().unwrap();
        arr.swap(0, 1);
        write_json(&path, &manifest);
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(err.contains("sorted") || err.contains("family"), "{err}");
        std::fs::remove_dir_all(temp).ok();
    }
    // map key leading zero
    {
        let temp = temporary_repo("leading-zero");
        install_complete_synthetic_registry(&temp);
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        let binding = &mut manifest["method_version_bindings"][0];
        let old = binding["request_schema_id_by_version"].clone();
        let mut map = serde_json::Map::new();
        for (k, v) in old.as_object().unwrap() {
            map.insert(format!("0{k}"), v.clone());
        }
        binding["request_schema_id_by_version"] = Value::Object(map);
        write_json(&path, &manifest);
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(err.contains("canonical") || err.contains("key"), "{err}");
        std::fs::remove_dir_all(temp).ok();
    }
    // wrong kind
    {
        let temp = temporary_repo("wrong-kind");
        install_complete_synthetic_registry(&temp);
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        // bind command method to a response id
        let binding = manifest["method_version_bindings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|b| b["method"] == "stop.activate")
            .unwrap();
        binding["request_schema_id_by_version"]["1"] =
            json!("https://schemas.shittim.local/v1/kcp/stop_activate_response.json");
        write_json(&path, &manifest);
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(err.contains("kcp_request") || err.contains("kind"), "{err}");
        std::fs::remove_dir_all(temp).ok();
    }
    // active references legacy-validation-only
    {
        let temp = temporary_repo("active-legacy");
        install_complete_synthetic_registry(&temp);
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        let binding = manifest["method_version_bindings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|b| b["method"] == "task.create")
            .unwrap();
        // make active point at retained v1 request (legacy-validation-only)
        binding["active_request_versions"] = json!([1]);
        binding["legacy_validation_versions"] = json!([]);
        binding["request_schema_id_by_version"] = json!({
            "1": "https://schemas.shittim.local/v1/kcp/task_create_request.json"
        });
        binding["response_schema_id_by_version"] = json!({
            "1": "https://schemas.shittim.local/v1/kcp/task_create_response.json"
        });
        write_json(&path, &manifest);
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(err.contains("legacy") || err.contains("active"), "{err}");
        std::fs::remove_dir_all(temp).ok();
    }
    // missing envelope authority with non-empty bindings
    {
        let temp = temporary_repo("missing-authority");
        // Production already ships V2 envelopes; strip them so non-empty bindings
        // fail closed for missing active authority rather than duplicate ids.
        remove_component_native_entries(
            &temp,
            &[
                "https://schemas.shittim.local/kcp/command_envelope/v2",
                "https://schemas.shittim.local/kcp/query_envelope/v2",
            ],
        );
        install_task_create_v2_pair(&temp);
        set_bindings(&temp, eight_method_bindings());
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(
            err.contains("KcpCommandEnvelopeV2")
                || err.contains("authority")
                || err.contains("Envelope"),
            "{err}"
        );
        std::fs::remove_dir_all(temp).ok();
    }
    // method coverage incomplete
    {
        let temp = temporary_repo("missing-method");
        install_complete_synthetic_registry(&temp);
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        let arr = manifest["method_version_bindings"].as_array_mut().unwrap();
        arr.pop();
        write_json(&path, &manifest);
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(err.contains("missing") || err.contains("cover"), "{err}");
        std::fs::remove_dir_all(temp).ok();
    }
    // response map includes legacy version
    {
        let temp = temporary_repo("legacy-response");
        install_complete_synthetic_registry(&temp);
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        let binding = manifest["method_version_bindings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|b| b["method"] == "task.create")
            .unwrap();
        binding["response_schema_id_by_version"] = json!({
            "1": "https://schemas.shittim.local/v1/kcp/task_create_response.json",
            "2": "https://schemas.shittim.local/kcp/task_create_response/v2"
        });
        write_json(&path, &manifest);
        let err = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(
            err.contains("response_schema_id_by_version") || err.contains("active"),
            "{err}"
        );
        std::fs::remove_dir_all(temp).ok();
    }
}

#[test]
fn production_cli_still_succeeds_while_library_accepts_synthetic() {
    let root = repo_root();
    let registry = SchemaRegistry::load(&root).expect("production");
    validate_production_manifest_stage(&registry).expect("gate");
    let production_profile = schema_tool::ProductionRegistry::new(&registry).unwrap();
    plan_artifacts(production_profile).expect("production plan");

    let temp = temporary_repo("stage-vs-library");
    install_complete_synthetic_registry(&temp);
    let synthetic = SchemaRegistry::load(&temp).expect("synthetic load");
    assert!(validate_production_manifest_stage(&synthetic).is_err());
    let synthetic_profile = schema_tool::SyntheticRegistry::new(&synthetic).unwrap();
    let plan = build_target_plan(synthetic_profile).expect("synthetic library target plan");
    lower_target_contract_graph(&plan, GenerationTarget::Rust).expect("lower synthetic");
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn v2_authority_with_empty_bindings_keeps_active_and_legacy_catalogs() {
    let temp = temporary_repo("authority-without-bindings");
    install_v2_envelopes(&temp, COMMAND_METHODS, QUERY_METHODS);
    let registry = SchemaRegistry::load(&temp).expect("authority registry");
    let (graph, _, catalog, _) = lower_and_render_rust_from_registry(
        schema_tool::SyntheticRegistry::new(&registry).unwrap(),
    )
    .expect("render authority without bindings");
    assert_eq!(graph.catalog.kcp_command_methods.len(), 2);
    assert_eq!(graph.catalog.kcp_query_methods.len(), 6);
    assert_eq!(graph.catalog.kcp_legacy_v1_command_methods.len(), 2);
    assert_eq!(graph.catalog.kcp_legacy_v1_query_methods.len(), 6);
    assert!(catalog.contains("KCP_ENVELOPE_AUTHORITY_METHODS"));
    assert!(catalog.contains("KCP_LEGACY_V1_METHODS"));
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn source_title_and_schema_version_contracts_fail_independently() {
    let cases = [
        ("missing-title", None, None, None),
        ("title-mismatch", Some(json!("WrongTitle")), None, None),
        ("version-alias", None, Some(json!("version")), None),
        ("version-optional", None, None, Some(json!(["goal"]))),
    ];
    for (label, source_title, field, required) in cases {
        let temp = temporary_repo(label);
        install_task_create_v2_pair(&temp);
        let source = temp.join("schemas/source/kcp/task_create_request.v2.json");
        let mut document = read_json(&source);
        if label == "missing-title" {
            document.as_object_mut().unwrap().remove("title");
        }
        if let Some(title) = source_title {
            document["title"] = title;
        }
        if let Some(required) = required {
            document["required"] = required;
        }
        write_json(&source, &document);
        if let Some(field) = field {
            let path = temp.join("schemas/manifest.json");
            let mut manifest = read_json(&path);
            let entry = manifest["schemas"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|entry| {
                    entry["id"] == "https://schemas.shittim.local/kcp/task_create_request/v2"
                })
                .unwrap();
            entry["schema_version_field"] = field;
            write_json(&path, &manifest);
        }
        assert!(SchemaRegistry::load(&temp).is_err(), "{label} must fail");
        std::fs::remove_dir_all(temp).ok();
    }
}

#[test]
fn production_lifecycle_ledger_rejects_label_swap() {
    let temp = temporary_repo("lifecycle-swap");
    let path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&path);
    let schemas = manifest["schemas"].as_array_mut().unwrap();
    let command = schemas
        .iter_mut()
        .find(|entry| entry["id"] == "https://schemas.shittim.local/v1/kcp/command_envelope.json")
        .unwrap();
    command["compatibility"] = json!("v1-stable");
    let stable = schemas
        .iter_mut()
        .find(|entry| {
            entry["id"] == "https://schemas.shittim.local/v1/kcp/task_create_request.json"
        })
        .unwrap();
    stable["compatibility"] = json!("v1-stable");
    write_json(&path, &manifest);
    let registry = SchemaRegistry::load(&temp).expect("generic load accepts generic combinations");
    assert!(schema_tool::ProductionRegistry::new(&registry).is_err());
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn cross_target_binding_without_complete_authority_fails() {
    let temp = temporary_repo("cross-target-binding");
    install_v2_envelopes_both_targets(&temp, COMMAND_METHODS, QUERY_METHODS);
    install_task_create_v2_pair(&temp);
    set_bindings(&temp, eight_method_bindings());
    let path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&path);
    let schemas = manifest["schemas"].as_array_mut().unwrap();
    for entry in schemas.iter_mut() {
        if entry["id"] == "https://schemas.shittim.local/kcp/task_create_request/v2" {
            entry["generation_targets"] = json!(["typescript"]);
        }
        if entry["id"] == "https://schemas.shittim.local/kcp/task_create_response/v2" {
            entry["generation_targets"] = json!(["rust"]);
        }
    }
    write_json(&path, &manifest);
    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(error.contains("no generation target shared"), "{error}");
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn envelope_partial_claimants_fail_closed() {
    for (label, mutate) in [
        ("wrong-compat", "compat"),
        ("wrong-title", "title"),
        ("wrong-id", "id"),
    ] {
        let temp = temporary_repo(label);
        install_v2_envelopes(&temp, COMMAND_METHODS, QUERY_METHODS);
        let path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&path);
        let entry = manifest["schemas"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["title"] == COMMAND_ENVELOPE_TITLE)
            .unwrap();
        match mutate {
            "compat" => entry["compatibility"] = json!("new-contract"),
            "title" => entry["title"] = json!("WrongCommandEnvelopeV2"),
            "id" => {
                entry["id"] = json!("https://schemas.shittim.local/kcp/command_envelope_alt/v2")
            }
            _ => unreachable!(),
        }
        write_json(&path, &manifest);
        assert!(SchemaRegistry::load(&temp).is_err(), "{label} must fail");
        std::fs::remove_dir_all(temp).ok();
    }
}

const COMMAND_ENVELOPE_TITLE: &str = "KcpCommandEnvelopeV2";

#[test]
fn rendered_selector_returns_typed_active_legacy_and_unsupported() {
    let temp = temporary_repo("selector-runtime");
    install_complete_synthetic_registry(&temp);
    let registry = SchemaRegistry::load(&temp).expect("synthetic registry");
    let (_, mut types, mut catalog, mut typed) = lower_and_render_rust_from_registry(
        schema_tool::SyntheticRegistry::new(&registry).unwrap(),
    )
    .expect("render");
    catalog.push_str(
        r#"
#[cfg(test)]
mod request_version_selection_contracts {
    use super::*;

    #[test]
    fn active_legacy_and_unsupported_are_typed() {
        assert_eq!(
            select_request_version(KcpMethodFamily::Command, "task.create", 2),
            RequestVersionSelection::Active {
                request_schema_id: "https://schemas.shittim.local/kcp/task_create_request/v2",
                response_schema_id: "https://schemas.shittim.local/kcp/task_create_response/v2",
            }
        );
        assert_eq!(
            select_request_version(KcpMethodFamily::Command, "task.create", 1),
            RequestVersionSelection::LegacyValidationOnly {
                request_schema_id: "https://schemas.shittim.local/v1/kcp/task_create_request.json",
            }
        );
        assert_eq!(
            select_request_version(KcpMethodFamily::Command, "task.create", 9),
            RequestVersionSelection::Unsupported
        );
    }
}
"#,
    );
    let generated = temp.join("rust/crates/kernel-contracts/src/generated");
    types.push('\n');
    typed.push('\n');
    std::fs::write(generated.join("types.rs"), types).unwrap();
    std::fs::write(generated.join("catalog.rs"), catalog).unwrap();
    std::fs::write(generated.join("typed.rs"), typed).unwrap();
    let output = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "-p",
            "kernel-contracts",
            "--lib",
            "request_version_selection_contracts",
            "--manifest-path",
        ])
        .arg(temp.join("rust/Cargo.toml"))
        .env("CARGO_TARGET_DIR", temp.join("cargo-target-selector"))
        .output()
        .expect("run generated selector contracts");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn production_cli_check_and_generate_reject_nonempty_stage() {
    let temp = temporary_repo("production-cli-nonempty");
    install_complete_synthetic_registry(&temp);
    let lock_path = temp.join(".schema-tool-generate.lock");
    std::fs::remove_file(&lock_path).ok();
    let binary = std::env::var("CARGO_BIN_EXE_schema-tool")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("rust/target/debug/schema-tool"));
    for command in ["check", "generate"] {
        let output = Command::new(&binary)
            .arg(command)
            .arg("--repo-root")
            .arg(&temp)
            .output()
            .expect("run schema-tool CLI");
        assert!(
            !output.status.success(),
            "{command} must reject nonempty production stage"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("production manifest stage gate"),
            "{stderr}"
        );
    }
    assert!(
        !lock_path.exists(),
        "generate must reject the production profile before ArtifactTransaction::begin"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn production_catalog_keeps_types_typed_mod_stable_and_renames_active_catalog() {
    let registry = SchemaRegistry::load(&repo_root()).expect("load");
    let (_graph, types, catalog, typed) = lower_and_render_rust_from_registry(
        schema_tool::ProductionRegistry::new(&registry).unwrap(),
    )
    .expect("render");
    let mod_rs = schema_tool::GENERATED_MOD_RS;

    assert!(catalog.contains("KCP_ENVELOPE_AUTHORITY_METHODS"));
    assert!(catalog.contains("KCP_LEGACY_V1_METHODS"));
    assert!(!catalog.contains("KCP_V1_METHODS"));
    assert!(catalog.contains("METHOD_VERSION_BINDINGS: &[MethodVersionBinding] = &[\n];"));

    // Anchor byte stability for types/typed/mod against the already generated production files.
    let generated = repo_root().join("rust/crates/kernel-contracts/src/generated");
    assert_eq!(
        types,
        std::fs::read_to_string(generated.join("types.rs")).unwrap()
    );
    assert_eq!(
        typed,
        std::fs::read_to_string(generated.join("typed.rs")).unwrap()
    );
    assert_eq!(
        mod_rs,
        std::fs::read_to_string(generated.join("mod.rs")).unwrap()
    );
    // Only types/typed/mod remain byte-stable; catalog intentionally changes with
    // the typed request-version selection API.
    assert!(catalog.contains("pub enum RequestVersionSelection"));
}
