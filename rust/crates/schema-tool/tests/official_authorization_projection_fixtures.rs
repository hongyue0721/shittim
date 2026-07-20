//! Independent schema-tool CLI oracle for official authorization projection fixtures.
//!
//! Owns Schema selection/validation and JCS mode outputs only. Production projection
//! ownership stays with `kernel-authorization`. Temporary mutated fixtures stay under
//! `CARGO_TARGET_DIR`, never under `/tmp`.

use schema_tool::official_fixture::{
    load_projection_fixture, Preimage, ProjectionFixture, CHILD_DELTA_TAMPER_CASE_COUNT,
    MATERIAL_TAMPER_CASE_COUNT, OBSERVATION_NOT_APPLICABLE_TAMPER_CASE_COUNT,
    OBSERVATION_OBSERVED_TAMPER_CASE_COUNT,
};
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

fn oracle_projection_fixture(relative: &str, expected_case_count: usize) {
    let path = fixture_path(relative);
    let fixture: ProjectionFixture =
        load_projection_fixture(&path).expect("load validated projection fixture");
    assert_eq!(fixture.tamper_cases.len(), expected_case_count);
    assert_validate_success(&path, &fixture.schema_id, "/normalized_object");
    assert_canonical_modes(&path, "/normalized_object", &fixture.preimage);
}

#[test]
fn child_delta_fixture_is_validated_and_canonicalized_by_real_cli() {
    oracle_projection_fixture(
        "schemas/fixtures/task/child_task_delta_projection.v1.json",
        CHILD_DELTA_TAMPER_CASE_COUNT,
    );
}

#[test]
fn material_fixture_is_validated_and_canonicalized_by_real_cli() {
    oracle_projection_fixture(
        "schemas/fixtures/policy/material_authorization_projection.v1.json",
        MATERIAL_TAMPER_CASE_COUNT,
    );
}

#[test]
fn observation_not_applicable_fixture_is_validated_and_canonicalized_by_real_cli() {
    oracle_projection_fixture(
        "schemas/fixtures/policy/observation_evidence_not_applicable.v1.json",
        OBSERVATION_NOT_APPLICABLE_TAMPER_CASE_COUNT,
    );
}

#[test]
fn observation_observed_fixture_is_validated_and_canonicalized_by_real_cli() {
    oracle_projection_fixture(
        "schemas/fixtures/policy/observation_evidence_observed.v1.json",
        OBSERVATION_OBSERVED_TAMPER_CASE_COUNT,
    );
}
