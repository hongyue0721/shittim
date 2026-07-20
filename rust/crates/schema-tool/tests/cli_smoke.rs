//! schema-tool CLI smoke tests (generate twice, check, validate examples).

use schema_tool::SchemaRegistry;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // schema-tool -> crates -> rust -> repo
    dir.pop();
    dir.pop();
    dir.pop();
    dir
}

fn schema_tool_bin() -> PathBuf {
    // Prefer cargo-built binary path via CARGO_BIN_EXE when available.
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_schema-tool") {
        return PathBuf::from(path);
    }
    // Fallback for non-cargo test harness.
    repo_root().join("rust/target/debug/schema-tool")
}

fn run_tool_for_root(args: &[&str], root: &Path) -> (i32, String, String) {
    let output = Command::new(schema_tool_bin())
        .args(args)
        .arg("--repo-root")
        .arg(root)
        .output()
        .expect("run schema-tool");
    let code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (code, stdout, stderr)
}

fn run_tool(args: &[&str]) -> (i32, String, String) {
    let _guard = production_repo_generate_guard();
    run_tool_for_root(args, &repo_root())
}

fn production_repo_generate_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("production repo generate test guard")
}

fn all_generated_artifact_bytes(root: &Path) -> Vec<(String, Vec<u8>)> {
    ["types.rs", "catalog.rs", "typed.rs", "mod.rs"]
        .into_iter()
        .map(|name| {
            let path = root
                .join("rust/crates/kernel-contracts/src/generated")
                .join(name);
            (
                name.to_string(),
                std::fs::read(path).expect("read generated artifact"),
            )
        })
        .collect()
}

#[test]
fn generate_is_byte_stable_across_two_runs() {
    let root = repo_root();
    let types = root.join("rust/crates/kernel-contracts/src/generated/types.rs");
    let catalog = root.join("rust/crates/kernel-contracts/src/generated/catalog.rs");
    let typed = root.join("rust/crates/kernel-contracts/src/generated/typed.rs");

    let (c1, o1, e1) = run_tool(&["generate"]);
    assert_eq!(c1, 0, "generate1 failed: {o1}\n{e1}");
    let first = std::fs::read(&types).expect("read types after generate1");
    let first_catalog = std::fs::read(&catalog).expect("read catalog after generate1");
    let first_typed = std::fs::read(&typed).expect("read typed bindings after generate1");

    let (c2, o2, e2) = run_tool(&["generate"]);
    assert_eq!(c2, 0, "generate2 failed: {o2}\n{e2}");
    let second = std::fs::read(&types).expect("read types after generate2");
    let second_catalog = std::fs::read(&catalog).expect("read catalog after generate2");
    let second_typed = std::fs::read(&typed).expect("read typed bindings after generate2");

    assert_eq!(
        first_typed, second_typed,
        "generated typed bindings must be byte-for-byte identical"
    );

    assert_eq!(
        first_catalog, second_catalog,
        "generated catalog must be byte-for-byte identical"
    );

    assert_eq!(
        first, second,
        "generate twice must be byte-for-byte identical"
    );
    let text = String::from_utf8(first).expect("utf8");
    assert!(text.contains("GENERATED"), "missing GENERATED marker");
    let catalog_text = String::from_utf8(first_catalog).expect("catalog utf8");
    assert!(
        catalog_text.contains("KCP_LEGACY_V1_METHODS"),
        "missing explicit legacy method catalog"
    );
    assert!(
        catalog_text.contains("KCP_ENVELOPE_AUTHORITY_METHODS"),
        "missing active method catalog"
    );
    assert!(
        catalog_text.contains("METHOD_VERSION_BINDINGS"),
        "missing method version binding catalog"
    );
}

#[test]
fn check_fails_closed_when_schema_declares_unknown_format() {
    let temp = temporary_repo("unknown-format");
    let schema_path = temp.join("schemas/source/kcp/command_envelope.v2.json");
    let mut schema = read_json(&schema_path);
    schema["properties"]["request_id"]["format"] = serde_json::json!("shittim-unknown-format");
    write_json(&schema_path, &schema);

    let registry = SchemaRegistry::load(&temp).expect("unknown format is a compile-time concern");
    let error = schema_tool::validate::compile_all(&registry)
        .expect_err("shared validator options must reject unknown format")
        .to_string();
    assert!(error.contains("shittim-unknown-format"), "{error}");

    let (code, stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0, "check unexpectedly passed: {stdout}");
    assert!(stderr.contains("shittim-unknown-format"), "{stderr}");
    std::fs::remove_dir_all(temp).expect("clean unknown-format repo");
}

