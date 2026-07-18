use super::fault_fs::FaultingRealFs;
use super::oracle::{
    assert_exact_fault_prefix, before_targets_from_events, expected_install_io_disposition,
    install_publication_landmarks, ModelCheckingObserver,
};
use super::reference_model::ReferenceState;
use super::scenario::{InitialRoot, Scenario, TRANSACTION_ID};
use super::snapshot::TreeSnapshot;
use crate::artifact_transaction::protocol::{
    OperationBoundary, OperationEvent, OperationTarget, SemanticPhase,
};
use crate::artifact_transaction::{
    ArtifactTransaction, RecoveryGoal, RecoveryReport, RecoveryRoute, RecoveryTerminal,
    TransactionFailureDisposition, TransactionRunError,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RecipeKind {
    Install,
    Recover,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RecipeKey {
    initial: InitialRoot,
    kind: RecipeKind,
    snapshot: TreeSnapshot,
}

#[derive(Clone)]
struct Recipe {
    initial: InitialRoot,
    kind: RecipeKind,
    snapshot: TreeSnapshot,
    reference: ReferenceState,
    trace: Vec<OperationEvent>,
}

struct RunOutcome {
    snapshot: TreeSnapshot,
    reference: ReferenceState,
    trace: Vec<OperationEvent>,
    result: Result<Option<RecoveryReport>, TransactionRunError>,
}

#[derive(Default)]
struct OnlineRollbackUniverse {
    references: Vec<ReferenceState>,
    discovered_crashes: HashSet<OperationTarget>,
    tested_crashes: HashSet<OperationTarget>,
    discovered_io: HashSet<OperationTarget>,
    tested_io: HashSet<OperationTarget>,
}

#[test]
fn online_rollback_targets_are_an_exact_closed_universe_seed() {
    for initial in [InitialRoot::Existing, InitialRoot::Absent] {
        let universe = discover_online_rollback_universe(initial);
        assert_eq!(universe.discovered_crashes, universe.tested_crashes);
        assert_eq!(universe.discovered_io, universe.tested_io);
        assert_eq!(
            universe.references.len(),
            universe.tested_crashes.len() + universe.tested_io.len()
        );
        eprintln!(
            "artifact transaction online rollback {:?}: sites={}, crash cases={}, Io cases={}, recovery seeds={}",
            initial,
            universe.discovered_io.len(),
            universe.tested_crashes.len(),
            universe.tested_io.len(),
            universe.references.len()
        );
    }
}

fn discover_online_rollback_universe(initial: InitialRoot) -> OnlineRollbackUniverse {
    let (primary, rollback_trace) = baseline_online_rollback(initial);
    let rollback_targets = before_targets(&rollback_trace);
    let mut universe = OnlineRollbackUniverse::default();
    for target in &rollback_targets {
        universe.discovered_io.insert(target.clone());
        for boundary in [OperationBoundary::Before, OperationBoundary::AfterSuccess] {
            let crash_target = OperationTarget {
                boundary,
                ..target.clone()
            };
            universe.discovered_crashes.insert(crash_target.clone());
            universe.references.push(crash_online_rollback(
                initial,
                &primary,
                &rollback_trace,
                crash_target.clone(),
            ));
            universe.tested_crashes.insert(crash_target);
        }
        universe.references.push(io_online_rollback(
            initial,
            &primary,
            &rollback_trace,
            target,
        ));
        universe.tested_io.insert(target.clone());
    }
    universe
}

fn baseline_online_rollback(initial: InitialRoot) -> (OperationTarget, Vec<OperationEvent>) {
    let primary = baseline_install(initial)
        .trace
        .iter()
        .find(|event| {
            event.target.boundary == OperationBoundary::Before
                && matches!(
                    event.target.site.operation,
                    crate::artifact_transaction::protocol::Operation::WriteBytes {
                        purpose: crate::artifact_transaction::protocol::WritePurpose::Artifact,
                        ..
                    }
                )
        })
        .expect("artifact-write primary")
        .target
        .clone();
    let scenario = Scenario::create(initial);
    let mut engine = ArtifactTransaction::test_engine(
        FaultingRealFs::io_no_effect(&primary),
        ArtifactTransaction::recording_observer(),
    );
    assert!(matches!(
        ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        ),
        Err(TransactionRunError::Failure(_))
    ));
    let (filesystem, observer) = engine.into_parts();
    filesystem.assert_consumed_once();
    let rollback_trace = observer
        .events()
        .iter()
        .filter(|event| event.target.site.phase == SemanticPhase::InstallRollback)
        .cloned()
        .collect();
    (primary, rollback_trace)
}

fn crash_online_rollback(
    initial: InitialRoot,
    primary: &OperationTarget,
    baseline: &[OperationEvent],
    target: OperationTarget,
) -> ReferenceState {
    let scenario = Scenario::create(initial);
    let mut engine = ArtifactTransaction::test_engine(
        FaultingRealFs::io_no_effect(primary),
        ModelCheckingObserver::new(&scenario, Some(target.clone())),
    );
    assert!(matches!(
        ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        ),
        Err(TransactionRunError::Crash(_))
    ));
    let (_, observer) = engine.into_parts();
    let actual_rollback: Vec<_> = observer
        .events()
        .iter()
        .filter(|event| event.target.site.phase == SemanticPhase::InstallRollback)
        .cloned()
        .collect();
    assert_exact_fault_prefix(baseline, &actual_rollback, &target);
    observer.reference_state().clone()
}

