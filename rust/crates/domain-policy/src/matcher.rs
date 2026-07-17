use crate::rate_limit::{RateLimitKey, RateLimitPort, RateLimitPreview, RateLimitRequest};
use crate::types::{
    binding_material, KernelInvariantBlock, KernelInvariantState, PermissionDecisionDraft,
    PolicyEvaluationContext, PolicyEvaluationResult,
};
use crate::uri::{
    any_uri_pattern, best_uri_pattern, normalize_uri_value, NormalizedUri, UriPatternScore,
};
use crate::{PolicyError, PolicyErrorCode, RateLimitConsume};
use chrono::{Datelike, NaiveTime, Timelike};
use chrono_tz::Tz;
use kernel_contracts::{
    ActorAuthenticationLevel, PermissionDecisionDecision, PolicyRule,
    PolicyRuleActorMatchAuthLevelMin, PolicyRuleConditionRateLimit,
    PolicyRuleConditionRateLimitKeyScope, PolicyRuleConditionTimeWindow,
    PolicyRuleConfirmationMode, PolicyRuleEffect, SideEffectClass,
};
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// Deterministic SECURITY §2.3 specificity tuple.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    /// Number of constrained matching dimensions.
    pub constrained_dimension_count: i32,
    /// Number of exact constrained dimensions.
    pub exact_dimension_count: i32,
    /// Literal URI components selected by actually matching alternatives.
    pub literal_uri_component_count: i32,
    /// Negative count of `*` URI segments and `.*` action prefixes.
    pub negative_single_segment_glob_count: i32,
    /// Negative count of `**` URI segments.
    pub negative_multi_segment_glob_count: i32,
    /// Number of Condition v1 constraints.
    pub condition_constraint_count: i32,
}

/// Private matcher step outcome. Ordinary non-match is never a [`PolicyError`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum MatchOutcome<T> {
    Matched(T),
    NotMatched,
}

/// Private matcher step result: real failures stay in [`PolicyError`]; non-match is typed.
type MatchResult<T> = Result<MatchOutcome<T>, PolicyError>;

#[derive(Debug, Clone)]
struct Candidate<'a> {
    rule: &'a PolicyRule,
    specificity: Specificity,
    rate_limit: Option<RateLimitCandidate>,
}

#[derive(Debug, Clone)]
struct RateLimitCandidate {
    key: RateLimitKey,
    count: i64,
    window_seconds: i64,
}

/// Evaluates rules with Freedom-first Default Allow and invariant-first blocking.
pub fn evaluate_policy(
    rules: &[PolicyRule],
    context: &PolicyEvaluationContext,
    rate_limits: &dyn RateLimitPort,
) -> PolicyEvaluationResult {
    match &context.kernel_invariant {
        KernelInvariantState::StopFence { generation } => {
            return PolicyEvaluationResult::BlockedByKernelInvariant(
                KernelInvariantBlock::StopFence {
                    generation: *generation,
                },
            );
        }
        KernelInvariantState::Recovery { reason_code } => {
            return PolicyEvaluationResult::BlockedByKernelInvariant(
                KernelInvariantBlock::Recovery {
                    reason_code: reason_code.clone(),
                },
            );
        }
        KernelInvariantState::Clear => {}
    }
    match evaluate_policy_inner(rules, context, rate_limits) {
        Ok(result) => result,
        Err(error) => PolicyEvaluationResult::Error(error),
    }
}

fn evaluate_policy_inner(
    rules: &[PolicyRule],
    context: &PolicyEvaluationContext,
    rate_limits: &dyn RateLimitPort,
) -> Result<PolicyEvaluationResult, PolicyError> {
    validate_context(context)?;
    let normalized_resources = context
        .resource_refs
        .iter()
        .map(|resource| normalize_uri_value(resource))
        .collect::<Result<Vec<_>, _>>()?;

    let mut candidates = Vec::new();
    for rule in rules {
        match match_rule(rule, context, &normalized_resources, rate_limits)? {
            MatchOutcome::Matched(candidate) => candidates.push(candidate),
            MatchOutcome::NotMatched => {}
        }
    }
    candidates.sort_by(candidate_cmp);

    while let Some(candidate) = candidates.first().cloned() {
        if let Some(limit) = &candidate.rate_limit {
            let request = RateLimitRequest {
                rule_id: &candidate.rule.id,
                rule_revision: candidate.rule.revision,
                key: &limit.key,
                window_seconds: limit.window_seconds,
                count: limit.count,
                instant: context.evaluation_instant,
            };
            match rate_limits.check_and_consume(&request)? {
                RateLimitConsume::Consumed => return build_rule_result(candidate.rule, context),
                RateLimitConsume::Exceeded => {
                    candidates.remove(0);
                    continue;
                }
            }
        }
        return build_rule_result(candidate.rule, context);
    }
    build_default_allow(context)
}

