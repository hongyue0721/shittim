//! TaskStatus NxN matrix, legal paths, invariants, revision, plan_version.

use domain_task::{
    apply_task_transition, is_task_transition_allowed, DomainTaskErrorCode, SideEffectRef,
    SuccessCriterionEvidence, TaskTransitionCommand, VerificationEvidenceSummary,
    TASK_STATUS_CATALOG,
};
use kernel_contracts::{TaskStatus, VerificationResultOutcome};

fn base_cmd(from: TaskStatus, to: TaskStatus) -> TaskTransitionCommand {
    TaskTransitionCommand {
        current_status: from,
        current_revision: 1,
        current_plan_version: 0,
        expected_revision: None,
        target_status: to,
        reason: "test".to_string(),
        replan: false,
        required_success_criteria: Vec::new(),
        success_criteria_evidence: Vec::new(),
        produced_side_effect_refs: Vec::new(),
        rollback_required_side_effect_refs: Vec::new(),
    }
}

fn criterion(content: &str) -> SuccessCriterionEvidence {
    SuccessCriterionEvidence {
        criterion: content.to_string(),
        satisfied: true,
        verification: Some(VerificationEvidenceSummary {
            outcome: VerificationResultOutcome::VerifiedOk,
            verification_result_ref: Some(format!("vr-{content}")),
            side_effect_confirmed: Some(true),
        }),
    }
}

fn with_success_cover(mut cmd: TaskTransitionCommand, contents: &[&str]) -> TaskTransitionCommand {
    cmd.required_success_criteria = contents.iter().map(|s| (*s).to_string()).collect();
    cmd.success_criteria_evidence = contents.iter().map(|c| criterion(c)).collect();
    cmd
}

fn with_partial_refs(mut cmd: TaskTransitionCommand) -> TaskTransitionCommand {
    cmd.produced_side_effect_refs = vec![SideEffectRef::new("effect://1")];
    cmd
}

fn with_rollback_refs(mut cmd: TaskTransitionCommand) -> TaskTransitionCommand {
    cmd.rollback_required_side_effect_refs = vec![SideEffectRef::new("effect://need-comp")];
    cmd
}

fn prepare(cmd: TaskTransitionCommand) -> TaskTransitionCommand {
    let mut cmd = cmd;
    if cmd.target_status == TaskStatus::Succeeded {
        cmd = with_success_cover(cmd, &["goal met"]);
    }
    if cmd.target_status == TaskStatus::PartiallyCompleted {
        cmd = with_partial_refs(cmd);
    }
    if cmd.target_status == TaskStatus::RollingBack {
        cmd = with_rollback_refs(cmd);
    }
    if matches!(
        (cmd.current_status, cmd.target_status),
        (TaskStatus::Failed, TaskStatus::Planned) | (TaskStatus::RolledBack, TaskStatus::Planned)
    ) {
        cmd.replan = true;
    }
    cmd
}

#[test]
fn task_nxn_matrix_matches_graph_and_apply() {
    for &from in TASK_STATUS_CATALOG {
        for &to in TASK_STATUS_CATALOG {
            let allowed = is_task_transition_allowed(from, to);
            let cmd = prepare(base_cmd(from, to));
            let result = apply_task_transition(&cmd);
            if allowed {
                result.unwrap_or_else(|e| {
                    panic!("expected ok for {} -> {}: {e}", from.as_str(), to.as_str())
                });
            } else {
                let err = result.expect_err("expected illegal");
                assert_eq!(
                    err.code,
                    DomainTaskErrorCode::IllegalTransition,
                    "{} -> {}: {err}",
                    from.as_str(),
                    to.as_str()
                );
            }
        }
    }
}

#[test]
fn task_create_plan_version_zero_preserved_until_replan() {
    let mut cmd = prepare(base_cmd(TaskStatus::Candidate, TaskStatus::Planned));
    cmd.current_revision = 1;
    cmd.current_plan_version = 0;
    let out = apply_task_transition(&cmd).unwrap();
    assert_eq!(out.new_status, TaskStatus::Planned);
    assert_eq!(out.new_plan_version, 0);
    assert!(!out.plan_version_incremented);
    assert_eq!(out.new_revision, 2);

    let mut cmd = prepare(base_cmd(TaskStatus::Planned, TaskStatus::Running));
    cmd.current_revision = 2;
    cmd.current_plan_version = 0;
    let out = apply_task_transition(&cmd).unwrap();
    assert_eq!(out.new_plan_version, 0);
    assert!(!out.plan_version_incremented);
}

