//! Linux-verified recoverable installation of one rendered artifact root.
//!
//! `ArtifactTransaction` owns the single-root durable protocol. It contains orchestration only:
//! all mutation and durability operations pass through `OperationExecutor`; filesystem inspection
//! remains deliberately outside the fault contract.

use crate::codegen::{ensure_trailing_newline, ArtifactPlan};
use crate::error::SchemaToolError;
use anyhow::{bail, Context, Result};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const JOURNAL_FILE: &str = ".schema-tool-generate-transaction.json";
const JOURNAL_TEMP_PREFIX: &str = ".schema-tool-generate-transaction.json.tmp-";
const PROTOCOL_VERSION: u32 = 2;

#[cfg(test)]
mod conformance;

mod error;
mod executor;
mod filesystem;
mod lock;
mod protocol;

#[cfg(test)]
pub(crate) use error::{
    CommittedJournalUncertain, JournalNotPublishedFailure, JournalTempCleanupFailure,
};
pub use error::{
    RecoveryGoal, SimulatedCrash, TransactionFailure, TransactionFailureDisposition,
    TransactionRunError,
};
pub(crate) use error::{RecoveryReport, RecoveryRoute, RecoveryTerminal};
use protocol::Operation;
use protocol::OperationBoundary;
#[cfg(test)]
use protocol::OperationTarget;
pub use protocol::SemanticPhase;

use executor::{NoFaultObserver, OperationExecutor, OperationObserver};
use filesystem::{RealTransactionFs, TransactionFs};
use lock::{acquire_real_lock, RealTransactionLock};
use protocol::{
    Journal, LogicalPath, PathRole, RenamePurpose, TransactionPhase, TreeEntryPath, TreeOwner,
    WritePurpose,
};

pub struct ArtifactTransaction<'a> {
    repo_root: &'a Path,
    _lock: RealTransactionLock,
}
impl<'a> ArtifactTransaction<'a> {
    pub fn begin(repo_root: &'a Path) -> Result<Self> {
        let lock = acquire_real_lock(repo_root).map_err(anyhow::Error::new)?;
        let mut engine = real_engine();
        recover_detailed_locked(repo_root, &mut engine).map(|_| ())?;
        Ok(Self {
            repo_root,
            _lock: lock,
        })
    }
    pub fn install(&mut self, plan: &ArtifactPlan) -> Result<()> {
        let mut engine = real_engine();
        install_locked(self.repo_root, plan, &mut engine)
    }
    #[cfg(test)]
    pub(crate) fn test_engine<F: TransactionFs, O: OperationObserver>(
        filesystem: F,
        observer: O,
    ) -> OperationExecutor<F, O> {
        OperationExecutor::new(filesystem, observer)
    }
    #[cfg(test)]
    pub(crate) fn journal_path(repo: &Path) -> PathBuf {
        repo.join(JOURNAL_FILE)
    }
    #[cfg(test)]
    pub(crate) fn journal_temp_path(repo: &Path, transaction_id: &str) -> PathBuf {
        repo.join(format!("{JOURNAL_TEMP_PREFIX}{transaction_id}"))
    }
    #[cfg(test)]
    pub(crate) fn install_detailed<F: TransactionFs, O: OperationObserver>(
        repo: &Path,
        plan: &ArtifactPlan,
        engine: &mut OperationExecutor<F, O>,
    ) -> std::result::Result<(), TransactionRunError> {
        install_locked(repo, plan, engine).map_err(as_run_error)
    }
    #[cfg(test)]
    pub(crate) fn install_detailed_with_id<F: TransactionFs, O: OperationObserver>(
        repo: &Path,
        plan: &ArtifactPlan,
        transaction_id: &str,
        engine: &mut OperationExecutor<F, O>,
    ) -> std::result::Result<(), TransactionRunError> {
        install_locked_with_id(repo, plan, transaction_id, engine).map_err(as_run_error)
    }
    #[cfg(test)]
    pub(crate) fn recover_detailed<F: TransactionFs, O: OperationObserver>(
        repo: &Path,
        engine: &mut OperationExecutor<F, O>,
    ) -> std::result::Result<RecoveryReport, TransactionRunError> {
        recover_detailed_locked(repo, engine).map_err(recovery_as_run_error)
    }
    #[cfg(test)]
    pub(crate) fn recording_observer() -> executor::RecordingObserver {
        executor::RecordingObserver::default()
    }
    #[cfg(test)]
    pub(crate) fn crash_observer(target: OperationTarget) -> executor::TargetedCrashObserver {
        executor::TargetedCrashObserver::new(target)
    }
}
#[cfg(test)]
fn as_run_error(error: anyhow::Error) -> TransactionRunError {
    match error.downcast::<TransactionRunError>() {
        Ok(error) => error,
        Err(error) => match error.downcast::<error::SimulatedCrash>() {
            Ok(crash) => TransactionRunError::Crash(crash),
            Err(primary) => TransactionRunError::Failure(TransactionFailure::new(
                TransactionFailureDisposition::NoMutation,
                primary,
            )),
        },
    }
}
#[cfg(test)]
fn recovery_as_run_error(error: anyhow::Error) -> TransactionRunError {
    match error.downcast::<TransactionRunError>() {
        Ok(error) => error,
        Err(error) => match error.downcast::<error::SimulatedCrash>() {
            Ok(crash) => TransactionRunError::Crash(crash),
            Err(primary) => TransactionRunError::Failure(TransactionFailure::new(
                TransactionFailureDisposition::RecoveryRequired {
                    goal: RecoveryGoal::CleanResidue,
                },
                primary,
            )),
        },
    }
}
pub fn recover_incomplete_transaction(repo_root: &Path) -> Result<()> {
    let _lock = acquire_real_lock(repo_root).map_err(anyhow::Error::new)?;
    let mut engine = real_engine();
    recover_detailed_locked(repo_root, &mut engine).map(|_| ())
}
pub fn install_artifact_plan(repo_root: &Path, plan: &ArtifactPlan) -> Result<()> {
    let mut transaction = ArtifactTransaction::begin(repo_root)?;
    transaction.install(plan)
}
fn real_engine() -> OperationExecutor<RealTransactionFs, NoFaultObserver> {
    OperationExecutor::new(RealTransactionFs, NoFaultObserver)
}

