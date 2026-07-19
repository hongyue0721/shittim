use kernel_task_creation::CanonicalProjection;
use schema_tool::official_fixture::{
    load_allocation_fixture, load_child_fixture, load_root_fixture, AllocationFixture,
    ChildFixture, MutationOperation, Preimage, RootFixture,
};
use schema_tool::{apply_json_mutation, JsonPointer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub fn repo_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.pop();
    path
}

pub trait OfficialFixture: Sized {
    fn load(path: PathBuf) -> Self;
}

impl OfficialFixture for RootFixture {
    fn load(path: PathBuf) -> Self {
        load_root_fixture(path).expect("load validated official root fixture")
    }
}

impl OfficialFixture for ChildFixture {
    fn load(path: PathBuf) -> Self {
        load_child_fixture(path).expect("load validated official child fixture")
    }
}

impl OfficialFixture for AllocationFixture {
    fn load(path: PathBuf) -> Self {
        load_allocation_fixture(path).expect("load validated official allocation fixture")
    }
}

pub fn read_fixture<T: OfficialFixture>(relative_path: &str) -> T {
    T::load(repo_root().join(relative_path))
}

pub fn mutate(
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

pub fn assert_projection_matches<T>(
    projection: &CanonicalProjection<T>,
    value: &Value,
    stored: &Preimage,
) where
    T: serde::Serialize,
{
    assert_eq!(&serde_json::to_value(&projection.value).unwrap(), value);
    assert_eq!(hex::encode(&projection.jcs_utf8), stored.jcs_utf8_hex);
    assert_eq!(projection.sha256, stored.sha256);
    assert_preimage_integrity(value, stored);
}

pub fn assert_preimage_integrity(value: &Value, stored: &Preimage) {
    assert_lowercase_hex(&stored.jcs_utf8_hex);
    assert_lowercase_hex(&stored.sha256);
    assert_eq!(stored.sha256.len(), 64);
    let bytes = hex::decode(&stored.jcs_utf8_hex).expect("strict lowercase JCS hex");
    assert!(
        !bytes.starts_with(&[0xef, 0xbb, 0xbf]),
        "JCS bytes have BOM"
    );
    assert_ne!(
        bytes.last(),
        Some(&b'\n'),
        "JCS bytes have trailing newline"
    );
    let decoded: Value = serde_json::from_slice(&bytes).expect("JCS bytes are UTF-8 JSON");
    assert_eq!(&decoded, value);
    assert_eq!(hex::encode(Sha256::digest(&bytes)), stored.sha256);
}

fn assert_lowercase_hex(value: &str) {
    assert!(!value.is_empty());
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
}
