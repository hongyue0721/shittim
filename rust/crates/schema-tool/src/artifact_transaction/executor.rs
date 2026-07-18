//! The sole mutation/durability execution gateway.

#[cfg(test)]
use super::error::SimulatedCrash;
use super::filesystem::TransactionFs;
use super::protocol::{
    Operation, OperationBoundary, OperationEvent, OperationSite, OperationTarget, SemanticPhase,
};
use anyhow::Result;
use std::collections::HashMap;

/// Observers may record an event or simulate a process crash. They cannot counterfeit an I/O
/// error: no-effect I/O faults belong in `TransactionFs` implementations.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObserverControl {
    Continue,
    SimulatedCrash,
}

#[cfg(not(test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObserverControl {
    Continue,
}
pub(crate) trait OperationObserver {
    fn observe(&mut self, event: &OperationEvent) -> ObserverControl;
}
pub(crate) struct NoFaultObserver;
impl OperationObserver for NoFaultObserver {
    fn observe(&mut self, _event: &OperationEvent) -> ObserverControl {
        ObserverControl::Continue
    }
}

/// Crashes exactly at a selected semantic boundary. Before skips the operation; AfterSuccess
/// observes its completed effect and then stops execution.
#[cfg(test)]
pub(crate) struct TargetedCrashObserver {
    target: OperationTarget,
}
#[cfg(test)]
impl TargetedCrashObserver {
    pub(crate) fn new(target: OperationTarget) -> Self {
        Self { target }
    }
}
#[cfg(test)]
impl OperationObserver for TargetedCrashObserver {
    fn observe(&mut self, event: &OperationEvent) -> ObserverControl {
        if event.target == self.target {
            ObserverControl::SimulatedCrash
        } else {
            ObserverControl::Continue
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct RecordingObserver {
    events: Vec<OperationEvent>,
}
#[cfg(test)]
impl RecordingObserver {
    pub(crate) fn events(&self) -> &[OperationEvent] {
        &self.events
    }
}
#[cfg(test)]
impl OperationObserver for RecordingObserver {
    fn observe(&mut self, event: &OperationEvent) -> ObserverControl {
        self.events.push(event.clone());
        ObserverControl::Continue
    }
}

pub(crate) struct OperationExecutor<F, O> {
    filesystem: F,
    observer: O,
    /// A site may legitimately run repeatedly (for example a temp-residue sweep); occurrences
    /// are assigned here, globally for the executor, so recursive traversal cannot collide.
    site_occurrences: HashMap<OperationSite, usize>,
}
impl<F, O> OperationExecutor<F, O>
where
    F: TransactionFs,
    O: OperationObserver,
{
    pub(crate) fn new(filesystem: F, observer: O) -> Self {
        Self {
            filesystem,
            observer,
            site_occurrences: HashMap::new(),
        }
    }
    pub(crate) fn execute<T>(
        &mut self,
        phase: SemanticPhase,
        operation: Operation,
        action: impl FnOnce(&mut F) -> Result<T>,
    ) -> Result<T> {
        let site = OperationSite {
            phase,
            operation: operation.clone(),
        };
        let occurrence = self.next_occurrence(&site);
        let before = event(site.clone(), occurrence, OperationBoundary::Before);
        self.observe(&before)?;
        self.filesystem.before_operation(&before.target)?;
        let result = action(&mut self.filesystem)?;
        let after = event(site, occurrence, OperationBoundary::AfterSuccess);
        self.observe(&after)?;
        Ok(result)
    }
    fn next_occurrence(&mut self, site: &OperationSite) -> Option<usize> {
        let count = self.site_occurrences.entry(site.clone()).or_insert(0);
        let occurrence = (*count > 0).then_some(*count);
        *count += 1;
        occurrence
    }
    fn observe(&mut self, event: &OperationEvent) -> Result<()> {
        match self.observer.observe(event) {
            ObserverControl::Continue => Ok(()),
            #[cfg(test)]
            ObserverControl::SimulatedCrash => {
                Err(SimulatedCrash::new(event.target.clone()).into())
            }
        }
    }
    pub(crate) fn filesystem_mut(&mut self) -> &mut F {
        &mut self.filesystem
    }
    #[cfg(test)]
    pub(crate) fn into_parts(self) -> (F, O) {
        (self.filesystem, self.observer)
    }
}
fn event(
    site: OperationSite,
    occurrence: Option<usize>,
    boundary: OperationBoundary,
) -> OperationEvent {
    OperationEvent {
        target: OperationTarget {
            site,
            occurrence,
            boundary,
        },
    }
}