type Engine<F, O> = OperationExecutor<F, O>;

fn install_locked<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    plan: &ArtifactPlan,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    let transaction_id = unique_transaction_id();
    install_locked_with_id(repo, plan, &transaction_id, engine)
}

fn install_locked_with_id<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    plan: &ArtifactPlan,
    transaction_id: &str,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    if plan.roots().len() != 1 {
        bail!("artifact transaction requires exactly one artifact root")
    }
    if repo.join(JOURNAL_FILE).exists() {
        bail!("artifact transaction journal remains after recovery")
    }
    let mut journal = make_journal(repo, plan, transaction_id)?;
    if let Err(primary) = prepare(repo, plan, &mut journal, engine) {
        return fail_before_commit(repo, &mut journal, engine, primary);
    }
    match commit(repo, &mut journal, engine) {
        CommitResult::Committed => cleanup_committed(repo, &journal, engine).map_err(|primary| {
            if let Some(crash) = primary.downcast_ref::<error::SimulatedCrash>() {
                return TransactionRunError::Crash(crash.clone()).into();
            }
            TransactionRunError::Failure(TransactionFailure::new(
                TransactionFailureDisposition::CleanupDeferred,
                primary,
            ))
            .into()
        }),
        // The formal Committed journal rename's AfterSuccess boundary is the sole commit point.
        // These failures happen after that explicit progress transition, never after inspecting a
        // possibly corrupt on-disk journal to guess whether rollback is safe.
        CommitResult::PostCommitFailure(primary) => {
            if let Some(crash) = primary.downcast_ref::<error::SimulatedCrash>() {
                return Err(TransactionRunError::Crash(crash.clone()).into());
            }
            Err(TransactionRunError::Failure(TransactionFailure::new(
                TransactionFailureDisposition::CleanupDeferred,
                primary,
            ))
            .into())
        }
        CommitResult::CommitOutcomeUncertain(primary) => {
            Err(TransactionRunError::Failure(TransactionFailure::new(
                TransactionFailureDisposition::CommitOutcomeUncertain,
                primary,
            ))
            .into())
        }
        CommitResult::PreCommitFailure(primary) => {
            fail_before_commit(repo, &mut journal, engine, primary)
        }
    }
}

/// Commit progress is carried by control flow rather than inferred by rereading the journal.
enum CommitResult {
    PreCommitFailure(anyhow::Error),
    CommitOutcomeUncertain(anyhow::Error),
    Committed,
    PostCommitFailure(anyhow::Error),
}

fn fail_before_commit<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    journal: &mut Journal,
    engine: &mut Engine<F, O>,
    primary: anyhow::Error,
) -> Result<()> {
    if let Some(crash) = primary.downcast_ref::<error::SimulatedCrash>() {
        return Err(TransactionRunError::Crash(crash.clone()).into());
    }
    if primary
        .downcast_ref::<error::JournalNotPublishedFailure>()
        .is_some()
    {
        return Err(TransactionRunError::Failure(TransactionFailure::new(
            TransactionFailureDisposition::NoMutation,
            primary,
        ))
        .into());
    }
    if let Some(failure) = primary.downcast_ref::<error::JournalTempCleanupFailure>() {
        if let Some(crash) = failure.cleanup.downcast_ref::<error::SimulatedCrash>() {
            return Err(TransactionRunError::Crash(crash.clone()).into());
        }
        if failure.context.is_initial() {
            return Err(TransactionRunError::Failure(TransactionFailure::new(
                TransactionFailureDisposition::RecoveryRequired {
                    goal: RecoveryGoal::CleanResidue,
                },
                primary,
            ))
            .into());
        }
        if let Some(prior_phase) = failure.context.prior_phase() {
            journal.phase = prior_phase;
        }
        // A failed replacement-temp cleanup leaves a known residue. Starting another online
        // publication would collide with that residue and overwrite the exact secondary failure.
        // Stop at the source and let recovery clean the temp before following the prior formal
        // phase.
        return Err(TransactionRunError::Failure(TransactionFailure::new(
            TransactionFailureDisposition::RecoveryRequired {
                goal: rollback_recovery_goal(repo, journal),
            },
            primary,
        ))
        .into());
    }
    let disposition = TransactionFailureDisposition::RolledBackBeforeReturn;
    match rollback(repo, journal, engine) {
        Ok(()) => {
            Err(TransactionRunError::Failure(TransactionFailure::new(disposition, primary)).into())
        }
        Err(secondary) => {
            if let Some(crash) = secondary.downcast_ref::<error::SimulatedCrash>() {
                return Err(TransactionRunError::Crash(crash.clone()).into());
            }
            Err(TransactionRunError::Failure(
                TransactionFailure::new(
                    TransactionFailureDisposition::RecoveryRequired {
                        goal: rollback_recovery_goal(repo, journal),
                    },
                    primary,
                )
                .with_compensation(secondary),
            )
            .into())
        }
    }
}