#[test]
fn conditional_payload_bindings_follow_schema_without_rust_template_changes() {
    let temp = temporary_repo("dynamic-typed-binding");
    let payload_id = "https://schemas.shittim.local/kcp/test_dynamic_request/v1";
    let payload_source = "schemas/source/kcp/test_dynamic_request.v1.json";
    write_json(
        &temp.join(payload_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": payload_id,
            "title": "TestDynamicRequestV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "value"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "value": {"type": "string"}
            }
        }),
    );
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"]
        .as_array_mut()
        .expect("manifest schemas")
        .push(serde_json::json!({
            "id": payload_id,
            "title": "TestDynamicRequestV1",
            "version": 1,
            "source": payload_source,
            "component": "kcp",
            "kind": "kcp_request",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }));
    write_json(&manifest_path, &manifest);

    let envelope_path = temp.join("schemas/source/kcp/command_envelope.v1.json");
    let mut envelope = read_json(&envelope_path);
    envelope["properties"]["command_type"]["enum"]
        .as_array_mut()
        .expect("command enum")
        .push(serde_json::json!("test.dynamic"));
    envelope["allOf"]
        .as_array_mut()
        .expect("command allOf")
        .push(serde_json::json!({
            "if": {
                "properties": {"command_type": {"const": "test.dynamic"}},
                "required": ["command_type"]
            },
            "then": {
                "properties": {"payload": {"$ref": payload_id}}
            }
        }));
    write_json(&envelope_path, &envelope);

    let (code, stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "generate failed: {stdout}\n{stderr}");
    let typed =
        std::fs::read_to_string(temp.join("rust/crates/kernel-contracts/src/generated/typed.rs"))
            .expect("read generated typed bindings");
    assert!(typed.contains("TestDynamic(Box<TestDynamicRequestV1>)"));
    assert!(typed.contains("\"test.dynamic\" => KcpCommandPayload::TestDynamic"));
    assert!(typed.contains("pub const KCP_COMMAND_ENVELOPE_SCHEMA_ID: &str"));
    assert!(typed.contains("pub fn decode_after_validation(value: Value)"));
    assert!(typed.contains("ContractError::WireDecodeAfterSchema"));
    assert!(typed.contains("ContractError::PayloadDecodeAfterSchema"));
    assert!(typed.contains("ContractError::GeneratedDiscriminatorMapping"));
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn conditional_payload_binding_rejects_missing_mapping() {
    let temp = temporary_repo("missing-typed-mapping");
    mutate_command_envelope(&temp, |envelope| {
        envelope["properties"]["command_type"]["enum"]
            .as_array_mut()
            .expect("command enum")
            .push(serde_json::json!("missing.mapping"));
    });
    assert_generate_fails(&temp, "enum/mapping mismatch");
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn conditional_payload_binding_rejects_duplicate_mapping() {
    let temp = temporary_repo("duplicate-typed-mapping");
    mutate_command_envelope(&temp, |envelope| {
        let duplicate = envelope["allOf"][0].clone();
        envelope["allOf"]
            .as_array_mut()
            .expect("command allOf")
            .push(duplicate);
    });
    assert_generate_fails(&temp, "duplicate payload mapping");
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn conditional_payload_binding_rejects_variant_collision() {
    let temp = temporary_repo("typed-variant-collision");
    mutate_command_envelope(&temp, |envelope| {
        envelope["properties"]["command_type"]["enum"]
            .as_array_mut()
            .expect("command enum")
            .push(serde_json::json!("task-create"));
        envelope["allOf"]
            .as_array_mut()
            .expect("command allOf")
            .push(serde_json::json!({
                "if": {
                    "properties": {"command_type": {"const": "task-create"}},
                    "required": ["command_type"]
                },
                "then": {
                    "properties": {
                        "payload": {
                            "$ref": "https://schemas.shittim.local/v1/kcp/task_create_request.json"
                        }
                    }
                }
            }));
    });
    assert_generate_fails(&temp, "variant collision");
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn check_passes_on_clean_tree() {
    let (c, o, e) = run_tool(&["generate"]);
    assert_eq!(c, 0, "generate before check: {o}\n{e}");
    let (c, o, e) = run_tool(&["check"]);
    assert_eq!(c, 0, "check failed: {o}\n{e}");
}

#[test]
fn envelope_field_ref_validation_siblings_fail_closed() {
    let temp = temporary_repo("envelope-ref-validation-sibling");
    let envelope_path = temp.join("schemas/source/kcp/command_envelope.v1.json");
    let mut envelope = read_json(&envelope_path);
    envelope["properties"]["actor"] = serde_json::json!({
        "$ref": "https://schemas.shittim.local/v1/common/actor.json",
        "description": "allowed annotation",
        "minLength": 1
    });
    write_json(&envelope_path, &envelope);

    assert_generate_fails(&temp, "$ref siblings with validation or shape semantics");
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn ref_validation_siblings_fail_closed_instead_of_being_ignored() {
    let temp = temporary_repo("ref-validation-sibling");
    let actor_path = temp.join("schemas/source/common/actor.v1.json");
    let mut actor = read_json(&actor_path);
    actor["properties"]["source"] = serde_json::json!({
        "$ref": "https://schemas.shittim.local/v1/common/actor.json",
        "minLength": 1
    });
    write_json(&actor_path, &actor);

    assert_generate_fails(&temp, "$ref siblings with validation or shape semantics");
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn optional_non_null_fields_emit_skip_serializing_if() {
    let temp = temporary_repo("optional-non-null-serde");
    let schema_id = "https://schemas.shittim.local/kcp/test_optional_nullability/v1";
    let schema_source = "schemas/source/kcp/test_optional_nullability.v1.json";
    write_json(
        &temp.join(schema_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "TestOptionalNullabilityV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "required_nullable", "required_present"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "required_present": {"type": "string"},
                "required_nullable": {"type": ["string", "null"]},
                "optional_non_null": {"type": "string"},
                "optional_nullable": {"type": ["string", "null"]},
                "optional_non_null_enum": {
                    "type": "string",
                    "enum": ["alpha", "beta"]
                },
                "optional_non_null_object": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "nested_optional_non_null": {"type": "string"}
                    }
                }
            }
        }),
    );

    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"]
        .as_array_mut()
        .expect("manifest schemas")
        .push(serde_json::json!({
            "id": schema_id,
            "title": "TestOptionalNullabilityV1",
            "version": 1,
            "source": schema_source,
            "component": "kcp",
            "kind": "kcp_request",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }));
    write_json(&manifest_path, &manifest);

    let (code, stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "generate failed: {stdout}\n{stderr}");
    let types =
        std::fs::read_to_string(temp.join("rust/crates/kernel-contracts/src/generated/types.rs"))
            .expect("read generated types");

    assert!(
        types.contains("pub struct TestOptionalNullabilityV1"),
        "missing generated struct"
    );
    assert!(
        types.contains(
            "#[serde(skip_serializing_if = \"Option::is_none\")]\n    pub optional_non_null: Option<String>,"
        ),
        "optional non-null string must omit None: {types}"
    );
    assert!(
        types.contains(
            "#[serde(skip_serializing_if = \"Option::is_none\")]\n    pub optional_non_null_enum: Option<TestOptionalNullabilityV1OptionalNonNullEnum>,"
        ),
        "optional non-null enum must omit None"
    );
    assert!(
        types.contains(
            "#[serde(skip_serializing_if = \"Option::is_none\")]\n    pub optional_non_null_object: Option<TestOptionalNullabilityV1OptionalNonNullObject>,"
        ),
        "optional non-null object must omit None"
    );
    assert!(
        types.contains(
            "#[serde(skip_serializing_if = \"Option::is_none\")]\n    pub nested_optional_non_null: Option<String>,"
        ),
        "nested optional non-null must omit None"
    );
    assert!(
        types.contains("pub optional_nullable: Option<String>,"),
        "optional nullable field must remain Option"
    );
    assert!(
        !types.contains("skip_serializing_if = \"Option::is_none\"\n    pub optional_nullable")
            && !types.lines().collect::<Vec<_>>().windows(2).any(|window| {
                window[0].contains("skip_serializing_if")
                    && window[1].contains("pub optional_nullable")
            }),
        "optional nullable must NOT skip (None stays explicit null)"
    );
    assert!(
        types.contains("pub required_nullable: Option<String>,"),
        "required nullable field must remain Option"
    );
    assert!(
        !types.lines().collect::<Vec<_>>().windows(2).any(|window| {
            window[0].contains("skip_serializing_if") && window[1].contains("pub required_nullable")
        }),
        "required nullable must NOT skip (None stays explicit null)"
    );
    assert!(
        types.contains("pub required_present: String,"),
        "required non-null must not be Option"
    );

    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn validate_example_actor() {
    // validate CLI expects a bare instance document (not the examples wrapper envelope).
    let bare = sample_actor_path();
    let (c, o, e) = run_tool(&[
        "validate",
        "--schema",
        "https://schemas.shittim.local/v1/common/actor.json",
        "--instance",
        bare.to_str().expect("utf8"),
    ]);
    assert_eq!(c, 0, "validate failed: {o}\n{e}");
}

fn sample_actor_path() -> PathBuf {
    let dir = std::env::temp_dir().join("shittim-schema-tool-tests");
    std::fs::create_dir_all(&dir).expect("tmpdir");
    let path = dir.join("actor.json");
    std::fs::write(
        &path,
        r#"{
  "schema_version": 1,
  "revision": 1,
  "id": "actor-local-user-1",
  "kind": "known_user",
  "source": "actor-source://local/desktop",
  "authentication_level": "unauthenticated",
  "confidence": null
}"#,
    )
    .expect("write actor");
    path
}

#[test]
fn task_create_fixture_validates_and_hashes_through_cli() {
    let fixture_path = repo_root().join("schemas/fixtures/kcp/task_create_normalized_hash.v1.json");
    let fixture = read_json(&fixture_path);
    let temp = std::env::temp_dir().join(format!(
        "shittim-task-create-cli-fixture-{}",
        std::process::id()
    ));
    if temp.exists() {
        std::fs::remove_dir_all(&temp).expect("remove old task.create CLI fixture dir");
    }
    std::fs::create_dir_all(&temp).expect("create task.create CLI fixture dir");

    let normalized_payload_path = temp.join("normalized_payload.json");
    let idempotency_projection_path = temp.join("idempotency_projection.json");
    write_json(&normalized_payload_path, &fixture["normalized_payload"]);
    write_json(
        &idempotency_projection_path,
        &fixture["idempotency_projection"],
    );

    let (code, stdout, stderr) = run_tool(&[
        "validate",
        "--schema",
        "https://schemas.shittim.local/v1/kcp/task_create_request.json",
        "--instance",
        normalized_payload_path
            .to_str()
            .expect("UTF-8 payload path"),
    ]);
    assert_eq!(code, 0, "payload validation failed: {stdout}\n{stderr}");

    for (path, expected_hash_field) in [
        (&normalized_payload_path, "receipt_content_hash"),
        (&idempotency_projection_path, "idempotency_projection_hash"),
    ] {
        let (code, stdout, stderr) = run_tool(&[
            "canonicalize",
            path.to_str().expect("UTF-8 canonicalize path"),
            "--hash",
        ]);
        assert_eq!(code, 0, "canonicalize failed: {stdout}\n{stderr}");
        assert_eq!(
            stdout.trim(),
            fixture[expected_hash_field]
                .as_str()
                .expect("fixture hash string"),
            "CLI hash mismatch for {expected_hash_field}"
        );
    }

    std::fs::remove_dir_all(temp).expect("clean task.create CLI fixture dir");
}

#[test]
fn canonicalize_hash_empty_object() {
    let dir = std::env::temp_dir().join("shittim-schema-tool-tests");
    std::fs::create_dir_all(&dir).expect("tmpdir");
    let path = dir.join("empty.json");
    std::fs::write(&path, "{}").expect("write");
    let (c, o, e) = run_tool(&["canonicalize", path.to_str().expect("utf8"), "--hash"]);
    assert_eq!(c, 0, "canonicalize failed: {o}\n{e}");
    assert_eq!(
        o.trim(),
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
}

#[test]
fn check_rejects_meta_schema_invalid_document() {
    let temp = std::env::temp_dir().join(format!("shittim-invalid-schema-{}", std::process::id()));
    if temp.exists() {
        std::fs::remove_dir_all(&temp).expect("remove old temp repo");
    }
    copy_tree(&repo_root(), &temp);
    let actor_path = temp.join("schemas/source/common/actor.v1.json");
    let mut actor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&actor_path).expect("read actor schema"))
            .expect("parse actor schema");
    actor["required"] = serde_json::json!("schema_version");
    write_json(&actor_path, &actor);

    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0, "meta-schema-invalid document must fail check");
    assert!(
        stderr.contains("meta-schema") || stderr.contains("required"),
        "{stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

fn temporary_repo(label: &str) -> PathBuf {
    let _guard = production_repo_generate_guard();
    let temp = std::env::temp_dir().join(format!("shittim-{label}-{}", std::process::id()));
    if temp.exists() {
        std::fs::remove_dir_all(&temp).expect("remove old temp repo");
    }
    copy_tree(&repo_root(), &temp);
    temp
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read JSON file"))
        .expect("parse JSON file")
}

fn write_json(path: &Path, value: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create JSON parent");
    }
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize JSON");
    bytes.push(b'\n');
    std::fs::write(path, bytes).expect("write JSON file");
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
    entry["source_sha256"] = serde_json::json!(hex::encode(Sha256::digest(bytes)));
    let mut baseline_bytes = serde_json::to_vec_pretty(&baseline).expect("serialize baseline");
    baseline_bytes.push(b'\n');
    std::fs::write(baseline_path, baseline_bytes)
        .expect("update retained baseline hash for coherent test fixture mutation");
}