fn io_online_rollback(
    initial: InitialRoot,
    primary: &OperationTarget,
    baseline: &[OperationEvent],
    target: &OperationTarget,
) -> ReferenceState {
    let scenario = Scenario::create(initial);
    let filesystem = FaultingRealFs::two_faults(
        (primary, super::fault_fs::FaultDirective::IoNoEffect),
        (target, super::fault_fs::FaultDirective::IoNoEffect),
    );
    let mut engine =
        ArtifactTransaction::test_engine(filesystem, ModelCheckingObserver::new(&scenario, None));
    assert!(matches!(
        ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        ),
        Err(TransactionRunError::Failure(_))
    ));
    let (filesystem, observer) = engine.into_parts();
    filesystem.assert_consumed(2);
    let actual_rollback: Vec<_> = observer
        .events()
        .iter()
        .filter(|event| event.target.site.phase == SemanticPhase::InstallRollback)
        .cloned()
        .collect();
    assert_exact_fault_prefix(baseline, &actual_rollback, target);
    observer.reference_state().clone()
}

#[test]
fn fixed_point_crash_and_io_universe_is_exact() {
    for initial in [InitialRoot::Existing, InitialRoot::Absent] {
        let (recipes, online) = discover_fixed_point(initial);
        let mut discovered = HashSet::new();
        let mut tested_crashes = HashSet::new();
        let mut tested_io = HashSet::new();
        let mut phase_counts: HashMap<SemanticPhase, usize> = HashMap::new();

        for (recipe_index, recipe) in recipes.iter().enumerate() {
            for event in &recipe.trace {
                discovered.insert((recipe_index, event.target.clone()));
            }
            for target in before_targets(&recipe.trace) {
                *phase_counts.entry(target.site.phase).or_default() += 1;
                exercise_io(recipe, &target);
                tested_io.insert((recipe_index, target.clone()));
                for boundary in [OperationBoundary::Before, OperationBoundary::AfterSuccess] {
                    let crash_target = OperationTarget {
                        boundary,
                        ..target.clone()
                    };
                    exercise_crash(recipe, &crash_target);
                    tested_crashes.insert((recipe_index, crash_target));
                }
            }
        }

        assert_eq!(discovered, tested_crashes);
        let before_discovered: HashSet<_> = discovered
            .iter()
            .filter(|(_, target)| target.boundary == OperationBoundary::Before)
            .cloned()
            .collect();
        assert_eq!(before_discovered, tested_io);
        let recovery_sites: usize = phase_counts
            .iter()
            .filter(|(phase, _)| is_recovery_phase(**phase))
            .map(|(_, count)| count)
            .sum();
        let rollback_sites = online.discovered_io.len();
        let rollback_crashes = online.tested_crashes.len();
        let rollback_io = online.tested_io.len();
        assert_eq!(online.discovered_crashes, online.tested_crashes);
        assert_eq!(online.discovered_io, online.tested_io);
        let install_sites = phase_counts.values().sum::<usize>() - recovery_sites;
        eprintln!(
            "artifact transaction unified fixed-point {:?}: recipes={}, install sites={}, recovery sites={}, rollback sites={}, crash cases={}, Io cases={}",
            initial,
            recipes.len(),
            install_sites,
            recovery_sites,
            rollback_sites,
            tested_crashes.len() + rollback_crashes,
            tested_io.len() + rollback_io
        );
    }
}

