use super::fault_fs::{operation_name, FaultingRealFs, PartialEffect};
use super::oracle::{expected_install_io_disposition, install_publication_landmarks};
use super::reference_model::{
    assert_snapshot_has_real_partial_effect, ExpectedArtifactVersion, ReferenceModel,
};
use super::scenario::{InitialRoot, Scenario, TRANSACTION_ID};
use super::snapshot::TreeSnapshot;
use crate::artifact_transaction::executor::RecordingObserver;
use crate::artifact_transaction::protocol::{
    LogicalPath, Operation, OperationBoundary, OperationSite, OperationTarget, RenamePurpose,
    SemanticPhase, TransactionPhase, WritePurpose,
};
use crate::artifact_transaction::{
    ArtifactTransaction, RecoveryReport, RecoveryRoute, RecoveryTerminal,
    TransactionFailureDisposition, TransactionRunError,
};
use std::collections::HashSet;

pub(super) fn successful_targets(initial: InitialRoot) -> Vec<OperationTarget> {
    let scenario = Scenario::create(initial);
    let mut engine = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    ArtifactTransaction::install_detailed_with_id(
        scenario.repo(),
        &scenario.plan,
        TRANSACTION_ID,
        &mut engine,
    )
    .unwrap();
    let (_, observer) = engine.into_parts();
    assert_adjacent_unique_trace(observer.events());
    observer
        .events()
        .iter()
        .filter(|event| event.target.boundary == OperationBoundary::Before)
        .map(|event| event.target.clone())
        .collect()
}

fn assert_adjacent_unique_trace(events: &[crate::artifact_transaction::protocol::OperationEvent]) {
    assert_eq!(events.len() % 2, 0);
    let mut targets = HashSet::new();
    for pair in events.chunks_exact(2) {
        assert_eq!(pair[0].target.boundary, OperationBoundary::Before);
        assert_eq!(pair[1].target.boundary, OperationBoundary::AfterSuccess);
        assert_eq!(pair[0].target.site, pair[1].target.site);
        assert_eq!(pair[0].target.occurrence, pair[1].target.occurrence);
        assert!(targets.insert((
            pair[0].target.site.clone(),
            pair[0].target.occurrence,
            pair[0].target.boundary,
        )));
        assert!(targets.insert((
            pair[1].target.site.clone(),
            pair[1].target.occurrence,
            pair[1].target.boundary,
        )));
    }
}

#[test]
fn success_trace_has_exact_identity_and_terminal_snapshots() {
    for initial in [InitialRoot::Existing, InitialRoot::Absent] {
        let scenario = Scenario::create(initial);
        let model = ReferenceModel::new(&scenario);
        let mut engine = ArtifactTransaction::test_engine(
            FaultingRealFs::no_fault(),
            ArtifactTransaction::recording_observer(),
        );
        ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        )
        .unwrap();
        let (_, observer) = engine.into_parts();
        assert_adjacent_unique_trace(observer.events());
        model.assert_terminal(
            &scenario,
            &TreeSnapshot::capture(scenario.repo()),
            ExpectedArtifactVersion::New,
        );
    }
}

