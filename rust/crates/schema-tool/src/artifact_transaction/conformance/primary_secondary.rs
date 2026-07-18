use super::fault_fs::{FaultDirective, FaultingRealFs};
use super::matrix::successful_targets;
use super::oracle::ModelCheckingObserver;
use super::reference_model::ReferenceState;
use super::scenario::{InitialRoot, Scenario, TRANSACTION_ID};
use super::snapshot::TreeSnapshot;
use crate::artifact_transaction::error::{JournalPublicationContext, JournalTempCleanupFailure};
use crate::artifact_transaction::protocol::{
    Operation, OperationBoundary, OperationSite, OperationTarget, TransactionPhase, WritePurpose,
};
use crate::artifact_transaction::{
    ArtifactTransaction, CommittedJournalUncertain, JournalNotPublishedFailure,
    JournalTempCleanupFailure as ExportedJournalTempCleanupFailure, RecoveryGoal,
    TransactionFailureDisposition, TransactionRunError,
};
use std::collections::{HashMap, HashSet};

#[test]
fn primary_secondary_two_star_covers_dynamic_compensation_classes() {
    let mut cases = Vec::new();
    for initial in [InitialRoot::Existing, InitialRoot::Absent] {
        let install = successful_targets(initial);
        cases.extend(discover_compensation_cases(initial, &install));
    }
    let representatives = representatives_by_signature(&cases);
    let anchor_case = representatives
        .values()
        .next()
        .expect("compensation representative");
    let anchor = anchor_case
        .secondary
        .first()
        .expect("compensation anchor")
        .clone();
    let mut tested = HashSet::new();

    for case in &cases {
        let compatible_anchor = case
            .secondary
            .iter()
            .find(|target| target.site == anchor.site)
            .cloned()
            .unwrap_or_else(|| case.secondary[0].clone());
        exercise_secondary_io(case, &compatible_anchor);
        tested.insert((case.primary.clone(), compatible_anchor, "io"));
    }
    for case in representatives.values() {
        for secondary in &case.secondary {
            exercise_secondary_io(case, secondary);
            tested.insert((case.primary.clone(), secondary.clone(), "io"));
            for boundary in [OperationBoundary::Before, OperationBoundary::AfterSuccess] {
                exercise_secondary_crash(case, secondary, boundary);
                tested.insert((
                    case.primary.clone(),
                    OperationTarget {
                        boundary,
                        ..secondary.clone()
                    },
                    "crash",
                ));
            }
        }
    }
    eprintln!(
        "artifact transaction secondary classes: {}, primary cases={}, two-star cases={}",
        representatives.len(),
        cases.len(),
        tested.len()
    );
}

#[test]
fn initial_and_replacement_temp_cleanup_double_failures_keep_exact_context() {
    for (phase, expect_initial) in [
        (TransactionPhase::Preparing, true),
        (TransactionPhase::Prepared, false),
    ] {
        let install = successful_targets(InitialRoot::Existing);
        let primary = install
            .iter()
            .find(|target| {
                matches!(
                    target.site.operation,
                    Operation::WriteBytes {
                        purpose: WritePurpose::JournalTemp,
                        journal_target_phase: Some(target_phase),
                        ..
                    } if target_phase == phase
                )
            })
            .expect("journal publication write")
            .clone();
        let secondary = OperationTarget {
            site: OperationSite {
                phase: primary.site.phase,
                operation: Operation::RemoveFile {
                    target: crate::artifact_transaction::protocol::LogicalPath::JournalTemp,
                },
            },
            occurrence: None,
            boundary: OperationBoundary::Before,
        };
        let scenario = Scenario::create(InitialRoot::Existing);
        let filesystem = FaultingRealFs::two_faults(
            (&primary, FaultDirective::IoNoEffect),
            (&secondary, FaultDirective::IoNoEffect),
        );
        let mut engine =
            ArtifactTransaction::test_engine(filesystem, ArtifactTransaction::recording_observer());
        let result = ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        );
        let (filesystem, _) = engine.into_parts();
        filesystem.assert_consumed(2);
        let failure = match result {
            Err(TransactionRunError::Failure(failure)) => failure,
            other => panic!("expected double publication failure, got {other:?}"),
        };
        let publication = failure
            .primary()
            .downcast_ref::<JournalTempCleanupFailure>()
            .expect("structured publication cleanup failure");
        assert_eq!(publication.context.is_initial(), expect_initial);
        match publication.context {
            JournalPublicationContext::Initial { target_phase } => {
                assert_eq!(target_phase, TransactionPhase::Preparing)
            }
            JournalPublicationContext::Replacement {
                prior_phase,
                target_phase,
            } => {
                assert_eq!(prior_phase, TransactionPhase::Preparing);
                assert_eq!(target_phase, TransactionPhase::Prepared);
            }
        }
    }
}

#[derive(Clone)]
struct CompensationCase {
    initial: InitialRoot,
    primary: OperationTarget,
    primary_reference_signature: TreeSnapshot,
    secondary: Vec<OperationTarget>,
}

