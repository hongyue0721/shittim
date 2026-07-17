use chrono::{DateTime, TimeZone, Utc};
use domain_policy::{
    evaluate_policy, parse_policy_rule_json, DelegationCoverageEvidence, KernelInvariantState,
    LocalPresenceEvidence, PolicyError, PolicyErrorCode, PolicyEvaluationContext,
    PolicyEvaluationResult, RateLimitConsume, RateLimitPort, RateLimitPreview, RateLimitRequest,
    RejectRateLimits,
};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, ContentOrigin,
    ContentOriginCarrierRef, ContentOriginCarrierRefKind, ContentOriginKernelReceipt,
    ContentOriginKind, ContentOriginProducerRef, ContentOriginProducerRefKind,
    ContentOriginSchemaVersion, EntryPoint, PermissionDecisionDecision, PolicyRule,
    PolicyRuleActionMatch, PolicyRuleActorMatch, PolicyRuleCondition, PolicyRuleConditionRateLimit,
    PolicyRuleConditionRateLimitKeyScope, PolicyRuleConditionTimeWindow,
    PolicyRuleConditionTimeWindowWeekdaysItem, PolicyRuleConfirmationMode,
    PolicyRuleContentOriginMatch, PolicyRuleCreatedBy, PolicyRuleEffect, PolicyRuleResourceMatch,
    PolicyRuleSchemaVersion, PolicyRuleSource, PolicyRuleUpdatedBy, SideEffectClass,
};
use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;

fn actor(kind: ActorKind, source: &str, auth: ActorAuthenticationLevel) -> Actor {
    Actor {
        authentication_level: auth,
        confidence: None,
        id: "actor-1".into(),
        kind,
        revision: 1,
        schema_version: ActorSchemaVersion,
        source: source.into(),
    }
}

fn origin(kind: ContentOriginKind, source: Option<&str>) -> ContentOrigin {
    ContentOrigin {
        carrier_ref: ContentOriginCarrierRef {
            id: "carrier".into(),
            kind: ContentOriginCarrierRefKind::Task,
        },
        entry_point: EntryPoint::WebApi,
        id: "00000000-0000-0000-0000-000000000001".into(),
        kernel_receipt: ContentOriginKernelReceipt {
            content_hash: "0".repeat(64),
            receipt_id: "00000000-0000-0000-0000-000000000002".into(),
            recorded_at: "2025-01-01T00:00:00Z".into(),
        },
        kind,
        parent_origin_refs: vec![],
        producer_ref: ContentOriginProducerRef {
            id: "producer".into(),
            kind: ContentOriginProducerRefKind::System,
        },
        received_at: "2025-01-01T00:00:00Z".into(),
        schema_version: ContentOriginSchemaVersion,
        source_uri: source.map(str::to_string),
        upstream_stable_id: None,
    }
}

fn context(class: SideEffectClass) -> PolicyEvaluationContext {
    PolicyEvaluationContext {
        actor: actor(
            ActorKind::KnownUser,
            "actor-source://Platform/Account",
            ActorAuthenticationLevel::Asserted,
        ),
        entry_point: EntryPoint::LocalDesktop,
        content_origins: vec![origin(
            ContentOriginKind::UserInput,
            Some("https://example.com/input"),
        )],
        task_id: Some("00000000-0000-0000-0000-000000000010".into()),
        action_id: Some("00000000-0000-0000-0000-000000000011".into()),
        plan_version: 3,
        resource_refs: vec!["HTTPS://Example.COM:443/a/../doc/%2f".into()],
        capability_id: "document.write".into(),
        operation: "document.write.update".into(),
        side_effect_class: class,
        structured_arguments: json!({"b": 2, "a": 1}),
        delegation: None,
        local_presence: None,
        evaluation_instant: Utc.with_ymd_and_hms(2025, 1, 6, 10, 0, 0).unwrap(),
        security_mode: "normal".into(),
        kernel_invariant: KernelInvariantState::Clear,
    }
}