#[test]
fn io_no_effect_matrix_covers_every_install_success_site() {
    for initial in [InitialRoot::Existing, InitialRoot::Absent] {
        let discovered = successful_targets(initial);
        let (first_formal_publication_index, commit_point_index) =
            install_publication_landmarks(&discovered);
        let discovered_set: HashSet<_> = discovered
            .iter()
            .map(|target| (target.site.clone(), target.occurrence))
            .collect();
        let mut tested = HashSet::new();
        for (target_index, target) in discovered.into_iter().enumerate() {
            let scenario = Scenario::create(initial);
            let model = ReferenceModel::new(&scenario);
            let fault_fs = FaultingRealFs::io_no_effect(&target);
            let mut engine = ArtifactTransaction::test_engine(
                fault_fs,
                ArtifactTransaction::recording_observer(),
            );
            let result = ArtifactTransaction::install_detailed_with_id(
                scenario.repo(),
                &scenario.plan,
                TRANSACTION_ID,
                &mut engine,
            );
            let (filesystem, observer) = engine.into_parts();
            filesystem.assert_consumed_once();
            assert_eq!(
                observer
                    .events()
                    .iter()
                    .filter(|event| event.target == target)
                    .count(),
                1,
                "selected before target must appear exactly once: {target:?}"
            );
            let failure = match result {
                Err(TransactionRunError::Failure(failure)) => failure,
                other => panic!(
                    "expected typed I/O failure at {}: {other:?}",
                    operation_name(&target.site.operation)
                ),
            };
            let expected_disposition = expected_install_io_disposition(
                &target,
                target_index,
                first_formal_publication_index,
                commit_point_index,
            );
            assert_eq!(
                failure.disposition(),
                &expected_disposition,
                "wrong disposition at index {target_index}: {target:?}"
            );
            let expected_report = match expected_disposition {
                TransactionFailureDisposition::CommitOutcomeUncertain => RecoveryReport {
                    route: RecoveryRoute::Committed,
                    terminal: RecoveryTerminal::Recovered,
                },
                TransactionFailureDisposition::CleanupDeferred => {
                    if target.site.phase == SemanticPhase::InstallCleanup
                        && !matches!(
                            target.site.operation,
                            Operation::SyncDir { .. } if target.occurrence.is_some()
                        )
                    {
                        RecoveryReport {
                            route: RecoveryRoute::Committed,
                            terminal: RecoveryTerminal::Recovered,
                        }
                    } else {
                        RecoveryReport {
                            route: RecoveryRoute::Noop,
                            terminal: RecoveryTerminal::Noop,
                        }
                    }
                }
                TransactionFailureDisposition::NoMutation
                | TransactionFailureDisposition::RolledBackBeforeReturn => RecoveryReport {
                    route: RecoveryRoute::Noop,
                    terminal: RecoveryTerminal::Noop,
                },
                other => panic!("unexpected install-matrix disposition: {other:?}"),
            };
            let mut recovery = ArtifactTransaction::test_engine(
                FaultingRealFs::no_fault(),
                ArtifactTransaction::recording_observer(),
            );
            let report = ArtifactTransaction::recover_detailed(scenario.repo(), &mut recovery)
                .expect("first recovery must complete");
            assert_eq!(
                report, expected_report,
                "wrong recovery report at index {target_index}: {target:?}"
            );
            let (_, first_observer) = recovery.into_parts();
            assert_adjacent_unique_trace(first_observer.events());
            let expected = if target_index > commit_point_index {
                ExpectedArtifactVersion::New
            } else if initial == InitialRoot::Existing {
                ExpectedArtifactVersion::Old
            } else {
                ExpectedArtifactVersion::Absent
            };
            model.assert_terminal(&scenario, &TreeSnapshot::capture(scenario.repo()), expected);
            let before_second = TreeSnapshot::capture(scenario.repo());
            let mut second = ArtifactTransaction::test_engine(
                FaultingRealFs::no_fault(),
                ArtifactTransaction::recording_observer(),
            );
            assert_eq!(
                ArtifactTransaction::recover_detailed(scenario.repo(), &mut second).unwrap(),
                RecoveryReport {
                    route: RecoveryRoute::Noop,
                    terminal: RecoveryTerminal::Noop,
                }
            );
            let (_, second_observer) = second.into_parts();
            assert!(second_observer.events().is_empty());
            assert_eq!(before_second, TreeSnapshot::capture(scenario.repo()));
            tested.insert((target.site, target.occurrence));
        }
        assert_eq!(discovered_set, tested);
        eprintln!(
            "artifact transaction IoNoEffect coverage {:?}: {} dynamic targets",
            initial,
            tested.len()
        );
    }
}

pub(super) fn committed_repo_sync_target(initial: InitialRoot) -> OperationTarget {
    let targets = successful_targets(initial);
    let commit_point = targets
        .iter()
        .position(|target| {
            matches!(
                target.site.operation,
                Operation::Rename {
                    purpose: RenamePurpose::PublishJournal,
                    ..
                }
            ) && target.site.phase == SemanticPhase::InstallCommit
        })
        .expect("discover committed publication");
    targets[commit_point + 1].clone()
}