fn validate_context(context: &PolicyEvaluationContext) -> Result<(), PolicyError> {
    if context.capability_id.is_empty() || context.operation.is_empty() {
        return Err(PolicyError::new(
            PolicyErrorCode::InvalidRule,
            "capability_id and operation must be non-empty",
        ));
    }
    Ok(())
}

fn match_rule<'a>(
    rule: &'a PolicyRule,
    context: &PolicyEvaluationContext,
    resources: &[NormalizedUri],
    rate_limits: &dyn RateLimitPort,
) -> MatchResult<Candidate<'a>> {
    if !rule.enabled || is_expired(rule, context)? {
        return Ok(MatchOutcome::NotMatched);
    }
    validate_rule_semantics(rule)?;
    let mut score = Specificity::default();

    if let Some(kind) = rule.actor_match.kind {
        constrain_exact(&mut score);
        if kind.as_str() != context.actor.kind.as_str() {
            return Ok(MatchOutcome::NotMatched);
        }
    }
    if let Some(patterns) = nonempty(rule.actor_match.source_patterns.as_deref()) {
        let actor_source = normalize_source_uri(&context.actor.source, "Actor source")?;
        match add_uri_score(&mut score, best_uri_pattern(patterns, &actor_source)?)? {
            MatchOutcome::Matched(()) => {}
            MatchOutcome::NotMatched => return Ok(MatchOutcome::NotMatched),
        }
    }
    if let Some(entry_point) = rule.actor_match.entry_point {
        constrain_exact(&mut score);
        if entry_point != context.entry_point {
            return Ok(MatchOutcome::NotMatched);
        }
    }
    if let Some(minimum) = rule.actor_match.auth_level_min {
        constrain_exact(&mut score);
        if auth_rank(context.actor.authentication_level) < rule_auth_rank(minimum) {
            return Ok(MatchOutcome::NotMatched);
        }
    }

    match match_content_origins(rule, context, &mut score)? {
        MatchOutcome::Matched(()) => {}
        MatchOutcome::NotMatched => return Ok(MatchOutcome::NotMatched),
    }
    match match_resources(rule, resources, &mut score)? {
        MatchOutcome::Matched(()) => {}
        MatchOutcome::NotMatched => return Ok(MatchOutcome::NotMatched),
    }
    match match_action(rule, context, &mut score)? {
        MatchOutcome::Matched(()) => {}
        MatchOutcome::NotMatched => return Ok(MatchOutcome::NotMatched),
    }
    let rate_limit = match match_conditions(rule, context, resources, rate_limits, &mut score)? {
        MatchOutcome::Matched(rate_limit) => rate_limit,
        MatchOutcome::NotMatched => return Ok(MatchOutcome::NotMatched),
    };

    Ok(MatchOutcome::Matched(Candidate {
        rule,
        specificity: score,
        rate_limit,
    }))
}

fn validate_rule_semantics(rule: &PolicyRule) -> Result<(), PolicyError> {
    if rule.id.is_empty() || rule.revision < 1 {
        return Err(PolicyError::new(
            PolicyErrorCode::InvalidRule,
            "PolicyRule id/revision is invalid",
        ));
    }
    match (rule.effect, rule.confirmation_mode) {
        (PolicyRuleEffect::Confirm, Some(_))
        | (PolicyRuleEffect::Allow | PolicyRuleEffect::Deny, None) => {}
        _ => {
            return Err(PolicyError::new(
                PolicyErrorCode::InvalidRule,
                "confirmation_mode must exist only for confirm",
            ));
        }
    }
    validate_action_patterns(&rule.action_match.capability_ids)?;
    validate_action_patterns(&rule.action_match.operation_patterns)?;
    for pattern in rule
        .actor_match
        .source_patterns
        .iter()
        .flatten()
        .chain(rule.content_origin_match.source_patterns.iter().flatten())
        .chain(rule.resource_match.scope_patterns.iter())
        .chain(rule.resource_match.exclude_patterns.iter())
    {
        let placeholder = normalize_uri_value("https://validation.invalid/")?;
        let _ = best_uri_pattern(std::slice::from_ref(pattern), &placeholder)?;
    }
    Ok(())
}