fn mutate_command_envelope(temp: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
    let path = temp.join("schemas/source/kcp/command_envelope.v1.json");
    let mut envelope = read_json(&path);
    mutate(&mut envelope);
    write_json(&path, &envelope);
}

fn assert_generate_fails(temp: &Path, expected: &str) {
    let (code, stdout, stderr) = run_tool_for_root(&["generate"], temp);
    assert_ne!(code, 0, "generate unexpectedly passed: {stdout}");
    assert!(
        stderr
            .to_ascii_lowercase()
            .contains(&expected.to_ascii_lowercase()),
        "expected {expected:?} in: {stderr}"
    );
}

fn assert_registry_load_fails_for_all_commands(temp: &Path, expected: &str) {
    let instance = temp.join("instance.json");
    write_json(&instance, &serde_json::json!({}));
    for args in [
        vec!["generate"],
        vec!["check"],
        vec![
            "validate",
            "--schema",
            "https://schemas.shittim.local/v1/common/actor.json",
            "--instance",
            instance.to_str().expect("UTF-8 instance path"),
        ],
    ] {
        let (code, stdout, stderr) = run_tool_for_root(&args, temp);
        assert_ne!(code, 0, "{args:?} unexpectedly passed: {stdout}");
        assert!(
            stderr.contains(expected),
            "expected {expected:?} for {args:?} in: {stderr}"
        );
    }
}

fn copy_tree(source: &Path, target: &Path) {
    for entry in walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            !entry.path().components().any(|part| {
                matches!(
                    part.as_os_str().to_str(),
                    Some("target" | "node_modules" | ".git")
                )
            }) && !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".schema-tool-")
                && !entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".schema-tool-stage-")
                && !entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".schema-tool-backup-")
        })
    {
        let relative = entry.path().strip_prefix(source).expect("relative path");
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&destination).expect("create copied directory");
        } else if entry.file_type().is_file() {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).expect("create copied parent");
            }
            // Retry once: parallel temp-repo copies can race with concurrent test cleanup.
            if let Err(first) = std::fs::copy(entry.path(), &destination) {
                std::fs::create_dir_all(destination.parent().unwrap_or(target))
                    .expect("recreate parent");
                std::fs::copy(entry.path(), &destination).unwrap_or_else(|second| {
                    panic!(
                        "copy file {} -> {}: first={first}, second={second}",
                        entry.path().display(),
                        destination.display()
                    )
                });
            }
        }
    }
}

