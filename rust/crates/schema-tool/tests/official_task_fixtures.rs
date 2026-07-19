//! Independent schema-tool CLI oracle for official task-creation fixtures.
//!
//! This process only owns Schema selection/validation and JCS mode outputs. It does
//! not reimplement production normalization or business hash relations; those remain
//! with `kernel-task-creation` harness. Temporary mutated fixtures stay under
//! `CARGO_TARGET_DIR` and never under `/tmp`.

use schema_tool::official_fixture::{
    load_allocation_fixture, load_child_fixture, load_root_fixture, AllocationFixture,
    AllocationSide, MutationOperation, Preimage, RootFixture, CHILD_ALLOCATION_TAMPER_CASE_COUNT,
    CHILD_TAMPER_CASE_COUNT, ROOT_ALLOCATION_TAMPER_CASE_COUNT, ROOT_TAMPER_CASE_COUNT,
};
use schema_tool::{apply_json_mutation, JsonPointer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn tool() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_schema-tool")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("rust/target/debug/schema-tool"))
}

fn run(arguments: &[&str]) -> Output {
    Command::new(tool())
        .args(arguments)
        .arg("--repo-root")
        .arg(repo_root())
        .output()
        .expect("run schema-tool CLI")
}

fn fixture_path(relative: &str) -> PathBuf {
    repo_root().join(relative)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn oracle_temp_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("rust/target"));
    let dir = target.join("official-fixture-oracle");
    assert!(
        !dir.starts_with("/tmp"),
        "oracle temp dir must not use /tmp: {}",
        dir.display()
    );
    std::fs::create_dir_all(&dir).expect("create CARGO_TARGET_DIR oracle temp");
    dir
}

fn write_temp_fixture(name: &str, document: &Value) -> PathBuf {
    let path = oracle_temp_dir().join(format!("{name}.json"));
    let bytes = serde_json::to_vec_pretty(document).expect("serialize temp fixture");
    std::fs::write(&path, bytes).expect("write temp fixture under CARGO_TARGET_DIR");
    path
}

fn assert_validate_success(path: &Path, schema: &str, pointer: &str) {
    let output = run(&[
        "validate",
        "--schema",
        schema,
        "--instance",
        &path_text(path),
        "--pointer",
        pointer,
    ]);
    assert!(
        output.status.success(),
        "expected schema_valid success for {pointer}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let expected = format!(
        "valid: instance {} against {} at pointer \"{}\"\n",
        path.display(),
        schema,
        pointer
    );
    assert_eq!(stdout, expected);
    assert!(output.stderr.is_empty());
}

fn assert_validate_failure(path: &Path, schema: &str, pointer: &str) {
    let output = run(&[
        "validate",
        "--schema",
        schema,
        "--instance",
        &path_text(path),
        "--pointer",
        pointer,
    ]);
    assert!(
        !output.status.success(),
        "expected schema rejection for {pointer}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("valid: instance"),
        "rejected validate must not emit success text: {stdout}"
    );
    assert!(
        !stdout.to_ascii_lowercase().contains("success"),
        "rejected validate stdout must not contain success: {stdout}"
    );
}

fn assert_canonical_modes(path: &Path, pointer: &str, preimage: &Preimage) {
    let path = path_text(path);
    let raw = run(&["canonicalize", &path, "--pointer", pointer]);
    assert!(
        raw.status.success(),
        "{}",
        String::from_utf8_lossy(&raw.stderr)
    );
    assert_eq!(hex::encode(&raw.stdout), preimage.jcs_utf8_hex);
    assert_eq!(hex::encode(Sha256::digest(&raw.stdout)), preimage.sha256);

    let hex_output = run(&["canonicalize", &path, "--pointer", pointer, "--hex"]);
    assert!(hex_output.status.success());
    assert_eq!(hex_output.stdout, preimage.jcs_utf8_hex.as_bytes());
    assert!(hex_output.stderr.is_empty());

    let hash_output = run(&["canonicalize", &path, "--pointer", pointer, "--hash"]);
    assert!(hash_output.status.success());
    assert_eq!(hash_output.stdout, preimage.sha256.as_bytes());
    assert!(hash_output.stderr.is_empty());
}