fn rule(id: &str, priority: i64, effect: PolicyRuleEffect) -> PolicyRule {
    let creator = actor(
        ActorKind::Companion,
        "actor-source://kernel/companion",
        ActorAuthenticationLevel::Unauthenticated,
    );
    PolicyRule {
        action_match: PolicyRuleActionMatch {
            capability_ids: vec![],
            operation_patterns: vec![],
            side_effect_max: None,
        },
        actor_match: PolicyRuleActorMatch {
            auth_level_min: None,
            entry_point: None,
            kind: None,
            source_patterns: None,
        },
        condition: PolicyRuleCondition {
            delegation_required: None,
            local_presence_required: None,
            rate_limit: None,
            time_window: None,
        },
        confirmation_mode: (effect == PolicyRuleEffect::Confirm)
            .then_some(PolicyRuleConfirmationMode::Generic),
        content_origin_match: PolicyRuleContentOriginMatch {
            kinds: None,
            source_patterns: None,
        },
        created_at: "2025-01-01T00:00:00Z".into(),
        created_by: PolicyRuleCreatedBy {
            actor: creator.clone(),
            entry_point: EntryPoint::SystemInternal,
        },
        description: String::new(),
        effect,
        enabled: true,
        expires_at: None,
        id: id.into(),
        name: id.into(),
        priority,
        resource_match: PolicyRuleResourceMatch {
            exclude_patterns: vec![],
            scope_patterns: vec![],
        },
        revision: 1,
        schema_version: PolicyRuleSchemaVersion,
        source: PolicyRuleSource::UserDefined,
        updated_at: "2025-01-01T00:00:00Z".into(),
        updated_by: PolicyRuleUpdatedBy {
            actor: creator,
            entry_point: EntryPoint::SystemInternal,
        },
    }
}

fn matched_id(result: PolicyEvaluationResult) -> Option<String> {
    match result {
        PolicyEvaluationResult::Allowed(draft)
        | PolicyEvaluationResult::Denied(draft)
        | PolicyEvaluationResult::RequiresConfirmation(_, draft) => draft.matched_rule_ref,
        _ => None,
    }
}

