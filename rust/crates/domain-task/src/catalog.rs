//! Exhaustive status catalogs derived from generated `kernel-contracts` enums.
//!
//! These catalogs exist so NxN matrix tests can traverse every status without
//! inventing a parallel hand-written enum. Adding a variant to the generated
//! enum forces a compile failure in the exhaustive match below.

use kernel_contracts::{ActionStatus, TaskStatus};

/// Every `TaskStatus` variant in schema order.
pub const TASK_STATUS_CATALOG: &[TaskStatus] = &[
    TaskStatus::Candidate,
    TaskStatus::AwaitingApproval,
    TaskStatus::Planned,
    TaskStatus::Rejected,
    TaskStatus::Running,
    TaskStatus::WaitingUser,
    TaskStatus::Paused,
    TaskStatus::PartiallyCompleted,
    TaskStatus::Succeeded,
    TaskStatus::Failed,
    TaskStatus::Cancelled,
    TaskStatus::RollingBack,
    TaskStatus::RolledBack,
    TaskStatus::Archived,
];

/// Every `ActionStatus` variant in schema order.
pub const ACTION_STATUS_CATALOG: &[ActionStatus] = &[
    ActionStatus::Pending,
    ActionStatus::Approved,
    ActionStatus::Leased,
    ActionStatus::InFlight,
    ActionStatus::Completed,
    ActionStatus::Failed,
    ActionStatus::UnknownSideEffect,
    ActionStatus::RollingBack,
    ActionStatus::RolledBack,
    ActionStatus::RollbackFailed,
    ActionStatus::Cancelled,
];

/// Compile-time exhaustiveness: every generated variant must appear in the catalog.
///
/// Call from tests (or `#[allow]`-free consumers) so new enum variants fail to compile here.
pub fn assert_task_catalog_exhaustive(status: TaskStatus) {
    match status {
        TaskStatus::Candidate
        | TaskStatus::AwaitingApproval
        | TaskStatus::Planned
        | TaskStatus::Rejected
        | TaskStatus::Running
        | TaskStatus::WaitingUser
        | TaskStatus::Paused
        | TaskStatus::PartiallyCompleted
        | TaskStatus::Succeeded
        | TaskStatus::Failed
        | TaskStatus::Cancelled
        | TaskStatus::RollingBack
        | TaskStatus::RolledBack
        | TaskStatus::Archived => {}
    }
}

/// Compile-time exhaustiveness: every generated variant must appear in the catalog.
pub fn assert_action_catalog_exhaustive(status: ActionStatus) {
    match status {
        ActionStatus::Pending
        | ActionStatus::Approved
        | ActionStatus::Leased
        | ActionStatus::InFlight
        | ActionStatus::Completed
        | ActionStatus::Failed
        | ActionStatus::UnknownSideEffect
        | ActionStatus::RollingBack
        | ActionStatus::RolledBack
        | ActionStatus::RollbackFailed
        | ActionStatus::Cancelled => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_catalog_covers_all_variants() {
        for status in TASK_STATUS_CATALOG {
            assert_task_catalog_exhaustive(*status);
        }
        assert_eq!(TASK_STATUS_CATALOG.len(), 14);
    }

    #[test]
    fn action_catalog_covers_all_variants() {
        for status in ACTION_STATUS_CATALOG {
            assert_action_catalog_exhaustive(*status);
        }
        assert_eq!(ACTION_STATUS_CATALOG.len(), 11);
    }
}