fn is_expired(rule: &PolicyRule, context: &PolicyEvaluationContext) -> Result<bool, PolicyError> {
    let Some(expires_at) = &rule.expires_at else {
        return Ok(false);
    };
    let expires = chrono::DateTime::parse_from_rfc3339(expires_at).map_err(|error| {
        PolicyError::new(
            PolicyErrorCode::InvalidTimestamp,
            format!("invalid expires_at for rule {}: {error}", rule.id),
        )
    })?;
    Ok(context.evaluation_instant >= expires.with_timezone(&chrono::Utc))
}

fn match_content_origins(
    rule: &PolicyRule,
    context: &PolicyEvaluationContext,
    score: &mut Specificity,
) -> MatchResult<()> {
    let kinds = nonempty(rule.content_origin_match.kinds.as_deref());
    let sources = nonempty(rule.content_origin_match.source_patterns.as_deref());
    if kinds.is_none() && sources.is_none() {
        return Ok(MatchOutcome::Matched(()));
    }
    if kinds.is_some() {
        constrain_exact(score);
    }
    if sources.is_some() {
        score.constrained_dimension_count += 1;
    }

    let mut best_source: Option<UriPatternScore> = None;
    let mut matched = false;
    for origin in &context.content_origins {
        let kind_matches = kinds.is_none_or(|items| {
            items
                .iter()
                .any(|item| item.as_str() == origin.kind.as_str())
        });
        if !kind_matches {
            continue;
        }
        let source_score = match sources {
            Some(patterns) => match origin.source_uri.as_deref() {
                Some(source) => {
                    let source = normalize_source_uri(source, "ContentOrigin source_uri")?;
                    best_uri_pattern(patterns, &source)?
                }
                None => None,
            },
            None => None,
        };
        if sources.is_some() && source_score.is_none() {
            continue;
        }
        matched = true;
        if let Some(source_score) = source_score {
            if best_source
                .as_ref()
                .is_none_or(|old| uri_score_cmp(&source_score, old).is_gt())
            {
                best_source = Some(source_score);
            }
        }
    }
    if !matched {
        return Ok(MatchOutcome::NotMatched);
    }
    if let Some(source_score) = best_source {
        accumulate_uri_score(score, &source_score);
    }
    Ok(MatchOutcome::Matched(()))
}

fn match_resources(
    rule: &PolicyRule,
    resources: &[NormalizedUri],
    score: &mut Specificity,
) -> MatchResult<()> {
    let includes = nonempty(Some(&rule.resource_match.scope_patterns));
    let excludes = nonempty(Some(&rule.resource_match.exclude_patterns));
    if (includes.is_some() || excludes.is_some()) && resources.is_empty() {
        return Ok(MatchOutcome::NotMatched);
    }
    if let Some(patterns) = excludes {
        for resource in resources {
            if any_uri_pattern(patterns, resource)? {
                return Ok(MatchOutcome::NotMatched);
            }
        }
    }
    if let Some(patterns) = includes {
        score.constrained_dimension_count += 1;
        let mut best = None;
        for resource in resources {
            if let Some(candidate) = best_uri_pattern(patterns, resource)? {
                if best
                    .as_ref()
                    .is_none_or(|old| uri_score_cmp(&candidate, old).is_gt())
                {
                    best = Some(candidate);
                }
            }
        }
        match add_uri_score(score, best)? {
            MatchOutcome::Matched(()) => {}
            MatchOutcome::NotMatched => return Ok(MatchOutcome::NotMatched),
        }
    }
    Ok(MatchOutcome::Matched(()))
}