#[test]
fn replacement_publication_failure_rolls_back_from_prior_formal_phase() {
    let targets = successful_targets(InitialRoot::Existing);
    let target = targets
        .iter()
        .find(|target| {
            matches!(
                target.site.operation,
                Operation::WriteBytes {
                    purpose: WritePurpose::JournalTemp,
                    journal_target_phase: Some(TransactionPhase::Prepared),
                    ..
                }
            )
        })
        .expect("Prepared replacement write")
        .clone();
    let scenario = Scenario::create(InitialRoot::Existing);
    let temp_path = ArtifactTransaction::journal_temp_path(scenario.repo(), TRANSACTION_ID);
    let mut engine = ArtifactTransaction::test_engine(
        FaultingRealFs::io_partial_at(
            scenario.repo(),
            &target,
            PartialEffect::WritePrefix { bytes: 5 },
        ),
        ArtifactTransaction::recording_observer(),
    );
    let result = ArtifactTransaction::install_detailed_with_id(
        scenario.repo(),
        &scenario.plan,
        TRANSACTION_ID,
        &mut engine,
    );
    let (_, observer) = engine.into_parts();
    assert!(
        !temp_path.exists(),
        "known temp must be cleaned before rollback"
    );
    let failure = match result {
        Err(TransactionRunError::Failure(failure)) => failure,
        other => panic!("expected replacement publication failure, got {other:?}"),
    };
    assert_eq!(
        failure.disposition(),
        &TransactionFailureDisposition::RolledBackBeforeReturn
    );
    assert!(observer.events().iter().any(|event| {
        matches!(
            event.target.site.operation,
            Operation::WriteBytes {
                purpose: WritePurpose::JournalTemp,
                journal_target_phase: Some(TransactionPhase::RollingBack(_)),
                ..
            }
        )
    }));
    ReferenceModel::new(&scenario).assert_terminal(
        &scenario,
        &TreeSnapshot::capture(scenario.repo()),
        ExpectedArtifactVersion::Old,
    );
}

#[test]
fn rollback_partial_effect_classes_are_real_modeled_and_recoverable() {
    let install = successful_targets(InitialRoot::Existing);
    let primary = install
        .iter()
        .find(|target| {
            matches!(
                target.site.operation,
                Operation::WriteBytes {
                    purpose: WritePurpose::Artifact,
                    ..
                }
            )
        })
        .expect("rollback-producing primary")
        .clone();
    let rollback = discover_rollback_trace(&primary);
    let representatives = [
        (
            "rollback_remove_tree",
            rollback
                .iter()
                .find(|target| matches!(target.site.operation, Operation::RemoveTree { .. }))
                .expect("rollback remove-tree")
                .clone(),
            PartialEffect::RemoveTreeFirstLexicalEntry,
        ),
        (
            "rollback_remove_file",
            rollback
                .iter()
                .find(|target| matches!(target.site.operation, Operation::RemoveFile { .. }))
                .expect("rollback remove-file")
                .clone(),
            PartialEffect::RemoveFileThenError,
        ),
    ];
    for (class, target, effect) in representatives {
        let scenario = Scenario::create(InitialRoot::Existing);
        let filesystem = FaultingRealFs::two_faults_at(
            scenario.repo(),
            (&primary, super::fault_fs::FaultDirective::IoNoEffect),
            (&target, super::fault_fs::FaultDirective::IoPartial(effect)),
        );
        let mut engine =
            ArtifactTransaction::test_engine(filesystem, ArtifactTransaction::recording_observer());
        let failure = match ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        ) {
            Err(TransactionRunError::Failure(failure)) => failure,
            other => panic!("partial rollback class {class} expected typed failure, got {other:?}"),
        };
        assert!(matches!(
            failure.disposition(),
            TransactionFailureDisposition::RecoveryRequired { .. }
        ));
        let (filesystem, _) = engine.into_parts();
        filesystem.assert_consumed(2);
        filesystem.assert_partial_performed();
        let (partial_before, actual_partial) = filesystem.partial_snapshots();
        let expected_partial = modeled_partial_snapshot_from(
            InitialRoot::Existing,
            partial_before.clone(),
            &target,
            effect,
        );
        assert_eq!(actual_partial, &expected_partial);
        assert_snapshot_has_real_partial_effect(partial_before, actual_partial);
        assert_partial_recovery_twice(&scenario, ExpectedArtifactVersion::Old);
        eprintln!("artifact transaction partial-effect class: {class}");
    }
}