fn discover_fixed_point(initial: InitialRoot) -> (Vec<Recipe>, OnlineRollbackUniverse) {
    let install = baseline_install(initial);
    let online = discover_online_rollback_universe(initial);
    let mut recipes = vec![install.clone()];
    let mut queued = VecDeque::from([install]);
    let mut known = HashSet::new();
    known.insert(RecipeKey {
        initial,
        kind: RecipeKind::Install,
        snapshot: recipes[0].snapshot.clone(),
    });

    for reference in online.references.iter().cloned() {
        let recovery = baseline_recovery(reference);
        if recovery.trace.is_empty() {
            continue;
        }
        let key = RecipeKey {
            initial,
            kind: RecipeKind::Recover,
            snapshot: recovery.snapshot.clone(),
        };
        if known.insert(key) {
            recipes.push(recovery.clone());
            queued.push_back(recovery);
        }
    }

    while let Some(recipe) = queued.pop_front() {
        for target in boundary_targets(&recipe.trace) {
            let crashed = run_recipe(&recipe, Some(target.clone()));
            assert!(
                matches!(crashed.result, Err(TransactionRunError::Crash(_))),
                "fixed-point recipe {:?} did not reach target {:?}; trace={:?}; result={:?}",
                recipe.kind,
                target,
                crashed.trace,
                crashed.result
            );
            let recovery = baseline_recovery(crashed.reference);
            if recovery.trace.is_empty() {
                continue;
            }
            let key = RecipeKey {
                initial,
                kind: RecipeKind::Recover,
                snapshot: recovery.snapshot.clone(),
            };
            if known.insert(key) {
                recipes.push(recovery.clone());
                queued.push_back(recovery);
            }
        }
        for target in before_targets(&recipe.trace) {
            let failed = run_io_recipe(&recipe, &target);
            let recovery = baseline_recovery(failed.reference);
            if recovery.trace.is_empty() {
                continue;
            }
            let key = RecipeKey {
                initial,
                kind: RecipeKind::Recover,
                snapshot: recovery.snapshot.clone(),
            };
            if known.insert(key) {
                recipes.push(recovery.clone());
                queued.push_back(recovery);
            }
        }
    }
    (recipes, online)
}

fn baseline_install(initial: InitialRoot) -> Recipe {
    let scenario = Scenario::create(initial);
    let before = TreeSnapshot::capture(scenario.repo());
    let reference = ReferenceState::new(&scenario);
    let observer = ModelCheckingObserver::new(&scenario, None);
    let mut engine = ArtifactTransaction::test_engine(FaultingRealFs::no_fault(), observer);
    ArtifactTransaction::install_detailed_with_id(
        scenario.repo(),
        &scenario.plan,
        TRANSACTION_ID,
        &mut engine,
    )
    .unwrap();
    let (_, observer) = engine.into_parts();
    let trace = observer.events().to_vec();
    Recipe {
        initial,
        kind: RecipeKind::Install,
        snapshot: before,
        reference,
        trace,
    }
}