fn match_action(
    rule: &PolicyRule,
    context: &PolicyEvaluationContext,
    score: &mut Specificity,
) -> MatchResult<()> {
    if let Some(patterns) = nonempty(Some(&rule.action_match.capability_ids)) {
        match add_action_score(
            score,
            best_action_pattern(patterns, &context.capability_id)?,
        )? {
            MatchOutcome::Matched(()) => {}
            MatchOutcome::NotMatched => return Ok(MatchOutcome::NotMatched),
        }
    }
    if let Some(patterns) = nonempty(Some(&rule.action_match.operation_patterns)) {
        match add_action_score(score, best_action_pattern(patterns, &context.operation)?)? {
            MatchOutcome::Matched(()) => {}
            MatchOutcome::NotMatched => return Ok(MatchOutcome::NotMatched),
        }
    }
    if let Some(ceiling) = rule.action_match.side_effect_max {
        constrain_exact(score);
        if side_effect_rank(context.side_effect_class) > side_effect_rank(ceiling) {
            return Ok(MatchOutcome::NotMatched);
        }
    }
    Ok(MatchOutcome::Matched(()))
}

fn match_conditions(
    rule: &PolicyRule,
    context: &PolicyEvaluationContext,
    resources: &[NormalizedUri],
    rate_limits: &dyn RateLimitPort,
    score: &mut Specificity,
) -> MatchResult<Option<RateLimitCandidate>> {
    if let Some(window) = &rule.condition.time_window {
        score.constrained_dimension_count += 1;
        score.condition_constraint_count += 1;
        if !time_window_matches(window, context)? {
            return Ok(MatchOutcome::NotMatched);
        }
    }
    let mut rate_limit_candidate = None;
    if let Some(limit) = &rule.condition.rate_limit {
        score.constrained_dimension_count += 1;
        score.condition_constraint_count += 1;
        validate_rate_limit(limit)?;
        let key = rate_limit_key(limit, rule, context, resources)?;
        let candidate = RateLimitCandidate {
            key,
            count: limit.count,
            window_seconds: limit.window_seconds,
        };
        let request = RateLimitRequest {
            rule_id: &rule.id,
            rule_revision: rule.revision,
            key: &candidate.key,
            window_seconds: limit.window_seconds,
            count: limit.count,
            instant: context.evaluation_instant,
        };
        if rate_limits.preview(&request)? == RateLimitPreview::Exceeded {
            return Ok(MatchOutcome::NotMatched);
        }
        rate_limit_candidate = Some(candidate);
    }
    if let Some(required) = rule.condition.delegation_required {
        constrain_condition_boolean(score);
        if context.delegation.is_some() != required {
            return Ok(MatchOutcome::NotMatched);
        }
    }
    if let Some(required) = rule.condition.local_presence_required {
        constrain_condition_boolean(score);
        if context.local_presence.is_some() != required {
            return Ok(MatchOutcome::NotMatched);
        }
    }
    Ok(MatchOutcome::Matched(rate_limit_candidate))
}

fn time_window_matches(
    window: &PolicyRuleConditionTimeWindow,
    context: &PolicyEvaluationContext,
) -> Result<bool, PolicyError> {
    let timezone: Tz = window.timezone.parse().map_err(|_| {
        PolicyError::new(
            PolicyErrorCode::UnsupportedPolicyCondition,
            format!("unsupported IANA timezone: {}", window.timezone),
        )
    })?;
    if window.weekdays.is_empty() {
        return Err(unsupported("time_window weekdays must not be empty"));
    }
    let mut weekdays = BTreeSet::new();
    for weekday in &window.weekdays {
        if !weekdays.insert(weekday.as_str()) {
            return Err(unsupported("time_window contains duplicate weekdays"));
        }
    }
    let start = parse_local_time(&window.local_start)?;
    let end = parse_local_time(&window.local_end)?;
    let local = context.evaluation_instant.with_timezone(&timezone);
    let seconds = local.time().num_seconds_from_midnight();
    let start_seconds = start.num_seconds_from_midnight();
    let end_seconds = end.num_seconds_from_midnight();
    let current = weekday_name(local.weekday());
    if start_seconds == end_seconds {
        return Ok(weekdays.contains(current));
    }
    if start_seconds < end_seconds {
        return Ok(weekdays.contains(current) && seconds >= start_seconds && seconds < end_seconds);
    }
    if seconds >= start_seconds {
        Ok(weekdays.contains(current))
    } else if seconds < end_seconds {
        Ok(weekdays.contains(previous_weekday_name(local.weekday())))
    } else {
        Ok(false)
    }
}

