//! Active Event catalog claimant, mapping bijection, and generated binding facts.

use schema_tool::{
    compile_target_event_catalog_facts, discover_event_catalog_authorities,
    lower_and_render_rust_from_registry, SchemaRegistry, SyntheticRegistry,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir.pop();
    dir.pop();
    dir
}

fn temporary_repo(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    let root = repo_root();
    let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temp = PathBuf::from(
        std::env::var("TMPDIR").unwrap_or_else(|_| "/mnt/data/shittim-build-tmp/tmp".into()),
    )
    .join(format!(
        "shittim-event-{}-{}-{}",
        label,
        std::process::id(),
        unique
    ));
    std::fs::create_dir_all(temp.parent().expect("temp parent")).expect("temp parent");
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

fn event_envelope_entry_mut(manifest: &mut Value) -> &mut Value {
    manifest["schemas"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["title"] == "EventEnvelopeV2")
        .expect("EventEnvelopeV2 entry")
}

fn event_source(root: &Path) -> PathBuf {
    root.join("schemas/source/event/event_envelope.v2.json")
}

#[test]
fn production_event_authority_is_exact_and_bindings_are_single_source() {
    let registry = SchemaRegistry::load(&repo_root()).expect("production load");
    let authority = discover_event_catalog_authorities(&registry).expect("authority");
    let active = authority.active.as_ref().expect("active authority");
    let legacy = authority.legacy_v1.as_ref().expect("legacy authority");
    assert_eq!(
        active.schema_id,
        "https://schemas.shittim.local/event/event_envelope/v2"
    );
    assert_eq!(active.bindings.len(), 5);
    assert_eq!(legacy.bindings.len(), 3);
    assert_eq!(
        active
            .bindings
            .iter()
            .map(|binding| binding.event_type.as_str())
            .collect::<Vec<_>>(),
        vec![
            "task.created",
            "task.state_changed",
            "action.state_changed",
            "approval.state_changed",
            "stop_fence.activated",
        ]
    );
    assert_eq!(
        active.bindings,
        vec![
            schema_tool::EventTypeBindingFact {
                event_type: "task.created".into(),
                aggregate_type: "task".into(),
                payload_schema_id:
                    "https://schemas.shittim.local/v1/event/task_created_payload.json".into(),
                payload_schema_version: 1,
            },
            schema_tool::EventTypeBindingFact {
                event_type: "task.state_changed".into(),
                aggregate_type: "task".into(),
                payload_schema_id:
                    "https://schemas.shittim.local/v1/event/task_state_changed_payload.json".into(),
                payload_schema_version: 1,
            },
            schema_tool::EventTypeBindingFact {
                event_type: "action.state_changed".into(),
                aggregate_type: "action".into(),
                payload_schema_id:
                    "https://schemas.shittim.local/event/action_state_changed_payload/v1".into(),
                payload_schema_version: 1,
            },
            schema_tool::EventTypeBindingFact {
                event_type: "approval.state_changed".into(),
                aggregate_type: "approval_chain".into(),
                payload_schema_id:
                    "https://schemas.shittim.local/event/approval_state_changed_payload/v1".into(),
                payload_schema_version: 1,
            },
            schema_tool::EventTypeBindingFact {
                event_type: "stop_fence.activated".into(),
                aggregate_type: "stop_fence".into(),
                payload_schema_id:
                    "https://schemas.shittim.local/v1/event/stop_fence_activated_payload.json"
                        .into(),
                payload_schema_version: 1,
            },
        ]
    );
    assert_eq!(
        legacy.bindings,
        active
            .bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.event_type.as_str(),
                    "task.created" | "task.state_changed" | "stop_fence.activated"
                )
            })
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(registry.schema_count(), 75);
    assert!(registry.manifest().method_version_bindings.is_empty());
    let event = registry
        .manifest()
        .components
        .iter()
        .find(|component| component.name == "event")
        .unwrap();
    assert_eq!(
        event.allowed_refs,
        vec!["common".to_string(), "policy".to_string()]
    );

    let (_, types, catalog, typed) =
        lower_and_render_rust_from_registry(SyntheticRegistry::new(&registry).unwrap())
            .expect("render");
    assert!(catalog.contains("pub struct EventTypeBinding"));
    assert!(catalog.contains("EVENT_ACTIVE_BINDINGS"));
    assert!(catalog.contains("EVENT_LEGACY_V1_BINDINGS"));
    assert!(catalog.contains("EVENT_ACTIVE_TYPES"));
    assert!(catalog.contains("EVENT_LEGACY_V1_TYPES"));
    assert!(catalog.contains("project_event_types"));
    assert!(!catalog.contains("EVENT_V1_TYPES"));
    assert!(typed.contains("TypedEventEnvelopeV2"));
    assert!(typed.contains("EventEnvelopeV2Payload"));
    assert!(typed.contains("TypedEventEnvelope"));
    assert!(typed.contains("EventPayload"));
    assert!(types.contains("pub struct EventEnvelopeV2OpenPayload"));
    let renderer_source = include_str!("../src/rust_codegen.rs");
    assert!(!renderer_source.contains("rust_name == \"EventEnvelopeV2\""));
    assert!(!renderer_source.contains("EventEnvelopeV2OpenPayload"));
}

