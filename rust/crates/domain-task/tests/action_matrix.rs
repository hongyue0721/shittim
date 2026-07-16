//! ActionStatus NxN matrix, legal paths, lease, confirm, compensation, recovery.

use domain_task::{
    apply_action_transition, apply_policy_evaluation_outcome, is_action_transition_allowed,
    validate_compensation_action_draft, validate_retry_original_candidate, ActionEvidence,
    ActionTransitionCommand, CompensationActionDraft, DispatchCertainty, DomainTaskErrorCode,
    PolicyEvaluationEffect, PolicyEvaluationOutcome, RetryOriginalFacts, UncertainOutcomeReason,
    VerificationEvidenceSummary, ACTION_STATUS_CATALOG,
};
use kernel_contracts::{ActionStatus, VerificationResultOutcome};

fn base_cmd(from: ActionStatus, to: ActionStatus) -> ActionTransitionCommand {
    ActionTransitionCommand {
        action_id: "act-1".into(),
        parent_action_id: None,
        current_status: from,
        current_revision: 1,
        expected_revision: None,
        target_status: to,
        reason: "test".to_string(),
        evidence: ActionEvidence::default(),
    }
}

fn verified_ok() -> VerificationEvidenceSummary {
    VerificationEvidenceSummary {
        outcome: VerificationResultOutcome::VerifiedOk,
        verification_result_ref: Some("vr-1".into()),
        side_effect_confirmed: Some(true),
    }
}

fn verified_failed() -> VerificationEvidenceSummary {
    VerificationEvidenceSummary {
        outcome: VerificationResultOutcome::VerifiedFailed,
        verification_result_ref: Some("vr-fail".into()),
        side_effect_confirmed: Some(false),
    }
}

fn prepare(mut cmd: ActionTransitionCommand) -> ActionTransitionCommand {
    use ActionStatus::*;
    match (cmd.current_status, cmd.target_status) {
        (Pending, Approved) => {
            cmd.evidence.permission_decision_ref = Some("pd-1".into());
        }
        (Leased, Approved) => {
            cmd.evidence.reason_code = Some("lease_expired".into());
        }
        (Leased, Cancelled) => {
            cmd.evidence.dispatch_certainty = Some(DispatchCertainty::NotStarted);
        }
        (Leased, UnknownSideEffect) => {
            cmd.evidence.dispatch_certainty = Some(DispatchCertainty::Uncertain);
        }
        (_, Completed) => {
            cmd.evidence.verification = Some(verified_ok());
        }
        (_, Failed) => {
            cmd.evidence.verification = Some(verified_failed());
        }
        (InFlight, UnknownSideEffect) => {
            cmd.evidence.uncertain_outcome_reason = Some(UncertainOutcomeReason::Crash);
        }
        _ => {}
    }
    cmd
}

#[test]
fn action_graph_nxn_no_self_loops() {
    let mut legal = 0usize;
    for &from in ACTION_STATUS_CATALOG {
        for &to in ACTION_STATUS_CATALOG {
            let allowed = is_action_transition_allowed(from, to);
            if from == to {
                assert!(
                    !allowed,
                    "self-loop must not be graph-legal: {}",
                    from.as_str()
                );
            }
            if allowed {
                legal += 1;
            }
        }
    }
    assert_eq!(legal, 17);
    assert!(!is_action_transition_allowed(
        ActionStatus::Pending,
        ActionStatus::Pending
    ));
}

#[test]
fn action_nxn_illegal_graph_edges_are_illegal_transition() {
    for &from in ACTION_STATUS_CATALOG {
        for &to in ACTION_STATUS_CATALOG {
            if is_action_transition_allowed(from, to) {
                continue;
            }
            let cmd = base_cmd(from, to);
            let err = apply_action_transition(&cmd).unwrap_err();
            assert_eq!(
                err.code,
                DomainTaskErrorCode::IllegalTransition,
                "illegal graph {} -> {} got {}",
                from.as_str(),
                to.as_str(),
                err
            );
        }
    }
}

#[test]
fn action_legal_edges_prepared_apply_ok() {
    for &from in ACTION_STATUS_CATALOG {
        for &to in ACTION_STATUS_CATALOG {
            if !is_action_transition_allowed(from, to) {
                continue;
            }
            let out = apply_action_transition(&prepare(base_cmd(from, to))).unwrap_or_else(|e| {
                panic!(
                    "prepared legal {} -> {} failed: {e}",
                    from.as_str(),
                    to.as_str()
                )
            });
            assert_eq!(out.new_status, to);
            assert!(out.status_changed);
            assert_eq!(out.event_intents.len(), 1);
        }
    }
}