fn baseline_recovery(reference: ReferenceState) -> Recipe {
    let initial = reference.initial_root();
    let snapshot = reference.snapshot().clone();
    let expected_report = expected_recovery_report(&reference);
    let scenario = Scenario::create(initial);
    restore_snapshot(scenario.repo(), &snapshot);
    let observer = ModelCheckingObserver::from_snapshot(
        scenario.repo().to_path_buf(),
        reference.clone(),
        None,
    );
    let mut engine = ArtifactTransaction::test_engine(FaultingRealFs::no_fault(), observer);
    let result = ArtifactTransaction::recover_detailed(scenario.repo(), &mut engine);
    match result {
        Ok(report) => assert_eq!(report, expected_report),
        Err(error) => panic!("baseline recovery failed: {error:?}"),
    }
    let (_, observer) = engine.into_parts();
    observer
        .reference_state()
        .assert_matches(&TreeSnapshot::capture(scenario.repo()));
    Recipe {
        initial,
        kind: RecipeKind::Recover,
        snapshot,
        reference,
        trace: observer.events().to_vec(),
    }
}

fn exercise_crash(recipe: &Recipe, target: &OperationTarget) {
    let outcome = run_recipe(recipe, Some(target.clone()));
    match outcome.result {
        Err(TransactionRunError::Crash(crash)) => assert_eq!(crash.target, *target),
        other => panic!("expected crash at {target:?}, got {other:?}"),
    }
    let expected_len = recipe
        .trace
        .iter()
        .position(|event| event.target == *target)
        .expect("target in recipe")
        + 1;
    assert_exact_fault_prefix(&recipe.trace, &outcome.trace, target);
    assert_eq!(outcome.trace.len(), expected_len);
    assert_eq!(&outcome.snapshot, outcome.reference.snapshot());
    assert_recovery_idempotence(outcome.reference);
}

fn exercise_io(recipe: &Recipe, target: &OperationTarget) {
    let outcome = run_io_recipe(recipe, target);
    assert_eq!(&outcome.snapshot, outcome.reference.snapshot());
    assert_recovery_idempotence(outcome.reference);
}

fn run_io_recipe(recipe: &Recipe, target: &OperationTarget) -> RunOutcome {
    let scenario = Scenario::create(recipe.initial);
    restore_snapshot(scenario.repo(), &recipe.snapshot);
    let observer = ModelCheckingObserver::from_snapshot(
        scenario.repo().to_path_buf(),
        recipe.reference.clone(),
        None,
    );
    let mut engine =
        ArtifactTransaction::test_engine(FaultingRealFs::io_no_effect(target), observer);
    let result = match recipe.kind {
        RecipeKind::Install => ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        )
        .map(|()| None),
        RecipeKind::Recover => {
            ArtifactTransaction::recover_detailed(scenario.repo(), &mut engine).map(Some)
        }
    };
    let failure = match &result {
        Err(TransactionRunError::Failure(failure)) => failure,
        other => panic!("expected typed I/O failure at {target:?}, got {other:?}"),
    };
    let (filesystem, observer) = engine.into_parts();
    filesystem.assert_consumed_once();
    let expected_len = recipe
        .trace
        .iter()
        .position(|event| event.target == *target)
        .expect("target in recipe")
        + 1;
    assert_eq!(
        &observer.events()[..expected_len],
        &recipe.trace[..expected_len]
    );
    assert_eq!(
        observer.events()[expected_len - 1].target.boundary,
        OperationBoundary::Before
    );
    let snapshot = TreeSnapshot::capture(scenario.repo());
    observer.reference_state().assert_matches(&snapshot);
    assert_io_disposition(recipe, target, failure.disposition());
    RunOutcome {
        snapshot,
        reference: observer.reference_state().clone(),
        trace: observer.events().to_vec(),
        result,
    }
}

fn run_recipe(recipe: &Recipe, crash_target: Option<OperationTarget>) -> RunOutcome {
    let scenario = Scenario::create(recipe.initial);
    restore_snapshot(scenario.repo(), &recipe.snapshot);
    let observer = ModelCheckingObserver::from_snapshot(
        scenario.repo().to_path_buf(),
        recipe.reference.clone(),
        crash_target,
    );
    let mut engine = ArtifactTransaction::test_engine(FaultingRealFs::no_fault(), observer);
    let result = match recipe.kind {
        RecipeKind::Install => ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        )
        .map(|()| None),
        RecipeKind::Recover => {
            ArtifactTransaction::recover_detailed(scenario.repo(), &mut engine).map(Some)
        }
    };
    let (_, observer) = engine.into_parts();
    let snapshot = TreeSnapshot::capture(scenario.repo());
    observer.reference_state().assert_matches(&snapshot);
    RunOutcome {
        snapshot,
        reference: observer.reference_state().clone(),
        trace: observer.events().to_vec(),
        result,
    }
}