fn mutate(
    document: &Value,
    operation: MutationOperation,
    pointer: &JsonPointer,
    value: Value,
) -> Value {
    let mut mutated = document.clone();
    apply_json_mutation(&mut mutated, operation.into(), pointer, value)
        .expect("fixture mutation must be structurally valid");
    mutated
}

fn oracle_allocation_side(original_document: &Value, side_key: &str, side: &AllocationSide) {
    let baseline_pointer = format!("/{side_key}/valid_allocation");
    assert_validate_success(
        &write_temp_fixture(
            &format!("allocation-baseline-{side_key}"),
            original_document,
        ),
        &side.schema_id,
        &baseline_pointer,
    );

    for case in &side.tamper_cases {
        let mutated_allocation = mutate(
            &side.valid_allocation,
            case.operation,
            &case.pointer,
            case.value.clone(),
        );
        let mut document = original_document.clone();
        document[side_key]["valid_allocation"] = mutated_allocation;
        let temp = write_temp_fixture(
            &format!("allocation-{}-{}", side_key, case.case_id),
            &document,
        );
        if case.expected.schema_valid {
            assert_validate_success(&temp, &side.schema_id, &baseline_pointer);
        } else {
            assert_validate_failure(&temp, &side.schema_id, &baseline_pointer);
        }
    }
}

#[test]
fn root_fixture_is_validated_and_canonicalized_by_real_cli() {
    let path = fixture_path("schemas/fixtures/kcp/task_create_normalized_hash.v2.json");
    let fixture: RootFixture = load_root_fixture(&path).expect("load validated root fixture");
    assert_eq!(fixture.tamper_cases.len(), ROOT_TAMPER_CASE_COUNT);
    assert_validate_success(
        &path,
        "https://schemas.shittim.local/kcp/command_envelope/v2",
        "/raw_envelope",
    );
    assert_validate_success(
        &path,
        "https://schemas.shittim.local/kcp/task_create_request/v2",
        "/raw_envelope/payload",
    );
    assert_validate_success(
        &path,
        "https://schemas.shittim.local/task/normalized_root_task_create_payload/v2",
        "/normalized_payload",
    );
    assert_validate_success(
        &path,
        "https://schemas.shittim.local/task/root_task_create_idempotency_projection/v1",
        "/idempotency_projection",
    );
    assert_canonical_modes(&path, "/normalized_payload", &fixture.receipt_preimage);
    assert_canonical_modes(
        &path,
        "/idempotency_projection",
        &fixture.idempotency_preimage,
    );
}

#[test]
fn child_fixture_is_validated_and_canonicalized_by_real_cli() {
    let path = fixture_path("schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json");
    let fixture = load_child_fixture(&path).expect("load validated child fixture");
    assert_eq!(fixture.tamper_cases.len(), CHILD_TAMPER_CASE_COUNT);
    assert_validate_success(
        &path,
        "https://schemas.shittim.local/task/child_task_proposal/v1",
        "/raw_proposal",
    );
    assert_validate_success(
        &path,
        "https://schemas.shittim.local/task/normalized_child_task_proposal/v1",
        "/normalized_proposal",
    );
    assert_canonical_modes(&path, "/normalized_proposal", &fixture.proposal_preimage);
}

#[test]
fn allocation_baselines_and_all_tampers_are_validated_by_real_cli() {
    let path = fixture_path("schemas/fixtures/task/task_creation_allocations.v1.json");
    let fixture: AllocationFixture =
        load_allocation_fixture(&path).expect("load validated allocation fixture");
    assert_eq!(
        fixture.root.tamper_cases.len(),
        ROOT_ALLOCATION_TAMPER_CASE_COUNT
    );
    assert_eq!(
        fixture.child.tamper_cases.len(),
        CHILD_ALLOCATION_TAMPER_CASE_COUNT
    );

    let original = serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap();
    oracle_allocation_side(&original, "root", &fixture.root);
    oracle_allocation_side(&original, "child", &fixture.child);
}
