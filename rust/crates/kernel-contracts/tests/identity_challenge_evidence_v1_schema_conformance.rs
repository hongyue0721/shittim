//! V2InitialBuildActive slice 1c-ii identity/challenge/evidence Schema conformance.

use kernel_contracts::{
    canonical_json_bytes, decode_validated, validate_json, CredentialRefV1,
    LocalPresenceEvidenceV1, RemoteApprovalChallengeV1, RemoteApprovalResponseV1,
    RemoteApprovalSignaturePreimageV1, RemoteSignatureAlgorithmV1, SchemaCatalog,
    SystemAuthenticationChallengeV1, SystemAuthenticationEvidenceV1,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const ALGO: &str = "https://schemas.shittim.local/policy/remote_signature_algorithm/v1";
const CRED: &str = "https://schemas.shittim.local/policy/credential_ref/v1";
const REMOTE_CHALLENGE: &str = "https://schemas.shittim.local/policy/remote_approval_challenge/v1";
const SYSTEM_CHALLENGE: &str =
    "https://schemas.shittim.local/policy/system_authentication_challenge/v1";
const LOCAL_EVIDENCE: &str = "https://schemas.shittim.local/policy/local_presence_evidence/v1";
const SYSTEM_EVIDENCE: &str =
    "https://schemas.shittim.local/policy/system_authentication_evidence/v1";
const REMOTE_RESPONSE: &str = "https://schemas.shittim.local/policy/remote_approval_response/v1";
const PREIMAGE: &str = "https://schemas.shittim.local/policy/remote_approval_signature_preimage/v1";
const ACTOR: &str = "https://schemas.shittim.local/v1/common/actor.json";
const ENTRY: &str = "https://schemas.shittim.local/v1/common/entry_point.json";

/// 32-byte payload encoded as base64url without padding (43 chars).
const NONCE_32: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
/// 64-byte payload encoded as base64url without padding (86 chars).
const SIG_64: &str =
    "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4vMDEyMzQ1Njc4OTo7PD0-Pw";

fn uuid(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02}")
}

fn hash(character: char) -> String {
    std::iter::repeat_n(character, 64).collect()
}

fn actor() -> Value {
    json!({
        "schema_version": 1,
        "revision": 2,
        "id": "actor-local-1",
        "kind": "known_user",
        "source": "local-desktop",
        "authentication_level": "platform_verified",
        "confidence": 0.9
    })
}

fn signature_algorithm() -> Value {
    json!({
        "algorithm_kind": "ed25519",
        "public_key_encoding": "base64url_no_pad",
        "public_key": NONCE_32
    })
}

fn credential_ref() -> Value {
    json!({
        "schema_version": 1,
        "credential_id": uuid(20),
        "credential_revision": 1,
        "actor_ref": "actor-local-1",
        "issuer_ref": "issuer-kernel-1",
        "signature_algorithm": signature_algorithm(),
        "not_before": "2026-07-20T07:00:00Z",
        "expires_at": "2026-12-31T00:00:00Z",
        "status": "active",
        "replaced_by_ref": null
    })
}

fn remote_challenge(state: &str) -> Value {
    let mut value = json!({
        "schema_version": 1,
        "challenge_id": uuid(21),
        "approval_chain_id": uuid(5),
        "request_ref": uuid(6),
        "audience": "https://remote.example/approval",
        "nonce": NONCE_32,
        "nonce_encoding": "base64url_no_pad",
        "task_id": uuid(1),
        "subject_hash": hash('c'),
        "material_authorization_fingerprint": hash('a'),
        "allowed_decisions": ["approved", "denied"],
        "credential_ref": credential_ref(),
        "issued_at": "2026-07-20T08:00:00Z",
        "expires_at": "2026-07-20T08:05:00Z",
        "state": state,
        "consumed_at": null,
        "revoked_at": null,
        "revocation_reason": null
    });
    match state {
        "consumed" => {
            value["consumed_at"] = json!("2026-07-20T08:01:00Z");
        }
        "revoked" => {
            value["revoked_at"] = json!("2026-07-20T08:01:30Z");
            value["revocation_reason"] = json!("operator_revoked");
        }
        "expired" | "issued" => {}
        other => panic!("unexpected challenge state fixture {other}"),
    }
    value
}