fn discover_rollback_trace(primary: &OperationTarget) -> Vec<OperationTarget> {
    let scenario = Scenario::create(InitialRoot::Existing);
    let mut engine = ArtifactTransaction::test_engine(
        FaultingRealFs::io_no_effect(primary),
        ArtifactTransaction::recording_observer(),
    );
    let failure = match ArtifactTransaction::install_detailed_with_id(
        scenario.repo(),
        &scenario.plan,
        TRANSACTION_ID,
        &mut engine,
    ) {
        Err(TransactionRunError::Failure(failure)) => failure,
        other => panic!("rollback trace discovery expected typed failure, got {other:?}"),
    };
    assert!(matches!(
        failure.disposition(),
        TransactionFailureDisposition::RolledBackBeforeReturn
            | TransactionFailureDisposition::RecoveryRequired { .. }
    ));
    let (_, observer) = engine.into_parts();
    observer
        .events()
        .iter()
        .filter(|event| {
            event.target.boundary == OperationBoundary::Before
                && event.target.site.phase == SemanticPhase::InstallRollback
        })
        .map(|event| event.target.clone())
        .collect()
}

#[test]
fn partial_effect_equivalence_classes_are_real_modeled_and_recoverable() {
    let install_targets = successful_targets(InitialRoot::Existing);
    let representatives = partial_representatives(&install_targets);
    let mut tested_classes = HashSet::new();

    for (class, initial, target, effect) in representatives {
        let scenario = Scenario::create(initial);
        let mut engine = ArtifactTransaction::test_engine(
            FaultingRealFs::io_partial_at(scenario.repo(), &target, effect),
            ArtifactTransaction::recording_observer(),
        );
        let result = ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        );
        let failure = match result {
            Err(TransactionRunError::Failure(failure)) => failure,
            other => panic!("partial class {class} expected typed failure, got {other:?}"),
        };
        assert!(matches!(
            failure.disposition(),
            TransactionFailureDisposition::NoMutation
                | TransactionFailureDisposition::RolledBackBeforeReturn
                | TransactionFailureDisposition::RecoveryRequired { .. }
                | TransactionFailureDisposition::CleanupDeferred
        ));
        let (filesystem, _) = engine.into_parts();
        filesystem.assert_consumed_once();
        filesystem.assert_partial_performed();
        let (partial_before, actual_partial) = filesystem.partial_snapshots();
        let expected_partial =
            modeled_partial_snapshot_from(initial, partial_before.clone(), &target, effect);
        assert_eq!(actual_partial, &expected_partial);
        assert_snapshot_has_real_partial_effect(partial_before, actual_partial);

        assert_partial_recovery_twice(
            &scenario,
            if class == "committed_cleanup_remove_tree" {
                ExpectedArtifactVersion::New
            } else if initial == InitialRoot::Existing {
                ExpectedArtifactVersion::Old
            } else {
                ExpectedArtifactVersion::Absent
            },
        );
        tested_classes.insert(class);
    }
    eprintln!(
        "artifact transaction partial-effect classes: {} ({:?})",
        tested_classes.len(),
        tested_classes
    );
}