#[test]
fn legal_edge_without_evidence_is_missing_evidence_not_illegal_transition() {
    let cmd = base_cmd(ActionStatus::InFlight, ActionStatus::Completed);
    assert!(is_action_transition_allowed(
        ActionStatus::InFlight,
        ActionStatus::Completed
    ));
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    let cmd = base_cmd(ActionStatus::InFlight, ActionStatus::Failed);
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    let cmd = base_cmd(ActionStatus::InFlight, ActionStatus::UnknownSideEffect);
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);
}

#[test]
fn happy_path_pending_to_completed() {
    let steps = [
        (ActionStatus::Pending, ActionStatus::Approved),
        (ActionStatus::Approved, ActionStatus::Leased),
        (ActionStatus::Leased, ActionStatus::InFlight),
        (ActionStatus::InFlight, ActionStatus::Completed),
    ];
    let mut rev = 1u64;
    let mut status = ActionStatus::Pending;
    for (from, to) in steps {
        assert_eq!(status, from);
        let mut cmd = prepare(base_cmd(from, to));
        cmd.current_revision = rev;
        let out = apply_action_transition(&cmd).unwrap();
        assert!(out.status_changed);
        assert_eq!(out.new_revision, rev + 1);
        rev = out.new_revision;
        status = out.new_status;
    }
    assert_eq!(status, ActionStatus::Completed);
}

#[test]
fn completed_requires_verified_ok_not_provider_success() {
    let mut cmd = base_cmd(ActionStatus::InFlight, ActionStatus::Completed);
    cmd.evidence.provider_reported_success = true;
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    cmd.evidence.verification = Some(VerificationEvidenceSummary {
        outcome: VerificationResultOutcome::Inconclusive,
        verification_result_ref: None,
        side_effect_confirmed: None,
    });
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);

    cmd.evidence.verification = Some(verified_ok());
    let out = apply_action_transition(&cmd).unwrap();
    assert_eq!(out.new_status, ActionStatus::Completed);
}

#[test]
fn failed_requires_verification_evidence() {
    let cmd = base_cmd(ActionStatus::InFlight, ActionStatus::Failed);
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    let mut cmd = base_cmd(ActionStatus::InFlight, ActionStatus::Failed);
    cmd.evidence.verification = Some(verified_ok());
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);

    let mut cmd = base_cmd(ActionStatus::InFlight, ActionStatus::Failed);
    cmd.evidence.verification = Some(VerificationEvidenceSummary {
        outcome: VerificationResultOutcome::VerifiedFailed,
        verification_result_ref: Some("vr".into()),
        side_effect_confirmed: Some(true),
    });
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);

    let mut cmd = base_cmd(ActionStatus::InFlight, ActionStatus::Failed);
    cmd.evidence.verification = Some(VerificationEvidenceSummary {
        outcome: VerificationResultOutcome::Inconclusive,
        verification_result_ref: None,
        side_effect_confirmed: None,
    });
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);

    let mut cmd = base_cmd(ActionStatus::InFlight, ActionStatus::Failed);
    cmd.evidence.verification = Some(VerificationEvidenceSummary {
        outcome: VerificationResultOutcome::Inconclusive,
        verification_result_ref: Some("vr".into()),
        side_effect_confirmed: Some(false),
    });
    let out = apply_action_transition(&cmd).unwrap();
    assert_eq!(out.new_status, ActionStatus::Failed);

    let out = apply_action_transition(&prepare(base_cmd(
        ActionStatus::InFlight,
        ActionStatus::Failed,
    )))
    .unwrap();
    assert_eq!(out.new_status, ActionStatus::Failed);

    let cmd = base_cmd(ActionStatus::UnknownSideEffect, ActionStatus::Failed);
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);
    let out = apply_action_transition(&prepare(cmd)).unwrap();
    assert_eq!(out.new_status, ActionStatus::Failed);
    assert_eq!(out.event_intents.len(), 1);
}

#[test]
fn confirm_is_metadata_update_not_graph_edge() {
    assert!(!is_action_transition_allowed(
        ActionStatus::Pending,
        ActionStatus::Pending
    ));

    let err = apply_action_transition(&base_cmd(ActionStatus::Pending, ActionStatus::Pending))
        .unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::IllegalTransition);

    let outcome = PolicyEvaluationOutcome {
        effect: PolicyEvaluationEffect::Confirm,
        permission_decision_ref: "pd-1".into(),
        approval_record_ref: None,
        reason: "needs user confirm".into(),
    };
    let err =
        apply_policy_evaluation_outcome("act-1", None, ActionStatus::Pending, 1, None, &outcome)
            .unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    let outcome = PolicyEvaluationOutcome {
        effect: PolicyEvaluationEffect::Confirm,
        permission_decision_ref: "pd-1".into(),
        approval_record_ref: Some("ar-1".into()),
        reason: "needs user confirm".into(),
    };
    let out =
        apply_policy_evaluation_outcome("act-1", None, ActionStatus::Pending, 1, None, &outcome)
            .unwrap();
    assert_eq!(out.new_status, ActionStatus::Pending);
    assert!(!out.status_changed);
    assert!(out.effects.requires_approval_record_ref);
    assert_eq!(out.new_revision, 2);
    assert!(out.event_intents.is_empty());
}