#[test]
fn every_side_effect_defaults_allow_and_templates_are_not_built_in() {
    for class in [
        SideEffectClass::S0,
        SideEffectClass::S1,
        SideEffectClass::S2,
        SideEffectClass::S3,
        SideEffectClass::S4,
        SideEffectClass::S5,
    ] {
        match evaluate_policy(&[], &context(class), &RejectRateLimits) {
            PolicyEvaluationResult::Allowed(draft) => {
                assert_eq!(draft.decision, PermissionDecisionDecision::Allow);
                assert_eq!(draft.reason_codes, ["default_allow"]);
                assert_eq!(draft.matched_rule_ref, None);
                assert_eq!(draft.binding.resource_refs, ["https://example.com/doc/%2F"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}

#[test]
fn disabled_expired_and_nonmatching_rules_default_allow() {
    let mut disabled = rule("disabled", 9, PolicyRuleEffect::Deny);
    disabled.enabled = false;
    let mut expired = rule("expired", 9, PolicyRuleEffect::Deny);
    expired.expires_at = Some("2025-01-06T09:59:59Z".into());
    let mut nonmatch = rule("nonmatch", 9, PolicyRuleEffect::Deny);
    nonmatch.action_match.capability_ids = vec!["mail.send".into()];
    for candidate in [disabled, expired, nonmatch] {
        assert_eq!(
            matched_id(evaluate_policy(
                &[candidate],
                &context(SideEffectClass::S5),
                &RejectRateLimits
            )),
            None
        );
    }
}

#[test]
fn priority_specificity_effect_revision_and_id_are_ordered() {
    let high_allow = rule("z", 2, PolicyRuleEffect::Allow);
    let low_deny = rule("a", 1, PolicyRuleEffect::Deny);
    assert_eq!(
        matched_id(evaluate_policy(
            &[low_deny, high_allow],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("z".into())
    );

    let generic_deny = rule("generic", 2, PolicyRuleEffect::Deny);
    let mut exact_allow = rule("exact", 2, PolicyRuleEffect::Allow);
    exact_allow.action_match.capability_ids = vec!["document.write".into()];
    assert_eq!(
        matched_id(evaluate_policy(
            &[generic_deny, exact_allow],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("exact".into())
    );

    let allow = rule("allow", 2, PolicyRuleEffect::Allow);
    let confirm = rule("confirm", 2, PolicyRuleEffect::Confirm);
    let deny = rule("deny", 2, PolicyRuleEffect::Deny);
    assert_eq!(
        matched_id(evaluate_policy(
            &[allow, confirm, deny],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("deny".into())
    );

    let mut older = rule("older", 2, PolicyRuleEffect::Allow);
    older.revision = 1;
    let mut newer = rule("newer", 2, PolicyRuleEffect::Allow);
    newer.revision = 2;
    assert_eq!(
        matched_id(evaluate_policy(
            &[older, newer],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("newer".into())
    );

    assert_eq!(
        matched_id(evaluate_policy(
            &[
                rule("b", 2, PolicyRuleEffect::Allow),
                rule("a", 2, PolicyRuleEffect::Allow)
            ],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("a".into())
    );
}

#[test]
fn unmatched_alternative_does_not_increase_specificity_and_exclude_wins() {
    let mut exact = rule("exact", 1, PolicyRuleEffect::Allow);
    exact.resource_match.scope_patterns = vec![
        "https://example.com/doc/%2F".into(),
        "https://never.invalid/a/b/c/d/e".into(),
    ];
    let mut glob = rule("glob", 1, PolicyRuleEffect::Deny);
    glob.resource_match.scope_patterns = vec!["https://example.com/**".into()];
    assert_eq!(
        matched_id(evaluate_policy(
            &[glob, exact.clone()],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("exact".into())
    );
    exact.resource_match.exclude_patterns = vec!["https://example.com/doc/*".into()];
    assert_eq!(
        matched_id(evaluate_policy(
            &[exact],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        None
    );
}

#[test]
fn action_patterns_auth_owner_and_side_ceiling_have_no_hidden_authority() {
    let mut candidate = rule("match", 1, PolicyRuleEffect::Deny);
    candidate.actor_match.kind = Some(kernel_contracts::PolicyRuleActorMatchKind::Owner);
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        None
    );
    candidate.actor_match.kind = None;
    candidate.actor_match.auth_level_min =
        Some(kernel_contracts::PolicyRuleActorMatchAuthLevelMin::PlatformVerified);
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        None
    );
    candidate.actor_match.auth_level_min = None;
    candidate.action_match.capability_ids = vec!["document.*".into()];
    candidate.action_match.operation_patterns = vec!["document.write.*".into()];
    candidate.action_match.side_effect_max = Some(SideEffectClass::S2);
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("match".into())
    );
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate],
            &context(SideEffectClass::S3),
            &RejectRateLimits
        )),
        None
    );
}

#[test]
fn invalid_action_pattern_is_error_not_default_allow() {
    let mut candidate = rule("bad", 1, PolicyRuleEffect::Deny);
    candidate.action_match.operation_patterns = vec!["document.*.write".into()];
    match evaluate_policy(
        &[candidate],
        &context(SideEffectClass::S2),
        &RejectRateLimits,
    ) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::InvalidActionPattern)
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn actor_source_and_content_origin_require_normalized_uri_and_same_origin() {
    let mut candidate = rule("origin", 1, PolicyRuleEffect::Deny);
    candidate.actor_match.source_patterns = Some(vec!["actor-source://platform/*".into()]);
    candidate.content_origin_match.kinds = Some(vec![
        kernel_contracts::PolicyRuleContentOriginMatchKindsItem::UserInput,
    ]);
    candidate.content_origin_match.source_patterns = Some(vec!["https://example.com/**".into()]);
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("origin".into())
    );

    let mut split = context(SideEffectClass::S2);
    split.content_origins = vec![
        origin(
            ContentOriginKind::UserInput,
            Some("https://other.example/a"),
        ),
        origin(ContentOriginKind::WebContent, Some("https://example.com/a")),
    ];
    assert_eq!(
        matched_id(evaluate_policy(&[candidate], &split, &RejectRateLimits)),
        None
    );
}

#[test]
fn specificity_counts_content_kind_and_source_as_separate_dimensions() {
    let mut content_rule = rule("content", 1, PolicyRuleEffect::Allow);
    content_rule.content_origin_match.kinds = Some(vec![
        kernel_contracts::PolicyRuleContentOriginMatchKindsItem::UserInput,
    ]);
    content_rule.content_origin_match.source_patterns = Some(vec!["https://example.com/**".into()]);

    let mut actor_rule = rule("actor", 1, PolicyRuleEffect::Deny);
    actor_rule.actor_match.kind = Some(kernel_contracts::PolicyRuleActorMatchKind::KnownUser);

    assert_eq!(
        matched_id(evaluate_policy(
            &[actor_rule, content_rule],
            &context(SideEffectClass::S2),
            &RejectRateLimits,
        )),
        Some("content".into())
    );
}

#[test]
fn invalid_actor_source_only_errors_when_source_dimension_is_used() {
    let mut invalid = context(SideEffectClass::S2);
    invalid.actor.source = "legacy-source".into();
    assert_eq!(
        matched_id(evaluate_policy(&[], &invalid, &RejectRateLimits)),
        None
    );

    let mut candidate = rule("source", 1, PolicyRuleEffect::Deny);
    candidate.actor_match.source_patterns = Some(vec!["actor-source://platform/**".into()]);
    match evaluate_policy(&[candidate], &invalid, &RejectRateLimits) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::UnsupportedPolicyCondition)
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn all_confirmation_modes_map_to_generated_decisions() {
    let cases = [
        (
            PolicyRuleConfirmationMode::Generic,
            PermissionDecisionDecision::RequireConfirmation,
        ),
        (
            PolicyRuleConfirmationMode::Local,
            PermissionDecisionDecision::RequireLocalConfirmation,
        ),
        (
            PolicyRuleConfirmationMode::SystemAuthentication,
            PermissionDecisionDecision::RequireSystemAuthentication,
        ),
        (
            PolicyRuleConfirmationMode::PlanRevision,
            PermissionDecisionDecision::RequirePlanRevision,
        ),
    ];
    for (mode, expected) in cases {
        let mut candidate = rule("confirm", 1, PolicyRuleEffect::Confirm);
        candidate.confirmation_mode = Some(mode);
        match evaluate_policy(
            &[candidate],
            &context(SideEffectClass::S5),
            &RejectRateLimits,
        ) {
            PolicyEvaluationResult::RequiresConfirmation(actual_mode, draft) => {
                assert_eq!(actual_mode, mode);
                assert_eq!(draft.decision, expected);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}

#[test]
fn delegation_and_presence_boole_are_exact() {
    let mut candidate = rule("conditions", 1, PolicyRuleEffect::Deny);
    candidate.condition.delegation_required = Some(false);
    candidate.condition.local_presence_required = Some(true);
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        None
    );
    let mut present = context(SideEffectClass::S2);
    present.local_presence = Some(LocalPresenceEvidence {
        evidence_ref: "presence".into(),
    });
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &present,
            &RejectRateLimits
        )),
        Some("conditions".into())
    );
    present.delegation = Some(DelegationCoverageEvidence {
        delegation_ref: "delegation".into(),
    });
    assert_eq!(
        matched_id(evaluate_policy(&[candidate], &present, &RejectRateLimits)),
        None
    );
}

#[test]
fn time_windows_cover_same_day_cross_midnight_all_day_and_dst() {
    let mut candidate = rule("time", 1, PolicyRuleEffect::Deny);
    candidate.condition.time_window = Some(PolicyRuleConditionTimeWindow {
        local_end: "11:00:00".into(),
        local_start: "09:00:00".into(),
        timezone: "Europe/Berlin".into(),
        weekdays: vec![PolicyRuleConditionTimeWindowWeekdaysItem::Monday],
    });
    let mut same_day = context(SideEffectClass::S2);
    same_day.evaluation_instant = Utc.with_ymd_and_hms(2025, 1, 6, 9, 30, 0).unwrap();
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &same_day,
            &RejectRateLimits
        )),
        Some("time".into())
    );

    candidate
        .condition
        .time_window
        .as_mut()
        .unwrap()
        .local_start = "23:00:00".into();
    candidate.condition.time_window.as_mut().unwrap().local_end = "02:00:00".into();
    let mut after_midnight = context(SideEffectClass::S2);
    after_midnight.evaluation_instant = Utc.with_ymd_and_hms(2025, 1, 6, 23, 30, 0).unwrap(); // Tue 00:30 local, belongs to Monday window
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &after_midnight,
            &RejectRateLimits
        )),
        Some("time".into())
    );

    candidate
        .condition
        .time_window
        .as_mut()
        .unwrap()
        .local_start = "00:00:00".into();
    candidate.condition.time_window.as_mut().unwrap().local_end = "00:00:00".into();
    assert_eq!(
        matched_id(evaluate_policy(
            &[candidate.clone()],
            &context(SideEffectClass::S2),
            &RejectRateLimits
        )),
        Some("time".into())
    );

    candidate.condition.time_window = Some(PolicyRuleConditionTimeWindow {
        local_end: "04:00:00".into(),
        local_start: "03:00:00".into(),
        timezone: "Europe/Berlin".into(),
        weekdays: vec![PolicyRuleConditionTimeWindowWeekdaysItem::Sunday],
    });
    let mut dst = context(SideEffectClass::S2);
    dst.evaluation_instant = Utc.with_ymd_and_hms(2025, 3, 30, 1, 30, 0).unwrap(); // 03:30 CEST
    assert_eq!(
        matched_id(evaluate_policy(&[candidate], &dst, &RejectRateLimits)),
        Some("time".into())
    );
}

#[test]
fn invalid_timezone_and_unknown_condition_fail_closed() {
    let mut candidate = rule("time", 1, PolicyRuleEffect::Deny);
    candidate.condition.time_window = Some(PolicyRuleConditionTimeWindow {
        local_end: "11:00:00".into(),
        local_start: "09:00:00".into(),
        timezone: "Mars/Olympus".into(),
        weekdays: vec![PolicyRuleConditionTimeWindowWeekdaysItem::Monday],
    });
    match evaluate_policy(
        &[candidate],
        &context(SideEffectClass::S2),
        &RejectRateLimits,
    ) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::UnsupportedPolicyCondition)
        }
        other => panic!("unexpected: {other:?}"),
    }

    let mut raw = serde_json::to_value(rule("raw", 1, PolicyRuleEffect::Allow)).unwrap();
    raw["condition"]["future_condition"] = json!(true);
    assert_eq!(
        parse_policy_rule_json(&raw).unwrap_err().code,
        PolicyErrorCode::UnsupportedPolicyCondition
    );
}

#[test]
fn stop_fence_and_recovery_are_not_policy_denies() {
    let mut stopped = context(SideEffectClass::S0);
    stopped.kernel_invariant = KernelInvariantState::StopFence { generation: 7 };
    assert!(matches!(
        evaluate_policy(
            &[rule("allow", 99, PolicyRuleEffect::Allow)],
            &stopped,
            &RejectRateLimits
        ),
        PolicyEvaluationResult::BlockedByKernelInvariant(_)
    ));
    stopped.kernel_invariant = KernelInvariantState::Recovery {
        reason_code: "unknown_side_effect".into(),
    };
    assert!(matches!(
        evaluate_policy(&[], &stopped, &RejectRateLimits),
        PolicyEvaluationResult::BlockedByKernelInvariant(_)
    ));
}

type RateEntryKey = (String, i64, String);
type RateEntries = BTreeMap<RateEntryKey, Vec<DateTime<Utc>>>;

#[derive(Default)]
struct MemoryRateLimit {
    entries: Mutex<RateEntries>,
}

impl RateLimitPort for MemoryRateLimit {
    fn preview(&self, request: &RateLimitRequest<'_>) -> Result<RateLimitPreview, PolicyError> {
        let entries = self.entries.lock().unwrap();
        Ok(
            if active_count(&entries, request) < request.count as usize {
                RateLimitPreview::Available
            } else {
                RateLimitPreview::Exceeded
            },
        )
    }

    fn check_and_consume(
        &self,
        request: &RateLimitRequest<'_>,
    ) -> Result<RateLimitConsume, PolicyError> {
        let mut entries = self.entries.lock().unwrap();
        let key = (
            request.rule_id.to_string(),
            request.rule_revision,
            request.key.0.clone(),
        );
        let instants = entries.entry(key).or_default();
        let boundary = request.instant - chrono::Duration::seconds(request.window_seconds);
        instants.retain(|instant| *instant > boundary);
        if instants.len() >= request.count as usize {
            Ok(RateLimitConsume::Exceeded)
        } else {
            instants.push(request.instant);
            Ok(RateLimitConsume::Consumed)
        }
    }
}

fn active_count(entries: &RateEntries, request: &RateLimitRequest<'_>) -> usize {
    let boundary = request.instant - chrono::Duration::seconds(request.window_seconds);
    entries
        .get(&(
            request.rule_id.to_string(),
            request.rule_revision,
            request.key.0.clone(),
        ))
        .map(|items| items.iter().filter(|instant| **instant > boundary).count())
        .unwrap_or(0)
}

fn rate_rule(id: &str, priority: i64) -> PolicyRule {
    let mut candidate = rule(id, priority, PolicyRuleEffect::Deny);
    candidate.condition.rate_limit = Some(PolicyRuleConditionRateLimit {
        count: 1,
        key_scope: PolicyRuleConditionRateLimitKeyScope::Actor,
        window_seconds: 60,
    });
    candidate
}

#[test]
fn only_final_winner_consumes_and_exhaustion_reselects() {
    let port = MemoryRateLimit::default();
    let high = rate_rule("high", 10);
    let low = rate_rule("low", 5);
    assert_eq!(
        matched_id(evaluate_policy(
            &[low.clone(), high.clone()],
            &context(SideEffectClass::S2),
            &port
        )),
        Some("high".into())
    );
    assert_eq!(
        matched_id(evaluate_policy(
            &[low.clone(), high.clone()],
            &context(SideEffectClass::S2),
            &port
        )),
        Some("low".into())
    );
    assert_eq!(
        matched_id(evaluate_policy(
            &[low, high],
            &context(SideEffectClass::S2),
            &port
        )),
        None
    );
    assert_eq!(
        port.entries
            .lock()
            .unwrap()
            .values()
            .map(Vec::len)
            .sum::<usize>(),
        2
    );
}

#[test]
fn rate_limit_window_boundary_expires_and_atomic_concurrency_allows_one() {
    let port = std::sync::Arc::new(MemoryRateLimit::default());
    let candidate = rate_rule("limited", 1);
    assert_eq!(
        matched_id(evaluate_policy(
            std::slice::from_ref(&candidate),
            &context(SideEffectClass::S2),
            port.as_ref()
        )),
        Some("limited".into())
    );
    let mut boundary = context(SideEffectClass::S2);
    boundary.evaluation_instant += chrono::Duration::seconds(60);
    assert_eq!(
        matched_id(evaluate_policy(
            std::slice::from_ref(&candidate),
            &boundary,
            port.as_ref()
        )),
        Some("limited".into())
    );

    let port = std::sync::Arc::new(MemoryRateLimit::default());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let port = port.clone();
            let candidate = candidate.clone();
            std::thread::spawn(move || {
                matched_id(evaluate_policy(
                    &[candidate],
                    &context(SideEffectClass::S2),
                    port.as_ref(),
                ))
            })
        })
        .collect();
    let winners = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(Option::is_some)
        .count();
    assert_eq!(winners, 1);
}

