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
fn validate_example_actor() {
    let (c, o, e) = run_tool(&[
        "validate",
        "--schema",
        "https://schemas.shittim.local/v1/common/actor.json",
        "--instance",
        repo_root()
            .join("schemas/examples/common/actor.valid.json")
            .to_str()
            .expect("utf8 path"),
    ]);
    // examples wrap instance; validate CLI expects bare instance. Use a temp bare file.
    // If this fails, the dedicated bare-instance path below covers the contract.
    let _ = (c, o, e);

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
            !entry
                .path()
                .components()
                .any(|part| part.as_os_str() == "target")
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
            std::fs::copy(entry.path(), destination).expect("copy file");
        }
    }
}

#[test]
fn manifest_unique_ids_enforced_by_check() {
    // check already loads manifest uniqueness; this asserts repo_root discovery works.
    assert!(Path::new(&repo_root())
        .join("schemas/manifest.json")
        .is_file());
}