fn assert_partial_recovery_twice(scenario: &Scenario, expected: ExpectedArtifactVersion) {
    let mut first = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    let report = ArtifactTransaction::recover_detailed(scenario.repo(), &mut first)
        .expect("partial-effect recovery completes");
    let (_, first_observer) = first.into_parts();
    assert_eq!(
        first_observer.events().is_empty(),
        report.route == RecoveryRoute::Noop,
        "Noop is the only recovery route without mutation events"
    );
    ReferenceModel::new(scenario).assert_terminal(
        scenario,
        &TreeSnapshot::capture(scenario.repo()),
        expected,
    );
    let terminal = TreeSnapshot::capture(scenario.repo());
    let mut second = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    assert_eq!(
        ArtifactTransaction::recover_detailed(scenario.repo(), &mut second).unwrap(),
        RecoveryReport {
            route: RecoveryRoute::Noop,
            terminal: RecoveryTerminal::Noop,
        }
    );
    let (_, observer) = second.into_parts();
    assert!(observer.events().is_empty());
    assert_eq!(terminal, TreeSnapshot::capture(scenario.repo()));
}

fn modeled_partial_snapshot_from(
    initial: InitialRoot,
    before: TreeSnapshot,
    target: &OperationTarget,
    effect: PartialEffect,
) -> TreeSnapshot {
    let mut model = ReferenceModel::from_snapshot(initial, before);
    model.apply_partial(&target.site.operation, effect);
    model.snapshot().clone()
}

fn partial_representatives(
    install: &[OperationTarget],
) -> Vec<(&'static str, InitialRoot, OperationTarget, PartialEffect)> {
    let find_install = |predicate: &dyn Fn(&Operation) -> bool| {
        install
            .iter()
            .find(|target| predicate(&target.site.operation))
            .cloned()
            .expect("partial class target")
    };
    vec![
        (
            "journal_initial_write_prefix",
            InitialRoot::Existing,
            find_install(&|operation| {
                matches!(
                    operation,
                    Operation::WriteBytes {
                        purpose: WritePurpose::JournalTemp,
                        journal_target_phase: Some(TransactionPhase::Preparing),
                        ..
                    }
                )
            }),
            PartialEffect::WritePrefix { bytes: 7 },
        ),
        (
            "journal_replacement_write_prefix",
            InitialRoot::Existing,
            find_install(&|operation| {
                matches!(
                    operation,
                    Operation::WriteBytes {
                        purpose: WritePurpose::JournalTemp,
                        journal_target_phase: Some(TransactionPhase::Prepared),
                        ..
                    }
                )
            }),
            PartialEffect::WritePrefix { bytes: 7 },
        ),
        (
            "journal_committed_write_prefix",
            InitialRoot::Existing,
            find_install(&|operation| {
                matches!(
                    operation,
                    Operation::WriteBytes {
                        purpose: WritePurpose::JournalTemp,
                        journal_target_phase: Some(TransactionPhase::Committed),
                        ..
                    }
                )
            }),
            PartialEffect::WritePrefix { bytes: 7 },
        ),
        (
            "artifact_overwrite_prefix",
            InitialRoot::Existing,
            find_install(
                &|operation| matches!(operation, Operation::WriteBytes { purpose: WritePurpose::Artifact, target: LogicalPath::TreeEntry { relative, .. }, .. } if relative.as_str() == "same.txt"),
            ),
            PartialEffect::WritePrefix { bytes: 4 },
        ),
        (
            "artifact_new_prefix",
            InitialRoot::Existing,
            find_install(
                &|operation| matches!(operation, Operation::WriteBytes { purpose: WritePurpose::Artifact, target: LogicalPath::TreeEntry { relative, .. }, .. } if relative.as_str() == "nested/new.txt"),
            ),
            PartialEffect::WritePrefix { bytes: 4 },
        ),
        (
            "copy_prefix",
            InitialRoot::Existing,
            find_install(&|operation| matches!(operation, Operation::CopyFile { .. })),
            PartialEffect::CopyPrefix { bytes: 3 },
        ),
        (
            "committed_cleanup_remove_tree",
            InitialRoot::Existing,
            find_install(&|operation| {
                matches!(
                    operation,
                    Operation::RemoveTree {
                        role: crate::artifact_transaction::protocol::PathRole::Backup,
                        ..
                    }
                )
            }),
            PartialEffect::RemoveTreeFirstLexicalEntry,
        ),
    ]
}

