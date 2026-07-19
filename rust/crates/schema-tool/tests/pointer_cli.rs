//! Pointer-aware schema-tool CLI integration tests.

use schema_tool::{apply_json_mutation, JsonMutationOperation, JsonPointer};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    let mut directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    directory.pop();
    directory.pop();
    directory.pop();
    directory
}

fn schema_tool_bin() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_schema-tool")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("rust/target/debug/schema-tool"))
}

fn run_tool(arguments: &[&str]) -> Output {
    Command::new(schema_tool_bin())
        .args(arguments)
        .arg("--repo-root")
        .arg(repo_root())
        .output()
        .expect("run schema-tool")
}

fn test_directory(label: &str) -> PathBuf {
    let identifier = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let test_root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("rust/target"))
        .join("schema-tool-pointer-tests");
    let directory = test_root.join(format!("{label}-{}-{identifier}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("remove stale test directory");
    }
    std::fs::create_dir_all(&directory).expect("create test directory");
    directory
}

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("serialize JSON")).expect("write JSON");
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn canonicalize_selects_nested_value_and_emits_exact_modes() {
    let directory = test_directory("canonical-modes");
    let file = directory.join("input.json");
    write_json(&file, &json!({"nested": {"b": 2, "a": 1}}));
    let file_arg = file.to_string_lossy().into_owned();

    let raw = run_tool(&["canonicalize", &file_arg, "--pointer", "/nested"]);
    assert!(raw.status.success(), "{}", stderr(&raw));
    assert_eq!(raw.stdout, br#"{"a":1,"b":2}"#);
    assert!(raw.stderr.is_empty());

    let hex = run_tool(&["canonicalize", &file_arg, "--pointer", "/nested", "--hex"]);
    assert!(hex.status.success(), "{}", stderr(&hex));
    assert_eq!(hex.stdout, hex::encode(br#"{"a":1,"b":2}"#).into_bytes());

    let hash = run_tool(&["canonicalize", &file_arg, "--pointer", "/nested", "--hash"]);
    assert!(hash.status.success(), "{}", stderr(&hash));
    assert_eq!(
        hash.stdout,
        hex::encode(Sha256::digest(br#"{"a":1,"b":2}"#)).into_bytes()
    );

    std::fs::remove_dir_all(directory).expect("clean test directory");
}

#[test]
fn canonicalize_empty_object_and_rfc8785_fixture_are_exact() {
    let directory = test_directory("rfc8785");
    let empty_file = directory.join("empty.json");
    write_json(&empty_file, &json!({}));
    let empty_arg = empty_file.to_string_lossy().into_owned();
    let empty = run_tool(&["canonicalize", &empty_arg]);
    assert!(empty.status.success(), "{}", stderr(&empty));
    assert_eq!(empty.stdout, b"{}");

    let fixture = repo_root().join("schemas/examples/jcs/rfc8785-example.input.json");
    let fixture_arg = fixture.to_string_lossy().into_owned();
    let expected_path = repo_root().join("schemas/examples/jcs/rfc8785-example.canonical.txt");
    let expected = std::fs::read(expected_path).expect("read canonical fixture");
    let output = run_tool(&["canonicalize", &fixture_arg]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        output.stdout,
        expected.strip_suffix(b"\n").unwrap_or(&expected)
    );

    std::fs::remove_dir_all(directory).expect("clean test directory");
}

#[test]
fn pointer_syntax_and_evaluation_fail_without_partial_stdout() {
    let directory = test_directory("pointer-errors");
    let file = directory.join("input.json");
    write_json(&file, &json!({"items": [1], "scalar": true}));
    let file_arg = file.to_string_lossy().into_owned();

    let syntax = run_tool(&["canonicalize", &file_arg, "--pointer", "/bad~2"]);
    assert!(!syntax.status.success());
    assert!(syntax.stdout.is_empty());
    assert!(stderr(&syntax).contains("invalid JSON Pointer syntax"));

    for pointer in ["/missing", "/items/01", "/items/-", "/items/1", "/scalar/x"] {
        let evaluation = run_tool(&["canonicalize", &file_arg, "--pointer", pointer]);
        assert!(
            !evaluation.status.success(),
            "pointer {pointer} unexpectedly passed"
        );
        assert!(evaluation.stdout.is_empty());
        assert!(stderr(&evaluation).contains("JSON Pointer evaluation failed"));
    }

    std::fs::remove_dir_all(directory).expect("clean test directory");
}

#[test]
fn hex_and_hash_are_clap_mutex_and_emit_no_stdout() {
    let directory = test_directory("mutex");
    let file = directory.join("input.json");
    write_json(&file, &json!({}));
    let file_arg = file.to_string_lossy().into_owned();

    let output = run_tool(&["canonicalize", &file_arg, "--hex", "--hash"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = stderr(&output);
    assert!(
        error.contains("cannot be used with") || error.contains("conflict"),
        "{error}"
    );

    std::fs::remove_dir_all(directory).expect("clean test directory");
}

#[test]
fn validate_nested_pointer_and_root_success_formats() {
    let directory = test_directory("validate");
    let nested_file = directory.join("nested.json");
    let actor = json!({
        "schema_version": 1,
        "revision": 1,
        "id": "actor-1",
        "kind": "known_user",
        "source": "test",
        "authentication_level": "asserted",
        "confidence": null
    });
    write_json(&nested_file, &json!({"wrapper": actor}));
    let nested_arg = nested_file.to_string_lossy().into_owned();
    let schema = "https://schemas.shittim.local/v1/common/actor.json";

    let nested = run_tool(&[
        "validate",
        "--schema",
        schema,
        "--instance",
        &nested_arg,
        "--pointer",
        "/wrapper",
    ]);
    assert!(nested.status.success(), "{}", stderr(&nested));
    assert_eq!(
        String::from_utf8(nested.stdout).unwrap(),
        format!(
            "valid: instance {} against {} at pointer \"/wrapper\"\n",
            nested_file.display(),
            schema
        )
    );

    let root_file = directory.join("root.json");
    write_json(&root_file, &actor);
    let root_arg = root_file.to_string_lossy().into_owned();
    let root = run_tool(&["validate", "--schema", schema, "--instance", &root_arg]);
    assert!(root.status.success(), "{}", stderr(&root));
    assert_eq!(
        String::from_utf8(root.stdout).unwrap(),
        format!(
            "valid: instance {} against {}\n",
            root_file.display(),
            schema
        )
    );

    std::fs::remove_dir_all(directory).expect("clean test directory");
}

#[test]
fn validate_pointer_failure_has_no_success_output() {
    let directory = test_directory("validate-error");
    let file = directory.join("input.json");
    write_json(&file, &json!({"wrapper": {}}));
    let file_arg = file.to_string_lossy().into_owned();

    let output = run_tool(&[
        "validate",
        "--schema",
        "https://schemas.shittim.local/v1/common/actor.json",
        "--instance",
        &file_arg,
        "--pointer",
        "/missing",
    ]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains("JSON Pointer evaluation failed"));

    std::fs::remove_dir_all(directory).expect("clean test directory");
}

#[test]
fn public_mutation_api_covers_add_replace_and_structured_failures() {
    let mut document = json!({"items": [1, 3], "object": {"old": true}});
    apply_json_mutation(
        &mut document,
        JsonMutationOperation::Add,
        &JsonPointer::parse("/items/1").unwrap(),
        json!(2),
    )
    .unwrap();
    apply_json_mutation(
        &mut document,
        JsonMutationOperation::Replace,
        &JsonPointer::parse("/object/old").unwrap(),
        json!(false),
    )
    .unwrap();
    assert_eq!(
        document,
        json!({"items": [1, 2, 3], "object": {"old": false}})
    );

    let error = apply_json_mutation(
        &mut document,
        JsonMutationOperation::Add,
        &JsonPointer::parse("/object/old").unwrap(),
        json!(true),
    )
    .expect_err("add existing member must fail");
    assert!(matches!(
        error,
        schema_tool::error::SchemaToolError::Mutation { .. }
    ));
}