#[test]
fn target_event_authority_partial_presence_fails_both_directions() {
    let registry = SchemaRegistry::load(&repo_root()).expect("production load");
    let authority = discover_event_catalog_authorities(&registry).expect("authority");
    let active = authority.active.expect("active authority");

    let payload_only: BTreeSet<_> = active
        .bindings
        .iter()
        .map(|binding| binding.payload_schema_id.clone())
        .collect();
    let error = compile_target_event_catalog_facts(&registry, "payload-only", &payload_only)
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing authority envelope"), "{error}");

    let mut envelope_missing_payloads = BTreeSet::from([active.schema_id.clone()]);
    for binding in active.bindings.iter().skip(1) {
        envelope_missing_payloads.insert(binding.payload_schema_id.clone());
    }
    let error = compile_target_event_catalog_facts(
        &registry,
        "envelope-missing-payload",
        &envelope_missing_payloads,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("missing payload root"), "{error}");
}

#[test]
fn target_event_authority_allows_legacy_only_closure() {
    let registry = SchemaRegistry::load(&repo_root()).expect("production load");
    let closure =
        BTreeSet::from(["https://schemas.shittim.local/v1/event/event_envelope.json".to_owned()]);
    let facts = compile_target_event_catalog_facts(&registry, "legacy-only", &closure)
        .expect("legacy-only facts");
    assert!(facts.active.is_none());
    assert_eq!(facts.legacy_v1.expect("legacy authority").bindings.len(), 3);
}

