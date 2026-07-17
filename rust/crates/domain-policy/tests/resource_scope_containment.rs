use std::error::Error;

use domain_policy::{
    resource_refs_within_task_scope, ResourceContainmentErrorCode, ResourceContainmentInputKind,
};

fn s(value: &str) -> String {
    value.to_string()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().copied().map(s).collect()
}

#[test]
fn empty_include_is_unrestricted() {
    let includes = strings(&[]);
    let exclusions = strings(&[]);
    let resources = strings(&["https://example.com/any/path"]);
    assert!(resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap());
}

#[test]
fn empty_include_still_applies_exclusions() {
    let includes = strings(&[]);
    let exclusions = strings(&["https://example.com/private/**"]);
    let allowed = strings(&["https://example.com/public/item"]);
    let excluded = strings(&["https://example.com/private/item"]);

    assert!(resource_refs_within_task_scope(&includes, &exclusions, &allowed).unwrap());
    assert!(!resource_refs_within_task_scope(&includes, &exclusions, &excluded).unwrap());
}

#[test]
fn exclusion_rejects_even_when_include_matches() {
    let includes = strings(&["https://example.com/docs/**"]);
    let exclusions = strings(&["https://example.com/docs/secret/**"]);
    let inside = strings(&["https://example.com/docs/readme.md"]);
    let excluded = strings(&["https://example.com/docs/secret/key"]);
    assert!(resource_refs_within_task_scope(&includes, &exclusions, &inside).unwrap());
    assert!(!resource_refs_within_task_scope(&includes, &exclusions, &excluded).unwrap());
}

#[test]
fn two_resources_may_hit_different_includes() {
    let includes = strings(&["https://example.com/a/**", "https://example.com/b/**"]);
    let exclusions = strings(&[]);
    let resources = strings(&["https://example.com/a/1", "https://example.com/b/2"]);
    assert!(resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap());
}

#[test]
fn one_resource_out_of_include_fails_containment() {
    let includes = strings(&["https://example.com/a/**"]);
    let exclusions = strings(&[]);
    let resources = strings(&["https://example.com/a/1", "https://example.com/b/2"]);
    assert!(!resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap());
}

#[test]
fn include_and_exclude_same_pattern_is_excluded() {
    let includes = strings(&["https://example.com/a/**"]);
    let exclusions = strings(&["https://example.com/a/**"]);
    let resources = strings(&["https://example.com/a/1"]);
    assert!(!resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap());
}

#[test]
fn empty_resources_with_valid_patterns_is_true() {
    let includes = strings(&["https://example.com/a/**"]);
    let exclusions = strings(&["https://example.com/a/secret/*"]);
    let resources = strings(&[]);
    assert!(resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap());
}

#[test]
fn empty_resources_still_reject_invalid_patterns() {
    let includes = strings(&["https://example.com/foo*"]);
    let exclusions = strings(&[]);
    let resources = strings(&[]);
    let error = resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap_err();
    assert_eq!(
        error.code,
        ResourceContainmentErrorCode::InvalidScopePattern
    );
    assert_eq!(
        error.input_kind,
        ResourceContainmentInputKind::ResourcePattern
    );
    assert_eq!(error.index, 0);
    assert_eq!(
        error.policy_error_code(),
        domain_policy::PolicyErrorCode::InvalidUriPattern
    );
}

#[test]
fn stored_pattern_must_already_be_normalized() {
    let includes = strings(&["HTTPS://Example.COM:443/a/**"]);
    let exclusions = strings(&[]);
    let resources = strings(&["https://example.com/a/1"]);
    let error = resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap_err();
    assert_eq!(
        error.code,
        ResourceContainmentErrorCode::InvalidScopePattern
    );
    assert_eq!(
        error.input_kind,
        ResourceContainmentInputKind::ResourcePattern
    );
    assert_eq!(error.index, 0);

    let includes = strings(&["https://example.com/a/**"]);
    let exclusions = strings(&["HTTPS://Example.COM/a/secret/*"]);
    let error = resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap_err();
    assert_eq!(
        error.code,
        ResourceContainmentErrorCode::InvalidScopePattern
    );
    assert_eq!(error.input_kind, ResourceContainmentInputKind::Exclusion);
    assert_eq!(error.index, 0);
}

#[test]
fn invalid_resource_uri_is_structured_error() {
    let includes = strings(&["https://example.com/**"]);
    let exclusions = strings(&[]);
    let resources = strings(&["https://example.com/foo*"]);
    let error = resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap_err();
    assert_eq!(error.code, ResourceContainmentErrorCode::InvalidResourceUri);
    assert_eq!(error.input_kind, ResourceContainmentInputKind::ResourceRef);
    assert_eq!(error.index, 0);
    let source = error.source().expect("underlying PolicyError");
    assert_eq!(source.to_string(), error.policy_error().to_string());
}

#[test]
fn invalid_pattern_precedes_ordinary_out_of_scope_result() {
    let includes = strings(&["https://example.com/a/**", "https://example.com/bad*"]);
    let exclusions = strings(&[]);
    let resources = strings(&["https://example.com/outside"]);
    let error = resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap_err();

    assert_eq!(
        error.code,
        ResourceContainmentErrorCode::InvalidScopePattern
    );
    assert_eq!(
        error.input_kind,
        ResourceContainmentInputKind::ResourcePattern
    );
    assert_eq!(error.index, 1);
}