fn parse_local_time(value: &str) -> Result<NaiveTime, PolicyError> {
    NaiveTime::parse_from_str(value, "%H:%M:%S").map_err(|_| {
        PolicyError::new(
            PolicyErrorCode::UnsupportedPolicyCondition,
            format!("invalid local time: {value}"),
        )
    })
}

fn validate_rate_limit(limit: &PolicyRuleConditionRateLimit) -> Result<(), PolicyError> {
    if limit.count <= 0 || limit.window_seconds <= 0 {
        return Err(unsupported(
            "rate_limit count/window_seconds must be positive",
        ));
    }
    Ok(())
}

fn rate_limit_key(
    limit: &PolicyRuleConditionRateLimit,
    rule: &PolicyRule,
    context: &PolicyEvaluationContext,
    resources: &[NormalizedUri],
) -> Result<RateLimitKey, PolicyError> {
    let value = match limit.key_scope {
        PolicyRuleConditionRateLimitKeyScope::Rule => rule.id.clone(),
        PolicyRuleConditionRateLimitKeyScope::Actor => context.actor.id.clone(),
        PolicyRuleConditionRateLimitKeyScope::Task => context
            .task_id
            .clone()
            .ok_or_else(|| unsupported("task rate-limit key requires task_id"))?,
        PolicyRuleConditionRateLimitKeyScope::Action => context
            .action_id
            .clone()
            .ok_or_else(|| unsupported("action rate-limit key requires action_id"))?,
        PolicyRuleConditionRateLimitKeyScope::Resource => {
            if resources.is_empty() {
                return Err(unsupported(
                    "resource rate-limit key requires resource_refs",
                ));
            }
            let mut values: Vec<&str> = resources
                .iter()
                .map(|resource| resource.value.as_str())
                .collect();
            values.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            values.dedup();
            kernel_contracts::sha256_hex(values.join("\u{001f}").as_bytes())
        }
    };
    if value.is_empty() {
        return Err(unsupported("rate-limit key fact must be non-empty"));
    }
    Ok(RateLimitKey(value))
}

fn best_action_pattern(
    patterns: &[String],
    value: &str,
) -> Result<Option<(i32, usize, String)>, PolicyError> {
    let mut unique: Vec<&String> = patterns.iter().collect();
    unique.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    unique.dedup();
    let mut best: Option<(i32, usize, String)> = None;
    for pattern in unique {
        let candidate = if let Some(prefix) = pattern.strip_suffix(".*") {
            if prefix.is_empty()
                || prefix.contains('*')
                || !value.starts_with(&format!("{prefix}."))
                || value.len() <= prefix.len() + 1
            {
                None
            } else {
                Some((0, prefix.chars().count(), pattern.clone()))
            }
        } else if pattern.contains('*') {
            return Err(PolicyError::new(
                PolicyErrorCode::InvalidActionPattern,
                format!("invalid action pattern: {pattern}"),
            ));
        } else if pattern == value {
            Some((1, pattern.chars().count(), pattern.clone()))
        } else {
            None
        };
        if let Some(candidate) = candidate {
            if best
                .as_ref()
                .is_none_or(|old| action_score_cmp(&candidate, old).is_gt())
            {
                best = Some(candidate);
            }
        }
    }
    Ok(best)
}

fn validate_action_patterns(patterns: &[String]) -> Result<(), PolicyError> {
    for pattern in patterns {
        if pattern.is_empty() {
            return Err(PolicyError::new(
                PolicyErrorCode::InvalidActionPattern,
                "action pattern must be non-empty",
            ));
        }
        if let Some(prefix) = pattern.strip_suffix(".*") {
            if prefix.is_empty() || prefix.contains('*') {
                return Err(PolicyError::new(
                    PolicyErrorCode::InvalidActionPattern,
                    format!("invalid action prefix pattern: {pattern}"),
                ));
            }
        } else if pattern.contains('*') || pattern.contains('[') || pattern.contains('(') {
            return Err(PolicyError::new(
                PolicyErrorCode::InvalidActionPattern,
                format!("invalid action pattern: {pattern}"),
            ));
        }
    }
    Ok(())
}