fn rollback_recovery_goal(repo: &Path, journal: &Journal) -> RecoveryGoal {
    let root = repo_path(repo, &journal.root);
    let backup = repo_path(repo, &journal.backup);
    let original_safely_restored = if journal.original_exists {
        root.is_dir() && !backup.exists()
    } else {
        !root.exists()
    };
    if original_safely_restored {
        RecoveryGoal::CleanResidue
    } else {
        RecoveryGoal::RestoreOriginal
    }
}

fn is_committed_journal_publication_crash(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<error::SimulatedCrash>()
        .is_some_and(|crash| {
            crash.target.site.phase == SemanticPhase::InstallCommit
                && crash.target.boundary == OperationBoundary::AfterSuccess
                && matches!(
                    &crash.target.site.operation,
                    Operation::Rename {
                        purpose: RenamePurpose::PublishJournal,
                        ..
                    }
                )
        })
}
fn make_journal(repo: &Path, plan: &ArtifactPlan, id: &str) -> Result<Journal> {
    validate_transaction_id(id)?;
    let root = repo_path(repo, &plan.roots()[0]);
    let parent = root
        .parent()
        .ok_or_else(|| SchemaToolError::msg("artifact root has no parent"))?;
    require_safe_directory_chain(repo, parent)?;
    inspect_optional_directory(&root, "artifact root")?;
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| SchemaToolError::msg("artifact root name is not UTF-8"))?;
    let stage = parent.join(format!(".{name}.schema-tool-stage-{id}"));
    let backup = parent.join(format!(".{name}.schema-tool-backup-{id}"));
    let discard = parent.join(format!(".{name}.schema-tool-rollback-discard-{id}"));
    for (path, label) in [
        (&stage, "stage"),
        (&backup, "backup"),
        (&discard, "discard"),
    ] {
        require_absent(path, label)?;
    }
    Ok(Journal {
        version: PROTOCOL_VERSION,
        transaction_id: id.into(),
        root: plan.roots()[0].clone(),
        stage: relative_path(repo, &stage)?,
        backup: relative_path(repo, &backup)?,
        rollback_discard: relative_path(repo, &discard)?,
        original_exists: root.exists(),
        phase: TransactionPhase::Preparing,
    })
}

fn prepare<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    plan: &ArtifactPlan,
    journal: &mut Journal,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    write_journal(repo, journal, SemanticPhase::InstallPrepare, engine)?;
    let root = repo_path(repo, &journal.root);
    let stage = repo_path(repo, &journal.stage);
    create_dir(
        engine,
        SemanticPhase::InstallPrepare,
        LogicalPath::Stage,
        &stage,
    )?;
    if journal.original_exists {
        require_directory(&root, "artifact root")?;
        copy_tree(
            &root,
            &root,
            &stage,
            &stage,
            SemanticPhase::InstallPrepare,
            engine,
        )?;
    }
    for artifact in plan.artifacts() {
        let relative = artifact
            .relative_path()
            .strip_prefix(&journal.root)
            .and_then(|path| path.strip_prefix('/'))
            .ok_or_else(|| SchemaToolError::msg("artifact is not strictly below root"))?;
        let destination = stage.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        create_parent_dirs(
            &stage,
            destination.parent().expect("artifact path has parent"),
            SemanticPhase::InstallPrepare,
            engine,
        )?;
        write_artifact(
            &stage,
            &destination,
            ensure_trailing_newline(artifact.content()).as_bytes(),
            engine,
        )?;
    }
    sync_tree(&stage, &stage, SemanticPhase::InstallPrepare, engine)?;
    sync_dir(
        engine,
        SemanticPhase::InstallPrepare,
        LogicalPath::Repo,
        stage.parent().expect("stage parent"),
    )?;
    journal.phase = TransactionPhase::Prepared;
    write_journal(repo, journal, SemanticPhase::InstallPrepare, engine)
}

fn commit<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    journal: &mut Journal,
    engine: &mut Engine<F, O>,
) -> CommitResult {
    let result = (|| -> Result<()> {
        validate_runtime_paths(repo, journal)?;
        let root = repo_path(repo, &journal.root);
        let stage = repo_path(repo, &journal.stage);
        let backup = repo_path(repo, &journal.backup);
        let parent = root.parent().expect("root parent");
        if journal.original_exists {
            rename(
                engine,
                SemanticPhase::InstallCommit,
                RenamePurpose::PreserveOriginal,
                LogicalPath::Root,
                LogicalPath::Backup,
                &root,
                &backup,
            )?;
            sync_dir(
                engine,
                SemanticPhase::InstallCommit,
                LogicalPath::Repo,
                parent,
            )?;
        }
        rename(
            engine,
            SemanticPhase::InstallCommit,
            RenamePurpose::PublishStage,
            LogicalPath::Stage,
            LogicalPath::Root,
            &stage,
            &root,
        )?;
        sync_dir(
            engine,
            SemanticPhase::InstallCommit,
            LogicalPath::Repo,
            parent,
        )?;
        journal.phase = TransactionPhase::Committed;
        write_journal(repo, journal, SemanticPhase::InstallCommit, engine)
    })();
    match result {
        Ok(()) => CommitResult::Committed,
        Err(error)
            if error
                .downcast_ref::<error::CommittedJournalUncertain>()
                .is_some() =>
        {
            CommitResult::CommitOutcomeUncertain(error)
        }
        // A crash at AfterSuccess of PublishJournal is also after the explicit commit point.
        Err(error) if is_committed_journal_publication_crash(&error) => {
            CommitResult::PostCommitFailure(error)
        }
        Err(error) => CommitResult::PreCommitFailure(error),
    }
}