#[test]
fn disabled_invalid_rule_is_ignored_before_semantic_validation() {
    let mut disabled = rule("disabled-invalid", 99, PolicyRuleEffect::Allow);
    disabled.enabled = false;
    disabled.confirmation_mode = Some(PolicyRuleConfirmationMode::Generic);

    match evaluate_policy(
        &[disabled],
        &context(SideEffectClass::S2),
        &RejectRateLimits,
    ) {
        PolicyEvaluationResult::Allowed(draft) => {
            assert_eq!(draft.reason_codes, ["default_allow"]);
            assert_eq!(draft.matched_rule_ref, None);
        }
        other => panic!("disabled invalid rule must be ignored: {other:?}"),
    }
}

#[test]
fn invalid_timestamp_rate_limit_port_and_resource_uri_fail_closed() {
    let mut invalid_timestamp = rule("bad-time", 1, PolicyRuleEffect::Deny);
    invalid_timestamp.expires_at = Some("not-rfc3339".into());
    match evaluate_policy(
        &[invalid_timestamp],
        &context(SideEffectClass::S2),
        &RejectRateLimits,
    ) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::InvalidTimestamp)
        }
        other => panic!("invalid timestamp must fail closed: {other:?}"),
    }

    match evaluate_policy(
        &[rate_rule("limited", 1)],
        &context(SideEffectClass::S2),
        &RejectRateLimits,
    ) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::RateLimitFailed)
        }
        other => panic!("missing authoritative rate-limit port must fail: {other:?}"),
    }

    let mut invalid_resource = context(SideEffectClass::S2);
    invalid_resource.resource_refs = vec!["not a uri".into()];
    match evaluate_policy(&[], &invalid_resource, &RejectRateLimits) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::InvalidUriPattern)
        }
        other => panic!("invalid resource URI must fail closed: {other:?}"),
    }
}