fn system_challenge(state: &str) -> Value {
    let mut value = json!({
        "schema_version": 1,
        "challenge_id": uuid(22),
        "approval_chain_id": uuid(5),
        "request_ref": uuid(6),
        "nonce": NONCE_32,
        "nonce_encoding": "base64url_no_pad",
        "task_id": uuid(1),
        "subject_hash": hash('c'),
        "material_authorization_fingerprint": hash('a'),
        "allowed_decisions": ["approved"],
        "issued_at": "2026-07-20T08:00:00Z",
        "expires_at": "2026-07-20T08:05:00Z",
        "state": state,
        "consumed_at": null,
        "revoked_at": null,
        "revocation_reason": null
    });
    match state {
        "consumed" => {
            value["consumed_at"] = json!("2026-07-20T08:01:00Z");
        }
        "revoked" => {
            value["revoked_at"] = json!("2026-07-20T08:01:30Z");
            value["revocation_reason"] = json!("operator_revoked");
        }
        "expired" | "issued" => {}
        other => panic!("unexpected challenge state fixture {other}"),
    }
    value
}

fn local_presence_evidence() -> Value {
    json!({
        "schema_version": 1,
        "id": uuid(23),
        "session_ref": "desktop-session-1",
        "transport_kind": "unix_peer",
        "peer_principal_ref": "uid:1000",
        "observed_actor": actor(),
        "entry_point": "local_desktop",
        "challenge_ref": null,
        "presence_kind": "interactive_session",
        "observed_at": "2026-07-20T08:00:00Z",
        "valid_until": "2026-07-20T08:10:00Z",
        "verifier_kind": "kernel_transport",
        "evidence_hash": hash('e')
    })
}

fn system_authentication_evidence() -> Value {
    json!({
        "schema_version": 1,
        "id": uuid(24),
        "mechanism": "polkit",
        "mechanism_version": "1",
        "platform_subject_ref": "unix-user:1000",
        "challenge_ref": uuid(22),
        "subject_hash": hash('c'),
        "material_authorization_fingerprint": hash('a'),
        "result": "verified",
        "verified_at": "2026-07-20T08:01:00Z",
        "valid_until": "2026-07-20T08:06:00Z",
        "verifier_ref": "os-adapter:polkit",
        "evidence_blob_hash": hash('b')
    })
}

fn remote_response() -> Value {
    json!({
        "schema_version": 1,
        "challenge_id": uuid(21),
        "approval_chain_id": uuid(5),
        "request_ref": uuid(6),
        "audience": "https://remote.example/approval",
        "nonce": NONCE_32,
        "credential_ref": credential_ref(),
        "actor": actor(),
        "task_id": uuid(1),
        "subject_hash": hash('c'),
        "material_authorization_fingerprint": hash('a'),
        "decision": "approved",
        "signed_at": "2026-07-20T08:01:00Z",
        "algorithm_kind": "ed25519",
        "signature_encoding": "base64url_no_pad",
        "signature": SIG_64
    })
}

fn signature_preimage() -> Value {
    json!({
        "schema_version": 1,
        "purpose": "shittim.remote-approval.v1",
        "algorithm_kind": "ed25519",
        "challenge_id": uuid(21),
        "approval_chain_id": uuid(5),
        "request_ref": uuid(6),
        "audience": "https://remote.example/approval",
        "nonce": NONCE_32,
        "credential_id": uuid(20),
        "credential_revision": 1,
        "actor_ref": "actor-local-1",
        "task_id": uuid(1),
        "subject_hash": hash('c'),
        "material_authorization_fingerprint": hash('a'),
        "decision": "approved",
        "signed_at": "2026-07-20T08:01:00Z",
        "challenge_issued_at": "2026-07-20T08:00:00Z",
        "challenge_expires_at": "2026-07-20T08:05:00Z"
    })
}

fn assert_round_trip<T>(schema_id: &str, value: &Value)
where
    T: DeserializeOwned + Serialize,
{
    let typed: T = decode_validated(schema_id, value).expect("typed decode");
    let encoded = serde_json::to_value(typed).expect("serialize");
    validate_json(schema_id, &encoded).expect("revalidate");
    assert_eq!(
        canonical_json_bytes(value).expect("input JCS"),
        canonical_json_bytes(&encoded).expect("encoded JCS")
    );
}