fn assert_recovery_idempotence(reference: ReferenceState) {
    let expected_report = expected_recovery_report(&reference);
    let initial = reference.initial_root();
    let snapshot = reference.snapshot().clone();
    let scenario = Scenario::create(initial);
    restore_snapshot(scenario.repo(), &snapshot);
    let mut first = ArtifactTransaction::test_engine(
        FaultingRealFs::no_fault(),
        ArtifactTransaction::recording_observer(),
    );
    let report = ArtifactTransaction::recover_detailed(scenario.repo(), &mut first)
        .expect("first recovery completes");
    assert_eq!(report, expected_report);
    let (_, first_observer) = first.into_parts();
    assert!(
        report.route != RecoveryRoute::Noop || first_observer.events().is_empty(),
        "Noop route must not mutate"
    );
    let final_snapshot = TreeSnapshot::capture(scenario.repo());
    reference.assert_terminal(&scenario, &final_snapshot);
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
    assert_eq!(final_snapshot, TreeSnapshot::capture(scenario.repo()));
}

fn assert_io_disposition(
    recipe: &Recipe,
    target: &OperationTarget,
    actual: &TransactionFailureDisposition,
) {
    let expected = match recipe.kind {
        RecipeKind::Recover => {
            let reference_at_fault = reference_before_target(recipe, target);
            let initial_phase = recipe.reference.protocol().formal_phase;
            if target.site.phase == SemanticPhase::RecoverTemp {
                match initial_phase {
                    None
                    | Some(crate::artifact_transaction::protocol::TransactionPhase::Preparing) => {
                        TransactionFailureDisposition::RecoveryRequired {
                            goal: RecoveryGoal::CleanResidue,
                        }
                    }
                    Some(crate::artifact_transaction::protocol::TransactionPhase::Prepared)
                    | Some(crate::artifact_transaction::protocol::TransactionPhase::RollingBack(
                        _,
                    )) => TransactionFailureDisposition::RecoveryRequired {
                        goal: RecoveryGoal::RestoreOriginal,
                    },
                    Some(crate::artifact_transaction::protocol::TransactionPhase::Committed) => {
                        TransactionFailureDisposition::CleanupDeferred
                    }
                }
            } else {
                match initial_phase {
                    None
                    | Some(crate::artifact_transaction::protocol::TransactionPhase::Preparing) => {
                        TransactionFailureDisposition::RecoveryRequired {
                            goal: RecoveryGoal::CleanResidue,
                        }
                    }
                    Some(crate::artifact_transaction::protocol::TransactionPhase::Prepared)
                    | Some(crate::artifact_transaction::protocol::TransactionPhase::RollingBack(
                        _,
                    )) => TransactionFailureDisposition::RecoveryRequired {
                        goal: if reference_at_fault.original_safely_restored() {
                            RecoveryGoal::CleanResidue
                        } else {
                            RecoveryGoal::RestoreOriginal
                        },
                    },
                    Some(crate::artifact_transaction::protocol::TransactionPhase::Committed) => {
                        TransactionFailureDisposition::CleanupDeferred
                    }
                }
            }
        }
        RecipeKind::Install => expected_install_io_disposition_for_recipe(recipe, target),
    };
    assert_eq!(actual, &expected, "wrong I/O disposition at {target:?}");
}

fn reference_before_target(recipe: &Recipe, target: &OperationTarget) -> ReferenceState {
    let mut reference = recipe.reference.clone();
    for event in &recipe.trace {
        if event.target == *target {
            return reference;
        }
        reference.apply_event(event);
    }
    panic!("target absent from recipe: {target:?}")
}

