use super::super::filesystem::{RealTransactionFs, TransactionFs};
use super::super::protocol::{Operation, OperationBoundary, OperationTarget};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PartialEffect {
    WritePrefix { bytes: usize },
    CopyPrefix { bytes: usize },
    RemoveFileThenError,
    RemoveTreeFirstLexicalEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FaultDirective {
    IoNoEffect,
    IoPartial(PartialEffect),
}

#[derive(Debug)]
pub(super) struct InjectedIo {
    pub(super) target: OperationTarget,
    pub(super) directive: FaultDirective,
}
impl fmt::Display for InjectedIo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "injected {:?} at {:?}",
            self.directive, self.target
        )
    }
}
impl Error for InjectedIo {}

pub(super) struct FaultingRealFs {
    real: RealTransactionFs,
    selected: VecDeque<(OperationTarget, FaultDirective)>,
    consumed: usize,
    armed_partial: Option<(OperationTarget, PartialEffect)>,
    partial_performed: bool,
    observation_root: Option<std::path::PathBuf>,
    partial_before: Option<super::snapshot::TreeSnapshot>,
    partial_after: Option<super::snapshot::TreeSnapshot>,
}

impl FaultingRealFs {
    pub(super) fn no_fault() -> Self {
        Self {
            real: RealTransactionFs,
            selected: VecDeque::new(),
            consumed: 0,
            armed_partial: None,
            partial_performed: false,
            observation_root: None,
            partial_before: None,
            partial_after: None,
        }
    }

    pub(super) fn io_no_effect(target: &OperationTarget) -> Self {
        Self::with_directive(target, FaultDirective::IoNoEffect)
    }

    pub(super) fn io_partial_at(
        observation_root: &Path,
        target: &OperationTarget,
        effect: PartialEffect,
    ) -> Self {
        let mut filesystem = Self::with_directive(target, FaultDirective::IoPartial(effect));
        filesystem.observation_root = Some(observation_root.to_path_buf());
        filesystem
    }

    pub(super) fn two_faults_at(
        observation_root: &Path,
        primary: (&OperationTarget, FaultDirective),
        secondary: (&OperationTarget, FaultDirective),
    ) -> Self {
        let mut filesystem = Self::two_faults(primary, secondary);
        filesystem.observation_root = Some(observation_root.to_path_buf());
        filesystem
    }

    pub(super) fn two_faults(
        primary: (&OperationTarget, FaultDirective),
        secondary: (&OperationTarget, FaultDirective),
    ) -> Self {
        assert_eq!(primary.0.boundary, OperationBoundary::Before);
        assert_eq!(secondary.0.boundary, OperationBoundary::Before);
        Self {
            real: RealTransactionFs,
            selected: VecDeque::from([
                (primary.0.clone(), primary.1),
                (secondary.0.clone(), secondary.1),
            ]),
            consumed: 0,
            armed_partial: None,
            partial_performed: false,
            observation_root: None,
            partial_before: None,
            partial_after: None,
        }
    }

    fn with_directive(target: &OperationTarget, directive: FaultDirective) -> Self {
        assert_eq!(target.boundary, OperationBoundary::Before);
        Self {
            real: RealTransactionFs,
            selected: VecDeque::from([(target.clone(), directive)]),
            consumed: 0,
            armed_partial: None,
            partial_performed: false,
            observation_root: None,
            partial_before: None,
            partial_after: None,
        }
    }

    pub(super) fn assert_consumed(&self, expected: usize) {
        assert_eq!(
            self.consumed, expected,
            "fault directives must be consumed exactly as selected"
        );
        assert!(
            self.selected.is_empty(),
            "unconsumed fault directives remain"
        );
    }

    pub(super) fn assert_consumed_once(&self) {
        self.assert_consumed(1);
    }

    pub(super) fn assert_partial_performed(&self) {
        assert!(
            self.partial_performed,
            "selected partial effect was not performed"
        );
    }

    pub(super) fn partial_snapshots(
        &self,
    ) -> (
        &super::snapshot::TreeSnapshot,
        &super::snapshot::TreeSnapshot,
    ) {
        (
            self.partial_before
                .as_ref()
                .expect("partial before snapshot"),
            self.partial_after.as_ref().expect("partial after snapshot"),
        )
    }

    fn capture_partial_before(&mut self) {
        let root = self
            .observation_root
            .as_ref()
            .expect("partial observation root configured");
        self.partial_before = Some(super::snapshot::TreeSnapshot::capture(root));
    }

    fn capture_partial_after(&mut self) {
        let root = self
            .observation_root
            .as_ref()
            .expect("partial observation root configured");
        self.partial_after = Some(super::snapshot::TreeSnapshot::capture(root));
    }

    fn take_partial(&mut self, operation: &Operation) -> Option<(OperationTarget, PartialEffect)> {
        let (target, effect) = self.armed_partial.take()?;
        assert_eq!(&target.site.operation, operation);
        Some((target, effect))
    }

    fn injected(target: OperationTarget, directive: FaultDirective) -> std::io::Error {
        std::io::Error::other(InjectedIo { target, directive })
    }
}

impl TransactionFs for FaultingRealFs {
    fn before_operation(&mut self, target: &OperationTarget) -> std::io::Result<()> {
        let Some((selected, directive)) = self.selected.front().cloned() else {
            return Ok(());
        };
        if target != &selected {
            return Ok(());
        }
        self.selected.pop_front();
        self.consumed += 1;
        match directive {
            FaultDirective::IoNoEffect => Err(Self::injected(target.clone(), directive)),
            FaultDirective::IoPartial(effect) => {
                self.capture_partial_before();
                self.armed_partial = Some((target.clone(), effect));
                Ok(())
            }
        }
    }