fn all_roots() -> [(&'static str, Value); 8] {
    [
        (ALGO, signature_algorithm()),
        (CRED, credential_ref()),
        (REMOTE_CHALLENGE, remote_challenge("issued")),
        (SYSTEM_CHALLENGE, system_challenge("issued")),
        (LOCAL_EVIDENCE, local_presence_evidence()),
        (SYSTEM_EVIDENCE, system_authentication_evidence()),
        (REMOTE_RESPONSE, remote_response()),
        (PREIMAGE, signature_preimage()),
    ]
}

#[test]
fn manifest_batch_contains_identity_roots_and_bindings_remain_empty() {
    let manifest: Value =
        serde_json::from_str(include_str!("../../../../schemas/manifest.json")).expect("manifest");
    let entries = manifest["schemas"].as_array().expect("schemas");
    // Pure production is exactly 83. Synthetic probe repos used by schema-tool
    // tagged-union tests may temporarily append component-native roots; those must
    // not weaken the eight-root identity assertions below.
    assert!(
        entries.len() >= 83,
        "production baseline (83) plus optional synthetic probe roots, got {}",
        entries.len()
    );
    assert!(manifest["method_version_bindings"]
        .as_array()
        .expect("bindings")
        .is_empty());
    for id in [
        ALGO,
        CRED,
        REMOTE_CHALLENGE,
        SYSTEM_CHALLENGE,
        LOCAL_EVIDENCE,
        SYSTEM_EVIDENCE,
        REMOTE_RESPONSE,
        PREIMAGE,
    ] {
        assert!(entries.iter().any(|entry| entry["id"] == id), "{id}");
    }
}

#[test]
fn typed_round_trips_cover_eight_roots_and_all_union_states() {
    assert_round_trip::<RemoteSignatureAlgorithmV1>(ALGO, &signature_algorithm());
    assert_round_trip::<CredentialRefV1>(CRED, &credential_ref());
    for state in ["issued", "consumed", "expired", "revoked"] {
        assert_round_trip::<RemoteApprovalChallengeV1>(REMOTE_CHALLENGE, &remote_challenge(state));
        assert_round_trip::<SystemAuthenticationChallengeV1>(
            SYSTEM_CHALLENGE,
            &system_challenge(state),
        );
    }
    assert_round_trip::<LocalPresenceEvidenceV1>(LOCAL_EVIDENCE, &local_presence_evidence());
    assert_round_trip::<SystemAuthenticationEvidenceV1>(
        SYSTEM_EVIDENCE,
        &system_authentication_evidence(),
    );
    assert_round_trip::<RemoteApprovalResponseV1>(REMOTE_RESPONSE, &remote_response());
    assert_round_trip::<RemoteApprovalSignaturePreimageV1>(PREIMAGE, &signature_preimage());
}

#[test]
fn required_and_unknown_fields_fail_closed_for_every_root() {
    for (schema, mut value, field) in [
        (ALGO, signature_algorithm(), "public_key"),
        (CRED, credential_ref(), "signature_algorithm"),
        (
            REMOTE_CHALLENGE,
            remote_challenge("issued"),
            "allowed_decisions",
        ),
        (SYSTEM_CHALLENGE, system_challenge("issued"), "nonce"),
        (LOCAL_EVIDENCE, local_presence_evidence(), "evidence_hash"),
        (SYSTEM_EVIDENCE, system_authentication_evidence(), "result"),
        (REMOTE_RESPONSE, remote_response(), "signature"),
        (PREIMAGE, signature_preimage(), "purpose"),
    ] {
        value.as_object_mut().expect("object").remove(field);
        assert!(
            validate_json(schema, &value).is_err(),
            "{schema} missing {field}"
        );
    }
    for (schema, mut value) in all_roots() {
        value["unexpected"] = json!(true);
        assert!(validate_json(schema, &value).is_err(), "{schema} unknown");
    }
}

#[test]
fn allowed_decisions_are_exact_ordered_const_arrays() {
    let mut remote = remote_challenge("issued");
    remote["allowed_decisions"] = json!(["denied", "approved"]);
    assert!(validate_json(REMOTE_CHALLENGE, &remote).is_err());

    let mut remote = remote_challenge("issued");
    remote["allowed_decisions"] = json!(["approved"]);
    assert!(validate_json(REMOTE_CHALLENGE, &remote).is_err());

    let mut remote = remote_challenge("issued");
    remote["allowed_decisions"] = json!(["approved", "denied", "approved"]);
    assert!(validate_json(REMOTE_CHALLENGE, &remote).is_err());

    let mut remote = remote_challenge("issued");
    remote["allowed_decisions"] = json!(["approved", "denied"]);
    assert!(validate_json(REMOTE_CHALLENGE, &remote).is_ok());

    let mut system = system_challenge("issued");
    system["allowed_decisions"] = json!(["approved", "denied"]);
    assert!(validate_json(SYSTEM_CHALLENGE, &system).is_err());

    let mut system = system_challenge("issued");
    system["allowed_decisions"] = json!([]);
    assert!(validate_json(SYSTEM_CHALLENGE, &system).is_err());

    let mut system = system_challenge("issued");
    system["allowed_decisions"] = json!(["approved"]);
    assert!(validate_json(SYSTEM_CHALLENGE, &system).is_ok());
}

#[test]
fn nonce_encoding_and_min_length_are_enforced() {
    let short = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh"; // 42 chars
    assert_eq!(short.len(), 42);

    for (schema, mut value) in [
        (REMOTE_CHALLENGE, remote_challenge("issued")),
        (SYSTEM_CHALLENGE, system_challenge("issued")),
        (REMOTE_RESPONSE, remote_response()),
        (PREIMAGE, signature_preimage()),
    ] {
        value["nonce"] = json!(short);
        assert!(
            validate_json(schema, &value).is_err(),
            "{schema} short nonce"
        );
        value["nonce"] = json!(NONCE_32);
        if schema == REMOTE_CHALLENGE || schema == SYSTEM_CHALLENGE {
            value["nonce_encoding"] = json!("base64");
            assert!(
                validate_json(schema, &value).is_err(),
                "{schema} bad encoding"
            );
        }
    }
}

#[test]
fn ed25519_public_key_and_signature_lengths_are_exact() {
    let mut algo = signature_algorithm();
    algo["public_key"] = json!(&NONCE_32[..42]);
    assert!(validate_json(ALGO, &algo).is_err());
    algo["public_key"] = json!(format!("{NONCE_32}A"));
    assert!(validate_json(ALGO, &algo).is_err());
    algo["public_key"] = json!(NONCE_32);
    assert!(validate_json(ALGO, &algo).is_ok());

    let mut response = remote_response();
    response["signature"] = json!(&SIG_64[..85]);
    assert!(validate_json(REMOTE_RESPONSE, &response).is_err());
    response["signature"] = json!(format!("{SIG_64}A"));
    assert!(validate_json(REMOTE_RESPONSE, &response).is_err());
    response["signature"] = json!(SIG_64);
    assert!(validate_json(REMOTE_RESPONSE, &response).is_ok());
}

#[test]
fn remote_signature_algorithm_is_true_tagged_union() {
    let mut unknown_branch = signature_algorithm();
    unknown_branch["algorithm_kind"] = json!("rsa_pss");
    assert!(validate_json(ALGO, &unknown_branch).is_err());

    let mut missing_key = signature_algorithm();
    missing_key
        .as_object_mut()
        .expect("object")
        .remove("public_key");
    assert!(validate_json(ALGO, &missing_key).is_err());

    let mut unknown_field = signature_algorithm();
    unknown_field["curve"] = json!("ed25519");
    assert!(validate_json(ALGO, &unknown_field).is_err());

    let mut wrong_encoding = signature_algorithm();
    wrong_encoding["public_key_encoding"] = json!("hex");
    assert!(validate_json(ALGO, &wrong_encoding).is_err());
}

#[test]
fn closed_enums_and_const_fields_reject_unknown_values() {
    let mut challenge = remote_challenge("issued");
    challenge["state"] = json!("pending");
    assert!(validate_json(REMOTE_CHALLENGE, &challenge).is_err());

    let mut local = local_presence_evidence();
    local["transport_kind"] = json!("tcp");
    assert!(validate_json(LOCAL_EVIDENCE, &local).is_err());
    local = local_presence_evidence();
    local["presence_kind"] = json!("presence");
    assert!(validate_json(LOCAL_EVIDENCE, &local).is_err());
    local = local_presence_evidence();
    local["verifier_kind"] = json!("user_asserted");
    assert!(validate_json(LOCAL_EVIDENCE, &local).is_err());

    let mut system = system_authentication_evidence();
    system["mechanism"] = json!("success");
    assert!(validate_json(SYSTEM_EVIDENCE, &system).is_err());
    system = system_authentication_evidence();
    system["result"] = json!("success");
    assert!(validate_json(SYSTEM_EVIDENCE, &system).is_err());
    system = system_authentication_evidence();
    system["result"] = json!("failed");
    assert!(validate_json(SYSTEM_EVIDENCE, &system).is_err());

    let mut cred = credential_ref();
    cred["status"] = json!("replaced");
    assert!(validate_json(CRED, &cred).is_err());

    let mut response = remote_response();
    response["decision"] = json!("abstain");
    assert!(validate_json(REMOTE_RESPONSE, &response).is_err());
    response = remote_response();
    response["algorithm_kind"] = json!("rsa");
    assert!(validate_json(REMOTE_RESPONSE, &response).is_err());

    let mut preimage = signature_preimage();
    preimage["purpose"] = json!("shittim.remote-approval.v2");
    assert!(validate_json(PREIMAGE, &preimage).is_err());
}

#[test]
fn challenge_state_boundary_values_remain_schema_valid_shapes() {
    // Schema only encodes closed state enum and terminal timestamp nullability shape.
    // Transition legality and TTL≤5min are repository obligations, not Schema assertions.
    for state in ["issued", "consumed", "expired", "revoked"] {
        assert!(
            validate_json(REMOTE_CHALLENGE, &remote_challenge(state)).is_ok(),
            "remote {state}"
        );
        assert!(
            validate_json(SYSTEM_CHALLENGE, &system_challenge(state)).is_ok(),
            "system {state}"
        );
    }

    // TTL upper bound is not expressible as pure Schema relative constraint; keep exact
    // issued/expires shape and leave CAS/TTL enforcement to repository.
    let mut remote = remote_challenge("issued");
    remote["expires_at"] = json!("2026-07-20T08:06:00Z");
    assert!(
        validate_json(REMOTE_CHALLENGE, &remote).is_ok(),
        "Schema must not invent relative TTL math"
    );
}

#[test]
fn legacy_v1_field_names_do_not_leak_into_identity_roots() {
    for forbidden in [
        "approval_record_ref",
        "approval_type",
        "evaluation_context_hash",
        "granted_scopes",
        "supersedes_ref",
        "current_head_ref",
        "public_key_pem",
        "signature_alg",
        "challenge_token",
        "auth_success",
    ] {
        for (schema, mut value) in all_roots() {
            value[forbidden] = json!(uuid(13));
            assert!(
                validate_json(schema, &value).is_err(),
                "{schema} leaked {forbidden}"
            );
        }
    }
}

#[test]
fn response_must_not_carry_expiry_or_public_key_override_fields() {
    let mut response = remote_response();
    response["expires_at"] = json!("2026-07-20T08:05:00Z");
    assert!(validate_json(REMOTE_RESPONSE, &response).is_err());
    response = remote_response();
    response["public_key"] = json!(NONCE_32);
    assert!(validate_json(REMOTE_RESPONSE, &response).is_err());
}

#[test]
fn local_presence_does_not_require_subject_hash_while_system_evidence_does() {
    let mut local = local_presence_evidence();
    local["subject_hash"] = json!(hash('c'));
    assert!(validate_json(LOCAL_EVIDENCE, &local).is_err());

    let mut system = system_authentication_evidence();
    system
        .as_object_mut()
        .expect("object")
        .remove("subject_hash");
    assert!(validate_json(SYSTEM_EVIDENCE, &system).is_err());
}

#[test]
fn official_fixtures_validate_and_preimage_jcs_matches() {
    let remote_fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/policy/remote_approval_challenge.v1.json"
    ))
    .expect("remote challenge fixture");
    let system_fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/policy/system_authentication_challenge.v1.json"
    ))
    .expect("system challenge fixture");
    let local_fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/policy/local_presence_evidence.v1.json"
    ))
    .expect("local fixture");
    let system_evidence_fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/policy/system_authentication_evidence.v1.json"
    ))
    .expect("system evidence fixture");
    let cred_fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/policy/credential_ref.v1.json"
    ))
    .expect("credential fixture");
    let algo_fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/policy/remote_signature_algorithm.v1.json"
    ))
    .expect("algo fixture");
    let response_fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/policy/remote_approval_response.v1.json"
    ))
    .expect("response fixture");
    let preimage_fixture: Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/policy/remote_approval_signature_preimage.v1.json"
    ))
    .expect("preimage fixture");

    for (schema, fixture) in [
        (REMOTE_CHALLENGE, &remote_fixture),
        (SYSTEM_CHALLENGE, &system_fixture),
        (LOCAL_EVIDENCE, &local_fixture),
        (SYSTEM_EVIDENCE, &system_evidence_fixture),
        (CRED, &cred_fixture),
        (ALGO, &algo_fixture),
        (REMOTE_RESPONSE, &response_fixture),
        (PREIMAGE, &preimage_fixture),
    ] {
        assert_eq!(fixture["schema_id"], schema);
        let object = &fixture["valid_object"];
        validate_json(schema, object).expect("fixture valid_object");
        for case in fixture["tamper_cases"].as_array().expect("tamper_cases") {
            let mut mutated = object.clone();
            apply_tamper(&mut mutated, case);
            let schema_valid = case["schema_valid"].as_bool().expect("schema_valid");
            assert_eq!(
                validate_json(schema, &mutated).is_ok(),
                schema_valid,
                "{schema} {}",
                case["case_id"]
            );
        }
    }

    let preimage = &preimage_fixture["valid_object"];
    let jcs = canonical_json_bytes(preimage).expect("jcs");
    let expected_hex = preimage_fixture["preimage"]["jcs_utf8_hex"]
        .as_str()
        .expect("jcs hex");
    let expected_hash = preimage_fixture["preimage"]["sha256"]
        .as_str()
        .expect("sha256");
    assert_eq!(kernel_contracts::sha256_hex(&jcs), expected_hash);
    // JCS bytes are the signed preimage; fixture stores lowercase hex of those bytes.
    assert_eq!(
        jcs.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        expected_hex
    );
}