fn candidate_cmp(left: &Candidate<'_>, right: &Candidate<'_>) -> Ordering {
    right
        .rule
        .priority
        .cmp(&left.rule.priority)
        .then_with(|| right.specificity.cmp(&left.specificity))
        .then_with(|| effect_rank(right.rule.effect).cmp(&effect_rank(left.rule.effect)))
        .then_with(|| right.rule.revision.cmp(&left.rule.revision))
        .then_with(|| left.rule.id.as_bytes().cmp(right.rule.id.as_bytes()))
}

fn build_default_allow(
    context: &PolicyEvaluationContext,
) -> Result<PolicyEvaluationResult, PolicyError> {
    let (binding, canonical_evaluation_input) = binding_material(context)?;
    Ok(PolicyEvaluationResult::Allowed(PermissionDecisionDraft {
        decision: PermissionDecisionDecision::Allow,
        reason_codes: vec!["default_allow".to_string()],
        matched_rule_ref: None,
        granted_scopes: binding.resource_refs.clone(),
        binding,
        canonical_evaluation_input,
    }))
}

fn build_rule_result(
    rule: &PolicyRule,
    context: &PolicyEvaluationContext,
) -> Result<PolicyEvaluationResult, PolicyError> {
    let (binding, canonical_evaluation_input) = binding_material(context)?;
    let decision = match (rule.effect, rule.confirmation_mode) {
        (PolicyRuleEffect::Allow, None) => PermissionDecisionDecision::Allow,
        (PolicyRuleEffect::Deny, None) => PermissionDecisionDecision::Deny,
        (PolicyRuleEffect::Confirm, Some(PolicyRuleConfirmationMode::Generic)) => {
            PermissionDecisionDecision::RequireConfirmation
        }
        (PolicyRuleEffect::Confirm, Some(PolicyRuleConfirmationMode::Local)) => {
            PermissionDecisionDecision::RequireLocalConfirmation
        }
        (PolicyRuleEffect::Confirm, Some(PolicyRuleConfirmationMode::SystemAuthentication)) => {
            PermissionDecisionDecision::RequireSystemAuthentication
        }
        (PolicyRuleEffect::Confirm, Some(PolicyRuleConfirmationMode::PlanRevision)) => {
            PermissionDecisionDecision::RequirePlanRevision
        }
        _ => {
            return Err(PolicyError::new(
                PolicyErrorCode::InvalidRule,
                "invalid effect/mode",
            ))
        }
    };
    let draft = PermissionDecisionDraft {
        decision,
        reason_codes: vec![rule.id.clone()],
        matched_rule_ref: Some(rule.id.clone()),
        granted_scopes: binding.resource_refs.clone(),
        binding,
        canonical_evaluation_input,
    };
    Ok(match rule.effect {
        PolicyRuleEffect::Allow => PolicyEvaluationResult::Allowed(draft),
        PolicyRuleEffect::Deny => PolicyEvaluationResult::Denied(draft),
        PolicyRuleEffect::Confirm => {
            let mode = rule.confirmation_mode.ok_or_else(|| {
                PolicyError::new(
                    PolicyErrorCode::InvalidRule,
                    "confirm rule lost confirmation_mode after validation",
                )
            })?;
            PolicyEvaluationResult::RequiresConfirmation(mode, draft)
        }
    })
}

fn constrain_exact(score: &mut Specificity) {
    score.constrained_dimension_count += 1;
    score.exact_dimension_count += 1;
}

fn constrain_condition_boolean(score: &mut Specificity) {
    constrain_exact(score);
    score.condition_constraint_count += 1;
}

fn add_uri_score(score: &mut Specificity, selected: Option<UriPatternScore>) -> MatchResult<()> {
    score.constrained_dimension_count += 1;
    let Some(selected) = selected else {
        return Ok(MatchOutcome::NotMatched);
    };
    accumulate_uri_score(score, &selected);
    Ok(MatchOutcome::Matched(()))
}

