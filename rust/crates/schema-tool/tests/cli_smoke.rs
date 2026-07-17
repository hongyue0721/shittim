//! schema-tool CLI smoke tests (generate twice, check, validate examples).

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
    run_tool_for_root(args, &repo_root())
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
        catalog_text.contains("KCP_V1_METHODS"),
        "missing method catalog"
    );
}

#[test]
fn conditional_payload_bindings_follow_schema_without_rust_template_changes() {
    let temp = temporary_repo("dynamic-typed-binding");
    let payload_id = "https://schemas.shittim.local/v1/kcp/test_dynamic_request.json";
    let payload_source = "schemas/source/kcp/test_dynamic_request.v1.json";
    write_json(
        &temp.join(payload_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": payload_id,
            "title": "TestDynamicRequest",
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
            "title": "TestDynamicRequest",
            "version": 1,
            "source": payload_source,
            "domain": "kcp",
            "kind": "kcp_request",
            "compatibility": "test-only",
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
    assert!(typed.contains("TestDynamic(Box<TestDynamicRequest>)"));
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
    let schema_id = "https://schemas.shittim.local/v1/kcp/test_optional_nullability.json";
    let schema_source = "schemas/source/kcp/test_optional_nullability.v1.json";
    write_json(
        &temp.join(schema_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "TestOptionalNullability",
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
            "title": "TestOptionalNullability",
            "version": 1,
            "source": schema_source,
            "domain": "kcp",
            "kind": "kcp_request",
            "compatibility": "test-only",
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
        types.contains("pub struct TestOptionalNullability"),
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
            "#[serde(skip_serializing_if = \"Option::is_none\")]\n    pub optional_non_null_enum: Option<TestOptionalNullabilityOptionalNonNullEnum>,"
        ),
        "optional non-null enum must omit None"
    );
    assert!(
        types.contains(
            "#[serde(skip_serializing_if = \"Option::is_none\")]\n    pub optional_non_null_object: Option<TestOptionalNullabilityOptionalNonNullObject>,"
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
    std::fs::write(
        &actor_path,
        serde_json::to_vec_pretty(&actor).expect("serialize invalid schema"),
    )
    .expect("write invalid schema");

    let (code, _stdout, stderr) = run_tool_for_root(&["check"], &temp);
    assert_ne!(code, 0, "meta-schema-invalid document must fail check");
    assert!(
        stderr.contains("meta-schema") || stderr.contains("required"),
        "{stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

fn temporary_repo(label: &str) -> PathBuf {
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
            })
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
    let parent_id = "https://schemas.shittim.local/v1/kcp/rel_parent.json";
    let child_id = "https://schemas.shittim.local/v1/kcp/rel_child.json";
    let parent_source = "schemas/source/kcp/rel_parent.v1.json";
    let child_source = "schemas/source/kcp/rel_child.v1.json";

    write_json(
        &temp.join(child_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": child_id,
            "title": "RelChild",
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
            "title": "RelParent",
            "type": "object",
            "additionalProperties": false,
            "required": ["schema_version", "child"],
            "properties": {
                "schema_version": {"type": "integer", "const": 1},
                "child": {"$ref": "./rel_child.json"}
            }
        }),
    );

    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let schemas = manifest["schemas"].as_array_mut().expect("schemas");
    schemas.push(serde_json::json!({
        "id": parent_id,
        "title": "RelParent",
        "version": 1,
        "source": parent_source,
        "domain": "kcp",
        "kind": "object",
        "compatibility": "test-only",
        "generation_targets": ["rust"],
        "schema_version_field": "schema_version"
    }));
    // Child is on the registry but deliberately missing the rust target.
    schemas.push(serde_json::json!({
        "id": child_id,
        "title": "RelChild",
        "version": 1,
        "source": child_source,
        "domain": "kcp",
        "kind": "object",
        "compatibility": "test-only",
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
    let types_path = temp.join("rust/crates/kernel-contracts/src/generated/types.rs");
    let before = std::fs::read(&types_path).expect("read types before");
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
    let after = std::fs::read(&types_path).expect("read types after");
    assert_eq!(before, after, "TS-only must not rewrite Rust artifacts");
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
    let types_path = temp.join("rust/crates/kernel-contracts/src/generated/types.rs");
    let before = std::fs::read(&types_path).expect("read types before");
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.to_ascii_lowercase().contains("typescript"),
        "expected typescript unimplemented: {stderr}"
    );
    let after = std::fs::read(&types_path).expect("read types after");
    assert_eq!(
        before, after,
        "declaring typescript must fail before writing any artifact"
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
fn deep_local_fragment_external_ref_joins_target_closure() {
    let temp = temporary_repo("deep-fragment-external");
    // Create parent (rust) with $defs that $ref an external child; child must list rust.
    let parent_id = "https://schemas.shittim.local/v1/kcp/deep_parent.json";
    let child_id = "https://schemas.shittim.local/v1/kcp/deep_child.json";
    let parent_source = "schemas/source/kcp/deep_parent.v1.json";
    let child_source = "schemas/source/kcp/deep_child.v1.json";
    write_json(
        &temp.join(child_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": child_id,
            "title": "DeepChild",
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
            "title": "DeepParent",
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
        "title": "DeepParent",
        "version": 1,
        "source": parent_source,
        "domain": "kcp",
        "kind": "kcp_request",
        "compatibility": "test-only",
        "generation_targets": ["rust"],
        "schema_version_field": "schema_version"
    }));
    schemas.push(serde_json::json!({
        "id": child_id,
        "title": "DeepChild",
        "version": 1,
        "source": child_source,
        "domain": "kcp",
        "kind": "kcp_request",
        "compatibility": "test-only",
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
    assert!(types.contains("pub struct DeepParent"));
    assert!(types.contains("pub struct DeepChild"));
    assert!(types.contains("pub nested: DeepParentNested"));

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
    let a_id = "https://schemas.shittim.local/v1/kcp/cycle_a.json";
    let b_id = "https://schemas.shittim.local/v1/kcp/cycle_b.json";
    let a_source = "schemas/source/kcp/cycle_a.v1.json";
    let b_source = "schemas/source/kcp/cycle_b.v1.json";
    write_json(
        &temp.join(a_source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": a_id,
            "title": "CycleA",
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
            "title": "CycleB",
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
    for (id, title, source) in [(a_id, "CycleA", a_source), (b_id, "CycleB", b_source)] {
        schemas.push(serde_json::json!({
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

    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_eq!(code, 0, "cyclic refs must not hang: {stderr}");
    let types =
        std::fs::read_to_string(temp.join("rust/crates/kernel-contracts/src/generated/types.rs"))
            .expect("types");
    assert!(types.contains("pub struct CycleA"));
    assert!(types.contains("pub struct CycleB"));
    // Direct mutual recursion must be boxed by the recursive layout pass.
    assert!(
        types.contains("Box<CycleA>") && types.contains("Box<CycleB>"),
        "mutual recursion must insert Box: {types}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn envelope_payload_closure_requires_payload_target() {
    let temp = temporary_repo("envelope-payload-closure");
    let envelope_id = "https://schemas.shittim.local/v1/kcp/command_envelope.json";
    let payload_id = "https://schemas.shittim.local/v1/kcp/task_create_request.json";
    // Remove rust target from task_create_request while command_envelope stays rust.
    set_generation_targets_for_id(&temp, payload_id, serde_json::json!(["typescript"]));
    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0, "payload missing rust target must fail: {stderr}");
    assert!(
        stderr.contains("generation target closure error"),
        "expected exact closure topic: {stderr}"
    );
    assert!(
        stderr.contains(envelope_id),
        "closure error must name the envelope (from) schema: {stderr}"
    );
    assert!(
        stderr.contains(payload_id),
        "closure error must name the payload dependency: {stderr}"
    );
    assert!(
        stderr.contains("rust"),
        "closure error must name the required target: {stderr}"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn off_manifest_source_file_fails_load() {
    let temp = temporary_repo("off-manifest");
    write_json(
        &temp.join("schemas/source/kcp/not_in_manifest.v1.json"),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "https://schemas.shittim.local/v1/kcp/not_in_manifest.json",
            "title": "NotInManifest",
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
fn same_title_collision_fails_with_both_identities() {
    let temp = temporary_repo("title-collision");
    let a_id = "https://schemas.shittim.local/v1/kcp/title_collision_a.json";
    let b_id = "https://schemas.shittim.local/v1/kcp/title_collision_b.json";
    let a_source = "schemas/source/kcp/title_collision_a.v1.json";
    let b_source = "schemas/source/kcp/title_collision_b.v1.json";
    for (id, source) in [(a_id, a_source), (b_id, b_source)] {
        write_json(
            &temp.join(source),
            &serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": id,
                "title": "SharedTitle",
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
            "title": "SharedTitle",
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

    let (code, _stdout, stderr) = run_tool_for_root(&["generate"], &temp);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("collision") || stderr.contains("SharedTitle"),
        "expected name collision: {stderr}"
    );
    assert!(
        stderr.contains(a_id) && stderr.contains(b_id),
        "collision must list both identities: {stderr}"
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
    let schema_id = "https://schemas.shittim.local/v1/kcp/nested_hint_collision.json";
    let source = "schemas/source/kcp/nested_hint_collision.v1.json";
    write_json(
        &temp.join(source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "NestedHintCollision",
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
            "title": "NestedHintCollision",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "object",
            "compatibility": "test-only",
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
    let schema_id = "https://schemas.shittim.local/v1/kcp/test_string_enum_order.json";
    let source = "schemas/source/kcp/test_string_enum_order.v1.json";
    write_json(
        &temp.join(source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "TestStringEnumOrder",
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
            "title": "TestStringEnumOrder",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "kcp_request",
            "compatibility": "test-only",
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
        .find("pub enum TestStringEnumOrder")
        .expect("enum declaration");
    let enum_block = &types[enum_idx..];
    let zeta = enum_block.find("Zeta").expect("Zeta variant");
    let alpha = enum_block.find("Alpha").expect("Alpha variant");
    let middle = enum_block.find("Middle").expect("Middle variant");
    assert!(
        zeta < alpha && alpha < middle,
        "variants must keep schema order zeta/alpha/middle: {enum_block}"
    );

    let all_idx = types.find("impl TestStringEnumOrder").expect("impl block");
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
        types.contains("fn test_string_enum_order_string_enum_contract"),
        "auto contract test must be projected for the synthetic enum"
    );
    std::fs::remove_dir_all(temp).expect("clean temp repo");
}

#[test]
fn string_enum_wire_collision_fails_with_type_wire_and_variant() {
    let temp = temporary_repo("string-enum-wire-collision");
    let schema_id = "https://schemas.shittim.local/v1/kcp/test_string_enum_collision.json";
    let source = "schemas/source/kcp/test_string_enum_collision.v1.json";
    write_json(
        &temp.join(source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "TestStringEnumCollision",
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
            "title": "TestStringEnumCollision",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "kcp_request",
            "compatibility": "test-only",
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
        stderr.contains("TestStringEnumCollision") || stderr.contains(schema_id),
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
    let schema_id = "https://schemas.shittim.local/v1/kcp/test_nullable_string_enum.json";
    let source = "schemas/source/kcp/test_nullable_string_enum.v1.json";
    write_json(
        &temp.join(source),
        &serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": schema_id,
            "title": "TestNullableStringEnum",
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
            "title": "TestNullableStringEnum",
            "version": 1,
            "source": source,
            "domain": "kcp",
            "kind": "kcp_request",
            "compatibility": "test-only",
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
        types.contains("pub proposer: Option<TestNullableStringEnumProposer>"),
        "nullable enum field must stay Option: {types}"
    );
    let enum_idx = types
        .find("pub enum TestNullableStringEnumProposer")
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
        .find("impl TestNullableStringEnumProposer")
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