#[test]
fn direct_refs_match_the_component_native_dag() {
    let catalog = SchemaCatalog::load_embedded().expect("catalog");
    let expected = [
        (ALGO, BTreeSet::from([])),
        (CRED, BTreeSet::from([ALGO])),
        (REMOTE_CHALLENGE, BTreeSet::from([CRED])),
        (SYSTEM_CHALLENGE, BTreeSet::from([])),
        (LOCAL_EVIDENCE, BTreeSet::from([ACTOR, ENTRY])),
        (SYSTEM_EVIDENCE, BTreeSet::from([])),
        (REMOTE_RESPONSE, BTreeSet::from([ACTOR, CRED])),
        (PREIMAGE, BTreeSet::from([])),
    ];
    for (schema, expected_refs) in expected {
        let mut refs = BTreeSet::new();
        collect_whole_root_refs(catalog.document(schema).expect("schema"), &mut refs);
        assert_eq!(refs, expected_refs, "{schema}");
    }
}

fn apply_tamper(value: &mut Value, case: &Value) {
    let pointer = case["pointer"].as_str().expect("pointer");
    let operation = case["operation"].as_str().expect("operation");
    let replacement = case["value"].clone();
    match operation {
        "replace" => {
            let target = value
                .pointer_mut(pointer)
                .unwrap_or_else(|| panic!("missing pointer {pointer}"));
            *target = replacement;
        }
        "add" => {
            let Some((parent_path, key)) = pointer.rsplit_once('/') else {
                panic!("bad add pointer {pointer}");
            };
            let parent = if parent_path.is_empty() {
                value
            } else {
                value
                    .pointer_mut(parent_path)
                    .unwrap_or_else(|| panic!("missing parent {parent_path}"))
            };
            parent
                .as_object_mut()
                .expect("object parent")
                .insert(key.to_string(), replacement);
        }
        "remove" => {
            let Some((parent_path, key)) = pointer.rsplit_once('/') else {
                panic!("bad remove pointer {pointer}");
            };
            let parent = if parent_path.is_empty() {
                value
            } else {
                value
                    .pointer_mut(parent_path)
                    .unwrap_or_else(|| panic!("missing parent {parent_path}"))
            };
            parent
                .as_object_mut()
                .expect("object parent")
                .remove(key)
                .unwrap_or_else(|| panic!("missing member {key}"));
        }
        other => panic!("unsupported operation {other}"),
    }
}

fn collect_whole_root_refs<'a>(value: &'a Value, output: &mut BTreeSet<&'a str>) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                if !reference.contains('#') {
                    output.insert(reference);
                }
            }
            for child in object.values() {
                collect_whole_root_refs(child, output);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_whole_root_refs(child, output);
            }
        }
        _ => {}
    }
}
