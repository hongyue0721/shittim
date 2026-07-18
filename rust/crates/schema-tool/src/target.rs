//! Generation targets, TargetPlan, and target-scoped schema closures.
//!
//! Pipeline stage: SchemaRegistry -> SchemaGraph edges -> TargetPlan/TargetSchemaSet.

use crate::error::SchemaToolError;
use crate::manifest::{GenerationTarget, SchemaRegistry};
use crate::resolve::resolve_ref;
use crate::schema_walk::walk_schema_nodes;
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeSet;

/// One planned generation target with its explicit roots and closed dependency set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSchemaSet {
    pub target: GenerationTarget,
    /// Manifest schemas that explicitly list this target.
    pub roots: BTreeSet<String>,
    /// Roots plus every external manifest `$ref` reached from roots (including through
    /// local fragments) and every envelope payload schema required by typed bindings.
    pub closure: BTreeSet<String>,
}

/// Canonical multi-target plan derived from the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPlan {
    pub targets: Vec<TargetSchemaSet>,
}

/// Canonical order is defined by [`GenerationTarget`] discriminant order: rust, then typescript.
pub fn is_canonical_order(targets: &[GenerationTarget]) -> bool {
    targets.windows(2).all(|pair| pair[0] < pair[1])
}

pub fn validate_generation_targets(entry_id: &str, targets: &[GenerationTarget]) -> Result<()> {
    if targets.is_empty() {
        return Err(SchemaToolError::msg(format!(
            "manifest entry {entry_id} generation_targets must be non-empty"
        ))
        .into());
    }
    let mut seen = BTreeSet::new();
    for target in targets {
        if !seen.insert(*target) {
            return Err(SchemaToolError::msg(format!(
                "manifest entry {entry_id} generation_targets contains duplicate {}",
                target.as_str()
            ))
            .into());
        }
    }
    if !is_canonical_order(targets) {
        return Err(SchemaToolError::msg(format!(
            "manifest entry {entry_id} generation_targets must be in canonical order (rust then typescript)"
        ))
        .into());
    }
    Ok(())
}

/// Build a TargetPlan: every target that appears on at least one schema gets an explicit root
/// set and a validated closure. Empty targets are omitted. Targets are discovered from the
/// manifest (no closed `ALL` enum walk).
pub fn build_target_plan(registry: &SchemaRegistry) -> Result<TargetPlan> {
    let mut discovered: BTreeSet<GenerationTarget> = BTreeSet::new();
    for entry in &registry.manifest().schemas {
        for target in &entry.generation_targets {
            discovered.insert(*target);
        }
    }
    let mut targets = Vec::new();
    for target in discovered {
        let roots: BTreeSet<String> = registry
            .manifest()
            .schemas
            .iter()
            .filter(|entry| entry.generation_targets.contains(&target))
            .map(|entry| entry.id.clone())
            .collect();
        if roots.is_empty() {
            continue;
        }
        let closure = compute_and_validate_closure(registry, target, &roots)?;
        targets.push(TargetSchemaSet {
            target,
            roots,
            closure,
        });
    }
    if targets.is_empty() {
        return Err(
            SchemaToolError::msg("no generation targets requested by any manifest schema").into(),
        );
    }
    Ok(TargetPlan { targets })
}

fn compute_and_validate_closure(
    registry: &SchemaRegistry,
    target: GenerationTarget,
    roots: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut closure = roots.clone();
    let mut stack: Vec<String> = roots.iter().cloned().collect();
    let mut visiting_local: BTreeSet<crate::contract_model::ContractTypeId> = BTreeSet::new();

    while let Some(id) = stack.pop() {
        let loaded = registry.get(&id)?;
        collect_external_deps(
            registry,
            &id,
            &loaded.document,
            &mut closure,
            &mut stack,
            &mut visiting_local,
        )?;

        // Envelope payload bindings are part of the target closure even when only reached
        // through conditional allOf branches (already walked), but re-check explicitly so
        // missing payload targets fail with a clear envelope-oriented message.
        if loaded.entry.kind == "envelope" {
            for payload_id in envelope_payload_ids(registry, &id, &loaded.document)? {
                ensure_dependency_in_target(registry, target, roots, &id, &payload_id)?;
                if closure.insert(payload_id.clone()) {
                    stack.push(payload_id);
                }
            }
        }
    }

    // Every external dependency discovered must itself list the same target.
    for id in &closure {
        if !roots.contains(id) {
            // Dependency reached only via $ref: still must declare the target.
            let loaded = registry.get(id)?;
            if !loaded.entry.generation_targets.contains(&target) {
                return Err(SchemaToolError::msg(format!(
                    "generation target closure error: schema dependency {id} is required by target {} but does not list that target",
                    target.as_str()
                ))
                .into());
            }
        }
    }

    Ok(closure)
}

