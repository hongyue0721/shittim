//! Typed transaction failures. Public CLI methods render these at their `anyhow` boundary.

use super::protocol::{OperationTarget, TransactionPhase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryGoal {
    RestoreOriginal,
    CleanResidue,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionFailureDisposition {
    NoMutation,
    RolledBackBeforeReturn,
    RecoveryRequired {
        goal: RecoveryGoal,
    },
    CommitOutcomeUncertain,
    CleanupDeferred,
    /// A formal journal exists but cannot be decoded or validated. Recovery must fail closed:
    /// the stored state is not trustworthy enough to authorize either rollback or cleanup.
    StoredStateInvalid,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalPublicationContext {
    Initial {
        target_phase: TransactionPhase,
    },
    Replacement {
        prior_phase: TransactionPhase,
        target_phase: TransactionPhase,
    },
}
impl JournalPublicationContext {
    pub(crate) fn is_initial(self) -> bool {
        matches!(self, Self::Initial { .. })
    }
    pub(crate) fn prior_phase(self) -> Option<TransactionPhase> {
        match self {
            Self::Initial { .. } => None,
            Self::Replacement { prior_phase, .. } => Some(prior_phase),
        }
    }
}

#[derive(Debug)]
pub struct JournalTempCleanupFailure {
    pub(crate) context: JournalPublicationContext,
    pub(crate) primary: anyhow::Error,
    pub(crate) cleanup: anyhow::Error,
}
impl JournalTempCleanupFailure {
    #[cfg(test)]
    pub(crate) fn source_error(&self) -> &std::io::Error {
        self.primary
            .downcast_ref::<std::io::Error>()
            .expect("journal cleanup primary is I/O")
    }
    #[cfg(test)]
    pub(crate) fn cleanup_error(&self) -> &std::io::Error {
        self.cleanup
            .downcast_ref::<std::io::Error>()
            .expect("journal cleanup compensation is I/O")
    }
}
impl std::fmt::Display for JournalTempCleanupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "journal temporary cleanup failed during {:?} publication after operation failure: {}; cleanup: {}",
            self.context, self.primary, self.cleanup
        )
    }
}
impl std::error::Error for JournalTempCleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.primary.as_ref())
    }
}

/// The first formal journal publication did not happen and its known temporary file was
/// durably removed. No transaction state exists that could justify an online rollback.
#[derive(Debug)]
pub struct JournalNotPublishedFailure {
    pub(crate) context: JournalPublicationContext,
    pub(crate) primary: anyhow::Error,
}
impl JournalNotPublishedFailure {
    #[cfg(test)]
    pub(crate) fn source_error(&self) -> &std::io::Error {
        self.primary
            .downcast_ref::<std::io::Error>()
            .expect("journal publication primary is I/O")
    }
}
impl std::fmt::Display for JournalNotPublishedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "initial transaction journal was not published ({:?}): {}",
            self.context, self.primary
        )
    }
}
impl std::error::Error for JournalNotPublishedFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.primary.as_ref())
    }
}

#[derive(Debug)]
pub struct CommittedJournalUncertain {
    pub(crate) primary: anyhow::Error,
}
impl CommittedJournalUncertain {
    #[cfg(test)]
    pub(crate) fn source_error(&self) -> &std::io::Error {
        self.primary
            .downcast_ref::<std::io::Error>()
            .expect("committed uncertainty primary is I/O")
    }
}
impl std::fmt::Display for CommittedJournalUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Committed journal durability is uncertain: {}",
            self.primary
        )
    }
}
impl std::error::Error for CommittedJournalUncertain {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.primary.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryRoute {
    Preparing,
    Prepared,
    ResumeRollback,
    Committed,
    Temp,
    Noop,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryTerminal {
    Recovered,
    CleanedResidue,
    Noop,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecoveryReport {
    pub(crate) route: RecoveryRoute,
    pub(crate) terminal: RecoveryTerminal,
}

#[derive(Debug)]
pub struct TransactionFailure {
    pub(crate) disposition: TransactionFailureDisposition,
    pub(crate) primary: anyhow::Error,
    pub(crate) compensation: Option<anyhow::Error>,
}
impl TransactionFailure {
    pub(crate) fn new(disposition: TransactionFailureDisposition, primary: anyhow::Error) -> Self {
        Self {
            disposition,
            primary,
            compensation: None,
        }
    }
    pub(crate) fn with_compensation(mut self, compensation: anyhow::Error) -> Self {
        self.compensation = Some(compensation);
        self
    }
    pub fn disposition(&self) -> &TransactionFailureDisposition {
        &self.disposition
    }
    pub fn primary(&self) -> &anyhow::Error {
        &self.primary
    }
    pub fn compensation(&self) -> Option<&anyhow::Error> {
        self.compensation.as_ref()
    }
}
impl std::fmt::Display for TransactionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "artifact transaction {:?}: {}",
            self.disposition, self.primary
        )
    }
}
impl std::error::Error for TransactionFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.primary.root_cause())
    }
}

#[derive(Debug)]
pub enum TransactionRunError {
    Failure(TransactionFailure),
    Crash(SimulatedCrash),
}
impl std::fmt::Display for TransactionRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failure(error) => error.fmt(f),
            Self::Crash(crash) => crash.fmt(f),
        }
    }
}
impl std::error::Error for TransactionRunError {}

#[derive(Debug, Clone)]
pub struct SimulatedCrash {
    pub(crate) target: OperationTarget,
}
impl SimulatedCrash {
    pub(crate) fn new(target: OperationTarget) -> Self {
        Self { target }
    }
}
impl std::fmt::Display for SimulatedCrash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "simulated crash at {:?}", self.target)
    }
}
impl std::error::Error for SimulatedCrash {}