#[test]
fn zero_event_claimants_is_legal_when_authority_absent() {
    let temp = temporary_repo("zero-claimant");
    let remove_ids = [
        "https://schemas.shittim.local/event/event_envelope/v2",
        "https://schemas.shittim.local/event/action_state_changed_payload/v1",
        "https://schemas.shittim.local/event/approval_state_changed_payload/v1",
        "https://schemas.shittim.local/audit/audit_allocation/v2",
        "https://schemas.shittim.local/audit/audit_record/v2",
        "https://schemas.shittim.local/common/causation_ref/v2",
        "https://schemas.shittim.local/common/action_transition_ref/v1",
        "https://schemas.shittim.local/common/confirmation_mode/v1",
        "https://schemas.shittim.local/policy/approval_record_kind/v2",
        "https://schemas.shittim.local/policy/approval_subject_kind/v2",
        "https://schemas.shittim.local/policy/approval_event_allocation/v1",
        "https://schemas.shittim.local/policy/permission_decision/v2",
        "https://schemas.shittim.local/policy/policy_rule/v2",
        "https://schemas.shittim.local/policy/approval_record/v2",
        "https://schemas.shittim.local/policy/subject_projection/v1",
    ];
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| !remove_ids.contains(&entry["id"].as_str().unwrap()));
    for component in manifest["components"].as_array_mut().unwrap() {
        if component["name"] == "event" {
            component["allowed_refs"] = json!(["common"]);
        }
    }
    write_json(&manifest_path, &manifest);
    for relative in [
        "schemas/source/event/event_envelope.v2.json",
        "schemas/source/event/action_state_changed_payload.v1.json",
        "schemas/source/event/approval_state_changed_payload.v1.json",
        "schemas/source/audit/audit_allocation.v2.json",
        "schemas/source/audit/audit_record.v2.json",
        "schemas/source/common/causation_ref.v2.json",
        "schemas/source/common/action_transition_ref.v1.json",
        "schemas/source/common/confirmation_mode.v1.json",
        "schemas/source/policy/approval_record_kind.v2.json",
        "schemas/source/policy/approval_subject_kind.v2.json",
        "schemas/source/policy/approval_event_allocation.v1.json",
        "schemas/source/policy/permission_decision.v2.json",
        "schemas/source/policy/policy_rule.v2.json",
        "schemas/source/policy/approval_record.v2.json",
        "schemas/source/policy/subject_projection.v1.json",
    ] {
        let _ = std::fs::remove_file(temp.join(relative));
    }
    let registry = SchemaRegistry::load(&temp).expect("zero claimant registry");
    let authority = discover_event_catalog_authorities(&registry).expect("authority");
    assert!(authority.active.is_none());
    assert_eq!(
        authority
            .legacy_v1
            .as_ref()
            .expect("legacy authority")
            .bindings
            .len(),
        3
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn two_event_claimants_fail_closed() {
    let temp = temporary_repo("two-claimants");
    let impostor_id = "https://schemas.shittim.local/event/event_envelope_impostor/v2";
    let impostor_source = "schemas/source/event/event_envelope_impostor.v2.json";
    let mut document = read_json(&event_source(&temp));
    document["$id"] = json!(impostor_id);
    document["title"] = json!("EventEnvelopeImpostorV2");
    write_json(&temp.join(impostor_source), &document);
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    let original = event_envelope_entry_mut(&mut manifest).clone();
    let mut impostor = original;
    impostor["id"] = json!(impostor_id);
    impostor["title"] = json!("EventEnvelopeImpostorV2");
    impostor["source"] = json!(impostor_source);
    manifest["schemas"].as_array_mut().unwrap().push(impostor);
    write_json(&manifest_path, &manifest);
    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        error.contains("claimant")
            || error.contains("partial")
            || error.contains("EventEnvelope")
            || error.contains("component-native"),
        "{error}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn partial_reserved_identity_claimants_fail_closed() {
    for (label, mutate) in [
        ("wrong-title", "title"),
        ("wrong-id", "id"),
        ("wrong-source", "source"),
        ("wrong-compat", "compat"),
        ("wrong-schema-version-field", "svf"),
    ] {
        let temp = temporary_repo(label);
        let manifest_path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&manifest_path);
        let entry = event_envelope_entry_mut(&mut manifest);
        match mutate {
            "title" => entry["title"] = json!("WrongEventEnvelopeV2"),
            "id" => {
                entry["id"] = json!("https://schemas.shittim.local/event/event_envelope_alt/v2")
            }
            "source" => entry["source"] = json!("schemas/source/event/event_envelope_alt.v2.json"),
            "compat" => entry["compatibility"] = json!("new-contract"),
            "svf" => entry["schema_version_field"] = Value::Null,
            _ => unreachable!(),
        }
        write_json(&manifest_path, &manifest);
        if mutate == "source" {
            let src = event_source(&temp);
            let doc = read_json(&src);
            write_json(
                &temp.join("schemas/source/event/event_envelope_alt.v2.json"),
                &doc,
            );
        } else if mutate == "id" {
            let src = event_source(&temp);
            let mut doc = read_json(&src);
            doc["$id"] = json!("https://schemas.shittim.local/event/event_envelope_alt/v2");
            write_json(&src, &doc);
        }
        let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(
            error.contains("partial")
                || error.contains("claimant")
                || error.contains("EventEnvelope")
                || error.contains("component-native")
                || error.contains("title")
                || error.contains("schema_version_field"),
            "{label}: {error}"
        );
        std::fs::remove_dir_all(temp).ok();
    }
}

