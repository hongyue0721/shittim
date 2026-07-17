//! Pure TaskScope resource containment checks.
//!
//! This module answers only whether concrete resource refs fall inside a stored TaskScope
//! include/exclude boundary. It does not authorize, create PermissionDecision drafts, mutate
//! scopes, or reuse PolicyRule applicability (`match_resources`).

use crate::uri::{
    normalize_uri_pattern_value, normalize_uri_value, uri_pattern_matches, NormalizedUri,
};
use crate::{PolicyError, PolicyErrorCode};
use std::error::Error;
use thiserror::Error;

/// Stable machine-readable TaskScope containment error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceContainmentErrorCode {
    /// A stored TaskScope include/exclude pattern is illegal or not already normalized.
    InvalidScopePattern,
    /// A concrete resource URI is illegal under the Policy URI grammar.
    InvalidResourceUri,
}

impl ResourceContainmentErrorCode {
    /// Returns the stable machine code string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidScopePattern => "invalid_scope_pattern",
            Self::InvalidResourceUri => "invalid_resource_uri",
        }
    }
}

/// Which input array produced a containment validation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceContainmentInputKind {
    /// `TaskScope.resource_patterns[index]`.
    ResourcePattern,
    /// `TaskScope.exclusions[index]`.
    Exclusion,
    /// `ActionRequest.resource_refs[index]` (or equivalent concrete URI list).
    ResourceRef,
}

impl ResourceContainmentInputKind {
    /// Returns a stable diagnostic label for the input kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResourcePattern => "resource_pattern",
            Self::Exclusion => "exclusion",
            Self::ResourceRef => "resource_ref",
        }
    }
}

/// Structured fail-closed error for TaskScope resource containment.
///
/// The boolean result is intentionally separate: callers must not treat `Ok(false)` as an
/// authorization decision, and must not treat validation failures as out-of-scope matches.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "{code}: {input_kind}[{index}]",
    code = .code.as_str(),
    input_kind = .input_kind.as_str()
)]
pub struct ResourceContainmentError {
    /// Stable machine code.
    pub code: ResourceContainmentErrorCode,
    /// Which input array failed validation.
    pub input_kind: ResourceContainmentInputKind,
    /// Zero-based index inside the failing input array.
    pub index: usize,
    /// Underlying Policy URI grammar/normalization failure.
    #[source]
    source: PolicyError,
}

impl ResourceContainmentError {
    fn new(
        code: ResourceContainmentErrorCode,
        input_kind: ResourceContainmentInputKind,
        index: usize,
        source: PolicyError,
    ) -> Self {
        Self {
            code,
            input_kind,
            index,
            source,
        }
    }

    /// Returns the structured Policy URI failure without relying on display text.
    pub fn policy_error(&self) -> &PolicyError {
        &self.source
    }

    /// Returns the stable Policy error code from the underlying URI failure.
    pub fn policy_error_code(&self) -> PolicyErrorCode {
        self.source.code
    }
}

/// Returns whether every concrete resource ref is inside the stored TaskScope boundary.
///
/// Semantics (SECURITY §2.1 URI grammar + IMPLEMENTATION_CONTRACTS §6.9):
/// - empty `resource_patterns` means include is unrestricted;
/// - any matching exclusion rejects that resource (exclude wins over include);
/// - every resource must satisfy the boundary; empty `resource_refs` yields `Ok(true)` after
///   patterns are fully validated;
/// - stored include/exclude patterns must already be legal and normalized (normalize result
///   must equal the stored string), otherwise `InvalidScopePattern`;
/// - concrete resource refs are normalized before matching; illegal values yield
///   `InvalidResourceUri`;
/// - all inputs are fully validated before any containment `false` is returned, so an early
///   out-of-scope resource cannot hide a later illegal URI/pattern;
/// - array order and duplicates do not change the boolean result and are never mutated;
/// - `true`/`false` is pure containment, not a Policy authorization decision and not a scope
///   mutation.
///
/// Matching reuses the existing Policy URI parser and segment-glob matcher only. It deliberately
/// does not call PolicyRule resource applicability.
pub fn resource_refs_within_task_scope(
    resource_patterns: &[String],
    exclusions: &[String],
    resource_refs: &[String],
) -> Result<bool, ResourceContainmentError> {
    let includes = validate_stored_patterns(
        resource_patterns,
        ResourceContainmentInputKind::ResourcePattern,
    )?;
    let exclusions = validate_stored_patterns(exclusions, ResourceContainmentInputKind::Exclusion)?;
    let normalized_resources = validate_resource_refs(resource_refs)?;

    for resource in &normalized_resources {
        if exclusions
            .iter()
            .any(|pattern| uri_pattern_matches(pattern, resource))
        {
            return Ok(false);
        }
        if !includes.is_empty()
            && !includes
                .iter()
                .any(|pattern| uri_pattern_matches(pattern, resource))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_stored_patterns(
    patterns: &[String],
    input_kind: ResourceContainmentInputKind,
) -> Result<Vec<NormalizedUri>, ResourceContainmentError> {
    let mut normalized_patterns = Vec::with_capacity(patterns.len());
    for (index, pattern) in patterns.iter().enumerate() {
        match normalize_uri_pattern_value(pattern) {
            Ok(normalized) if normalized.value == *pattern => normalized_patterns.push(normalized),
            Ok(_) => {
                return Err(ResourceContainmentError::new(
                    ResourceContainmentErrorCode::InvalidScopePattern,
                    input_kind,
                    index,
                    PolicyError::new(
                        PolicyErrorCode::InvalidUriPattern,
                        "stored TaskScope URI pattern must already be normalized",
                    ),
                ));
            }
            Err(error) => {
                return Err(ResourceContainmentError::new(
                    ResourceContainmentErrorCode::InvalidScopePattern,
                    input_kind,
                    index,
                    error,
                ));
            }
        }
    }
    Ok(normalized_patterns)
}

fn validate_resource_refs(
    resource_refs: &[String],
) -> Result<Vec<NormalizedUri>, ResourceContainmentError> {
    let mut normalized = Vec::with_capacity(resource_refs.len());
    for (index, resource) in resource_refs.iter().enumerate() {
        match normalize_uri_value(resource) {
            Ok(value) => normalized.push(value),
            Err(error) => {
                return Err(ResourceContainmentError::new(
                    ResourceContainmentErrorCode::InvalidResourceUri,
                    ResourceContainmentInputKind::ResourceRef,
                    index,
                    error,
                ));
            }
        }
    }
    Ok(normalized)
}

// Keep `Error::source` available without forcing callers to import the trait solely for docs.
const _: fn(&ResourceContainmentError) -> Option<&(dyn Error + 'static)> =
    <ResourceContainmentError as Error>::source;