#[test]
fn policy_allow_and_deny() {
    let allow = PolicyEvaluationOutcome {
        effect: PolicyEvaluationEffect::Allow,
        permission_decision_ref: "pd-allow".into(),
        approval_record_ref: None,
        reason: "default_allow".into(),
    };
    let out =
        apply_policy_evaluation_outcome("act-1", None, ActionStatus::Pending, 1, None, &allow)
            .unwrap();
    assert_eq!(out.new_status, ActionStatus::Approved);

    let deny = PolicyEvaluationOutcome {
        effect: PolicyEvaluationEffect::Deny,
        permission_decision_ref: "pd-deny".into(),
        approval_record_ref: None,
        reason: "matched deny rule".into(),
    };
    let out = apply_policy_evaluation_outcome("act-1", None, ActionStatus::Pending, 2, None, &deny)
        .unwrap();
    assert_eq!(out.new_status, ActionStatus::Cancelled);
}

#[test]
fn lease_expired_emits_atomic_release_effects_bound_to_action_id() {
    let cmd = prepare(base_cmd(ActionStatus::Leased, ActionStatus::Approved));
    let out = apply_action_transition(&cmd).unwrap();
    let release = out.effects.release_lease_and_locks.expect("release effect");
    assert!(release.invalidate_lease);
    assert!(release.release_all_resource_locks);
    assert_eq!(release.reason, "lease_expired");
    assert_eq!(release.action_id, "act-1");
}

#[test]
fn empty_action_id_rejected() {
    let mut cmd = prepare(base_cmd(ActionStatus::Approved, ActionStatus::Leased));
    cmd.action_id = "  ".into();
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvalidInput);
}

#[test]
fn parent_action_id_empty_or_self_rejected() {
    let mut cmd = prepare(base_cmd(ActionStatus::Approved, ActionStatus::Leased));
    cmd.parent_action_id = Some("  ".into());
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvalidInput);

    let mut cmd = prepare(base_cmd(ActionStatus::Approved, ActionStatus::Leased));
    cmd.parent_action_id = Some("act-1".into());
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvalidInput);
}

#[test]
fn leased_to_approved_without_lease_expired_rejected() {
    let mut cmd = base_cmd(ActionStatus::Leased, ActionStatus::Approved);
    cmd.evidence.reason_code = Some("timeout".into());
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);
}

#[test]
fn leased_cancel_requires_dispatch_not_started() {
    let mut cmd = base_cmd(ActionStatus::Leased, ActionStatus::Cancelled);
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    cmd.evidence.dispatch_certainty = Some(DispatchCertainty::Uncertain);
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);

    cmd.evidence.dispatch_certainty = Some(DispatchCertainty::NotStarted);
    let out = apply_action_transition(&cmd).unwrap();
    assert_eq!(out.new_status, ActionStatus::Cancelled);
    let release = out.effects.release_lease_and_locks.unwrap();
    assert_eq!(release.action_id, "act-1");
}

#[test]
fn dispatch_uncertain_and_in_flight_unknown_forbid_replay() {
    let out = apply_action_transition(&prepare(base_cmd(
        ActionStatus::Leased,
        ActionStatus::UnknownSideEffect,
    )))
    .unwrap();
    assert_eq!(out.new_status, ActionStatus::UnknownSideEffect);
    assert!(out.effects.forbid_automatic_replay);
    assert_eq!(
        out.effects.release_lease_and_locks.unwrap().action_id,
        "act-1"
    );
    assert_eq!(out.event_intents.len(), 1);

    let mut cmd = base_cmd(ActionStatus::InFlight, ActionStatus::UnknownSideEffect);
    cmd.evidence.uncertain_outcome_reason = Some(UncertainOutcomeReason::Timeout);
    let out = apply_action_transition(&cmd).unwrap();
    assert!(out.effects.forbid_automatic_replay);
    assert_eq!(out.event_intents.len(), 1);
}