#[test]
fn structural_impostor_without_reserved_identity_fails_closed() {
    let temp = temporary_repo("structural-impostor");
    let impostor_id = "https://schemas.shittim.local/event/future_event_envelope/v2";
    let impostor_source = "schemas/source/event/future_event_envelope.v2.json";
    let mut document = read_json(&event_source(&temp));
    document["$id"] = json!(impostor_id);
    document["title"] = json!("FutureEventEnvelopeV2");
    write_json(&temp.join(impostor_source), &document);
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"].as_array_mut().unwrap().retain(|entry| {
        entry["id"].as_str() != Some("https://schemas.shittim.local/event/event_envelope/v2")
    });
    let _ = std::fs::remove_file(event_source(&temp));
    manifest["schemas"].as_array_mut().unwrap().push(json!({
        "id": impostor_id,
        "title": "FutureEventEnvelopeV2",
        "version": 2,
        "source": impostor_source,
        "kind": "envelope",
        "compatibility": "breaking-replacement",
        "generation_targets": ["rust"],
        "schema_version_field": "schema_version",
        "component": "event"
    }));
    write_json(&manifest_path, &manifest);
    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        error.contains("partial") || error.contains("claimant") || error.contains("EventEnvelope"),
        "{error}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn structural_impostor_with_drifted_catalog_facts_fails_as_partial_claimant() {
    let temp = temporary_repo("structural-facts-impostor");
    let impostor_id = "https://schemas.shittim.local/event/future_event_envelope/v2";
    let impostor_source = "schemas/source/event/future_event_envelope.v2.json";
    let mut document = read_json(&event_source(&temp));
    document["$id"] = json!(impostor_id);
    document["title"] = json!("FutureEventEnvelopeV2");
    document["properties"]["type"]["enum"] = json!(["future.happened"]);
    document["allOf"] = json!([{
        "if": {
            "properties": {"type": {"const": "future.happened"}},
            "required": ["type"]
        },
        "then": {
            "properties": {
                "aggregate_type": {"const": "future"},
                "payload": {
                    "$ref": "https://schemas.shittim.local/v1/event/task_created_payload.json"
                }
            }
        }
    }]);
    write_json(&temp.join(impostor_source), &document);
    let manifest_path = temp.join("schemas/manifest.json");
    let mut manifest = read_json(&manifest_path);
    manifest["schemas"].as_array_mut().unwrap().retain(|entry| {
        entry["id"].as_str() != Some("https://schemas.shittim.local/event/event_envelope/v2")
    });
    let _ = std::fs::remove_file(event_source(&temp));
    manifest["schemas"].as_array_mut().unwrap().push(json!({
        "id": impostor_id,
        "title": "FutureEventEnvelopeV2",
        "version": 2,
        "source": impostor_source,
        "kind": "envelope",
        "compatibility": "breaking-replacement",
        "generation_targets": ["rust"],
        "schema_version_field": "schema_version",
        "component": "event"
    }));
    write_json(&manifest_path, &manifest);
    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        error.contains("partial active EventEnvelopeV2 claimant"),
        "{error}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn near_shape_event_impostors_are_candidates_and_fail_closed() {
    for (label, mutate) in [
        ("missing-additional-properties", "missing-additional"),
        ("true-additional-properties", "true-additional"),
        ("wrong-root-type", "wrong-root-type"),
        ("missing-required", "missing-required"),
        ("partial-mapping-branch", "partial-branch"),
    ] {
        let temp = temporary_repo(label);
        let stem = label.replace('-', "_");
        let impostor_id = format!("https://schemas.shittim.local/event/{stem}/v2");
        let impostor_source = format!("schemas/source/event/{stem}.v2.json");
        let impostor_title = format!(
            "{}V2",
            label
                .split('-')
                .map(|part| {
                    let mut chars = part.chars();
                    chars
                        .next()
                        .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                        .unwrap_or_default()
                })
                .collect::<String>()
        );
        let mut document = read_json(&event_source(&temp));
        document["$id"] = json!(impostor_id);
        document["title"] = json!(impostor_title);
        match mutate {
            "missing-additional" => {
                document
                    .as_object_mut()
                    .unwrap()
                    .remove("additionalProperties");
            }
            "true-additional" => document["additionalProperties"] = json!(true),
            "wrong-root-type" => document["type"] = json!("array"),
            "missing-required" => document["required"]
                .as_array_mut()
                .unwrap()
                .retain(|field| field != "aggregate_type"),
            "partial-branch" => {
                document["allOf"][0]["then"]["properties"]["payload"] =
                    json!({"description": "not a strict mapping"});
            }
            _ => unreachable!(),
        }
        write_json(&temp.join(&impostor_source), &document);

        let manifest_path = temp.join("schemas/manifest.json");
        let mut manifest = read_json(&manifest_path);
        manifest["schemas"].as_array_mut().unwrap().retain(|entry| {
            entry["id"].as_str() != Some("https://schemas.shittim.local/event/event_envelope/v2")
        });
        let _ = std::fs::remove_file(event_source(&temp));
        manifest["schemas"].as_array_mut().unwrap().push(json!({
            "id": impostor_id,
            "title": impostor_title,
            "version": 2,
            "source": impostor_source,
            "kind": "envelope",
            "compatibility": "breaking-replacement",
            "generation_targets": ["rust"],
            "schema_version_field": "schema_version",
            "component": "event"
        }));
        write_json(&manifest_path, &manifest);

        let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
        assert!(
            error.contains("partial active EventEnvelopeV2 claimant")
                || error.contains("conditional payload mapping")
                || error.contains("exact keys"),
            "{label}: {error}"
        );
        std::fs::remove_dir_all(temp).ok();
    }
}

