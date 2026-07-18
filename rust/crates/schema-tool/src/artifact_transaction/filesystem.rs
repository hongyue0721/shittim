//! Mutation and durability primitives for an artifact transaction.
//!
//! Metadata inspection deliberately stays outside this port. Every operation here is invoked by
//! `OperationExecutor`, which is the only production mutation/durability gateway.

use super::protocol::OperationTarget;
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

pub(crate) trait TransactionFs {
    /// Test implementations may inject an I/O outcome for the exact operation about to run.
    /// Production uses the no-op default. A returned error is guaranteed to happen before the
    /// action closure, so it has no filesystem effect.
    fn before_operation(&mut self, _target: &OperationTarget) -> std::io::Result<()> {
        Ok(())
    }
    fn create_dir(&mut self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir(path)
    }
    fn copy_file(&mut self, source: &Path, destination: &Path) -> std::io::Result<u64> {
        std::fs::copy(source, destination)
    }
    fn rename(&mut self, source: &Path, destination: &Path) -> std::io::Result<()> {
        std::fs::rename(source, destination)
    }
    fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
    fn remove_tree(&mut self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_dir_all(path)
    }
    fn create_file(&mut self, path: &Path) -> std::io::Result<File> {
        OpenOptions::new().write(true).create_new(true).open(path)
    }
    fn open_existing_file(&mut self, path: &Path) -> std::io::Result<File> {
        OpenOptions::new().write(true).open(path)
    }
    fn truncate(&mut self, file: &File) -> std::io::Result<()> {
        file.set_len(0)
    }
    fn write_bytes(&mut self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
        file.write_all(bytes)
    }
    fn sync_file(&mut self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
    fn sync_file_at(&mut self, path: &Path) -> Result<()> {
        File::open(path)
            .with_context(|| format!("open file for fsync {}", path.display()))?
            .sync_all()
            .with_context(|| format!("fsync file {}", path.display()))
    }
    fn sync_dir(&mut self, path: &Path) -> Result<()> {
        sync_directory(path)
    }
}

pub(crate) struct RealTransactionFs;
impl TransactionFs for RealTransactionFs {}

#[cfg(target_os = "linux")]
pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .with_context(|| format!("open directory for fsync {}", path.display()))?
        .sync_all()
        .with_context(|| format!("fsync directory {}", path.display()))
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn sync_directory(_path: &Path) -> Result<()> {
    anyhow::bail!("artifact transaction directory durability is currently Linux verified only")
}