fn ensure_dependency_in_target(
    registry: &SchemaRegistry,
    target: GenerationTarget,
    roots: &BTreeSet<String>,
    from_id: &str,
    dep_id: &str,
) -> Result<()> {
    if !registry.get(dep_id).is_ok() {
        return Err(SchemaToolError::msg(format!(
            "generation target closure error: schema {from_id} references unknown dependency {dep_id}"
        ))
        .into());
    }
    if roots.contains(dep_id) {
        return Ok(());
    }
    let dep = registry.get(dep_id)?;
    if !dep.entry.generation_targets.contains(&target) {
        return Err(SchemaToolError::msg(format!(
            "generation target closure error: schema {from_id} targets {} but $ref dependency {dep_id} does not",
            target.as_str()
        ))
        .into());
    }
    Ok(())
}

fn collect_external_deps(
    registry: &SchemaRegistry,
    base_id: &str,
    schema: &Value,
    closure: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    seen: &mut BTreeSet<crate::contract_model::ContractTypeId>,
) -> Result<()> {
    walk_schema_nodes(schema, |pointer, _, node| {
        let Some(object) = node.as_object() else {
            return Ok(());
        };
        let Some(reference_value) = object.get("$ref") else {
            return Ok(());
        };
        let reference = reference_value.as_str().ok_or_else(|| {
            SchemaToolError::msg(format!(
                "$ref must be a string at {base_id}#{}",
                pointer.as_str()
            ))
        })?;
        let resolved = resolve_ref(registry, base_id, reference)?;
        if seen.insert(resolved.type_id.clone()) {
            let resolved_id = resolved.type_id.schema_id.clone();
            if resolved_id != base_id
                && registry.get(&resolved_id).is_ok()
                && closure.insert(resolved_id.clone())
            {
                stack.push(resolved_id.clone());
            }
            collect_external_deps(registry, &resolved_id, resolved.node, closure, stack, seen)?;
        }
        Ok(())
    })
}

fn envelope_payload_ids(
    registry: &SchemaRegistry,
    base_id: &str,
    document: &Value,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let Some(branches) = document.get("allOf").and_then(Value::as_array) else {
        return Ok(ids);
    };
    for branch in branches {
        if let Some(payload_ref) = branch
            .pointer("/then/properties/payload/$ref")
            .and_then(Value::as_str)
        {
            if payload_ref.contains('#') {
                continue;
            }
            let resolved = resolve_ref(registry, base_id, payload_ref)?;
            ids.insert(resolved.type_id.schema_id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::GenerationTarget;

    #[test]
    fn accepts_rust_only_and_both_canonical() {
        validate_generation_targets("x", &[GenerationTarget::Rust]).unwrap();
        validate_generation_targets("x", &[GenerationTarget::Typescript]).unwrap();
        validate_generation_targets("x", &[GenerationTarget::Rust, GenerationTarget::Typescript])
            .unwrap();
    }

    #[test]
    fn rejects_empty_duplicate_and_reverse() {
        assert!(validate_generation_targets("x", &[]).is_err());
        assert!(validate_generation_targets(
            "x",
            &[GenerationTarget::Rust, GenerationTarget::Rust]
        )
        .is_err());
        assert!(validate_generation_targets(
            "x",
            &[GenerationTarget::Typescript, GenerationTarget::Rust]
        )
        .is_err());
    }

    #[test]
    fn serde_rejects_unknown_target() {
        let err = serde_json::from_str::<GenerationTarget>("\"python\"").unwrap_err();
        assert!(err.to_string().contains("unknown variant") || err.to_string().contains("python"));
        assert_eq!(
            serde_json::from_str::<GenerationTarget>("\"rust\"").unwrap(),
            GenerationTarget::Rust
        );
        assert_eq!(
            serde_json::from_str::<GenerationTarget>("\"typescript\"").unwrap(),
            GenerationTarget::Typescript
        );
    }
}