    fn create_dir(&mut self, path: &Path) -> std::io::Result<()> {
        self.real.create_dir(path)
    }
    fn copy_file(&mut self, source: &Path, destination: &Path) -> std::io::Result<u64> {
        let operation = Operation::CopyFile {
            source: match &self.armed_partial {
                Some((target, _)) => match &target.site.operation {
                    Operation::CopyFile { source, .. } => source.clone(),
                    _ => return self.real.copy_file(source, destination),
                },
                None => return self.real.copy_file(source, destination),
            },
            destination: match &self.armed_partial {
                Some((target, _)) => match &target.site.operation {
                    Operation::CopyFile { destination, .. } => destination.clone(),
                    _ => unreachable!(),
                },
                None => unreachable!(),
            },
        };
        let Some((target, PartialEffect::CopyPrefix { bytes })) = self.take_partial(&operation)
        else {
            return self.real.copy_file(source, destination);
        };
        let source_bytes = fs::read(source)?;
        fs::write(destination, &source_bytes[..source_bytes.len().min(bytes)])?;
        self.partial_performed = true;
        self.capture_partial_after();
        Err(Self::injected(
            target,
            FaultDirective::IoPartial(PartialEffect::CopyPrefix { bytes }),
        ))
    }
    fn rename(&mut self, source: &Path, destination: &Path) -> std::io::Result<()> {
        self.real.rename(source, destination)
    }
    fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
        let Some((target, effect)) = self.armed_partial.take() else {
            return self.real.remove_file(path);
        };
        assert!(matches!(
            target.site.operation,
            Operation::RemoveFile { .. }
        ));
        if effect != PartialEffect::RemoveFileThenError {
            self.armed_partial = Some((target, effect));
            return self.real.remove_file(path);
        }
        self.real.remove_file(path)?;
        self.partial_performed = true;
        self.capture_partial_after();
        Err(Self::injected(
            target,
            FaultDirective::IoPartial(PartialEffect::RemoveFileThenError),
        ))
    }
    fn remove_tree(&mut self, path: &Path) -> std::io::Result<()> {
        let Some((target, effect)) = self.armed_partial.take() else {
            return self.real.remove_tree(path);
        };
        assert!(matches!(
            target.site.operation,
            Operation::RemoveTree { .. }
        ));
        if effect != PartialEffect::RemoveTreeFirstLexicalEntry {
            self.armed_partial = Some((target, effect));
            return self.real.remove_tree(path);
        }
        remove_first_lexical_entry(path)?;
        self.partial_performed = true;
        self.capture_partial_after();
        Err(Self::injected(
            target,
            FaultDirective::IoPartial(PartialEffect::RemoveTreeFirstLexicalEntry),
        ))
    }
    fn create_file(&mut self, path: &Path) -> std::io::Result<std::fs::File> {
        self.real.create_file(path)
    }
    fn open_existing_file(&mut self, path: &Path) -> std::io::Result<std::fs::File> {
        self.real.open_existing_file(path)
    }
    fn truncate(&mut self, file: &std::fs::File) -> std::io::Result<()> {
        self.real.truncate(file)
    }
    fn write_bytes(&mut self, file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
        let operation = match &self.armed_partial {
            Some((target, _)) => target.site.operation.clone(),
            None => return self.real.write_bytes(file, bytes),
        };
        let Some((target, PartialEffect::WritePrefix { bytes: prefix })) =
            self.take_partial(&operation)
        else {
            return self.real.write_bytes(file, bytes);
        };
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&bytes[..bytes.len().min(prefix)])?;
        self.partial_performed = true;
        self.capture_partial_after();
        Err(Self::injected(
            target,
            FaultDirective::IoPartial(PartialEffect::WritePrefix { bytes: prefix }),
        ))
    }
    fn sync_file(&mut self, file: &std::fs::File) -> std::io::Result<()> {
        self.real.sync_file(file)
    }
    fn sync_file_at(&mut self, path: &Path) -> anyhow::Result<()> {
        self.real.sync_file_at(path)
    }
    fn sync_dir(&mut self, path: &Path) -> anyhow::Result<()> {
        self.real.sync_dir(path)
    }
}

fn remove_first_lexical_entry(root: &Path) -> std::io::Result<()> {
    let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let Some(entry) = entries.into_iter().next() else {
        fs::remove_dir(root)?;
        return Ok(());
    };
    let metadata = fs::symlink_metadata(entry.path())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(entry.path())
    } else {
        fs::remove_file(entry.path())
    }
}

pub(super) fn operation_name(operation: &Operation) -> &'static str {
    match operation {
        Operation::CreateDir { .. } => "create_dir",
        Operation::CreateFile { .. } => "create_file",
        Operation::Truncate { .. } => "truncate",
        Operation::WriteBytes { .. } => "write_bytes",
        Operation::CopyFile { .. } => "copy_file",
        Operation::SyncFile { .. } => "sync_file",
        Operation::SyncDir { .. } => "sync_dir",
        Operation::Rename { .. } => "rename",
        Operation::RemoveFile { .. } => "remove_file",
        Operation::RemoveTree { .. } => "remove_tree",
    }
}
