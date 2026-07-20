//! Explicit registry profiles for plan/render entry points.
//!
//! Loading remains a profile-neutral inspection operation. Any operation that
//! creates a TargetPlan or generated artifacts must first select and validate a
//! typed profile, preventing library callers from silently bypassing production
//! lifecycle gates.

use crate::compatibility::validate_production_lifecycle_ledger;
use crate::error::SchemaToolError;
use crate::manifest::{ManifestMethodVersionBinding, MethodFamily, SchemaRegistry};
use crate::method_bindings::discover_active_envelope_authority;
use anyhow::{bail, Result};
use std::collections::BTreeSet;
use std::marker::PhantomData;

mod sealed {
    pub trait Sealed {}
}

pub trait RegistryProfile: sealed::Sealed {
    fn validate(registry: &SchemaRegistry) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub struct ProductionSchemaStage;

#[derive(Debug, Clone, Copy)]
pub struct SyntheticNonProduction;

impl sealed::Sealed for ProductionSchemaStage {}
impl sealed::Sealed for SyntheticNonProduction {}

impl RegistryProfile for ProductionSchemaStage {
    fn validate(registry: &SchemaRegistry) -> Result<()> {
        validate_production_method_version_bindings_stage(registry)?;
        validate_production_lifecycle_ledger(&registry.manifest().schemas)
    }
}

impl RegistryProfile for SyntheticNonProduction {
    fn validate(_registry: &SchemaRegistry) -> Result<()> {
        Ok(())
    }
}

/// A registry whose intended lifecycle profile has been explicitly validated.
///
/// This proof is intentionally only constructible through [`ValidatedRegistry::new`].
/// Compilation stages consume it rather than accepting a bare [`SchemaRegistry`].
#[derive(Debug, Clone, Copy)]
pub struct ValidatedRegistry<'a, P: RegistryProfile> {
    registry: &'a SchemaRegistry,
    _profile: PhantomData<P>,
}

impl<'a, P: RegistryProfile> ValidatedRegistry<'a, P> {
    pub fn new(registry: &'a SchemaRegistry) -> Result<Self> {
        P::validate(registry)?;
        Ok(Self {
            registry,
            _profile: PhantomData,
        })
    }

    pub(crate) fn registry(&self) -> &'a SchemaRegistry {
        self.registry
    }
}

pub type ProductionRegistry<'a> = ValidatedRegistry<'a, ProductionSchemaStage>;
pub type SyntheticRegistry<'a> = ValidatedRegistry<'a, SyntheticNonProduction>;

/// Backward-compatible explicit validation function. New plan/render code should
/// carry `ProductionRegistry` rather than discarding the profile proof.
pub fn validate_production_manifest_stage(registry: &SchemaRegistry) -> Result<()> {
    ProductionRegistry::new(registry).map(|_| ())
}

/// Production stage owner for MethodVersionBinding (IC §13.5 / §13.7).
///
/// Method coverage is derived from the same active V2 Envelope authority facts
/// used by the generic binding validator — never a second handwritten method table.
/// Lifecycle targets follow IC §13.5: `task.create` active=`[2]` legacy=`[1]`;
/// every other Envelope method active=`[1]` legacy=`[]`.
fn validate_production_method_version_bindings_stage(registry: &SchemaRegistry) -> Result<()> {
    let authority = discover_active_envelope_authority(registry)?;
    if authority.is_empty() {
        return Err(SchemaToolError::msg(
            "production manifest stage gate: active V2 Envelope authority is required so method_version_bindings can equal the complete expected set derived from Envelope facts (V2InitialBuildActive)",
        )
        .into());
    }

    let expected = authority.expected_bindings();
    let bindings = &registry.manifest().method_version_bindings;
    let actual = bindings
        .iter()
        .map(|binding| (binding.family, binding.method.clone()))
        .collect::<BTreeSet<_>>();

    if actual != expected {
        return Err(SchemaToolError::msg(format!(
            "production manifest stage gate: method_version_bindings must exactly equal the complete expected set derived from active V2 Envelope facts (V2InitialBuildActive); missing={:?}, extra={:?}",
            expected.difference(&actual).collect::<Vec<_>>(),
            actual.difference(&expected).collect::<Vec<_>>(),
        ))
        .into());
    }

    for binding in bindings {
        validate_production_binding_lifecycle_target(binding)?;
    }
    Ok(())
}

/// IC §13.5 production lifecycle target for one Envelope method.
///
/// The method set itself is not listed here; callers already proved coverage
/// against Envelope-derived `expected_bindings()`.
fn validate_production_binding_lifecycle_target(
    binding: &ManifestMethodVersionBinding,
) -> Result<()> {
    let is_task_create = binding.family == MethodFamily::Command && binding.method == "task.create";
    let (expected_active, expected_legacy): (&[u32], &[u32]) = if is_task_create {
        (&[2], &[1])
    } else {
        (&[1], &[])
    };

    if binding.active_request_versions.as_slice() != expected_active {
        bail!(
            "production manifest stage gate: binding {:?}/{} active_request_versions must be {:?}, got {:?} (IC §13.5 V2InitialBuildActive target)",
            binding.family,
            binding.method,
            expected_active,
            binding.active_request_versions
        );
    }
    if binding.legacy_validation_versions.as_slice() != expected_legacy {
        bail!(
            "production manifest stage gate: binding {:?}/{} legacy_validation_versions must be {:?}, got {:?} (IC §13.5 V2InitialBuildActive target)",
            binding.family,
            binding.method,
            expected_legacy,
            binding.legacy_validation_versions
        );
    }
    Ok(())
}
