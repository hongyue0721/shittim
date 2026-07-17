//! Property tests over generated Task/Action status closed sets (proptest).

use domain_task::{
    apply_action_transition, apply_task_transition, is_action_transition_allowed,
    is_task_transition_allowed, ActionEvidence, ActionTransitionCommand, DispatchCertainty,
    DomainTaskErrorCode, SideEffectRef, SuccessCriterionEvidence, TaskTransitionCommand,
    UncertainOutcomeReason, VerificationEvidenceSummary,
};
use kernel_contracts::{ActionStatus, TaskStatus, VerificationResultOutcome};
use proptest::prelude::*;

fn task_status_strategy() -> impl Strategy<Value = TaskStatus> {
    prop::sample::select(TaskStatus::ALL.to_vec())
}

fn action_status_strategy() -> impl Strategy<Value = ActionStatus> {
    prop::sample::select(ActionStatus::ALL.to_vec())
}

fn prepare_task(from: TaskStatus, to: TaskStatus) -> TaskTransitionCommand {
    let mut cmd = TaskTransitionCommand {
        current_status: from,
        current_revision: 1,
        current_plan_version: 0,
        expected_revision: Some(1),
        target_status: to,
        reason: "prop-test".into(),
        replan: matches!(
            (from, to),
            (TaskStatus::Failed, TaskStatus::Planned)
                | (TaskStatus::RolledBack, TaskStatus::Planned)
        ),
        required_success_criteria: Vec::new(),
        success_criteria_evidence: Vec::new(),
        produced_side_effect_refs: Vec::new(),
        rollback_required_side_effect_refs: Vec::new(),
    };
    if to == TaskStatus::Succeeded {
        cmd.required_success_criteria = vec!["goal content".into()];
        cmd.success_criteria_evidence = vec![SuccessCriterionEvidence {
            criterion: "goal content".into(),
            satisfied: true,
            verification: Some(VerificationEvidenceSummary {
                outcome: VerificationResultOutcome::VerifiedOk,
                verification_result_ref: Some("vr".into()),
                side_effect_confirmed: Some(true),
            }),
        }];
    }
    if to == TaskStatus::PartiallyCompleted {
        cmd.produced_side_effect_refs = vec![SideEffectRef::new("fx")];
    }
    if to == TaskStatus::RollingBack {
        cmd.rollback_required_side_effect_refs = vec![SideEffectRef::new("rb")];
    }
    cmd
}

fn prepare_action(from: ActionStatus, to: ActionStatus) -> ActionTransitionCommand {
    let mut cmd = ActionTransitionCommand {
        action_id: "act-prop".into(),
        parent_action_id: None,
        current_status: from,
        current_revision: 1,
        expected_revision: Some(1),
        target_status: to,
        reason: "prop-test".into(),
        evidence: ActionEvidence::default(),
    };
    match (from, to) {
        (ActionStatus::Pending, ActionStatus::Approved) => {
            cmd.evidence.permission_decision_ref = Some("pd".into());
        }
        (ActionStatus::Leased, ActionStatus::Approved) => {
            cmd.evidence.reason_code = Some("lease_expired".into());
        }
        (ActionStatus::Leased, ActionStatus::Cancelled) => {
            cmd.evidence.dispatch_certainty = Some(DispatchCertainty::NotStarted);
        }
        (ActionStatus::Leased, ActionStatus::UnknownSideEffect) => {
            cmd.evidence.dispatch_certainty = Some(DispatchCertainty::Uncertain);
        }
        (_, ActionStatus::Completed) => {
            cmd.evidence.verification = Some(VerificationEvidenceSummary {
                outcome: VerificationResultOutcome::VerifiedOk,
                verification_result_ref: Some("vr".into()),
                side_effect_confirmed: Some(true),
            });
        }
        (_, ActionStatus::Failed) => {
            cmd.evidence.verification = Some(VerificationEvidenceSummary {
                outcome: VerificationResultOutcome::VerifiedFailed,
                verification_result_ref: Some("vr".into()),
                side_effect_confirmed: Some(false),
            });
        }
        (ActionStatus::InFlight, ActionStatus::UnknownSideEffect) => {
            cmd.evidence.uncertain_outcome_reason = Some(UncertainOutcomeReason::Ambiguous);
        }
        _ => {}
    }
    cmd
}

proptest! {
    #[test]
    fn task_apply_agrees_with_graph(
        from in task_status_strategy(),
        to in task_status_strategy(),
    ) {
        let allowed = is_task_transition_allowed(from, to);
        let result = apply_task_transition(&prepare_task(from, to));
        if allowed {
            let out = result.expect("legal edge must apply");
            assert_eq!(out.new_status, to);
            assert_eq!(out.new_revision, 2);
            if matches!(
                (from, to),
                (TaskStatus::Failed, TaskStatus::Planned)
                    | (TaskStatus::RolledBack, TaskStatus::Planned)
            ) {
                assert!(out.plan_version_incremented);
                assert_eq!(out.new_plan_version, 1);
            } else {
                assert!(!out.plan_version_incremented);
                assert_eq!(out.new_plan_version, 0);
            }
        } else {
            prop_assert_eq!(result.unwrap_err().code, DomainTaskErrorCode::IllegalTransition);
        }
    }

    #[test]
    fn action_apply_agrees_with_graph(
        from in action_status_strategy(),
        to in action_status_strategy(),
    ) {
        let allowed = is_action_transition_allowed(from, to);
        let result = apply_action_transition(&prepare_action(from, to));
        if allowed {
            let out = result.expect("legal edge must apply");
            assert_eq!(out.new_status, to);
            assert_eq!(out.new_revision, 2);
            assert!(out.status_changed);
        } else {
            prop_assert_eq!(result.unwrap_err().code, DomainTaskErrorCode::IllegalTransition);
        }
    }

    #[test]
    fn wrong_expected_revision_always_conflicts(expected in 2u64..100u64) {
        let mut cmd = prepare_task(TaskStatus::Planned, TaskStatus::Running);
        cmd.current_revision = 1;
        cmd.expected_revision = Some(expected);
        let err = apply_task_transition(&cmd).unwrap_err();
        prop_assert_eq!(err.code, DomainTaskErrorCode::ExpectedRevisionConflict);
    }
}