#[test]
fn compensation_draft_validation() {
    let ok = CompensationActionDraft {
        action_id: "act-comp".into(),
        parent_action_id: "act-orig".into(),
        idempotency_key: "idem-comp".into(),
        original_action_id: "act-orig".into(),
        original_idempotency_key: "idem-orig".into(),
        status: ActionStatus::Pending,
        permission_decision_ref: None,
    };
    validate_compensation_action_draft(&ok).unwrap();

    let mut bad = ok.clone();
    bad.action_id = "act-orig".into();
    assert_eq!(
        validate_compensation_action_draft(&bad).unwrap_err().code,
        DomainTaskErrorCode::IllegalCompensationDraft
    );
}

#[test]
fn compensation_parent_cannot_enter_rollback_orchestration() {
    // parent_action_id Some => compensation; cannot rolling_back
    let mut cmd = prepare(base_cmd(ActionStatus::Failed, ActionStatus::RollingBack));
    cmd.parent_action_id = Some("act-orig".into());
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);

    let mut cmd = base_cmd(ActionStatus::RollingBack, ActionStatus::RollbackFailed);
    cmd.parent_action_id = Some("act-orig".into());
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);

    let mut cmd = base_cmd(ActionStatus::RollingBack, ActionStatus::RolledBack);
    cmd.parent_action_id = Some("act-orig".into());
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);

    // compensation ordinary completed ok
    let mut cmd = prepare(base_cmd(ActionStatus::InFlight, ActionStatus::Completed));
    cmd.parent_action_id = Some("act-orig".into());
    let out = apply_action_transition(&cmd).unwrap();
    assert_eq!(out.new_status, ActionStatus::Completed);

    // compensation ordinary failed ok
    let mut cmd = prepare(base_cmd(ActionStatus::InFlight, ActionStatus::Failed));
    cmd.parent_action_id = Some("act-orig".into());
    let out = apply_action_transition(&cmd).unwrap();
    assert_eq!(out.new_status, ActionStatus::Failed);

    // original (None) may rolling_back
    let out = apply_action_transition(&prepare(base_cmd(
        ActionStatus::Failed,
        ActionStatus::RollingBack,
    )))
    .unwrap();
    assert_eq!(out.new_status, ActionStatus::RollingBack);
}

#[test]
fn rollback_failed_only_from_rolling_back_for_original() {
    let err = apply_action_transition(&base_cmd(
        ActionStatus::Failed,
        ActionStatus::RollbackFailed,
    ))
    .unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::IllegalTransition);

    let out = apply_action_transition(&base_cmd(
        ActionStatus::RollingBack,
        ActionStatus::RollbackFailed,
    ))
    .unwrap();
    assert_eq!(out.new_status, ActionStatus::RollbackFailed);
}

#[test]
fn original_action_rolling_back_to_rolled_back_after_compensation_success() {
    let out = apply_action_transition(&prepare(base_cmd(
        ActionStatus::Failed,
        ActionStatus::RollingBack,
    )))
    .unwrap();
    assert_eq!(out.new_status, ActionStatus::RollingBack);
    let out = apply_action_transition(&ActionTransitionCommand {
        action_id: "act-1".into(),
        parent_action_id: None,
        current_status: ActionStatus::RollingBack,
        current_revision: out.new_revision,
        expected_revision: None,
        target_status: ActionStatus::RolledBack,
        reason: "compensation completed".into(),
        evidence: ActionEvidence::default(),
    })
    .unwrap();
    assert_eq!(out.new_status, ActionStatus::RolledBack);
}

#[test]
fn retry_original_recovery_legality() {
    validate_retry_original_candidate(&RetryOriginalFacts {
        side_effect_confirmed: Some(false),
        original_idempotency_guaranteed: true,
    })
    .unwrap();

    let err = validate_retry_original_candidate(&RetryOriginalFacts {
        side_effect_confirmed: Some(true),
        original_idempotency_guaranteed: true,
    })
    .unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::IllegalRecoveryCandidate);
}

#[test]
fn expected_revision_conflict_on_action() {
    let mut cmd = prepare(base_cmd(ActionStatus::Approved, ActionStatus::Leased));
    cmd.current_revision = 9;
    cmd.expected_revision = Some(8);
    let err = apply_action_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::ExpectedRevisionConflict);
}

#[test]
fn terminals_have_no_exits() {
    for from in [
        ActionStatus::Completed,
        ActionStatus::RolledBack,
        ActionStatus::RollbackFailed,
        ActionStatus::Cancelled,
    ] {
        for &to in ACTION_STATUS_CATALOG {
            assert!(
                !is_action_transition_allowed(from, to),
                "terminal {} must not go to {}",
                from.as_str(),
                to.as_str()
            );
        }
    }
}