#[test]
fn manifest_unique_ids_enforced_by_check() {
    let temp = temporary_repo("manifest-duplicate-id");
    let actor_id = "https://schemas.shittim.local/v1/common/actor.json";
    let dup_source = "schemas/source/common/actor_dup.v1.json";
    let original = temp.join("schemas/source/common/actor.v1.json");
    std::fs::copy(&original, temp.join(dup_source)).expect("copy actor source under new path");

    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let original_entry = manifest["schemas"]
        .as_array()
        .expect("schemas")
        .iter()
        .find(|entry| entry["id"].as_str() == Some(actor_id))
        .cloned()
        .expect("actor manifest entry");
    let mut duplicate = original_entry;
    duplicate["source"] = serde_json::json!(dup_source);
    manifest["schemas"]
        .as_array_mut()
        .expect("schemas")
        .push(duplicate);
    write_json(&manifest_path, &manifest);

    let (code, stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0, "duplicate $id must fail check: {stdout}\n{stderr}");
    assert!(
        stderr.contains("duplicate $id in manifest"),
        "expected exact duplicate $id topic: {stderr}"
    );
    assert!(
        stderr.contains(actor_id),
        "duplicate $id error must name the colliding id: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn generation_targets_accept_rust_typescript_and_both() {
    for targets in [
        serde_json::json!(["rust"]),
        serde_json::json!(["typescript"]),
        serde_json::json!(["rust", "typescript"]),
    ] {
        let temp = temporary_repo(&format!(
            "targets-accept-{}",
            targets
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>()
                .join("-")
        ));
        set_all_generation_targets(&temp, &targets);
        // typescript-only / both currently fail later at unimplemented TS codegen, but
        // manifest load + target validation itself must accept the list shape first.
        // For accept of load path we only need check's first stages; use a unit-style
        // generate attempt and distinguish validation vs unimplemented errors.
        let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
        let lower = stderr.to_ascii_lowercase();
        let only_rust = targets == serde_json::json!(["rust"]);
        if only_rust {
            assert_eq!(code, 0, "rust-only must generate: {stderr}");
        } else {
            // accepted by target validation; fails because TS renderer is not implemented
            // OR succeeds only if no TS path is taken — must not fail on empty/duplicate/order.
            assert!(
                lower.contains("typescript") || lower.contains("not implemented"),
                "expected unimplemented typescript path, got: {stderr}"
            );
            assert_ne!(code, 0);
        }
        std::fs::remove_dir_all(temp).expect("clean temp repo");
    }
}

#[test]
fn generation_targets_reject_empty_duplicate_reverse_unknown() {
    let cases: Vec<(&str, serde_json::Value, &str)> = vec![
        ("empty", serde_json::json!([]), "non-empty"),
        (
            "duplicate",
            serde_json::json!(["rust", "rust"]),
            "duplicate",
        ),
        (
            "reverse",
            serde_json::json!(["typescript", "rust"]),
            "canonical order",
        ),
        ("unknown", serde_json::json!(["python"]), "unknown"),
    ];
    for (label, targets, expected) in cases {
        let temp = temporary_repo(&format!("targets-reject-{label}"));
        set_all_generation_targets(&temp, &targets);
        let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
        assert_ne!(code, 0, "{label} must fail");
        assert!(
            stderr
                .to_ascii_lowercase()
                .contains(&expected.to_ascii_lowercase())
                || stderr.to_ascii_lowercase().contains("unknown variant")
                || stderr.to_ascii_lowercase().contains("python"),
            "expected {expected:?} in: {stderr}"
        );
        std::fs::remove_dir_all(temp).expect("clean temp repo");
    }
}

#[test]
fn generation_target_closure_rejects_missing_dependency_target() {
    let temp = temporary_repo("targets-closure-missing");
    let envelope_id = "https://schemas.shittim.local/v1/kcp/command_envelope.json";
    let stop_activate_id = "https://schemas.shittim.local/v1/kcp/stop_activate_request.json";
    // Target command envelope with typescript while $ref/payload dependencies stay rust-only.
    set_generation_targets_for_id(
        &temp,
        envelope_id,
        serde_json::json!(["rust", "typescript"]),
    );

    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0, "closure missing must fail: {stderr}");
    assert!(
        stderr.contains("generation target closure error"),
        "expected exact closure topic: {stderr}"
    );
    assert!(
        stderr.contains(envelope_id),
        "closure error must name the from schema: {stderr}"
    );
    assert!(
        stderr.contains(stop_activate_id),
        "closure error must name the missing-target dependency: {stderr}"
    );
    assert!(
        stderr.contains("typescript"),
        "closure error must name the missing target: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn relative_external_ref_missing_target_fails_closure() {
    let temp = temporary_repo("relative-external-missing-target");
    let parent_id = "https://schemas.shittim.local/kcp/rel_parent/v1";
    let child_id = "https://schemas.shittim.local/kcp/rel_child/v1";
    let parent_source = "schemas/source/kcp/rel_parent.v1.json";
    let child_source = "schemas/source/kcp/rel_child.v1.json";

    write_json(
        &temp.join(child_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": child_id,
            "title": "RelChildV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "label"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "label": {"type": "string"}
            }
        }),
    );
    // Parent uses a *relative* external $ref (not absolute id) to the sibling schema.
    write_json(
        &temp.join(parent_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": parent_id,
            "title": "RelParentV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "child"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "child": {"$ref": "../rel_child/v1"}
            }
        }),
    );

    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    schemas.push(serde_json::json!({
        "id": parent_id,
        "title": "RelParentV1",
        "version": 1,
        "source": parent_source,
        "component": "kcp",
        "kind": "object",
        "compatibility": "new-contract",
        "generation_targets": ["rust"],
        "schema_version_field": "schema_version"
    }));
    // Child is on the registry but deliberately missing the rust target.
    schemas.push(serde_json::json!({
        "id": child_id,
        "title": "RelChildV1",
        "version": 1,
        "source": child_source,
        "component": "kcp",
        "kind": "object",
        "compatibility": "new-contract",
        "generation_targets": ["typescript"],
        "schema_version_field": "schema_version"
    }));
    write_json(&manifest_path, &manifest);

    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(
        code, 0,
        "relative external missing target must fail: {stderr}"
    );
    assert!(
        stderr.contains("generation target closure error"),
        "expected exact closure topic: {stderr}"
    );
    assert!(
        stderr.contains(child_id),
        "closure error must name the dependency schema id: {stderr}"
    );
    assert!(
        stderr.contains("rust"),
        "closure error must name the required target: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

fn set_all_generation_targets(temp: &std::path::Path, targets: &serde_json::Value) {
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    for entry in manifest["schemas"]
        .as_array_mut()
        .expect("manifest schemas")
    {
        entry["generation_targets"] = targets.clone();
    }
    write_json(&manifest_path, &manifest);
}

fn set_generation_targets_for_id(temp: &Path, id: &str, targets: serde_json::Value) {
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let entry = manifest["schemas"]
        .as_array_mut()
        .expect("schemas")
        .iter_mut()
        .find(|entry| entry["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("missing schema {id}"));
    entry["generation_targets"] = targets;
    write_json(&manifest_path, &manifest);
}

#[test]
fn typescript_only_fails_before_any_write() {
    let temp = temporary_repo("ts-only-no-partial");
    set_all_generation_targets(&temp, &serde_json::json!(["typescript"]));
    let before = all_generated_artifact_bytes(&temp);
    // Marker file that must remain untouched proves no partial write path ran.
    let marker = temp.join("rust/crates/kernel-contracts/src/generated/DO_NOT_TOUCH");
    std::fs::write(&marker, b"keep").expect("write marker");

    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.to_ascii_lowercase().contains("typescript")
            && stderr.to_ascii_lowercase().contains("not implemented"),
        "expected typescript unimplemented: {stderr}"
    );
    let after = all_generated_artifact_bytes(&temp);
    assert_eq!(before, after, "TS-only must not rewrite any Rust artifact");
    assert_eq!(
        std::fs::read(&marker).expect("marker"),
        b"keep",
        "generate must not touch unrelated files on TS failure"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn both_targets_fail_closed_without_partial_rust_rewrite_when_ts_declared() {
    let temp = temporary_repo("both-targets-no-partial");
    set_all_generation_targets(&temp, &serde_json::json!(["rust", "typescript"]));
    let before = all_generated_artifact_bytes(&temp);
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.to_ascii_lowercase().contains("typescript"),
        "expected typescript unimplemented: {stderr}"
    );
    let after = all_generated_artifact_bytes(&temp);
    assert_eq!(
        before, after,
        "declaring typescript must fail before writing any Rust artifact"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn mixed_target_closure_requires_dependency_on_same_target() {
    let temp = temporary_repo("mixed-target-closure");
    let envelope_id = "https://schemas.shittim.local/v1/kcp/command_envelope.json";
    let stop_activate_id = "https://schemas.shittim.local/v1/kcp/stop_activate_request.json";
    // Keep dependencies rust-only; put command_envelope on rust+typescript — typescript closure fails.
    set_generation_targets_for_id(
        &temp,
        envelope_id,
        serde_json::json!(["rust", "typescript"]),
    );
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0, "mixed-target closure must fail: {stderr}");
    assert!(
        stderr.contains("generation target closure error"),
        "expected exact closure topic: {stderr}"
    );
    assert!(
        stderr.contains(envelope_id),
        "closure error must name the from schema: {stderr}"
    );
    assert!(
        stderr.contains(stop_activate_id),
        "closure error must name the dependency missing typescript: {stderr}"
    );
    assert!(
        stderr.contains("typescript"),
        "closure error must name the missing target: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn schema_node_walker_covers_all_locations_and_skips_instance_data() {
    type SchemaMutation = fn(&mut serde_json::Value);
    let cases: &[(&str, SchemaMutation)] = &[
        ("properties", |root| {
            root["properties"]["walk_test"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("patternProperties", |root| {
            root["patternProperties"] = serde_json::json!({"^x$": {"$dynamicRef": "#bad"}})
        }),
        ("dependentSchemas", |root| {
            root["dependentSchemas"] = serde_json::json!({"x": {"$dynamicRef": "#bad"}})
        }),
        ("$defs", |root| {
            root["$defs"]["walk_test"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("definitions", |root| {
            root["definitions"] = serde_json::json!({"walk_test": {"$dynamicRef": "#bad"}})
        }),
        ("additionalProperties", |root| {
            root["additionalProperties"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("unevaluatedProperties", |root| {
            root["unevaluatedProperties"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("propertyNames", |root| {
            root["propertyNames"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("items", |root| {
            root["items"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("contains", |root| {
            root["contains"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("unevaluatedItems", |root| {
            root["unevaluatedItems"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("contentSchema", |root| {
            root["contentSchema"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("not", |root| {
            root["not"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("if", |root| {
            root["if"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("then", |root| {
            root["then"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("else", |root| {
            root["else"] = serde_json::json!({"$dynamicRef": "#bad"})
        }),
        ("prefixItems", |root| {
            root["prefixItems"] = serde_json::json!([{"$dynamicRef": "#bad"}])
        }),
        ("allOf", |root| {
            root["allOf"] = serde_json::json!([{"$dynamicRef": "#bad"}])
        }),
        ("anyOf", |root| {
            root["anyOf"] = serde_json::json!([{"$dynamicRef": "#bad"}])
        }),
        ("oneOf", |root| {
            root["oneOf"] = serde_json::json!([{"$dynamicRef": "#bad"}])
        }),
    ];

    for (keyword, mutate) in cases {
        let temp = temporary_repo(&format!("schema-walk-{keyword}"));
        let actor_path = temp.join("schemas/source/common/actor.v1.json");
        let mut actor = read_json(&actor_path);
        mutate(&mut actor);
        write_json(&actor_path, &actor);
        assert_registry_load_fails_for_all_commands(&temp, "$dynamicRef");
        std::fs::remove_dir_all(temp).expect("clean temp repo");
    }

    let temp = temporary_repo("schema-walk-instance-values");
    let actor_path = temp.join("schemas/source/common/actor.v1.json");
    let mut actor = read_json(&actor_path);
    actor["const"] = serde_json::json!({"$ref": "https://instance.invalid/const"});
    actor["default"] = serde_json::json!({"$dynamicRef": "instance"});
    actor["examples"] = serde_json::json!([{"$id": "instance"}]);
    actor["enum"] = serde_json::json!([{"$ref": "https://instance.invalid/enum"}]);
    actor["properties"]["$ref"] = serde_json::json!({"type": "string"});
    actor["properties"]["$id"] = serde_json::json!({"type": "string"});
    actor["properties"]["$schema"] = serde_json::json!({"type": "string"});
    actor["properties"]["$dynamicRef"] = serde_json::json!({"type": "string"});
    write_json(&actor_path, &actor);
    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0, "shape changes may fail restricted codegen");
    assert!(
        !stderr.contains("instance.invalid")
            && !stderr.contains("unsupported JSON Schema identity/ref keyword"),
        "instance data/property names must not be audited as Schema keywords: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn reserved_property_name_value_still_enforces_real_ref_gate() {
    let temp = temporary_repo("reserved-property-real-ref");
    let actor_path = temp.join("schemas/source/common/actor.v1.json");
    let mut actor = read_json(&actor_path);
    actor["properties"]["$ref"] = serde_json::json!({
        "$ref": "https://schemas.shittim.local/v1/task/action_request.json"
    });
    write_json(&actor_path, &actor);
    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("component") && stderr.contains("allowed"),
        "real ref in property named $ref must pass through component gate: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn deep_local_fragment_external_ref_joins_target_closure() {
    let temp = temporary_repo("deep-fragment-external");
    // Create parent (rust) with $defs that $ref an external child; child must list rust.
    let parent_id = "https://schemas.shittim.local/kcp/deep_parent/v1";
    let child_id = "https://schemas.shittim.local/kcp/deep_child/v1";
    let parent_source = "schemas/source/kcp/deep_parent.v1.json";
    let child_source = "schemas/source/kcp/deep_child.v1.json";
    write_json(
        &temp.join(child_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": child_id,
            "title": "DeepChildV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "label"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "label": {"type": "string"}
            }
        }),
    );
    write_json(
        &temp.join(parent_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": parent_id,
            "title": "DeepParentV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "nested"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "nested": {"$ref": "#/$defs/wrapper"}
            },
            "$defs": {
                "wrapper": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["child"],
                    "properties": {
                        "child": {"$ref": child_id}
                    }
                }
            }
        }),
    );
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    schemas.push(serde_json::json!({
        "id": parent_id,
        "title": "DeepParentV1",
        "version": 1,
        "source": parent_source,
        "component": "kcp",
        "kind": "kcp_request",
        "compatibility": "new-contract",
        "generation_targets": ["rust"],
        "schema_version_field": "schema_version"
    }));
    schemas.push(serde_json::json!({
        "id": child_id,
        "title": "DeepChildV1",
        "version": 1,
        "source": child_source,
        "component": "kcp",
        "kind": "kcp_request",
        "compatibility": "new-contract",
        "generation_targets": ["rust"],
        "schema_version_field": "schema_version"
    }));
    write_json(&manifest_path, &manifest);

    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(
        code, 0,
        "deep fragment external ref must generate: {stderr}"
    );
    let types =
        std::fs::read_to_string(temp.join("rust/crates/kernel-contracts/src/generated/types.rs"))
            .expect("types");
    assert!(types.contains("pub struct DeepParentV1"));
    assert!(types.contains("pub struct DeepChildV1"));
    assert!(types.contains("pub nested: DeepParentV1Nested"));

    // Child without rust target must fail closure with exact topic + ids.
    set_generation_targets_for_id(&temp, child_id, serde_json::json!(["typescript"]));
    // Parent stays rust-only; child ts-only => rust closure error (before any TS render).
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(
        code, 0,
        "deep fragment missing rust target must fail: {stderr}"
    );
    assert!(
        stderr.contains("generation target closure error"),
        "expected exact closure topic: {stderr}"
    );
    assert!(
        stderr.contains(child_id),
        "closure error must name the dependency schema id: {stderr}"
    );
    assert!(
        stderr.contains("rust"),
        "closure error must name the required target: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn cyclic_refs_do_not_hang_and_generate() {
    let temp = temporary_repo("cyclic-refs");
    // Self-cycle via optional property $ref to self is enough to exercise seen-set.
    // Restricted codegen forbids most recursive shapes if they aren't simple $ref roots.
    // Use two schemas that $ref each other at the root property level.
    let a_id = "https://schemas.shittim.local/kcp/cycle_a/v1";
    let b_id = "https://schemas.shittim.local/kcp/cycle_b/v1";
    let a_source = "schemas/source/kcp/cycle_a.v1.json";
    let b_source = "schemas/source/kcp/cycle_b.v1.json";
    write_json(
        &temp.join(a_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": a_id,
            "title": "CycleAV1",
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
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": b_id,
            "title": "CycleBV1",
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
    for (id, title, source) in [(a_id, "CycleAV1", a_source), (b_id, "CycleBV1", b_source)] {
        schemas.push(serde_json::json!({
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

    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "cyclic refs must not hang: {stderr}");
    let types =
        std::fs::read_to_string(temp.join("rust/crates/kernel-contracts/src/generated/types.rs"))
            .expect("types");
    assert!(types.contains("pub struct CycleAV1"));
    assert!(types.contains("pub struct CycleBV1"));
    // Direct mutual recursion must be boxed by the recursive layout pass.
    assert!(
        types.contains("Box<CycleAV1>") && types.contains("Box<CycleBV1>"),
        "mutual recursion must insert Box: {types}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn envelope_payload_closure_requires_payload_target_via_unified_conditional_ir() {
    let temp = temporary_repo("envelope-payload-closure");
    let envelope_id = "https://schemas.shittim.local/v1/kcp/command_envelope.json";
    let payload_id = "https://schemas.shittim.local/v1/kcp/task_create_request.json";
    // Isolate retained v1 Envelope conditional-ref closure from MethodVersionBinding
    // common-target checks (production bindings are non-empty after slice 3a).
    // Production CLI stage gate requires the complete binding set, so this case
    // uses the explicit non-production library profile with empty bindings.
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["method_version_bindings"] = serde_json::json!([]);
    write_json(&manifest_path, &manifest);
    // Remove rust target from task_create_request while command_envelope stays rust.
    set_generation_targets_for_id(&temp, payload_id, serde_json::json!(["typescript"]));
    let registry = SchemaRegistry::load(&temp).expect("load stripped-binding registry");
    let profile = schema_tool::SyntheticRegistry::new(&registry).expect("synthetic profile");
    let error = schema_tool::codegen::plan_artifacts(profile)
        .expect_err("payload missing rust target must fail")
        .to_string();
    assert!(
        error.contains("generation target closure error"),
        "expected exact closure topic: {error}"
    );
    assert!(
        error.contains(envelope_id),
        "closure error must name the envelope (from) schema: {error}"
    );
    assert!(
        error.contains(payload_id),
        "closure error must name the payload dependency: {error}"
    );
    assert!(
        error.contains("rust"),
        "closure error must name the required target: {error}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn non_event_kcp_conditional_mapping_shape_drift_fails_closed() {
    for (label, mutate) in [
        ("branch-extra-key", "branch"),
        ("if-extra-key", "if"),
        ("then-extra-key", "then"),
        ("payload-annotation-sibling", "payload"),
    ] {
        let temp = temporary_repo(label);
        mutate_command_envelope(&temp, |envelope| {
            let branch = &mut envelope["allOf"][0];
            match mutate {
                "branch" => branch["description"] = serde_json::json!("shape drift"),
                "if" => branch["if"]["type"] = serde_json::json!("object"),
                "then" => branch["then"]["required"] = serde_json::json!(["payload"]),
                "payload" => {
                    branch["then"]["properties"]["payload"]["description"] =
                        serde_json::json!("not a pure whole-root ref")
                }
                _ => unreachable!(),
            }
        });
        assert_generate_fails(&temp, "exact keys");
        std::fs::remove_dir_all(temp).expect("clean temp repo");
    }
}

#[test]
fn off_manifest_source_file_fails_load() {
    let temp = temporary_repo("off-manifest");
    write_json(
        &temp.join("schemas/source/kcp/not_in_manifest.v1.json"),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "https://schemas.shittim.local/kcp/not_in_manifest/v1",
            "title": "NotInManifestV1",
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
    );
    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("not listed in manifest") || stderr.contains("not_in_manifest"),
        "expected off-manifest error: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn same_title_cannot_bind_two_component_native_ids() {
    // Component-native stem is title-derived. Two entries cannot share one title
    // while claiming different IDs/sources; the hard gate fails at load.
    let temp = temporary_repo("title-collision");
    let a_id = "https://schemas.shittim.local/kcp/title_collision_a/v1";
    let b_id = "https://schemas.shittim.local/kcp/title_collision_b/v1";
    let a_source = "schemas/source/kcp/title_collision_a.v1.json";
    let b_source = "schemas/source/kcp/title_collision_b.v1.json";
    for (id, source) in [(a_id, a_source), (b_id, b_source)] {
        write_json(
            &temp.join(source),
            &serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": id,
                "title": "SharedTitleV1",
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version", "value"],
                "properties": {
                    "schema_version": {"type": "integer", "const": 1},
                    "value": {"type": "string"}
                }
            }),
        );
    }
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    for (id, source) in [(a_id, a_source), (b_id, b_source)] {
        schemas.push(serde_json::json!({
            "id": id,
            "title": "SharedTitleV1",
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

    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        error.contains("component-native") || error.contains("title"),
        "expected component-native title/id mismatch: {error}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn nested_hint_collision_fails_with_both_identities() {
    let temp = temporary_repo("nested-hint-collision");
    // One schema with two properties whose PascalCase path both become XyzItem under the
    // same parent title prefix is hard; instead use two roots that both emit a nested
    // type with the same logical title via identical nested property names under same title.
    // Simpler: two schemas titled differently but each has property "status" with inline
    // string enum — names Status collide only if logical titles are empty. With titles
    // RootA/RootB they become RootAStatus/RootBStatus.
    // Force collision by giving both schemas the same title and same nested property path
    // — that is the same-title case. For nested-only collision across different roots with
    // different titles, hand-craft two schemas that both produce logical_title "SharedNested"
    // via property path Parent + SharedNested field name under different parents with titles
    // that yield the same concatenation... Use identical titles for two different $ids.
    // Covered by same_title_collision. Here force two nested const types under one schema
    // with property names that PascalCase collide ("status" and "Status") — JSON property
    // names are case-sensitive; "status" and "Status" both become Status under the parent.
    let schema_id = "https://schemas.shittim.local/kcp/nested_hint_collision/v1";
    let source = "schemas/source/kcp/nested_hint_collision.v1.json";
    write_json(
        &temp.join(source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "NestedHintCollisionV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "status", "Status"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "status": {"type": "string", "enum": ["a", "b"]},
                "Status": {"type": "string", "enum": ["c", "d"]}
            }
        }),
    );
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"]
        .as_array_mut()
        .expect("schemas")
        .push(serde_json::json!({
            "id": schema_id,
            "title": "NestedHintCollisionV1",
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "object",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }));
    write_json(&manifest_path, &manifest);

    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("collision"),
        "expected nested hint collision: {stderr}"
    );
    assert!(
        stderr.contains(schema_id),
        "collision must mention schema identity: {stderr}"
    );
    assert!(
        stderr.contains("/properties/status") && stderr.contains("/properties/Status")
            || stderr.contains("NestedHintCollisionStatus"),
        "collision must identify both property paths or shared name: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn check_rejects_extra_file_under_generated_root() {
    let temp = temporary_repo("extra-generated-file");
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "setup generate: {stderr}");
    let extra = temp.join("rust/crates/kernel-contracts/src/generated/extra.rs");
    std::fs::write(&extra, b"// extra\n").expect("write extra");
    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("extra") || stderr.contains("mismatch"),
        "expected extra file detection: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn check_rejects_unexpected_directory_under_generated_root() {
    let temp = temporary_repo("extra-generated-dir");
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "setup generate: {stderr}");
    let nested = temp.join("rust/crates/kernel-contracts/src/generated/nested");
    std::fs::create_dir_all(&nested).expect("mkdir nested");
    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("directory")
            || stderr.contains("mismatch")
            || stderr.contains("unexpected"),
        "expected unexpected directory detection: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[cfg(unix)]
#[test]
fn check_rejects_symlink_under_generated_root() {
    let temp = temporary_repo("generated-symlink");
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "setup generate: {stderr}");
    let target = temp.join("rust/crates/kernel-contracts/src/generated/types.rs");
    let link = temp.join("rust/crates/kernel-contracts/src/generated/types.link.rs");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(
        code, 0,
        "symlink under generated root must fail check: {stderr}"
    );
    assert!(
        stderr.contains("symlink") || stderr.contains("mismatch") || stderr.contains("unexpected"),
        "expected symlink detection: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

/// Non-unix platforms cannot create symlinks portably; still assert that a regular
/// unexpected file under the generated root is rejected (same file-set oracle path).
#[cfg(not(unix))]
#[test]
fn check_rejects_extra_regular_file_under_generated_root_on_non_unix() {
    let temp = temporary_repo("generated-extra-file-non-unix");
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "setup generate: {stderr}");
    let extra = temp.join("rust/crates/kernel-contracts/src/generated/types.link.rs");
    std::fs::write(&extra, b"// not a planned artifact\n").expect("write extra");
    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0, "extra regular file must fail check: {stderr}");
    assert!(
        stderr.contains("extra") || stderr.contains("mismatch") || stderr.contains("unexpected"),
        "expected extra file detection: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn shared_defs_two_fields_same_ref_generate_two_rust_types() {
    // Production PolicyRule already has created_by/updated_by -> same $defs node.
    let (code, _stdout, stderr) = run_tool(&["generate"]);
    assert_eq!(code, 0, "generate: {stderr}");
    let types = std::fs::read_to_string(
        repo_root().join("rust/crates/kernel-contracts/src/generated/types.rs"),
    )
    .expect("types");
    assert!(types.contains("pub struct PolicyRuleCreatedBy"));
    assert!(types.contains("pub struct PolicyRuleUpdatedBy"));
    assert!(types.contains("pub created_by: PolicyRuleCreatedBy"));
    assert!(types.contains("pub updated_by: PolicyRuleUpdatedBy"));
}

#[test]
fn response_envelope_remains_untyped() {
    let (code, _stdout, stderr) = run_tool(&["generate"]);
    assert_eq!(code, 0, "generate: {stderr}");
    let typed = std::fs::read_to_string(
        repo_root().join("rust/crates/kernel-contracts/src/generated/typed.rs"),
    )
    .expect("typed");
    assert!(!typed.contains("TypedKcpResponseEnvelope"));
    assert!(typed.contains("TypedKcpCommandEnvelope"));
}

#[test]
fn string_enum_all_preserves_schema_declaration_order_not_lexicographic() {
    let temp = temporary_repo("string-enum-order");
    let schema_id = "https://schemas.shittim.local/kcp/test_string_enum_order/v1";
    let source = "schemas/source/kcp/test_string_enum_order.v1.json";
    write_json(
        &temp.join(source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "TestStringEnumOrderV1",
            "type": "string",
            "enum": ["zeta", "alpha", "middle"]
        }),
    );
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"]
        .as_array_mut()
        .expect("manifest schemas")
        .push(serde_json::json!({
            "id": schema_id,
            "title": "TestStringEnumOrderV1",
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "kcp_request",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": null
        }));
    write_json(&manifest_path, &manifest);

    let (code, stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "generate failed: {stdout}\n{stderr}");
    let types =
        std::fs::read_to_string(temp.join("rust/crates/kernel-contracts/src/generated/types.rs"))
            .expect("types");

    let enum_idx = types
        .find("pub enum TestStringEnumOrderV1")
        .expect("enum declaration");
    let enum_block = &types[enum_idx..];
    let zeta = enum_block.find("Zeta").expect("Zeta variant");
    let alpha = enum_block.find("Alpha").expect("Alpha variant");
    let middle = enum_block.find("Middle").expect("Middle variant");
    assert!(
        zeta < alpha && alpha < middle,
        "variants must keep schema order zeta/alpha/middle: {enum_block}"
    );

    let all_idx = types
        .find("impl TestStringEnumOrderV1")
        .expect("impl block");
    let impl_block = &types[all_idx..];
    let all_start = impl_block
        .find("pub const ALL: &'static [Self] = &[")
        .expect("ALL const");
    let all_end = impl_block[all_start..].find("];").expect("ALL end");
    let all_block = &impl_block[all_start..all_start + all_end];
    let all_zeta = all_block.find("Self::Zeta").expect("ALL Zeta");
    let all_alpha = all_block.find("Self::Alpha").expect("ALL Alpha");
    let all_middle = all_block.find("Self::Middle").expect("ALL Middle");
    assert!(
        all_zeta < all_alpha && all_alpha < all_middle,
        "ALL must keep schema order: {all_block}"
    );

    let as_str_start = impl_block.find("pub fn as_str").expect("as_str");
    let as_str_block = &impl_block[as_str_start..];
    let wire_zeta = as_str_block.find("\"zeta\"").expect("wire zeta");
    let wire_alpha = as_str_block.find("\"alpha\"").expect("wire alpha");
    let wire_middle = as_str_block.find("\"middle\"").expect("wire middle");
    assert!(
        wire_zeta < wire_alpha && wire_alpha < wire_middle,
        "as_str must keep schema order: {as_str_block}"
    );

    assert!(
        types.contains("fn test_string_enum_order_v1_string_enum_contract"),
        "auto contract test must be projected for the synthetic enum"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn string_enum_wire_collision_fails_with_type_wire_and_variant() {
    let temp = temporary_repo("string-enum-wire-collision");
    let schema_id = "https://schemas.shittim.local/kcp/test_string_enum_collision/v1";
    let source = "schemas/source/kcp/test_string_enum_collision.v1.json";
    write_json(
        &temp.join(source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "TestStringEnumCollisionV1",
            "type": "string",
            "enum": ["a-b", "a_b"]
        }),
    );
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"]
        .as_array_mut()
        .expect("manifest schemas")
        .push(serde_json::json!({
            "id": schema_id,
            "title": "TestStringEnumCollisionV1",
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "kcp_request",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": null
        }));
    write_json(&manifest_path, &manifest);

    let (code, stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0, "generate must fail on wire collision: {stdout}");
    let lower = stderr.to_ascii_lowercase();
    assert!(
        lower.contains("collision") || lower.contains("variant"),
        "expected collision wording: {stderr}"
    );
    assert!(
        stderr.contains("TestStringEnumCollisionV1") || stderr.contains(schema_id),
        "error must include type identity: {stderr}"
    );
    assert!(
        stderr.contains("a-b") && stderr.contains("a_b"),
        "error must include both wire values: {stderr}"
    );
    assert!(
        stderr.contains("AB") || lower.contains("variant"),
        "error must include mapped variant: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn nullable_string_enum_all_excludes_null_and_keeps_option_wire() {
    let temp = temporary_repo("nullable-string-enum-all");
    let schema_id = "https://schemas.shittim.local/kcp/test_nullable_string_enum/v1";
    let source = "schemas/source/kcp/test_nullable_string_enum.v1.json";
    write_json(
        &temp.join(source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "TestNullableStringEnumV1",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "proposer"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "proposer": {
                    "type": ["string", "null"],
                    "enum": ["user", "companion", "system", null]
                }
            }
        }),
    );
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"]
        .as_array_mut()
        .expect("manifest schemas")
        .push(serde_json::json!({
            "id": schema_id,
            "title": "TestNullableStringEnumV1",
            "version": 1,
            "source": source,
            "component": "kcp",
            "kind": "kcp_request",
            "compatibility": "new-contract",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version"
        }));
    write_json(&manifest_path, &manifest);

    let (code, stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "generate failed: {stdout}\n{stderr}");
    let types =
        std::fs::read_to_string(temp.join("rust/crates/kernel-contracts/src/generated/types.rs"))
            .expect("types");

    assert!(
        types.contains("pub proposer: Option<TestNullableStringEnumV1Proposer>"),
        "nullable enum field must stay Option: {types}"
    );
    let enum_idx = types
        .find("pub enum TestNullableStringEnumV1Proposer")
        .expect("proposer enum");
    let enum_end = types[enum_idx..].find("\n}\n").expect("enum end");
    let enum_block = &types[enum_idx..enum_idx + enum_end];
    assert!(
        !enum_block.lines().any(|line| {
            let trimmed = line.trim();
            trimmed == "Null,"
                || trimmed == "Null"
                || trimmed.contains("rename = \"null\"")
                || trimmed.contains("=> \"null\"")
        }),
        "string enum variants must exclude null: {enum_block}"
    );

    let impl_idx = types
        .find("impl TestNullableStringEnumV1Proposer")
        .expect("impl");
    let impl_slice = &types[impl_idx..];
    let all_start = impl_slice
        .find("pub const ALL: &'static [Self] = &[")
        .expect("ALL");
    let all_end = impl_slice[all_start..].find("];").expect("ALL end");
    let all_block = &impl_slice[all_start..all_start + all_end];
    assert!(all_block.contains("Self::User"));
    assert!(all_block.contains("Self::Companion"));
    assert!(all_block.contains("Self::System"));
    assert!(
        !all_block.to_ascii_lowercase().contains("null"),
        "ALL must exclude null: {all_block}"
    );
    // required nullable Option must NOT skip-serialize None
    assert!(
        !types.lines().collect::<Vec<_>>().windows(2).any(|window| {
            window[0].contains("skip_serializing_if") && window[1].contains("pub proposer")
        }),
        "required nullable proposer must serialize None as explicit null"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn temporary_task_status_variant_is_appended_to_generated_all_without_renderer_change() {
    let temp = temporary_repo("task-status-append-variant");
    let status_path = temp.join("schemas/source/common/task_status.v1.json");
    let mut status = read_json(&status_path);
    status["enum"]
        .as_array_mut()
        .expect("task_status enum")
        .push(serde_json::json!("awaiting_external"));
    write_json(&status_path, &status);

    let (code, stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "generate failed: {stdout}\n{stderr}");
    let types =
        std::fs::read_to_string(temp.join("rust/crates/kernel-contracts/src/generated/types.rs"))
            .expect("types");
    let task_impl = types
        .split("impl TaskStatus")
        .nth(1)
        .expect("TaskStatus impl");
    let all_start = task_impl
        .find("pub const ALL: &'static [Self] = &[")
        .expect("ALL");
    let all_end = task_impl[all_start..].find("];").expect("ALL end");
    let all_block = &task_impl[all_start..all_start + all_end];
    assert!(
        all_block.trim_end().ends_with("Self::AwaitingExternal,")
            || all_block.contains("Self::AwaitingExternal"),
        "new variant must appear in ALL: {all_block}"
    );
    // Must be last among ALL members.
    let last_self = all_block.rfind("Self::").expect("last Self");
    assert!(
        all_block[last_self..].starts_with("Self::AwaitingExternal"),
        "appended schema variant must be ALL tail: {all_block}"
    );
    assert!(
        types.contains("Self::AwaitingExternal => \"awaiting_external\""),
        "as_str must include appended wire"
    );
    assert!(
        types.contains("#[serde(rename = \"awaiting_external\")]\n    AwaitingExternal,"),
        "variant must be generated"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}