fn discover_compensation_cases(
    initial: InitialRoot,
    install: &[OperationTarget],
) -> Vec<CompensationCase> {
    install
        .iter()
        .filter_map(|primary| {
            let scenario = Scenario::create(initial);
            let mut engine = ArtifactTransaction::test_engine(
                FaultingRealFs::io_no_effect(primary),
                ModelCheckingObserver::new(&scenario, None),
            );
            let result = ArtifactTransaction::install_detailed_with_id(
                scenario.repo(),
                &scenario.plan,
                TRANSACTION_ID,
                &mut engine,
            );
            let failure = match result {
                Err(TransactionRunError::Failure(failure)) => failure,
                other => {
                    panic!("primary discovery expected typed failure at {primary:?}, got {other:?}")
                }
            };
            assert_primary_injected(failure.primary(), primary);
            let (_, observer) = engine.into_parts();
            let secondary: Vec<_> = observer
                .events()
                .iter()
                .skip_while(|event| event.target != *primary)
                .skip(1)
                .filter(|event| event.target.boundary == OperationBoundary::Before)
                .map(|event| event.target.clone())
                .collect();
            let primary_reference_signature =
                reference_at_target(&scenario, observer.events(), primary)
                    .snapshot()
                    .clone();
            (!secondary.is_empty()).then(|| CompensationCase {
                initial,
                primary: primary.clone(),
                primary_reference_signature,
                secondary,
            })
        })
        .collect()
}

type CompensationSignature = (
    InitialRoot,
    OperationSite,
    Option<usize>,
    TreeSnapshot,
    Vec<OperationSite>,
);

fn representatives_by_signature(
    cases: &[CompensationCase],
) -> HashMap<CompensationSignature, CompensationCase> {
    let mut representatives = HashMap::new();
    for case in cases {
        let signature = (
            case.initial,
            case.primary.site.clone(),
            case.primary.occurrence,
            case.primary_reference_signature.clone(),
            case.secondary
                .iter()
                .map(|target| target.site.clone())
                .collect::<Vec<_>>(),
        );
        representatives
            .entry(signature)
            .or_insert_with(|| case.clone());
    }
    representatives
}

fn exercise_secondary_io(case: &CompensationCase, secondary: &OperationTarget) {
    let scenario = Scenario::create(case.initial);
    let filesystem = FaultingRealFs::two_faults(
        (&case.primary, FaultDirective::IoNoEffect),
        (secondary, FaultDirective::IoNoEffect),
    );
    let mut engine =
        ArtifactTransaction::test_engine(filesystem, ModelCheckingObserver::new(&scenario, None));
    let result = ArtifactTransaction::install_detailed_with_id(
        scenario.repo(),
        &scenario.plan,
        TRANSACTION_ID,
        &mut engine,
    );
    let (filesystem, observer) = engine.into_parts();
    filesystem.assert_consumed(2);
    let actual = TreeSnapshot::capture(scenario.repo());
    observer.reference_state().assert_matches(&actual);
    match result {
        Err(TransactionRunError::Failure(failure)) => {
            assert_primary_injected(failure.primary(), &case.primary);
            let publication_cleanup = failure
                .primary()
                .downcast_ref::<JournalTempCleanupFailure>();
            let expected_disposition = if publication_cleanup.is_some() {
                TransactionFailureDisposition::RecoveryRequired {
                    goal: if observer.reference_state().original_safely_restored() {
                        RecoveryGoal::CleanResidue
                    } else {
                        RecoveryGoal::RestoreOriginal
                    },
                }
            } else if failure.compensation().is_none()
                && observer.reference_state().original_safely_restored()
                && !observer.reference_state().protocol().temp_exists
            {
                TransactionFailureDisposition::RolledBackBeforeReturn
            } else {
                TransactionFailureDisposition::RecoveryRequired {
                    goal: if observer.reference_state().original_safely_restored() {
                        RecoveryGoal::CleanResidue
                    } else {
                        RecoveryGoal::RestoreOriginal
                    },
                }
            };
            assert_eq!(failure.disposition(), &expected_disposition);
            if let Some(compensation) = failure.compensation() {
                // Compensation is always a typed secondary failure from online rollback:
                // either an injected I/O error (FaultingRealFs) or a plain std::io::Error.
                // No string-shape assertions; unknown types fail at the source.
                assert_injected(compensation, secondary, FaultDirective::IoNoEffect);
            } else if let Some(publication) = failure
                .primary()
                .downcast_ref::<JournalTempCleanupFailure>()
            {
                // Double failure during journal-temp cleanup is folded into the primary
                // typed wrapper, not TransactionFailure::compensation.
                assert_injected_io(
                    publication.source_error(),
                    &case.primary,
                    FaultDirective::IoNoEffect,
                );
                assert_injected_io(
                    publication.cleanup_error(),
                    secondary,
                    FaultDirective::IoNoEffect,
                );
            } else {
                // Secondary ran but produced no independent compensation error (e.g. primary
                // already carried a structured publication failure without a cleanup pair).
                // Require the selected secondary site was observed on the exact event stream.
                assert!(
                    observer
                        .events()
                        .iter()
                        .any(|event| event.target == *secondary),
                    "selected secondary target must be observed exactly when no compensation error is attached: secondary={secondary:?}"
                );
            }
        }
        other => panic!("expected typed primary-secondary failure, got {other:?}"),
    }
    assert_recovery_twice(&scenario, case.initial);
}

