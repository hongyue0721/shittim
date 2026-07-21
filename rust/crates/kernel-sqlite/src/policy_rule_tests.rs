//! PolicyRuleV2 repository tests (slice 4b).

use super::*;
use chrono::{TimeZone, Utc};
use kernel_contracts::{
    Actor, ActorAuthenticationLevel, ActorKind, ActorSchemaVersion, ConfirmationModeV1, EntryPoint,
    PolicyRuleV2, PolicyRuleV2ActionMatch, PolicyRuleV2ActorMatch, PolicyRuleV2Condition,
    PolicyRuleV2ContentOriginMatch, PolicyRuleV2CreatedBy, PolicyRuleV2Effect,
    PolicyRuleV2ResourceMatch, PolicyRuleV2SchemaVersion, PolicyRuleV2Source,
    PolicyRuleV2UpdatedBy,
};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

struct PolicyDatabase {
    _directory: TempDir,
    path: PathBuf,
    config: SqliteConfig,
}

impl PolicyDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("directory");
        Self {
            path: directory.path().join("policy.sqlite3"),
            _directory: directory,
            config: SqliteConfig::new(Duration::from_secs(2)).expect("config"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path, self.config).expect("open")
    }
}

#[test]
fn migration_0007_bootstraps_empty_policy_set_revision_zero() {
    let database = PolicyDatabase::new();
    let store = database.open();
    assert_eq!(store.get_policy_set_revision().expect("rev"), 0);
    assert!(store.list_current_policy_rules().expect("list").is_empty());
}

#[test]
fn append_policy_rule_increments_policy_set_and_canonical_readback() {
    let database = PolicyDatabase::new();
    let store = database.open();
    let rule = sample_rule("rule-allow", 1, PolicyRuleV2Effect::Allow, true, None);
    let result = store
        .with_write_transaction(|tx| tx.append_policy_rule_revision(rule.clone()))
        .expect("append");
    assert_eq!(result.policy_set_revision, 1);
    assert_eq!(result.rule, rule);
    let loaded = store
        .get_policy_rule_revision("rule-allow", 1)
        .expect("get")
        .expect("exists");
    assert_eq!(loaded, rule);
    assert_eq!(store.get_policy_set_revision().expect("rev"), 1);
    let heads = store.list_current_policy_rules().expect("heads");
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0], rule);
}

#[test]
fn policy_rule_revision_must_be_continuous() {
    let database = PolicyDatabase::new();
    let store = database.open();
    let first = sample_rule("rule-a", 1, PolicyRuleV2Effect::Deny, true, None);
    store
        .with_write_transaction(|tx| tx.append_policy_rule_revision(first))
        .expect("first");
    let gap = sample_rule("rule-a", 3, PolicyRuleV2Effect::Deny, true, None);
    assert_eq!(
        store
            .with_write_transaction(|tx| tx.append_policy_rule_revision(gap))
            .expect_err("gap")
            .code,
        StoreErrorCode::ConstraintViolation
    );
    let second = sample_rule("rule-a", 2, PolicyRuleV2Effect::Deny, false, None);
    let result = store
        .with_write_transaction(|tx| tx.append_policy_rule_revision(second.clone()))
        .expect("second");
    assert_eq!(result.policy_set_revision, 2);
    let current = store
        .get_current_policy_rule("rule-a")
        .expect("current")
        .expect("exists");
    assert_eq!(current, second);
    assert!(!current.enabled);
}

#[test]
fn empty_policy_set_is_legal_and_mutations_are_transactional() {
    let database = PolicyDatabase::new();
    let store = database.open();
    assert_eq!(store.get_policy_set_revision().expect("rev"), 0);
    let bad = sample_rule("rule-b", 2, PolicyRuleV2Effect::Allow, true, None);
    let _ = store
        .with_write_transaction(|tx| tx.append_policy_rule_revision(bad))
        .expect_err("first must be 1");
    assert_eq!(store.get_policy_set_revision().expect("still 0"), 0);
    assert!(store
        .get_policy_rule_revision("rule-b", 2)
        .expect("get")
        .is_none());
}

#[test]
fn confirm_rule_requires_confirmation_mode_on_schema() {
    let database = PolicyDatabase::new();
    let store = database.open();
    let mut rule = sample_rule("rule-confirm", 1, PolicyRuleV2Effect::Confirm, true, None);
    // Schema rejects confirm without mode.
    assert_eq!(
        store
            .with_write_transaction(|tx| tx.append_policy_rule_revision(rule.clone()))
            .expect_err("schema")
            .code,
        StoreErrorCode::ContractInvalid
    );
    rule.confirmation_mode = Some(ConfirmationModeV1::Generic);
    store
        .with_write_transaction(|tx| tx.append_policy_rule_revision(rule))
        .expect("ok");
}

fn sample_rule(
    id: &str,
    revision: i64,
    effect: PolicyRuleV2Effect,
    enabled: bool,
    confirmation_mode: Option<ConfirmationModeV1>,
) -> PolicyRuleV2 {
    let actor = Actor {
        authentication_level: ActorAuthenticationLevel::PlatformVerified,
        confidence: Some(0.9),
        id: "actor-policy".into(),
        kind: ActorKind::KnownUser,
        revision: 1,
        schema_version: ActorSchemaVersion,
        source: "actor-source://local/desktop".into(),
    };
    let now = Utc
        .with_ymd_and_hms(2026, 7, 21, 12, 0, 0)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    PolicyRuleV2 {
        id: id.into(),
        schema_version: PolicyRuleV2SchemaVersion,
        revision,
        name: id.into(),
        description: "test rule".into(),
        priority: 100,
        enabled,
        actor_match: PolicyRuleV2ActorMatch {
            kind: None,
            source_patterns: None,
            entry_point: None,
            auth_level_min: None,
        },
        content_origin_match: PolicyRuleV2ContentOriginMatch {
            kinds: None,
            source_patterns: None,
        },
        resource_match: PolicyRuleV2ResourceMatch {
            scope_patterns: vec![],
            exclude_patterns: vec![],
        },
        action_match: PolicyRuleV2ActionMatch {
            capability_ids: vec![],
            operation_patterns: vec![],
            side_effect_max: None,
        },
        condition: PolicyRuleV2Condition {
            time_window: None,
            rate_limit: None,
            delegation_required: None,
            local_presence_required: None,
        },
        effect,
        confirmation_mode,
        expires_at: None,
        created_by: PolicyRuleV2CreatedBy {
            actor: actor.clone(),
            entry_point: EntryPoint::LocalDesktop,
        },
        updated_by: PolicyRuleV2UpdatedBy {
            actor,
            entry_point: EntryPoint::LocalDesktop,
        },
        created_at: now.clone(),
        updated_at: now,
        source: PolicyRuleV2Source::UserDefined,
    }
}