#[test]
fn every_legal_path_increments_revision() {
    let paths = [
        (TaskStatus::Candidate, TaskStatus::Planned),
        (TaskStatus::Planned, TaskStatus::Running),
        (TaskStatus::Running, TaskStatus::Succeeded),
        (TaskStatus::Succeeded, TaskStatus::Archived),
    ];
    let mut revision = 1u64;
    let mut status = TaskStatus::Candidate;
    let mut plan_version = 0u64;
    for (from, to) in paths {
        assert_eq!(status, from);
        let mut cmd = prepare(base_cmd(from, to));
        cmd.current_revision = revision;
        cmd.current_plan_version = plan_version;
        let out = apply_task_transition(&cmd).unwrap();
        assert_eq!(out.new_revision, revision + 1);
        assert!(!out.plan_version_incremented);
        assert_eq!(out.new_plan_version, plan_version);
        revision = out.new_revision;
        status = out.new_status;
        plan_version = out.new_plan_version;
    }
    assert_eq!(status, TaskStatus::Archived);
    assert_eq!(plan_version, 0);
}

#[test]
fn failed_to_planned_increments_plan_version_from_zero() {
    let mut cmd = prepare(base_cmd(TaskStatus::Failed, TaskStatus::Planned));
    cmd.current_plan_version = 0;
    let out = apply_task_transition(&cmd).unwrap();
    assert!(out.plan_version_incremented);
    assert_eq!(out.new_plan_version, 1);
}

#[test]
fn rolled_back_to_planned_increments_plan_version() {
    let mut cmd = prepare(base_cmd(TaskStatus::RolledBack, TaskStatus::Planned));
    cmd.current_plan_version = 2;
    let out = apply_task_transition(&cmd).unwrap();
    assert!(out.plan_version_incremented);
    assert_eq!(out.new_plan_version, 3);
}

#[test]
fn non_replan_does_not_bump_plan_version() {
    let mut cmd = prepare(base_cmd(TaskStatus::Running, TaskStatus::Failed));
    cmd.current_plan_version = 5;
    let out = apply_task_transition(&cmd).unwrap();
    assert!(!out.plan_version_incremented);
    assert_eq!(out.new_plan_version, 5);
}

#[test]
fn replan_flag_on_non_replan_edge_rejected() {
    let mut cmd = base_cmd(TaskStatus::Running, TaskStatus::Failed);
    cmd.replan = true;
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);
}

#[test]
fn expected_revision_conflict() {
    let mut cmd = prepare(base_cmd(TaskStatus::Planned, TaskStatus::Running));
    cmd.current_revision = 4;
    cmd.expected_revision = Some(3);
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::ExpectedRevisionConflict);
    assert!(err.message.contains("expected 3"));
}

#[test]
fn succeeded_requires_exact_criteria_content_multiset() {
    // missing required list
    let mut cmd = base_cmd(TaskStatus::Running, TaskStatus::Succeeded);
    cmd.success_criteria_evidence = vec![criterion("a")];
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    // missing one occurrence
    let mut cmd = with_success_cover(
        base_cmd(TaskStatus::Running, TaskStatus::Succeeded),
        &["a", "b"],
    );
    cmd.success_criteria_evidence.pop();
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);
    assert!(err.message.contains("missing") || err.message.contains("criterion content"));

    // extra evidence content
    let mut cmd = with_success_cover(base_cmd(TaskStatus::Running, TaskStatus::Succeeded), &["a"]);
    cmd.success_criteria_evidence.push(criterion("extra"));
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);
    assert!(err.message.contains("extra"));

    // duplicate same content twice in required: needs two evidence entries
    let cmd = with_success_cover(
        base_cmd(TaskStatus::Running, TaskStatus::Succeeded),
        &["same", "same"],
    );
    let out = apply_task_transition(&cmd).unwrap();
    assert_eq!(out.new_status, TaskStatus::Succeeded);

    // duplicate required with only one evidence fails
    let mut cmd = with_success_cover(
        base_cmd(TaskStatus::Running, TaskStatus::Succeeded),
        &["same", "same"],
    );
    cmd.success_criteria_evidence = vec![criterion("same")];
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvariantViolation);
    assert!(err.message.contains("same"));

    // full exact multiset cover ok (distinct contents)
    let cmd = with_success_cover(
        base_cmd(TaskStatus::Running, TaskStatus::Succeeded),
        &["goal A", "goal B"],
    );
    let out = apply_task_transition(&cmd).unwrap();
    assert_eq!(out.new_status, TaskStatus::Succeeded);
}