fn expected_install_io_disposition_for_recipe(
    recipe: &Recipe,
    target: &OperationTarget,
) -> TransactionFailureDisposition {
    // Index against success-path before targets so the formula matches matrix.rs exactly.
    let before_targets = before_targets_from_events(&recipe.trace);
    let (first_formal_publication_index, commit_publication_index) =
        install_publication_landmarks(&before_targets);
    let before_index = before_targets
        .iter()
        .position(|candidate| candidate == target)
        .unwrap_or_else(|| panic!("install before-target absent from recipe: {target:?}"));
    expected_install_io_disposition(
        target,
        before_index,
        first_formal_publication_index,
        commit_publication_index,
    )
}

fn expected_recovery_report(reference: &ReferenceState) -> RecoveryReport {
    let (route, terminal) = match reference.protocol().formal_phase {
        Some(crate::artifact_transaction::protocol::TransactionPhase::Preparing) => {
            (RecoveryRoute::Preparing, RecoveryTerminal::Recovered)
        }
        Some(crate::artifact_transaction::protocol::TransactionPhase::Prepared) => {
            (RecoveryRoute::Prepared, RecoveryTerminal::Recovered)
        }
        Some(crate::artifact_transaction::protocol::TransactionPhase::RollingBack(_)) => {
            (RecoveryRoute::ResumeRollback, RecoveryTerminal::Recovered)
        }
        Some(crate::artifact_transaction::protocol::TransactionPhase::Committed) => {
            (RecoveryRoute::Committed, RecoveryTerminal::Recovered)
        }
        None if reference.protocol().temp_exists => {
            (RecoveryRoute::Temp, RecoveryTerminal::CleanedResidue)
        }
        None => (RecoveryRoute::Noop, RecoveryTerminal::Noop),
    };
    RecoveryReport { route, terminal }
}

fn before_targets(trace: &[OperationEvent]) -> Vec<OperationTarget> {
    trace
        .iter()
        .filter(|event| event.target.boundary == OperationBoundary::Before)
        .map(|event| event.target.clone())
        .collect()
}

fn boundary_targets(trace: &[OperationEvent]) -> Vec<OperationTarget> {
    trace.iter().map(|event| event.target.clone()).collect()
}

fn is_recovery_phase(phase: SemanticPhase) -> bool {
    matches!(
        phase,
        SemanticPhase::RecoverPreparing
            | SemanticPhase::RecoverPrepared
            | SemanticPhase::RecoverResumeRollback
            | SemanticPhase::RecoverCommitted
            | SemanticPhase::RecoverTemp
            | SemanticPhase::RecoverNoop
    )
}

fn restore_snapshot(repo: &std::path::Path, snapshot: &TreeSnapshot) {
    for entry in std::fs::read_dir(repo).expect("read scenario repo") {
        let path = entry.expect("read scenario entry").path();
        let metadata = std::fs::symlink_metadata(&path).expect("scenario entry metadata");
        if metadata.is_dir() {
            std::fs::remove_dir_all(path).expect("remove scenario directory");
        } else {
            std::fs::remove_file(path).expect("remove scenario file");
        }
    }
    for (path, node) in &snapshot.0 {
        let path = repo.join(std::str::from_utf8(path).expect("scenario paths UTF-8"));
        match node {
            super::snapshot::TreeNode::Directory => {
                std::fs::create_dir_all(path).expect("restore directory");
            }
            super::snapshot::TreeNode::File(bytes) => {
                std::fs::create_dir_all(path.parent().expect("file parent"))
                    .expect("restore file parent");
                std::fs::write(path, bytes).expect("restore file");
            }
            super::snapshot::TreeNode::Symlink(target) => {
                std::fs::create_dir_all(path.parent().expect("symlink parent"))
                    .expect("restore symlink parent");
                std::os::unix::fs::symlink(
                    std::path::Path::new(std::str::from_utf8(target).expect("target UTF-8")),
                    path,
                )
                .expect("restore symlink");
            }
            super::snapshot::TreeNode::Other => panic!("cannot restore other node"),
        }
    }
}