#[test]
fn out_of_scope_then_later_invalid_resource_returns_error_not_false() {
    let includes = strings(&["https://example.com/a/**"]);
    let exclusions = strings(&[]);
    let resources = strings(&["https://example.com/outside", "https://example.com/foo*"]);
    let error = resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap_err();
    assert_eq!(error.code, ResourceContainmentErrorCode::InvalidResourceUri);
    assert_eq!(error.input_kind, ResourceContainmentInputKind::ResourceRef);
    assert_eq!(error.index, 1);
}

#[test]
fn single_and_multi_segment_globs_match() {
    let includes = strings(&["https://example.com/*/docs/**"]);
    let exclusions = strings(&[]);
    let matched = strings(&["https://example.com/tenant/docs/a/b"]);
    let missed = strings(&["https://example.com/tenant/other/a"]);
    assert!(resource_refs_within_task_scope(&includes, &exclusions, &matched).unwrap());
    assert!(!resource_refs_within_task_scope(&includes, &exclusions, &missed).unwrap());
}

#[test]
fn multi_segment_glob_matches_zero_trailing_segments() {
    let includes = strings(&["https://example.com/a/**"]);
    let exclusions = strings(&[]);
    let resource = strings(&["https://example.com/a"]);

    assert!(resource_refs_within_task_scope(&includes, &exclusions, &resource).unwrap());
}

#[test]
fn query_and_fragment_follow_existing_exact_semantics() {
    let includes = strings(&["https://example.com/a?q=1#part"]);
    let exclusions = strings(&[]);
    let exact = strings(&["https://example.com/a?q=1#part"]);
    let different_query = strings(&["https://example.com/a?q=2#part"]);
    let different_fragment = strings(&["https://example.com/a?q=1#other"]);
    // Pattern without query/fragment does not constrain those parts.
    let unconstrained = strings(&["https://example.com/a"]);
    assert!(resource_refs_within_task_scope(&includes, &exclusions, &exact).unwrap());
    assert!(!resource_refs_within_task_scope(&includes, &exclusions, &different_query).unwrap());
    assert!(!resource_refs_within_task_scope(&includes, &exclusions, &different_fragment).unwrap());
    assert!(resource_refs_within_task_scope(&unconstrained, &exclusions, &exact).unwrap());
}

#[test]
fn concrete_resource_is_normalized_before_matching() {
    let includes = strings(&["https://example.com/a/c"]);
    let exclusions = strings(&[]);
    let resources = strings(&["HTTPS://Example.COM:443/a/./b/../c"]);
    assert!(resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap());
}

#[test]
fn order_and_duplicates_do_not_change_result_or_mutate_inputs() {
    let includes_a = strings(&[
        "https://example.com/a/**",
        "https://example.com/b/**",
        "https://example.com/a/**",
    ]);
    let includes_b = strings(&["https://example.com/b/**", "https://example.com/a/**"]);
    let exclusions_a = strings(&[
        "https://example.com/a/secret/*",
        "https://example.com/a/secret/*",
    ]);
    let exclusions_b = strings(&["https://example.com/a/secret/*"]);
    let resources_a = strings(&[
        "https://example.com/a/1",
        "https://example.com/b/2",
        "https://example.com/a/1",
    ]);
    let resources_b = strings(&["https://example.com/b/2", "https://example.com/a/1"]);

    let includes_a_before = includes_a.clone();
    let exclusions_a_before = exclusions_a.clone();
    let resources_a_before = resources_a.clone();

    let left = resource_refs_within_task_scope(&includes_a, &exclusions_a, &resources_a).unwrap();
    let right = resource_refs_within_task_scope(&includes_b, &exclusions_b, &resources_b).unwrap();
    assert!(left);
    assert!(right);
    assert_eq!(includes_a, includes_a_before);
    assert_eq!(exclusions_a, exclusions_a_before);
    assert_eq!(resources_a, resources_a_before);

    let excluded = strings(&["https://example.com/a/secret/x", "https://example.com/b/2"]);
    assert!(!resource_refs_within_task_scope(&includes_a, &exclusions_a, &excluded).unwrap());
    assert!(!resource_refs_within_task_scope(
        &includes_b,
        &exclusions_b,
        &strings(&["https://example.com/b/2", "https://example.com/a/secret/x"])
    )
    .unwrap());
}

#[test]
fn invalid_exclusion_is_reported_with_index() {
    let includes = strings(&["https://example.com/**"]);
    let exclusions = strings(&["https://example.com/ok/*", "https://example.com/bad*"]);
    let resources = strings(&["https://example.com/ok/1"]);
    let error = resource_refs_within_task_scope(&includes, &exclusions, &resources).unwrap_err();
    assert_eq!(
        error.code,
        ResourceContainmentErrorCode::InvalidScopePattern
    );
    assert_eq!(error.input_kind, ResourceContainmentInputKind::Exclusion);
    assert_eq!(error.index, 1);
}