#[test]
fn partially_completed_requires_side_effect_refs() {
    let cmd = base_cmd(TaskStatus::Running, TaskStatus::PartiallyCompleted);
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    let cmd = with_partial_refs(cmd);
    let out = apply_task_transition(&cmd).unwrap();
    assert_eq!(out.new_status, TaskStatus::PartiallyCompleted);
}

#[test]
fn rolling_back_requires_explicit_side_effect_refs() {
    let mut cmd = base_cmd(TaskStatus::PartiallyCompleted, TaskStatus::RollingBack);
    cmd.produced_side_effect_refs = vec![SideEffectRef::new("effect://old")];
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::MissingEvidence);

    for from in [
        TaskStatus::Running,
        TaskStatus::WaitingUser,
        TaskStatus::Paused,
        TaskStatus::PartiallyCompleted,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ] {
        let out = apply_task_transition(&prepare(base_cmd(from, TaskStatus::RollingBack))).unwrap();
        assert_eq!(out.new_status, TaskStatus::RollingBack);
    }
}

#[test]
fn archived_only_from_terminal_success_failure_cancel_rollback() {
    for from in [
        TaskStatus::Succeeded,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
        TaskStatus::RolledBack,
    ] {
        let out = apply_task_transition(&prepare(base_cmd(from, TaskStatus::Archived))).unwrap();
        assert_eq!(out.new_status, TaskStatus::Archived);
    }
    let err =
        apply_task_transition(&base_cmd(TaskStatus::Running, TaskStatus::Archived)).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::IllegalTransition);
}

#[test]
fn rejected_and_archived_are_terminal() {
    for from in [TaskStatus::Rejected, TaskStatus::Archived] {
        for &to in TASK_STATUS_CATALOG {
            assert!(!is_task_transition_allowed(from, to));
        }
    }
}

#[test]
fn empty_reason_rejected() {
    let mut cmd = base_cmd(TaskStatus::Planned, TaskStatus::Running);
    cmd.reason = "   ".into();
    let err = apply_task_transition(&cmd).unwrap_err();
    assert_eq!(err.code, DomainTaskErrorCode::InvalidInput);
}

#[test]
fn happy_path_candidate_to_archived_via_failure_replan_and_success() {
    let steps = [
        (TaskStatus::Candidate, TaskStatus::Planned),
        (TaskStatus::Planned, TaskStatus::Running),
        (TaskStatus::Running, TaskStatus::Failed),
        (TaskStatus::Failed, TaskStatus::Planned),
        (TaskStatus::Planned, TaskStatus::Running),
        (TaskStatus::Running, TaskStatus::Succeeded),
        (TaskStatus::Succeeded, TaskStatus::Archived),
    ];
    let mut rev = 1u64;
    let mut plan = 0u64;
    let mut status = TaskStatus::Candidate;
    for (from, to) in steps {
        assert_eq!(status, from);
        let mut cmd = prepare(base_cmd(from, to));
        cmd.current_revision = rev;
        cmd.current_plan_version = plan;
        let out = apply_task_transition(&cmd).unwrap();
        if matches!((from, to), (TaskStatus::Failed, TaskStatus::Planned)) {
            assert!(out.plan_version_incremented);
            assert_eq!(out.new_plan_version, plan + 1);
        } else {
            assert!(!out.plan_version_incremented);
        }
        rev = out.new_revision;
        plan = out.new_plan_version;
        status = out.new_status;
    }
    assert_eq!(plan, 1);
    assert_eq!(status, TaskStatus::Archived);
}