#[test]
fn committed_publication_repo_fsync_failure_is_outcome_uncertain_and_never_rolls_back() {
    let target = committed_repo_sync_target(InitialRoot::Existing);
    let scenario = Scenario::create(InitialRoot::Existing);
    let mut engine = ArtifactTransaction::test_engine(
        FaultingRealFs::io_no_effect(&target),
        RecordingObserver::default(),
    );
    let result = ArtifactTransaction::install_detailed_with_id(
        scenario.repo(),
        &scenario.plan,
        TRANSACTION_ID,
        &mut engine,
    );
    let (filesystem, observer) = engine.into_parts();
    filesystem.assert_consumed_once();
    let failure = match result {
        Err(TransactionRunError::Failure(failure)) => failure,
        other => panic!("expected uncertain commit failure, got {other:?}"),
    };
    assert_eq!(
        failure.disposition(),
        &TransactionFailureDisposition::CommitOutcomeUncertain
    );
    assert!(!observer
        .events()
        .iter()
        .any(|event| { event.target.site.phase == SemanticPhase::InstallRollback }));
    let mut recovery = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    assert_eq!(
        ArtifactTransaction::recover_detailed(scenario.repo(), &mut recovery).unwrap(),
        RecoveryReport {
            route: RecoveryRoute::Committed,
            terminal: RecoveryTerminal::Recovered,
        }
    );
    ReferenceModel::new(&scenario).assert_terminal(
        &scenario,
        &TreeSnapshot::capture(scenario.repo()),
        ExpectedArtifactVersion::New,
    );
}

#[test]
fn corrupt_formal_journal_fails_closed_without_mutation() {
    let scenario = Scenario::create(InitialRoot::Existing);
    let before = TreeSnapshot::capture(scenario.repo());
    std::fs::write(
        ArtifactTransaction::journal_path(scenario.repo()),
        b"{corrupt",
    )
    .expect("write corrupt journal");
    let after_corruption = TreeSnapshot::capture(scenario.repo());
    let mut recovery = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    let failure = match ArtifactTransaction::recover_detailed(scenario.repo(), &mut recovery) {
        Err(TransactionRunError::Failure(failure)) => failure,
        other => panic!("expected stored-state failure, got {other:?}"),
    };
    assert_eq!(
        failure.disposition(),
        &TransactionFailureDisposition::StoredStateInvalid
    );
    let (_, observer) = recovery.into_parts();
    assert!(observer.events().is_empty());
    assert_eq!(after_corruption, TreeSnapshot::capture(scenario.repo()));
    assert_ne!(before, after_corruption);
}

#[cfg(unix)]
#[test]
fn tree_entry_paths_reject_non_utf8_without_lossy_aliasing() {
    use std::os::unix::ffi::OsStringExt;
    let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![b'a', 0xff]));
    assert!(
        crate::artifact_transaction::protocol::TreeEntryPath::from_relative_path(&path).is_err()
    );
}

#[test]
fn committed_publication_after_success_crash_recovers_committed_route() {
    let site = OperationSite {
        phase: SemanticPhase::InstallCommit,
        operation: Operation::Rename {
            purpose: RenamePurpose::PublishJournal,
            source: LogicalPath::JournalTemp,
            destination: LogicalPath::FormalJournal,
        },
    };
    let before = successful_targets(InitialRoot::Existing)
        .into_iter()
        .find(|target| target.site == site)
        .expect("discover committed publication");
    let crash = OperationTarget {
        boundary: OperationBoundary::AfterSuccess,
        ..before
    };
    let scenario = Scenario::create(InitialRoot::Existing);
    let mut engine = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::crash_observer(crash),
    );
    assert!(matches!(
        ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine
        ),
        Err(TransactionRunError::Crash(_))
    ));
    let mut recovery = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    assert_eq!(
        ArtifactTransaction::recover_detailed(scenario.repo(), &mut recovery).unwrap(),
        RecoveryReport {
            route: RecoveryRoute::Committed,
            terminal: RecoveryTerminal::Recovered,
        }
    );
    ReferenceModel::new(&scenario).assert_terminal(
        &scenario,
        &TreeSnapshot::capture(scenario.repo()),
        ExpectedArtifactVersion::New,
    );
}