fn accumulate_uri_score(score: &mut Specificity, selected: &UriPatternScore) {
    score.exact_dimension_count += selected.exact;
    score.literal_uri_component_count += selected.literal_components;
    score.negative_single_segment_glob_count -= selected.single_globs;
    score.negative_multi_segment_glob_count -= selected.multi_globs;
}

fn add_action_score(
    score: &mut Specificity,
    selected: Option<(i32, usize, String)>,
) -> MatchResult<()> {
    score.constrained_dimension_count += 1;
    let Some((exact, _, _)) = selected else {
        return Ok(MatchOutcome::NotMatched);
    };
    score.exact_dimension_count += exact;
    if exact == 0 {
        score.negative_single_segment_glob_count -= 1;
    }
    Ok(MatchOutcome::Matched(()))
}

fn nonempty<T>(value: Option<&[T]>) -> Option<&[T]> {
    value.filter(|items| !items.is_empty())
}

fn auth_rank(level: ActorAuthenticationLevel) -> i32 {
    match level {
        ActorAuthenticationLevel::Unauthenticated => 0,
        ActorAuthenticationLevel::Asserted => 1,
        ActorAuthenticationLevel::PlatformVerified => 2,
        ActorAuthenticationLevel::SystemAuthenticated => 3,
    }
}

fn rule_auth_rank(level: PolicyRuleActorMatchAuthLevelMin) -> i32 {
    match level {
        PolicyRuleActorMatchAuthLevelMin::Unauthenticated => 0,
        PolicyRuleActorMatchAuthLevelMin::Asserted => 1,
        PolicyRuleActorMatchAuthLevelMin::PlatformVerified => 2,
        PolicyRuleActorMatchAuthLevelMin::SystemAuthenticated => 3,
    }
}

fn side_effect_rank(class: SideEffectClass) -> i32 {
    match class {
        SideEffectClass::S0 => 0,
        SideEffectClass::S1 => 1,
        SideEffectClass::S2 => 2,
        SideEffectClass::S3 => 3,
        SideEffectClass::S4 => 4,
        SideEffectClass::S5 => 5,
    }
}

fn effect_rank(effect: PolicyRuleEffect) -> i32 {
    match effect {
        PolicyRuleEffect::Allow => 0,
        PolicyRuleEffect::Confirm => 1,
        PolicyRuleEffect::Deny => 2,
    }
}

fn uri_score_cmp(left: &UriPatternScore, right: &UriPatternScore) -> Ordering {
    (
        left.exact,
        left.literal_components,
        -left.single_globs,
        -left.multi_globs,
    )
        .cmp(&(
            right.exact,
            right.literal_components,
            -right.single_globs,
            -right.multi_globs,
        ))
        .then_with(|| right.pattern.as_bytes().cmp(left.pattern.as_bytes()))
}

fn action_score_cmp(left: &(i32, usize, String), right: &(i32, usize, String)) -> Ordering {
    (left.0, left.1)
        .cmp(&(right.0, right.1))
        .then_with(|| right.2.as_bytes().cmp(left.2.as_bytes()))
}

fn weekday_name(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "monday",
        chrono::Weekday::Tue => "tuesday",
        chrono::Weekday::Wed => "wednesday",
        chrono::Weekday::Thu => "thursday",
        chrono::Weekday::Fri => "friday",
        chrono::Weekday::Sat => "saturday",
        chrono::Weekday::Sun => "sunday",
    }
}

fn previous_weekday_name(weekday: chrono::Weekday) -> &'static str {
    weekday_name(weekday.pred())
}

fn normalize_source_uri(value: &str, field: &str) -> Result<NormalizedUri, PolicyError> {
    normalize_uri_value(value).map_err(|error| {
        PolicyError::new(
            PolicyErrorCode::UnsupportedPolicyCondition,
            format!(
                "{field} cannot be normalized for Policy matching: {}",
                error.message
            ),
        )
    })
}

fn unsupported(message: impl Into<String>) -> PolicyError {
    PolicyError::new(PolicyErrorCode::UnsupportedPolicyCondition, message)
}