#[test]
fn enabled_invalid_uri_pattern_fails_closed() {
    let mut candidate = rule("bad-uri", 1, PolicyRuleEffect::Deny);
    candidate.resource_match.scope_patterns = vec!["https://example.com/foo*".into()];
    match evaluate_policy(
        &[candidate],
        &context(SideEffectClass::S2),
        &RejectRateLimits,
    ) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::InvalidUriPattern)
        }
        other => panic!("enabled invalid URI pattern must fail closed: {other:?}"),
    }
}

/// Historical production debt used `PolicyErrorCode::InvalidRule` + magic message
/// `"__not_matched__"` as an ordinary non-match sentinel. After the typed-outcome fix,
/// a real `RateLimitPort` (crate-external authority) that returns that same code/message
/// pair must still surface as `PolicyEvaluationResult::Error`, never Default Allow.
#[test]
fn legacy_not_matched_sentinel_from_rate_limit_port_still_propagates_as_error() {
    struct SentinelCollisionPort;

    impl RateLimitPort for SentinelCollisionPort {
        fn preview(
            &self,
            _request: &RateLimitRequest<'_>,
        ) -> Result<RateLimitPreview, PolicyError> {
            Err(PolicyError {
                code: PolicyErrorCode::InvalidRule,
                message: "__not_matched__".into(),
            })
        }

        fn check_and_consume(
            &self,
            _request: &RateLimitRequest<'_>,
        ) -> Result<domain_policy::RateLimitConsume, PolicyError> {
            unreachable!("preview fails first")
        }
    }

    let candidate = rate_rule("sentinel-collision", 1);
    match evaluate_policy(
        &[candidate],
        &context(SideEffectClass::S2),
        &SentinelCollisionPort,
    ) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::InvalidRule);
            assert_eq!(error.message, "__not_matched__");
        }
        other => panic!(
            "legacy sentinel message from RateLimitPort must fail closed, not Default Allow: {other:?}"
        ),
    }
}

