//! Durable artifact-transaction protocol vocabulary.
//!
//! Journal types below are the persisted protocol.  Execution vocabulary deliberately keeps a
//! concrete logical location in every operation: a fault site must identify *what* was touched,
//! not merely which primitive happened to run.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RollbackState {
    Started,
    NewMovedToDiscard,
    OriginalRestored,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "name", content = "state", rename_all = "snake_case")]
pub(crate) enum TransactionPhase {
    Preparing,
    Prepared,
    RollingBack(RollbackState),
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Journal {
    pub(crate) version: u32,
    pub(crate) transaction_id: String,
    pub(crate) root: String,
    pub(crate) stage: String,
    pub(crate) backup: String,
    pub(crate) rollback_discard: String,
    pub(crate) original_exists: bool,
    pub(crate) phase: TransactionPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticPhase {
    InstallPrepare,
    InstallCommit,
    InstallRollback,
    InstallCleanup,
    RecoverPreparing,
    RecoverPrepared,
    RecoverResumeRollback,
    RecoverCommitted,
    RecoverTemp,
    RecoverNoop,
}

/// The tree whose relative namespace owns a [`TreeEntryPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TreeOwner {
    Root,
    Stage,
}

/// A validated POSIX-relative path below one transaction tree.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TreeEntryPath(String);
impl TreeEntryPath {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('/')
            || value.ends_with('/')
            || value.contains('\\')
            || value
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            bail!("tree entry path must be a non-empty POSIX relative path")
        }
        Ok(Self(value))
    }
    pub(crate) fn from_relative_path(path: &std::path::Path) -> Result<Self> {
        let value = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("tree entry path is not valid UTF-8"))?;
        Self::parse(value.replace(std::path::MAIN_SEPARATOR, "/"))
    }
    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for TreeEntryPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("TreeEntryPath")
            .field(&self.0)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum LogicalPath {
    Repo,
    Root,
    Stage,
    Backup,
    Discard,
    FormalJournal,
    JournalTemp,
    TreeEntry {
        owner: TreeOwner,
        relative: TreeEntryPath,
    },
}
impl LogicalPath {
    pub(crate) fn tree_entry(owner: TreeOwner, relative: TreeEntryPath) -> Self {
        Self::TreeEntry { owner, relative }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PathRole {
    Backup,
    Stage,
    Discard,
}
impl PathRole {
    pub(crate) fn logical_path(self) -> LogicalPath {
        match self {
            Self::Backup => LogicalPath::Backup,
            Self::Stage => LogicalPath::Stage,
            Self::Discard => LogicalPath::Discard,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WritePurpose {
    JournalTemp,
    Artifact,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RenamePurpose {
    PreserveOriginal,
    PublishStage,
    DiscardNew,
    RestoreOriginal,
    PublishJournal,
}

/// The primitive and all static semantic identity required to locate its effect.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Operation {
    CreateDir {
        target: LogicalPath,
    },
    CreateFile {
        target: LogicalPath,
    },
    Truncate {
        target: LogicalPath,
    },
    WriteBytes {
        purpose: WritePurpose,
        target: LogicalPath,
        journal_target_phase: Option<TransactionPhase>,
    },
    CopyFile {
        source: LogicalPath,
        destination: LogicalPath,
    },
    SyncFile {
        target: LogicalPath,
    },
    SyncDir {
        target: LogicalPath,
    },
    Rename {
        purpose: RenamePurpose,
        source: LogicalPath,
        destination: LogicalPath,
    },
    RemoveFile {
        target: LogicalPath,
    },
    RemoveTree {
        role: PathRole,
        target: LogicalPath,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum OperationBoundary {
    Before,
    AfterSuccess,
}

/// Stable identity of a semantic operation, independent from a boundary or dynamic repetition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct OperationSite {
    pub(crate) phase: SemanticPhase,
    pub(crate) operation: Operation,
}

/// A fault target is a site plus an optional dynamic occurrence and a boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct OperationTarget {
    pub(crate) site: OperationSite,
    /// Only repeated execution of the same site receives an ordinal; the first execution is None.
    pub(crate) occurrence: Option<usize>,
    pub(crate) boundary: OperationBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct OperationEvent {
    pub(crate) target: OperationTarget,
}
