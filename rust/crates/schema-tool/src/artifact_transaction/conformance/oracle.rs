use super::reference_model::ReferenceState;
use super::scenario::Scenario;
use super::snapshot::TreeSnapshot;
use crate::artifact_transaction::executor::{ObserverControl, OperationObserver};
use crate::artifact_transaction::protocol::{
    Operation, OperationBoundary, OperationEvent, OperationTarget, RenamePurpose, SemanticPhase,
};
use crate::artifact_transaction::TransactionFailureDisposition;
use std::path::PathBuf;

pub(super) struct ModelCheckingObserver {
    repo: PathBuf,
    model: ReferenceState,
    crash_target: Option<OperationTarget>,
    events: Vec<OperationEvent>,
}

impl ModelCheckingObserver {
    pub(super) fn new(scenario: &Scenario, crash_target: Option<OperationTarget>) -> Self {
        Self::from_snapshot(
            scenario.repo().to_path_buf(),
            ReferenceState::new(scenario),
            crash_target,
        )
    }

    pub(super) fn from_snapshot(
        repo: PathBuf,
        model: ReferenceState,
        crash_target: Option<OperationTarget>,
    ) -> Self {
        Self {
            repo,
            model,
            crash_target,
            events: Vec::new(),
        }
    }

    pub(super) fn reference_state(&self) -> &ReferenceState {
        &self.model
    }

    pub(super) fn events(&self) -> &[OperationEvent] {
        &self.events
    }
}

impl OperationObserver for ModelCheckingObserver {
    fn observe(&mut self, event: &OperationEvent) -> ObserverControl {
        self.events.push(event.clone());
        self.model.apply_event(event);
        if event.target.boundary
            == crate::artifact_transaction::protocol::OperationBoundary::AfterSuccess
        {
            self.model
                .assert_matches(&TreeSnapshot::capture(&self.repo));
        }
        if self.crash_target.as_ref() == Some(&event.target) {
            ObserverControl::SimulatedCrash
        } else {
            ObserverControl::Continue
        }
    }
}

pub(super) fn assert_exact_fault_prefix(
    baseline: &[OperationEvent],
    fault: &[OperationEvent],
    target: &OperationTarget,
) {
    let baseline_index = baseline
        .iter()
        .position(|event| event.target == *target)
        .unwrap_or_else(|| panic!("target absent from baseline: {target:?}"));
    let expected_len = baseline_index + 1;
    assert!(
        fault.len() >= expected_len,
        "fault trace ended before selected target: {target:?}"
    );
    assert_eq!(
        &fault[..expected_len],
        &baseline[..expected_len],
        "trace prefix diverged before selected target: {target:?}"
    );
    assert_eq!(&fault[baseline_index].target, target);
}

/// Test-only oracle for install I/O failure disposition on the success-path trace.
///
/// Shared by matrix and worklist so the expected disposition formula has a single
/// source of truth. This is deliberately independent of production disposition
/// construction; it only consumes success-path before-target indices.
///
/// - `before_index`: index of the injected before-target among success before targets
/// - `first_formal_publication_before_index`: first `PublishJournal` rename before-target
/// - `commit_publication_before_index`: `InstallCommit` `PublishJournal` rename before-target
pub(super) fn expected_install_io_disposition(
    target: &OperationTarget,
    before_index: usize,
    first_formal_publication_before_index: usize,
    commit_publication_before_index: usize,
) -> TransactionFailureDisposition {
    if before_index < commit_publication_before_index {
        if before_index <= first_formal_publication_before_index {
            TransactionFailureDisposition::NoMutation
        } else {
            TransactionFailureDisposition::RolledBackBeforeReturn
        }
    } else if before_index == commit_publication_before_index {
        // Commit publication rename itself still rolls back online before return.
        TransactionFailureDisposition::RolledBackBeforeReturn
    } else if target.site.phase == SemanticPhase::InstallCommit {
        TransactionFailureDisposition::CommitOutcomeUncertain
    } else {
        TransactionFailureDisposition::CleanupDeferred
    }
}

/// Locate the formal-publication and commit-publication before-target indices on a
/// success-path before-target list. Panics if the protocol landmarks are missing.
pub(super) fn install_publication_landmarks(before_targets: &[OperationTarget]) -> (usize, usize) {
    let first_formal_publication_before_index = before_targets
        .iter()
        .position(|target| {
            matches!(
                target.site.operation,
                Operation::Rename {
                    purpose: RenamePurpose::PublishJournal,
                    ..
                }
            )
        })
        .expect("successful install trace contains initial formal publication");
    let commit_publication_before_index = before_targets
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
        .expect("successful install trace contains formal Committed publication");
    (
        first_formal_publication_before_index,
        commit_publication_before_index,
    )
}

/// Before-boundary targets drawn from a full success event stream (Before + AfterSuccess).
pub(super) fn before_targets_from_events(events: &[OperationEvent]) -> Vec<OperationTarget> {
    events
        .iter()
        .filter(|event| event.target.boundary == OperationBoundary::Before)
        .map(|event| event.target.clone())
        .collect()
}
