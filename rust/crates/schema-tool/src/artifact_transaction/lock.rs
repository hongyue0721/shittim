//! Advisory-lock coordination, deliberately separate from transaction durability.
//!
//! The coordinator owns the algorithm; `LockPort` owns platform operations.  This keeps lock
//! failures out of the artifact transaction fault model and lets focused tests inject one exact
//! failed lock operation at a time.

use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockOperation {
    RepositoryInspect,
    LockPathInspect,
    OpenCreate,
    OpenedTypeInspect,
    TryLock,
    Truncate,
    Seek,
    OwnerWrite,
    SyncFile,
    SyncRepositoryDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockFailureKind {
    InvalidRepository,
    Symlink,
    NonRegular,
    Contention,
    Io,
}

#[derive(Debug)]
pub(crate) struct LockFailure {
    pub(crate) operation: LockOperation,
    pub(crate) kind: LockFailureKind,
    pub(crate) source: io::Error,
}
impl LockFailure {
    fn new(operation: LockOperation, kind: LockFailureKind, source: io::Error) -> Self {
        Self {
            operation,
            kind,
            source,
        }
    }
}
impl std::fmt::Display for LockFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lock acquisition failed at {:?} ({:?}): {}",
            self.operation, self.kind, self.source
        )
    }
}
impl std::error::Error for LockFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockPathKind {
    Missing,
    Directory,
    RegularFile,
    Symlink,
    Other,
}

/// Low-level lock operations.  `LockCoordinator` is the only algorithm consumer.
pub(crate) trait LockPort {
    type Handle;

    fn inspect_path(&mut self, path: &Path) -> io::Result<LockPathKind>;
    fn open_or_create(&mut self, path: &Path) -> io::Result<Self::Handle>;
    fn opened_kind(&mut self, handle: &Self::Handle) -> io::Result<LockPathKind>;
    fn try_lock(&mut self, handle: &Self::Handle) -> io::Result<()>;
    fn truncate(&mut self, handle: &Self::Handle) -> io::Result<()>;
    fn seek_start(&mut self, handle: &mut Self::Handle) -> io::Result<()>;
    fn write_owner(&mut self, handle: &mut Self::Handle, bytes: &[u8]) -> io::Result<()>;
    fn sync_file(&mut self, handle: &Self::Handle) -> io::Result<()>;
    fn sync_repo_dir(&mut self, repo_root: &Path) -> io::Result<()>;
    fn unlock(&mut self, handle: &Self::Handle);
}

pub(crate) struct LockCoordinator<P> {
    port: P,
}
impl<P: LockPort> LockCoordinator<P> {
    pub(crate) fn new(port: P) -> Self {
        Self { port }
    }

    pub(crate) fn acquire(mut self, repo_root: &Path) -> Result<TransactionLock<P>, LockFailure> {
        validate_repository(&mut self.port, repo_root)?;
        let lock_path = repo_root.join(".schema-tool-generate.lock");
        validate_lock_path(&mut self.port, &lock_path)?;
        let handle = self.port.open_or_create(&lock_path).map_err(|error| {
            LockFailure::new(LockOperation::OpenCreate, LockFailureKind::Io, error)
        })?;
        validate_opened_file(&mut self.port, &handle)?;
        self.port.try_lock(&handle).map_err(|error| {
            let kind = if error.kind() == io::ErrorKind::WouldBlock {
                LockFailureKind::Contention
            } else {
                LockFailureKind::Io
            };
            LockFailure::new(LockOperation::TryLock, kind, error)
        })?;

        // From here every failure must release the authoritative FD lock before transaction
        // inspection can begin. `TransactionLock` owns that release even on an early error.
        let mut guard = TransactionLock {
            port: self.port,
            handle: Some(handle),
        };
        write_owner_metadata(&mut guard, repo_root)?;
        Ok(guard)
    }
}