fn mutate_event_source_and_expect_fail(label: &str, mutate: impl FnOnce(&mut Value)) {
    let temp = temporary_repo(label);
    let source = event_source(&temp);
    let mut document = read_json(&source);
    mutate(&mut document);
    write_json(&source, &document);
    let error = SchemaRegistry::load(&temp).unwrap_err().to_string();
    assert!(
        error.contains("mapping")
            || error.contains("enum")
            || error.contains("payload")
            || error.contains("claimant")
            || error.contains("branch")
            || error.contains("type")
            || error.contains("aggregate")
            || error.contains("exact keys")
            || error.contains("required"),
        "{label}: {error}"
    );
    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn event_mapping_inline_payload_fails_closed() {
    mutate_event_source_and_expect_fail("inline-payload", |document| {
        let branch = document["allOf"]
            .as_array_mut()
            .unwrap()
            .get_mut(0)
            .unwrap();
        branch["then"]["properties"]["payload"] = json!({
            "type": "object",
            "required": ["schema_version"],
            "properties": {"schema_version": {"type": "integer", "const": 1}},
            "additionalProperties": false
        });
    });
}

#[test]
fn event_mapping_fragment_payload_fails_closed() {
    mutate_event_source_and_expect_fail("fragment-payload", |document| {
        let branch = document["allOf"]
            .as_array_mut()
            .unwrap()
            .get_mut(0)
            .unwrap();
        branch["then"]["properties"]["payload"] = json!({
            "$ref": "https://schemas.shittim.local/v1/event/task_created_payload.json#/$defs/missing"
        });
    });
}

#[test]
fn event_mapping_missing_branch_fails_closed() {
    mutate_event_source_and_expect_fail("missing-branch", |document| {
        document["allOf"].as_array_mut().unwrap().pop();
    });
}

#[test]
fn event_mapping_duplicate_branch_fails_closed() {
    mutate_event_source_and_expect_fail("duplicate-branch", |document| {
        let branch = document["allOf"].as_array().unwrap()[0].clone();
        document["allOf"].as_array_mut().unwrap().push(branch);
    });
}

#[test]
fn event_mapping_mixed_branch_without_aggregate_fails_closed() {
    mutate_event_source_and_expect_fail("mixed-branch", |document| {
        let branch = document["allOf"]
            .as_array_mut()
            .unwrap()
            .get_mut(2)
            .unwrap();
        branch["then"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("aggregate_type");
    });
}

#[test]
fn event_enum_without_branch_fails_closed() {
    mutate_event_source_and_expect_fail("enum-no-branch", |document| {
        document["properties"]["type"]["enum"]
            .as_array_mut()
            .unwrap()
            .push(json!("task.unknown"));
    });
}

#[test]
fn event_mapping_branch_shape_drift_fails_closed() {
    for (label, mutate) in [
        ("branch-extra-key", "branch"),
        ("if-extra-key", "if"),
        ("then-extra-key", "then"),
        ("discriminator-extra-key", "discriminator"),
        ("payload-annotation-sibling", "payload"),
        ("required-extra", "required"),
    ] {
        mutate_event_source_and_expect_fail(label, |document| {
            let branch = document["allOf"]
                .as_array_mut()
                .unwrap()
                .get_mut(0)
                .unwrap();
            match mutate {
                "branch" => branch["description"] = json!("not part of mapping IR"),
                "if" => branch["if"]["type"] = json!("object"),
                "then" => branch["then"]["required"] = json!(["payload"]),
                "discriminator" => branch["if"]["properties"]["type"]["type"] = json!("string"),
                "payload" => {
                    branch["then"]["properties"]["payload"]["description"] =
                        json!("annotation sibling is not an exact whole-root mapping")
                }
                "required" => branch["if"]["required"] = json!(["type", "payload"]),
                _ => unreachable!(),
            }
        });
    }
}

#[test]
fn event_authority_contract_drift_fails_closed() {
    for (label, mutate) in [
        ("active-order", "order"),
        ("active-aggregate", "aggregate"),
        ("active-payload", "payload"),
    ] {
        mutate_event_source_and_expect_fail(label, |document| match mutate {
            "order" => document["properties"]["type"]["enum"]
                .as_array_mut()
                .unwrap()
                .swap(0, 1),
            "aggregate" => {
                document["allOf"][2]["then"]["properties"]["aggregate_type"]["const"] =
                    json!("task")
            }
            "payload" => {
                document["allOf"][2]["then"]["properties"]["payload"]["$ref"] =
                    json!("https://schemas.shittim.local/v1/event/task_created_payload.json")
            }
            _ => unreachable!(),
        });
    }
}

#[test]
fn ordinary_event_payload_is_not_a_claimant() {
    let temp = temporary_repo("ordinary-payload");
    let payload = temp.join("schemas/source/event/action_state_changed_payload.v1.json");
    let mut document = read_json(&payload);
    document["description"] = json!("still a payload, not an envelope claimant");
    write_json(&payload, &document);
    SchemaRegistry::load(&temp).expect("payload mutation is not a claimant");
    std::fs::remove_dir_all(temp).ok();
}