fn recovery_error(
    disposition: TransactionFailureDisposition,
    primary: anyhow::Error,
) -> anyhow::Error {
    if let Some(crash) = primary.downcast_ref::<error::SimulatedCrash>() {
        return TransactionRunError::Crash(crash.clone()).into();
    }
    TransactionRunError::Failure(TransactionFailure::new(disposition, primary)).into()
}

fn recovery_disposition(phase: TransactionPhase) -> TransactionFailureDisposition {
    match phase {
        TransactionPhase::Preparing => TransactionFailureDisposition::RecoveryRequired {
            goal: RecoveryGoal::CleanResidue,
        },
        TransactionPhase::Prepared | TransactionPhase::RollingBack(_) => {
            TransactionFailureDisposition::RecoveryRequired {
                goal: RecoveryGoal::RestoreOriginal,
            }
        }
        TransactionPhase::Committed => TransactionFailureDisposition::CleanupDeferred,
    }
}

fn recover_detailed_locked<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    engine: &mut Engine<F, O>,
) -> Result<RecoveryReport> {
    let journal_path = repo.join(JOURNAL_FILE);
    let journal = read_journal_if_present(repo, &journal_path).map_err(|error| {
        recovery_error(TransactionFailureDisposition::StoredStateInvalid, error)
    })?;
    let Some(mut journal) = journal else {
        let removed = cleanup_temps(repo, SemanticPhase::RecoverTemp, engine).map_err(|error| {
            recovery_error(
                TransactionFailureDisposition::RecoveryRequired {
                    goal: RecoveryGoal::CleanResidue,
                },
                error,
            )
        })?;
        return Ok(RecoveryReport {
            route: if removed {
                RecoveryRoute::Temp
            } else {
                RecoveryRoute::Noop
            },
            terminal: if removed {
                RecoveryTerminal::CleanedResidue
            } else {
                RecoveryTerminal::Noop
            },
        });
    };
    cleanup_temps(repo, SemanticPhase::RecoverTemp, engine)
        .map_err(|error| recovery_error(recovery_disposition(journal.phase), error))?;
    validate_runtime_paths(repo, &journal)
        .map_err(|error| recovery_error(recovery_disposition(journal.phase), error))?;
    let route = match journal.phase {
        TransactionPhase::Preparing => {
            recover_preparing(repo, &journal, engine).map_err(|error| {
                recovery_error(
                    TransactionFailureDisposition::RecoveryRequired {
                        goal: RecoveryGoal::CleanResidue,
                    },
                    error,
                )
            })?;
            RecoveryRoute::Preparing
        }
        TransactionPhase::Prepared => {
            rollback_with_phase(repo, &mut journal, SemanticPhase::RecoverPrepared, engine)
                .map_err(|error| {
                    recovery_error(
                        TransactionFailureDisposition::RecoveryRequired {
                            goal: rollback_recovery_goal(repo, &journal),
                        },
                        error,
                    )
                })?;
            RecoveryRoute::Prepared
        }
        TransactionPhase::RollingBack(_) => {
            rollback_with_phase(
                repo,
                &mut journal,
                SemanticPhase::RecoverResumeRollback,
                engine,
            )
            .map_err(|error| {
                recovery_error(
                    TransactionFailureDisposition::RecoveryRequired {
                        goal: rollback_recovery_goal(repo, &journal),
                    },
                    error,
                )
            })?;
            RecoveryRoute::ResumeRollback
        }
        TransactionPhase::Committed => {
            cleanup_committed_with_phase(repo, &journal, SemanticPhase::RecoverCommitted, engine)
                .map_err(|error| {
                recovery_error(TransactionFailureDisposition::CleanupDeferred, error)
            })?;
            RecoveryRoute::Committed
        }
    };
    Ok(RecoveryReport {
        route,
        terminal: RecoveryTerminal::Recovered,
    })
}
fn recover_preparing<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    journal: &Journal,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    let root = repo_path(repo, &journal.root);
    let stage = repo_path(repo, &journal.stage);
    if journal.original_exists {
        require_directory(&root, "original root")?;
    } else if root.exists() {
        bail!("root unexpectedly exists while recovering Preparing")
    }
    remove_optional_tree(
        engine,
        SemanticPhase::RecoverPreparing,
        PathRole::Stage,
        &stage,
    )?;
    remove_journal(repo, SemanticPhase::RecoverPreparing, engine)?;
    cleanup_temps(repo, SemanticPhase::RecoverTemp, engine).map(|_| ())
}

