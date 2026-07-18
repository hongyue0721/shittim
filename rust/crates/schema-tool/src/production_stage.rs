//! Explicit registry profiles for plan/render entry points.
//!
//! Loading remains a profile-neutral inspection operation. Any operation that
//! creates a TargetPlan or generated artifacts must first select and validate a
//! typed profile, preventing library callers from silently bypassing production
//! lifecycle gates.

use crate::compatibility::validate_production_lifecycle_ledger;
use crate::error::SchemaToolError;
use crate::manifest::SchemaRegistry;
use anyhow::Result;
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
        if !registry.manifest().method_version_bindings.is_empty() {
            return Err(SchemaToolError::msg(
                "production manifest stage gate: method_version_bindings must be empty until V2ProductionWriteCutover",
            )
            .into());
        }
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