fn validate_repository<P: LockPort>(port: &mut P, repo_root: &Path) -> Result<(), LockFailure> {
    match port.inspect_path(repo_root).map_err(|error| {
        LockFailure::new(LockOperation::RepositoryInspect, LockFailureKind::Io, error)
    })? {
        LockPathKind::Directory => Ok(()),
        LockPathKind::Symlink => Err(LockFailure::new(
            LockOperation::RepositoryInspect,
            LockFailureKind::Symlink,
            invalid_input("repository root is a symlink"),
        )),
        _ => Err(LockFailure::new(
            LockOperation::RepositoryInspect,
            LockFailureKind::InvalidRepository,
            invalid_input("repository root is not a directory"),
        )),
    }
}
fn validate_lock_path<P: LockPort>(port: &mut P, lock_path: &Path) -> Result<(), LockFailure> {
    match port.inspect_path(lock_path).map_err(|error| {
        LockFailure::new(LockOperation::LockPathInspect, LockFailureKind::Io, error)
    })? {
        LockPathKind::Missing | LockPathKind::RegularFile => Ok(()),
        LockPathKind::Symlink => Err(LockFailure::new(
            LockOperation::LockPathInspect,
            LockFailureKind::Symlink,
            invalid_input("lock path is a symlink"),
        )),
        _ => Err(LockFailure::new(
            LockOperation::LockPathInspect,
            LockFailureKind::NonRegular,
            invalid_input("lock path is not a regular file"),
        )),
    }
}
fn validate_opened_file<P: LockPort>(port: &mut P, handle: &P::Handle) -> Result<(), LockFailure> {
    match port.opened_kind(handle).map_err(|error| {
        LockFailure::new(LockOperation::OpenedTypeInspect, LockFailureKind::Io, error)
    })? {
        LockPathKind::RegularFile => Ok(()),
        LockPathKind::Symlink => Err(LockFailure::new(
            LockOperation::OpenedTypeInspect,
            LockFailureKind::Symlink,
            invalid_input("opened lock is a symlink"),
        )),
        _ => Err(LockFailure::new(
            LockOperation::OpenedTypeInspect,
            LockFailureKind::NonRegular,
            invalid_input("opened lock is not regular"),
        )),
    }
}
fn write_owner_metadata<P: LockPort>(
    guard: &mut TransactionLock<P>,
    repo_root: &Path,
) -> Result<(), LockFailure> {
    let owner = LockOwner {
        pid: std::process::id(),
        acquired_unix_nanos: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    };
    let mut bytes = serde_json::to_vec(&owner).expect("LockOwner serialization is infallible");
    bytes.push(b'\n');
    guard.with_handle(
        |port, handle| port.truncate(handle),
        LockOperation::Truncate,
    )?;
    guard.with_handle_mut(|port, handle| port.seek_start(handle), LockOperation::Seek)?;
    guard.with_handle_mut(
        |port, handle| port.write_owner(handle, &bytes),
        LockOperation::OwnerWrite,
    )?;
    guard.with_handle(
        |port, handle| port.sync_file(handle),
        LockOperation::SyncFile,
    )?;
    guard.port.sync_repo_dir(repo_root).map_err(|error| {
        LockFailure::new(
            LockOperation::SyncRepositoryDirectory,
            LockFailureKind::Io,
            error,
        )
    })
}
fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Debug)]
pub(crate) struct TransactionLock<P: LockPort> {
    port: P,
    handle: Option<P::Handle>,
}
impl<P: LockPort> TransactionLock<P> {
    fn with_handle<T>(
        &mut self,
        operation: impl FnOnce(&mut P, &P::Handle) -> io::Result<T>,
        at: LockOperation,
    ) -> Result<T, LockFailure> {
        let handle = self
            .handle
            .as_ref()
            .expect("held transaction lock has a handle");
        operation(&mut self.port, handle)
            .map_err(|error| LockFailure::new(at, LockFailureKind::Io, error))
    }
    fn with_handle_mut<T>(
        &mut self,
        operation: impl FnOnce(&mut P, &mut P::Handle) -> io::Result<T>,
        at: LockOperation,
    ) -> Result<T, LockFailure> {
        let handle = self
            .handle
            .as_mut()
            .expect("held transaction lock has a handle");
        operation(&mut self.port, handle)
            .map_err(|error| LockFailure::new(at, LockFailureKind::Io, error))
    }
}
impl<P: LockPort> Drop for TransactionLock<P> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.port.unlock(&handle);
        }
    }
}

#[derive(Serialize)]
struct LockOwner {
    pid: u32,
    acquired_unix_nanos: u128,
}

#[derive(Debug)]
pub(crate) struct RealLockPort;
impl LockPort for RealLockPort {
    type Handle = File;
    fn inspect_path(&mut self, path: &Path) -> io::Result<LockPathKind> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => Ok(kind_for(&metadata)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(LockPathKind::Missing),
            Err(error) => Err(error),
        }
    }
    fn open_or_create(&mut self, path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
    }
    fn opened_kind(&mut self, handle: &File) -> io::Result<LockPathKind> {
        handle.metadata().map(|metadata| kind_for(&metadata))
    }
    fn try_lock(&mut self, handle: &File) -> io::Result<()> {
        Ok(handle.try_lock()?)
    }
    fn truncate(&mut self, handle: &File) -> io::Result<()> {
        handle.set_len(0)
    }
    fn seek_start(&mut self, handle: &mut File) -> io::Result<()> {
        handle.seek(SeekFrom::Start(0)).map(|_| ())
    }
    fn write_owner(&mut self, handle: &mut File, bytes: &[u8]) -> io::Result<()> {
        handle.write_all(bytes)
    }
    fn sync_file(&mut self, handle: &File) -> io::Result<()> {
        handle.sync_all()
    }
    fn sync_repo_dir(&mut self, repo_root: &Path) -> io::Result<()> {
        File::open(repo_root)?.sync_all()
    }
    fn unlock(&mut self, handle: &File) {
        let _ = File::unlock(handle);
    }
}
fn kind_for(metadata: &std::fs::Metadata) -> LockPathKind {
    if metadata.file_type().is_symlink() {
        LockPathKind::Symlink
    } else if metadata.is_dir() {
        LockPathKind::Directory
    } else if metadata.is_file() {
        LockPathKind::RegularFile
    } else {
        LockPathKind::Other
    }
}
pub(crate) type RealTransactionLock = TransactionLock<RealLockPort>;
pub(crate) fn acquire_real_lock(repo_root: &Path) -> Result<RealTransactionLock, LockFailure> {
    LockCoordinator::new(RealLockPort).acquire(repo_root)
}

#[cfg(test)]
#[path = "lock_conformance.rs"]
mod lock_conformance;