fn rollback<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    journal: &mut Journal,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    rollback_with_phase(repo, journal, SemanticPhase::InstallRollback, engine)
}
fn rollback_with_phase<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    journal: &mut Journal,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    journal.phase = TransactionPhase::RollingBack(protocol::RollbackState::Started);
    write_journal(repo, journal, phase, engine)?;
    let root = repo_path(repo, &journal.root);
    let stage = repo_path(repo, &journal.stage);
    let backup = repo_path(repo, &journal.backup);
    let discard = repo_path(repo, &journal.rollback_discard);
    let parent = root.parent().expect("root parent");
    if journal.original_exists && backup.exists() {
        if root.exists() {
            rename(
                engine,
                phase,
                RenamePurpose::DiscardNew,
                LogicalPath::Root,
                LogicalPath::Discard,
                &root,
                &discard,
            )?;
            sync_dir(engine, phase, LogicalPath::Repo, parent)?;
        }
        journal.phase = TransactionPhase::RollingBack(protocol::RollbackState::NewMovedToDiscard);
        write_journal(repo, journal, phase, engine)?;
        if !root.exists() {
            rename(
                engine,
                phase,
                RenamePurpose::RestoreOriginal,
                LogicalPath::Backup,
                LogicalPath::Root,
                &backup,
                &root,
            )?;
            sync_dir(engine, phase, LogicalPath::Repo, parent)?;
        }
    } else if !journal.original_exists && root.exists() {
        rename(
            engine,
            phase,
            RenamePurpose::DiscardNew,
            LogicalPath::Root,
            LogicalPath::Discard,
            &root,
            &discard,
        )?;
        sync_dir(engine, phase, LogicalPath::Repo, parent)?;
    }
    journal.phase = TransactionPhase::RollingBack(protocol::RollbackState::OriginalRestored);
    write_journal(repo, journal, phase, engine)?;
    remove_optional_tree(engine, phase, PathRole::Stage, &stage)?;
    remove_optional_tree(engine, phase, PathRole::Discard, &discard)?;
    remove_optional_tree(engine, phase, PathRole::Backup, &backup)?;
    sync_dir(engine, phase, LogicalPath::Repo, parent)?;
    remove_journal(repo, phase, engine)?;
    cleanup_temps(repo, SemanticPhase::RecoverTemp, engine).map(|_| ())
}
fn cleanup_committed<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    journal: &Journal,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    cleanup_committed_with_phase(repo, journal, SemanticPhase::InstallCleanup, engine)
}
fn cleanup_committed_with_phase<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    journal: &Journal,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    let root = repo_path(repo, &journal.root);
    require_directory(&root, "committed root")?;
    let parent = root.parent().expect("root parent");
    remove_optional_tree(
        engine,
        phase,
        PathRole::Stage,
        &repo_path(repo, &journal.stage),
    )?;
    remove_optional_tree(
        engine,
        phase,
        PathRole::Backup,
        &repo_path(repo, &journal.backup),
    )?;
    remove_optional_tree(
        engine,
        phase,
        PathRole::Discard,
        &repo_path(repo, &journal.rollback_discard),
    )?;
    sync_dir(engine, phase, LogicalPath::Repo, parent)?;
    remove_journal(repo, phase, engine)?;
    cleanup_temps(repo, SemanticPhase::RecoverTemp, engine).map(|_| ())
}