fn exercise_secondary_crash(
    case: &CompensationCase,
    secondary_before: &OperationTarget,
    boundary: OperationBoundary,
) {
    let target = OperationTarget {
        boundary,
        ..secondary_before.clone()
    };
    let scenario = Scenario::create(case.initial);
    let mut engine = ArtifactTransaction::test_engine(
        FaultingRealFs::io_no_effect(&case.primary),
        ArtifactTransaction::crash_observer(target.clone()),
    );
    match ArtifactTransaction::install_detailed_with_id(
        scenario.repo(),
        &scenario.plan,
        TRANSACTION_ID,
        &mut engine,
    ) {
        Err(TransactionRunError::Crash(crash)) => assert_eq!(crash.target, target),
        other => panic!("secondary crash must propagate unchanged, got {other:?}"),
    }
    assert_recovery_twice(&scenario, case.initial);
}

fn assert_primary_injected(error: &anyhow::Error, target: &OperationTarget) {
    if let Some(io) = error.downcast_ref::<std::io::Error>() {
        assert_injected_io(io, target, FaultDirective::IoNoEffect);
        return;
    }
    if let Some(injected) = find_injected(error) {
        assert_eq!(&injected.target, target);
        assert_eq!(injected.directive, FaultDirective::IoNoEffect);
        return;
    }
    if let Some(publication) = error.downcast_ref::<JournalNotPublishedFailure>() {
        assert_injected_io(
            publication.source_error(),
            target,
            FaultDirective::IoNoEffect,
        );
        return;
    }
    if let Some(publication) = error.downcast_ref::<ExportedJournalTempCleanupFailure>() {
        assert_injected_io(
            publication.source_error(),
            target,
            FaultDirective::IoNoEffect,
        );
        return;
    }
    if let Some(uncertain) = error.downcast_ref::<CommittedJournalUncertain>() {
        assert_injected_io(uncertain.source_error(), target, FaultDirective::IoNoEffect);
        return;
    }
    panic!("expected structured primary InjectedIo, got {error:?}");
}

fn reference_at_target(
    scenario: &Scenario,
    events: &[crate::artifact_transaction::protocol::OperationEvent],
    target: &OperationTarget,
) -> ReferenceState {
    let mut reference = ReferenceState::new(scenario);
    for event in events {
        reference.apply_event(event);
        if event.target == *target {
            return reference;
        }
    }
    panic!("primary target absent from trace: {target:?}")
}

fn assert_injected_io(error: &std::io::Error, target: &OperationTarget, directive: FaultDirective) {
    let injected = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<super::fault_fs::InjectedIo>())
        .unwrap_or_else(|| panic!("expected InjectedIo, got {error:?}"));
    assert_eq!(&injected.target, target);
    assert_eq!(injected.directive, directive);
}

fn assert_injected(error: &anyhow::Error, target: &OperationTarget, directive: FaultDirective) {
    if let Some(io) = error.downcast_ref::<std::io::Error>() {
        assert_injected_io(io, target, directive);
        return;
    }
    let injected =
        find_injected(error).unwrap_or_else(|| panic!("expected InjectedIo, got {error:?}"));
    assert_eq!(&injected.target, target);
    assert_eq!(injected.directive, directive);
}

fn find_injected(error: &anyhow::Error) -> Option<&super::fault_fs::InjectedIo> {
    error
        .chain()
        .find_map(|source| source.downcast_ref::<super::fault_fs::InjectedIo>())
}

fn assert_recovery_twice(scenario: &Scenario, initial: InitialRoot) {
    let mut first = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    ArtifactTransaction::recover_detailed(scenario.repo(), &mut first)
        .expect("primary-secondary recovery completes");
    let model = super::reference_model::ReferenceModel::new(scenario);
    model.assert_terminal(
        scenario,
        &TreeSnapshot::capture(scenario.repo()),
        if initial == InitialRoot::Existing {
            super::reference_model::ExpectedArtifactVersion::Old
        } else {
            super::reference_model::ExpectedArtifactVersion::Absent
        },
    );
    let terminal = TreeSnapshot::capture(scenario.repo());
    let mut second = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    let report = ArtifactTransaction::recover_detailed(scenario.repo(), &mut second).unwrap();
    assert_eq!(
        report.route,
        crate::artifact_transaction::RecoveryRoute::Noop
    );
    assert_eq!(
        report.terminal,
        crate::artifact_transaction::RecoveryTerminal::Noop
    );
    let (_, observer) = second.into_parts();
    assert!(observer.events().is_empty());
    assert_eq!(terminal, TreeSnapshot::capture(scenario.repo()));
}