/// Ordinary non-match paths (URI include, action capability, condition, resource exclude)
/// must remain Freedom-first Default Allow and must not be routed through PolicyError.
#[test]
fn ordinary_uri_action_condition_resource_nonmatches_default_allow() {
    let mut uri_nonmatch = rule("uri-nonmatch", 9, PolicyRuleEffect::Deny);
    uri_nonmatch.resource_match.scope_patterns = vec!["https://other.example/doc".into()];

    let mut action_nonmatch = rule("action-nonmatch", 9, PolicyRuleEffect::Deny);
    action_nonmatch.action_match.capability_ids = vec!["mail.send".into()];

    let mut condition_nonmatch = rule("condition-nonmatch", 9, PolicyRuleEffect::Deny);
    condition_nonmatch.condition.delegation_required = Some(true);

    let mut exclude_nonmatch = rule("exclude-nonmatch", 9, PolicyRuleEffect::Deny);
    exclude_nonmatch.resource_match.scope_patterns = vec!["https://example.com/**".into()];
    exclude_nonmatch.resource_match.exclude_patterns = vec!["https://example.com/doc/%2F".into()];

    for candidate in [
        uri_nonmatch,
        action_nonmatch,
        condition_nonmatch,
        exclude_nonmatch,
    ] {
        match evaluate_policy(
            &[candidate],
            &context(SideEffectClass::S5),
            &RejectRateLimits,
        ) {
            PolicyEvaluationResult::Allowed(draft) => {
                assert_eq!(draft.decision, PermissionDecisionDecision::Allow);
                assert_eq!(draft.reason_codes, ["default_allow"]);
                assert_eq!(draft.matched_rule_ref, None);
            }
            other => panic!("ordinary non-match must Default Allow: {other:?}"),
        }
    }
}