fn write_journal<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    journal: &Journal,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    let path = repo.join(JOURNAL_FILE);
    inspect_optional_file(&path, "journal")?;
    let temp = repo.join(format!("{JOURNAL_TEMP_PREFIX}{}", journal.transaction_id));
    require_absent(&temp, "journal temporary file")?;
    let bytes = serde_json::to_vec_pretty(journal)?;
    let publication_context = match read_journal_if_present(repo, &path)? {
        None => error::JournalPublicationContext::Initial {
            target_phase: journal.phase,
        },
        Some(prior) => error::JournalPublicationContext::Replacement {
            prior_phase: prior.phase,
            target_phase: journal.phase,
        },
    };
    let mut file = match engine.execute(
        phase,
        Operation::CreateFile {
            target: LogicalPath::JournalTemp,
        },
        |fs| Ok(fs.create_file(&temp)?),
    ) {
        Ok(file) => file,
        Err(primary) => {
            return journal_pre_rename_failure(
                repo,
                &temp,
                phase,
                publication_context,
                engine,
                primary,
            )
        }
    };
    if let Err(primary) = engine.execute(
        phase,
        Operation::WriteBytes {
            purpose: WritePurpose::JournalTemp,
            target: LogicalPath::JournalTemp,
            journal_target_phase: Some(journal.phase),
        },
        |fs| Ok(fs.write_bytes(&mut file, &bytes)?),
    ) {
        return journal_pre_rename_failure(
            repo,
            &temp,
            phase,
            publication_context,
            engine,
            primary,
        );
    }
    if let Err(primary) = engine.execute(
        phase,
        Operation::SyncFile {
            target: LogicalPath::JournalTemp,
        },
        |fs| Ok(fs.sync_file(&file)?),
    ) {
        return journal_pre_rename_failure(
            repo,
            &temp,
            phase,
            publication_context,
            engine,
            primary,
        );
    }
    if let Err(primary) = rename(
        engine,
        phase,
        RenamePurpose::PublishJournal,
        LogicalPath::JournalTemp,
        LogicalPath::FormalJournal,
        &temp,
        &path,
    ) {
        return journal_pre_rename_failure(
            repo,
            &temp,
            phase,
            publication_context,
            engine,
            primary,
        );
    }
    match sync_dir(engine, phase, LogicalPath::Repo, repo) {
        Ok(()) => Ok(()),
        Err(primary) if primary.downcast_ref::<error::SimulatedCrash>().is_some() => Err(primary),
        Err(primary) if journal.phase == TransactionPhase::Committed => {
            Err(error::CommittedJournalUncertain { primary }.into())
        }
        Err(primary) => Err(primary),
    }
}
fn journal_pre_rename_failure<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    temp: &Path,
    phase: SemanticPhase,
    publication_context: error::JournalPublicationContext,
    engine: &mut Engine<F, O>,
    primary: anyhow::Error,
) -> Result<()> {
    if let Some(crash) = primary.downcast_ref::<error::SimulatedCrash>() {
        return Err(error::SimulatedCrash::new(crash.target.clone()).into());
    }
    match cleanup_known_journal_temp(repo, temp, phase, engine) {
        Ok(()) if publication_context.is_initial() => {
            Err(anyhow::Error::new(error::JournalNotPublishedFailure {
                context: publication_context,
                primary,
            }))
        }
        Ok(()) => Err(primary),
        Err(cleanup) => Err(anyhow::Error::new(error::JournalTempCleanupFailure {
            context: publication_context,
            primary,
            cleanup,
        })),
    }
}
fn cleanup_known_journal_temp<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    temp: &Path,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    match std::fs::symlink_metadata(temp) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => bail!(
            "known journal temporary is not a regular file: {}",
            temp.display()
        ),
    }
    engine.execute(
        phase,
        Operation::RemoveFile {
            target: LogicalPath::JournalTemp,
        },
        |fs| Ok(fs.remove_file(temp)?),
    )?;
    sync_dir(engine, phase, LogicalPath::Repo, repo)
}
fn remove_journal<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    let path = repo.join(JOURNAL_FILE);
    inspect_optional_file(&path, "journal")?;
    if path.exists() {
        engine.execute(
            phase,
            Operation::RemoveFile {
                target: LogicalPath::FormalJournal,
            },
            |fs| Ok(fs.remove_file(&path)?),
        )?;
        sync_dir(engine, phase, LogicalPath::Repo, repo)?;
    }
    Ok(())
}
fn cleanup_temps<F: TransactionFs, O: OperationObserver>(
    repo: &Path,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<bool> {
    let mut paths = std::fs::read_dir(repo)?.collect::<std::result::Result<Vec<_>, _>>()?;
    paths.sort_by_key(|entry| entry.file_name());
    let mut removed = false;
    for entry in paths {
        let path = entry.path();
        let file_name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("repository entry name is not valid UTF-8"))?;
        if !file_name.starts_with(JOURNAL_TEMP_PREFIX) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!(
                "journal temporary residue is not a regular file: {}",
                path.display()
            )
        }
        engine.execute(
            phase,
            Operation::RemoveFile {
                target: LogicalPath::JournalTemp,
            },
            |fs| Ok(fs.remove_file(&path)?),
        )?;
        removed = true;
    }
    if removed {
        sync_dir(engine, phase, LogicalPath::Repo, repo)?;
    }
    Ok(removed)
}

fn copy_tree<F: TransactionFs, O: OperationObserver>(
    source_tree: &Path,
    source: &Path,
    destination_tree: &Path,
    destination: &Path,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    let mut entries = std::fs::read_dir(source)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let source_target = tree_entry(TreeOwner::Root, source_tree, &from)?;
        let destination_target = tree_entry(TreeOwner::Stage, destination_tree, &to)?;
        let metadata = std::fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() {
            bail!("artifact root contains symlink: {}", from.display())
        }
        if metadata.is_dir() {
            create_dir(engine, phase, destination_target, &to)?;
            copy_tree(source_tree, &from, destination_tree, &to, phase, engine)?;
        } else if metadata.is_file() {
            engine.execute(
                phase,
                Operation::CopyFile {
                    source: source_target,
                    destination: destination_target.clone(),
                },
                |fs| Ok(fs.copy_file(&from, &to)?),
            )?;
            engine.execute(
                phase,
                Operation::SyncFile {
                    target: destination_target,
                },
                |fs| fs.sync_file_at(&to),
            )?;
        } else {
            bail!("artifact root contains special file: {}", from.display())
        }
    }
    Ok(())
}