#[test]
fn enabled_invalid_effect_mode_fails_closed_while_disabled_invalid_is_ignored() {
    let mut enabled_invalid = rule("enabled-invalid-mode", 99, PolicyRuleEffect::Allow);
    enabled_invalid.confirmation_mode = Some(PolicyRuleConfirmationMode::Generic);
    match evaluate_policy(
        &[enabled_invalid],
        &context(SideEffectClass::S2),
        &RejectRateLimits,
    ) {
        PolicyEvaluationResult::Error(error) => {
            assert_eq!(error.code, PolicyErrorCode::InvalidRule);
        }
        other => panic!("enabled invalid rule must fail closed: {other:?}"),
    }

    let mut disabled_invalid = rule("disabled-invalid-mode", 99, PolicyRuleEffect::Allow);
    disabled_invalid.enabled = false;
    disabled_invalid.confirmation_mode = Some(PolicyRuleConfirmationMode::Generic);
    match evaluate_policy(
        &[disabled_invalid],
        &context(SideEffectClass::S2),
        &RejectRateLimits,
    ) {
        PolicyEvaluationResult::Allowed(draft) => {
            assert_eq!(draft.reason_codes, ["default_allow"]);
            assert_eq!(draft.matched_rule_ref, None);
        }
        other => panic!("disabled invalid rule must be ignored: {other:?}"),
    }
}

proptest! {
    #[test]
    fn sorting_is_invariant_to_input_permutation(order in prop::collection::vec(0usize..4, 4)) {
        let rules = [
            rule("a", 1, PolicyRuleEffect::Allow),
            rule("b", 2, PolicyRuleEffect::Confirm),
            rule("c", 2, PolicyRuleEffect::Deny),
            rule("d", 0, PolicyRuleEffect::Deny),
        ];
        let mut permutation = Vec::new();
        for index in order { permutation.push(rules[index].clone()); }
        permutation.extend(rules.iter().cloned());
        prop_assert_eq!(matched_id(evaluate_policy(&permutation, &context(SideEffectClass::S2), &RejectRateLimits)), Some("c".into()));
    }
}