fn create_parent_dirs<F: TransactionFs, O: OperationObserver>(
    base: &Path,
    parent: &Path,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    let mut current = base.to_path_buf();
    for part in parent.strip_prefix(base)?.components() {
        current.push(part.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => bail!("staged parent is not a directory: {}", current.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_dir(
                engine,
                phase,
                tree_entry(TreeOwner::Stage, base, &current)?,
                &current,
            )?,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn write_artifact<F: TransactionFs, O: OperationObserver>(
    stage: &Path,
    path: &Path,
    bytes: &[u8],
    engine: &mut Engine<F, O>,
) -> Result<()> {
    inspect_optional_file(path, "staged artifact")?;
    let target = tree_entry(TreeOwner::Stage, stage, path)?;
    let existed = path.exists();
    let mut file = if existed {
        engine.filesystem_mut().open_existing_file(path)?
    } else {
        engine.execute(
            SemanticPhase::InstallPrepare,
            Operation::CreateFile {
                target: target.clone(),
            },
            |fs| Ok(fs.create_file(path)?),
        )?
    };
    engine.execute(
        SemanticPhase::InstallPrepare,
        Operation::Truncate {
            target: target.clone(),
        },
        |fs| Ok(fs.truncate(&file)?),
    )?;
    engine.execute(
        SemanticPhase::InstallPrepare,
        Operation::WriteBytes {
            purpose: WritePurpose::Artifact,
            target,
            journal_target_phase: None,
        },
        |fs| Ok(fs.write_bytes(&mut file, bytes)?),
    )?;
    engine.execute(
        SemanticPhase::InstallPrepare,
        Operation::SyncFile {
            target: tree_entry(TreeOwner::Stage, stage, path)?,
        },
        |fs| Ok(fs.sync_file(&file)?),
    )
}

fn sync_tree<F: TransactionFs, O: OperationObserver>(
    stage: &Path,
    path: &Path,
    phase: SemanticPhase,
    engine: &mut Engine<F, O>,
) -> Result<()> {
    let mut entries = std::fs::read_dir(path)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child = entry.path();
        let metadata = std::fs::symlink_metadata(&child)?;
        if metadata.is_dir() {
            sync_tree(stage, &child, phase, engine)?;
        } else if metadata.is_file() {
            engine.execute(
                phase,
                Operation::SyncFile {
                    target: tree_entry(TreeOwner::Stage, stage, &child)?,
                },
                |fs| fs.sync_file_at(&child),
            )?;
        } else {
            bail!("stage contains non-file entry")
        }
    }
    let target = if path == stage {
        LogicalPath::Stage
    } else {
        tree_entry(TreeOwner::Stage, stage, path)?
    };
    sync_dir(engine, phase, target, path)
}

fn tree_entry(owner: TreeOwner, tree: &Path, entry: &Path) -> Result<LogicalPath> {
    Ok(LogicalPath::tree_entry(
        owner,
        TreeEntryPath::from_relative_path(entry.strip_prefix(tree)?)?,
    ))
}

fn create_dir<F: TransactionFs, O: OperationObserver>(
    engine: &mut Engine<F, O>,
    phase: SemanticPhase,
    target: LogicalPath,
    path: &Path,
) -> Result<()> {
    engine.execute(phase, Operation::CreateDir { target }, |fs| {
        Ok(fs.create_dir(path)?)
    })
}

fn rename<F: TransactionFs, O: OperationObserver>(
    engine: &mut Engine<F, O>,
    phase: SemanticPhase,
    purpose: RenamePurpose,
    source_role: LogicalPath,
    destination_role: LogicalPath,
    source: &Path,
    destination: &Path,
) -> Result<()> {
    let metadata = std::fs::symlink_metadata(source)
        .with_context(|| format!("inspect rename source {}", source.display()))?;
    if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
        bail!(
            "rename source is neither a regular file nor directory: {}",
            source.display()
        )
    }
    if destination_role == LogicalPath::FormalJournal {
        inspect_optional_file(destination, "journal")?;
    } else {
        require_absent(destination, "rename destination")?;
    }
    engine.execute(
        phase,
        Operation::Rename {
            purpose,
            source: source_role,
            destination: destination_role,
        },
        |fs| Ok(fs.rename(source, destination)?),
    )
}

fn sync_dir<F: TransactionFs, O: OperationObserver>(
    engine: &mut Engine<F, O>,
    phase: SemanticPhase,
    target: LogicalPath,
    path: &Path,
) -> Result<()> {
    engine.execute(phase, Operation::SyncDir { target }, |fs| fs.sync_dir(path))
}

fn remove_optional_tree<F: TransactionFs, O: OperationObserver>(
    engine: &mut Engine<F, O>,
    phase: SemanticPhase,
    role: PathRole,
    path: &Path,
) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => engine.execute(
            phase,
            Operation::RemoveTree {
                role,
                target: role.logical_path(),
            },
            |fs| Ok(fs.remove_tree(path)?),
        ),
        Ok(_) => bail!("transaction directory is not regular: {}", path.display()),
    }
}

fn read_journal_if_present(repo: &Path, path: &Path) -> Result<Option<Journal>> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            bail!("journal is not regular")
        }
        Ok(_) => {}
    }
    let journal: Journal =
        serde_json::from_slice(&std::fs::read(path)?).context("parse transaction journal")?;
    validate_journal(repo, &journal)?;
    Ok(Some(journal))
}

fn validate_journal(repo: &Path, journal: &Journal) -> Result<()> {
    if journal.version != PROTOCOL_VERSION {
        bail!("unsupported artifact transaction journal version")
    }
    validate_transaction_id(&journal.transaction_id)?;
    for path in [
        &journal.root,
        &journal.stage,
        &journal.backup,
        &journal.rollback_discard,
    ] {
        validate_relative_path(path)?;
    }
    let root = repo_path(repo, &journal.root);
    let parent = root
        .parent()
        .ok_or_else(|| SchemaToolError::msg("journal root has no parent"))?;
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| SchemaToolError::msg("root name not UTF-8"))?;
    let expected = [
        parent.join(format!(
            ".{name}.schema-tool-stage-{}",
            journal.transaction_id
        )),
        parent.join(format!(
            ".{name}.schema-tool-backup-{}",
            journal.transaction_id
        )),
        parent.join(format!(
            ".{name}.schema-tool-rollback-discard-{}",
            journal.transaction_id
        )),
    ];
    if [
        repo_path(repo, &journal.stage),
        repo_path(repo, &journal.backup),
        repo_path(repo, &journal.rollback_discard),
    ] != expected
    {
        bail!("journal paths do not match fixed transaction protocol")
    }
    Ok(())
}
fn validate_runtime_paths(repo: &Path, journal: &Journal) -> Result<()> {
    validate_journal(repo, journal)?;
    for path in [
        &journal.root,
        &journal.stage,
        &journal.backup,
        &journal.rollback_discard,
    ] {
        let metadata = std::fs::symlink_metadata(repo_path(repo, path));
        if matches!(metadata, Ok(ref value) if value.file_type().is_symlink()) {
            bail!("transaction path must not be symlink")
        }
    }
    Ok(())
}
fn require_directory(path: &Path, label: &str) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path).with_context(|| format!("inspect {label}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("{label} is not regular directory: {}", path.display())
    }
    Ok(())
}
fn inspect_optional_directory(path: &Path, label: &str) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => bail!("{label} is not regular directory"),
        Err(error) => Err(error.into()),
    }
}
fn inspect_optional_file(path: &Path, label: &str) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => bail!("{label} is not regular file"),
        Err(error) => Err(error.into()),
    }
}
fn require_safe_directory_chain(repo: &Path, path: &Path) -> Result<()> {
    require_directory(repo, "repository root")?;
    let mut current = repo.to_path_buf();
    for part in path.strip_prefix(repo)?.components() {
        current.push(part.as_os_str());
        require_directory(&current, "root parent")?;
    }
    Ok(())
}
fn require_absent(path: &Path, label: &str) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => bail!("{label} already exists: {}", path.display()),
        Err(error) => Err(error.into()),
    }
}
fn validate_relative_path(path: &str) -> Result<()> {
    let value = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || value.is_absolute()
        || value.components().any(|part| {
            matches!(
                part,
                Component::ParentDir
                    | Component::CurDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        bail!("unsafe transaction path")
    }
    Ok(())
}
fn validate_transaction_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        bail!("unsafe transaction id")
    }
    Ok(())
}
fn relative_path(repo: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(repo)?;
    let value = relative
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("transaction path is not valid UTF-8"))?;
    Ok(value.replace(std::path::MAIN_SEPARATOR, "/"))
}
fn repo_path(repo: &Path, relative: &str) -> PathBuf {
    repo.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR))
}
fn unique_transaction_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::{ArtifactKind, GeneratedArtifact};
    use crate::manifest::GenerationTarget;
    #[test]
    fn detailed_harness_returns_typed_success_and_records_operations() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("out")).unwrap();
        let plan = ArtifactPlan::try_new(
            vec![GeneratedArtifact::new(
                GenerationTarget::Rust,
                ArtifactKind::Types,
                "out/generated/a.rs",
                "new",
                vec![],
            )],
            ["out/generated".into()],
        )
        .unwrap();
        let mut engine = ArtifactTransaction::test_engine(
            RealTransactionFs,
            ArtifactTransaction::recording_observer(),
        );
        ArtifactTransaction::install_detailed(temp.path(), &plan, &mut engine).unwrap();
        let (_, observer) = engine.into_parts();
        assert!(!observer.events().is_empty());
        let mut recovery = ArtifactTransaction::test_engine(
            RealTransactionFs,
            ArtifactTransaction::recording_observer(),
        );
        assert_eq!(
            ArtifactTransaction::recover_detailed(temp.path(), &mut recovery).unwrap(),
            RecoveryReport {
                route: RecoveryRoute::Noop,
                terminal: RecoveryTerminal::Noop
            }
        );
    }

    #[test]
    fn targeted_crash_is_propagated_as_the_crash_sentinel() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("out")).unwrap();
        let plan = ArtifactPlan::try_new(
            vec![GeneratedArtifact::new(
                GenerationTarget::Rust,
                ArtifactKind::Types,
                "out/generated/a.rs",
                "new",
                vec![],
            )],
            ["out/generated".into()],
        )
        .unwrap();
        let target = OperationTarget {
            site: protocol::OperationSite {
                phase: SemanticPhase::InstallPrepare,
                operation: Operation::CreateFile {
                    target: LogicalPath::JournalTemp,
                },
            },
            occurrence: None,
            boundary: OperationBoundary::Before,
        };
        let mut engine = ArtifactTransaction::test_engine(
            RealTransactionFs,
            ArtifactTransaction::crash_observer(target),
        );
        assert!(matches!(
            ArtifactTransaction::install_detailed(temp.path(), &plan, &mut engine),
            Err(TransactionRunError::Crash(_))
        ));
    }

    #[test]
    fn successful_install_preserves_unplanned_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("out/generated");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("extra"), b"old").unwrap();
        let plan = ArtifactPlan::try_new(
            vec![GeneratedArtifact::new(
                GenerationTarget::Rust,
                ArtifactKind::Types,
                "out/generated/a.rs",
                "new",
                vec![],
            )],
            ["out/generated".into()],
        )
        .unwrap();
        install_artifact_plan(temp.path(), &plan).unwrap();
        assert_eq!(std::fs::read(root.join("a.rs")).unwrap(), b"new\n");
        assert_eq!(std::fs::read(root.join("extra")).unwrap(), b"old");
    }
}
