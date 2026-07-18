//! Exact LockPort / LockCoordinator conformance.
//!
//! Fake port cases inject one failed operation at a time and assert exact operation/kind plus the
//! call prefix. Real-port cases cover same-process contention and cross-process FD release via a
//! TCP READY/RELEASE handshake (timeout is watchdog only; no sleep/polling).

use super::{
    acquire_real_lock, LockCoordinator, LockFailure, LockFailureKind, LockOperation, LockPathKind,
    LockPort, RealLockPort,
};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const LOCK_FILE: &str = ".schema-tool-generate.lock";
const JOURNAL_FILE: &str = ".schema-tool-generate-transaction.json";
const CORRUPT_JOURNAL: &[u8] = b"{not-valid-json@@@";
const HELPER_MODE_HOLD: &str = "hold-until-release";
const HELPER_MODE_DIE: &str = "hold-and-die";
const READY: &[u8] = b"READY\n";
const RELEASE: &[u8] = b"RELEASE\n";
const WATCHDOG: Duration = Duration::from_secs(15);

/// Port-level call trace. `Unlock` is recorded only from `LockPort::unlock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TracedCall {
    Operation(LockOperation),
    Unlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InjectedFailure {
    Io,
    WouldBlock,
    PartialOwnerWrite,
}

#[derive(Debug, Clone)]
struct FakeConfig {
    repository_kind: LockPathKind,
    lock_path_kind: LockPathKind,
    opened_kind: LockPathKind,
    fail_at: Option<(LockOperation, InjectedFailure)>,
}

impl Default for FakeConfig {
    fn default() -> Self {
        Self {
            repository_kind: LockPathKind::Directory,
            lock_path_kind: LockPathKind::Missing,
            opened_kind: LockPathKind::RegularFile,
            fail_at: None,
        }
    }
}

#[derive(Debug, Default)]
struct FakeHandle {
    written: Vec<u8>,
}

#[derive(Debug, Default)]
struct SharedTrace {
    calls: Vec<TracedCall>,
    unlock_count: usize,
}

#[derive(Debug)]
struct TracingFakeLockPort {
    repo_root: PathBuf,
    lock_path: PathBuf,
    config: FakeConfig,
    trace: Rc<RefCell<SharedTrace>>,
}

impl TracingFakeLockPort {
    fn new(repo_root: &Path, config: FakeConfig, trace: Rc<RefCell<SharedTrace>>) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            lock_path: repo_root.join(LOCK_FILE),
            config,
            trace,
        }
    }

    fn record(&self, call: TracedCall) {
        self.trace.borrow_mut().calls.push(call);
    }

    fn record_op(&self, operation: LockOperation) {
        self.record(TracedCall::Operation(operation));
    }

    fn injected(&self, operation: LockOperation) -> Option<InjectedFailure> {
        self.config
            .fail_at
            .and_then(|(at, failure)| (at == operation).then_some(failure))
    }

    fn io_err(operation: LockOperation) -> io::Error {
        io::Error::other(format!("injected I/O failure at {operation:?}"))
    }

    fn fail_io(&self, operation: LockOperation) -> io::Result<()> {
        match self.injected(operation) {
            Some(InjectedFailure::Io) => Err(Self::io_err(operation)),
            Some(InjectedFailure::WouldBlock) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "injected contention",
            )),
            Some(InjectedFailure::PartialOwnerWrite) => Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "injected partial owner write",
            )),
            None => Ok(()),
        }
    }
}

impl LockPort for TracingFakeLockPort {
    type Handle = FakeHandle;

    fn inspect_path(&mut self, path: &Path) -> io::Result<LockPathKind> {
        let operation = if path == self.repo_root {
            LockOperation::RepositoryInspect
        } else if path == self.lock_path {
            LockOperation::LockPathInspect
        } else {
            panic!("unexpected inspect_path: {}", path.display());
        };
        self.record_op(operation);
        self.fail_io(operation)?;
        Ok(match operation {
            LockOperation::RepositoryInspect => self.config.repository_kind,
            LockOperation::LockPathInspect => self.config.lock_path_kind,
            other => panic!("inspect_path mapped to unexpected operation {other:?}"),
        })
    }

    fn open_or_create(&mut self, path: &Path) -> io::Result<Self::Handle> {
        assert_eq!(
            path, self.lock_path,
            "open_or_create path must be lock file"
        );
        self.record_op(LockOperation::OpenCreate);
        self.fail_io(LockOperation::OpenCreate)?;
        Ok(FakeHandle::default())
    }

    fn opened_kind(&mut self, _handle: &Self::Handle) -> io::Result<LockPathKind> {
        self.record_op(LockOperation::OpenedTypeInspect);
        self.fail_io(LockOperation::OpenedTypeInspect)?;
        Ok(self.config.opened_kind)
    }

    fn try_lock(&mut self, _handle: &Self::Handle) -> io::Result<()> {
        self.record_op(LockOperation::TryLock);
        self.fail_io(LockOperation::TryLock)
    }

    fn truncate(&mut self, _handle: &Self::Handle) -> io::Result<()> {
        self.record_op(LockOperation::Truncate);
        self.fail_io(LockOperation::Truncate)
    }

    fn seek_start(&mut self, _handle: &mut Self::Handle) -> io::Result<()> {
        self.record_op(LockOperation::Seek);
        self.fail_io(LockOperation::Seek)
    }

    fn write_owner(&mut self, handle: &mut Self::Handle, bytes: &[u8]) -> io::Result<()> {
        self.record_op(LockOperation::OwnerWrite);
        match self.injected(LockOperation::OwnerWrite) {
            Some(InjectedFailure::PartialOwnerWrite) => {
                let keep = bytes.len() / 2;
                handle.written.extend_from_slice(&bytes[..keep]);
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "injected partial owner write",
                ))
            }
            Some(InjectedFailure::Io) => Err(Self::io_err(LockOperation::OwnerWrite)),
            Some(InjectedFailure::WouldBlock) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "injected contention",
            )),
            None => {
                handle.written.extend_from_slice(bytes);
                Ok(())
            }
        }
    }

    fn sync_file(&mut self, _handle: &Self::Handle) -> io::Result<()> {
        self.record_op(LockOperation::SyncFile);
        self.fail_io(LockOperation::SyncFile)
    }

    fn sync_repo_dir(&mut self, repo_root: &Path) -> io::Result<()> {
        assert_eq!(repo_root, self.repo_root);
        self.record_op(LockOperation::SyncRepositoryDirectory);
        self.fail_io(LockOperation::SyncRepositoryDirectory)
    }

    fn unlock(&mut self, _handle: &Self::Handle) {
        self.record(TracedCall::Unlock);
        self.trace.borrow_mut().unlock_count += 1;
    }
}

fn success_prefix() -> Vec<TracedCall> {
    [
        LockOperation::RepositoryInspect,
        LockOperation::LockPathInspect,
        LockOperation::OpenCreate,
        LockOperation::OpenedTypeInspect,
        LockOperation::TryLock,
        LockOperation::Truncate,
        LockOperation::Seek,
        LockOperation::OwnerWrite,
        LockOperation::SyncFile,
        LockOperation::SyncRepositoryDirectory,
    ]
    .into_iter()
    .map(TracedCall::Operation)
    .collect()
}

fn prefix_through(operation: LockOperation) -> Vec<TracedCall> {
    success_prefix()
        .into_iter()
        .take_while(|call| *call != TracedCall::Operation(operation))
        .chain(std::iter::once(TracedCall::Operation(operation)))
        .collect()
}

fn try_lock_succeeded(operation: LockOperation) -> bool {
    matches!(
        operation,
        LockOperation::Truncate
            | LockOperation::Seek
            | LockOperation::OwnerWrite
            | LockOperation::SyncFile
            | LockOperation::SyncRepositoryDirectory
    )
}

struct AcquireOutcome {
    result: Result<(), LockFailure>,
    calls: Vec<TracedCall>,
    unlock_count: usize,
    /// Held only on success so the caller can drop and observe unlock.
    guard_trace: Option<Rc<RefCell<SharedTrace>>>,
    _guard: Option<super::TransactionLock<TracingFakeLockPort>>,
}

fn run_acquire(repo: &Path, config: FakeConfig) -> AcquireOutcome {
    let trace = Rc::new(RefCell::new(SharedTrace::default()));
    let port = TracingFakeLockPort::new(repo, config, Rc::clone(&trace));
    match LockCoordinator::new(port).acquire(repo) {
        Ok(guard) => {
            let calls = trace.borrow().calls.clone();
            let unlock_count = trace.borrow().unlock_count;
            AcquireOutcome {
                result: Ok(()),
                calls,
                unlock_count,
                guard_trace: Some(trace),
                _guard: Some(guard),
            }
        }
        Err(failure) => {
            let calls = trace.borrow().calls.clone();
            let unlock_count = trace.borrow().unlock_count;
            AcquireOutcome {
                result: Err(failure),
                calls,
                unlock_count,
                guard_trace: None,
                _guard: None,
            }
        }
    }
}

fn assert_exact_failure(
    outcome: AcquireOutcome,
    expected_operation: LockOperation,
    expected_kind: LockFailureKind,
) {
    let failure = outcome.result.expect_err("expected lock failure");
    assert_eq!(
        failure.operation, expected_operation,
        "operation mismatch: {failure}"
    );
    assert_eq!(
        failure.kind, expected_kind,
        "kind mismatch for {expected_operation:?}: {failure}"
    );

    let mut expected = prefix_through(expected_operation);
    let expected_unlocks = usize::from(try_lock_succeeded(expected_operation));
    if expected_unlocks == 1 {
        expected.push(TracedCall::Unlock);
    }
    assert_eq!(
        outcome.calls, expected,
        "call prefix mismatch for {expected_operation:?}/{expected_kind:?}"
    );
    assert_eq!(
        outcome.unlock_count, expected_unlocks,
        "unlock count for {expected_operation:?}"
    );
}

/// Fixture with sentinel tree entries that lock acquisition must never mutate.
struct RepoFixture {
    temp: TempDir,
    root: PathBuf,
    stage: PathBuf,
    backup: PathBuf,
    discard: PathBuf,
    journal: PathBuf,
    lock_path: PathBuf,
    root_token: Vec<u8>,
    stage_token: Vec<u8>,
    backup_token: Vec<u8>,
    discard_token: Vec<u8>,
    journal_bytes: Vec<u8>,
}

impl RepoFixture {
    fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let repo = temp.path().to_path_buf();
        let root = repo.join("generated-root");
        let stage = repo.join(".generated-root.schema-tool-stage-fixture");
        let backup = repo.join(".generated-root.schema-tool-backup-fixture");
        let discard = repo.join(".generated-root.schema-tool-rollback-discard-fixture");
        let journal = repo.join(JOURNAL_FILE);
        let lock_path = repo.join(LOCK_FILE);
        let root_token = b"root-sentinel-v1".to_vec();
        let stage_token = b"stage-sentinel-v1".to_vec();
        let backup_token = b"backup-sentinel-v1".to_vec();
        let discard_token = b"discard-sentinel-v1".to_vec();
        let journal_bytes = CORRUPT_JOURNAL.to_vec();

        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::create_dir_all(&discard).unwrap();
        std::fs::write(root.join("marker"), &root_token).unwrap();
        std::fs::write(stage.join("marker"), &stage_token).unwrap();
        std::fs::write(backup.join("marker"), &backup_token).unwrap();
        std::fs::write(discard.join("marker"), &discard_token).unwrap();
        std::fs::write(&journal, &journal_bytes).unwrap();

        Self {
            temp,
            root,
            stage,
            backup,
            discard,
            journal,
            lock_path,
            root_token,
            stage_token,
            backup_token,
            discard_token,
            journal_bytes,
        }
    }

    fn repo(&self) -> &Path {
        self.temp.path()
    }

    fn assert_transaction_surface_unchanged(&self) {
        assert_eq!(
            std::fs::read(self.journal.as_path()).unwrap(),
            self.journal_bytes,
            "formal journal must be untouched by lock acquisition"
        );
        assert_eq!(
            std::fs::read(self.root.join("marker")).unwrap(),
            self.root_token
        );
        assert_eq!(
            std::fs::read(self.stage.join("marker")).unwrap(),
            self.stage_token
        );
        assert_eq!(
            std::fs::read(self.backup.join("marker")).unwrap(),
            self.backup_token
        );
        assert_eq!(
            std::fs::read(self.discard.join("marker")).unwrap(),
            self.discard_token
        );
    }

    fn assert_lock_not_unlinked_if_present(&self) {
        // Persistent lock file is allowed; when present it must remain a regular non-symlink file.
        match std::fs::symlink_metadata(&self.lock_path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(metadata) => {
                assert!(metadata.is_file());
                assert!(!metadata.file_type().is_symlink());
            }
            Err(error) => panic!("inspect lock path: {error}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Fake LockPort exact matrix
// ---------------------------------------------------------------------------

#[test]
fn successful_fake_acquire_records_exact_sequence_and_unlocks_once_on_drop() {
    let fixture = RepoFixture::new();
    let mut outcome = run_acquire(fixture.repo(), FakeConfig::default());
    outcome.result.expect("successful acquire");
    assert_eq!(outcome.calls, success_prefix());
    assert_eq!(outcome.unlock_count, 0);

    let trace = outcome.guard_trace.take().expect("success holds trace");
    drop(outcome._guard);
    assert_eq!(trace.borrow().unlock_count, 1);
    assert_eq!(
        trace.borrow().calls.last().copied(),
        Some(TracedCall::Unlock)
    );
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn repository_inspect_io_failure() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            fail_at: Some((LockOperation::RepositoryInspect, InjectedFailure::Io)),
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(
        outcome,
        LockOperation::RepositoryInspect,
        LockFailureKind::Io,
    );
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn repository_inspect_symlink_rejected() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            repository_kind: LockPathKind::Symlink,
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(
        outcome,
        LockOperation::RepositoryInspect,
        LockFailureKind::Symlink,
    );
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn repository_inspect_non_directory_rejected() {
    let fixture = RepoFixture::new();
    for kind in [
        LockPathKind::Missing,
        LockPathKind::RegularFile,
        LockPathKind::Other,
    ] {
        let outcome = run_acquire(
            fixture.repo(),
            FakeConfig {
                repository_kind: kind,
                ..FakeConfig::default()
            },
        );
        assert_exact_failure(
            outcome,
            LockOperation::RepositoryInspect,
            LockFailureKind::InvalidRepository,
        );
    }
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn lock_path_inspect_io_failure() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            fail_at: Some((LockOperation::LockPathInspect, InjectedFailure::Io)),
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(outcome, LockOperation::LockPathInspect, LockFailureKind::Io);
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn lock_path_inspect_symlink_rejected() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            lock_path_kind: LockPathKind::Symlink,
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(
        outcome,
        LockOperation::LockPathInspect,
        LockFailureKind::Symlink,
    );
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn lock_path_inspect_nonregular_rejected() {
    let fixture = RepoFixture::new();
    for kind in [LockPathKind::Directory, LockPathKind::Other] {
        let outcome = run_acquire(
            fixture.repo(),
            FakeConfig {
                lock_path_kind: kind,
                ..FakeConfig::default()
            },
        );
        assert_exact_failure(
            outcome,
            LockOperation::LockPathInspect,
            LockFailureKind::NonRegular,
        );
    }
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn lock_path_missing_and_regular_are_accepted_prefix() {
    let fixture = RepoFixture::new();
    for kind in [LockPathKind::Missing, LockPathKind::RegularFile] {
        let outcome = run_acquire(
            fixture.repo(),
            FakeConfig {
                lock_path_kind: kind,
                ..FakeConfig::default()
            },
        );
        outcome.result.expect("accepted lock path kind");
        assert_eq!(outcome.calls, success_prefix());
        drop(outcome._guard);
    }
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn open_create_io_failure() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            fail_at: Some((LockOperation::OpenCreate, InjectedFailure::Io)),
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(outcome, LockOperation::OpenCreate, LockFailureKind::Io);
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn opened_type_inspect_io_failure() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            fail_at: Some((LockOperation::OpenedTypeInspect, InjectedFailure::Io)),
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(
        outcome,
        LockOperation::OpenedTypeInspect,
        LockFailureKind::Io,
    );
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn opened_type_symlink_rejected() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            opened_kind: LockPathKind::Symlink,
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(
        outcome,
        LockOperation::OpenedTypeInspect,
        LockFailureKind::Symlink,
    );
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn opened_type_nonregular_rejected() {
    let fixture = RepoFixture::new();
    for kind in [
        LockPathKind::Missing,
        LockPathKind::Directory,
        LockPathKind::Other,
    ] {
        let outcome = run_acquire(
            fixture.repo(),
            FakeConfig {
                opened_kind: kind,
                ..FakeConfig::default()
            },
        );
        assert_exact_failure(
            outcome,
            LockOperation::OpenedTypeInspect,
            LockFailureKind::NonRegular,
        );
    }
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn try_lock_would_block_is_contention_without_unlock() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            fail_at: Some((LockOperation::TryLock, InjectedFailure::WouldBlock)),
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(outcome, LockOperation::TryLock, LockFailureKind::Contention);
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn try_lock_io_failure_without_unlock() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            fail_at: Some((LockOperation::TryLock, InjectedFailure::Io)),
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(outcome, LockOperation::TryLock, LockFailureKind::Io);
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn post_lock_io_failures_unlock_exactly_once() {
    let fixture = RepoFixture::new();
    let cases = [
        (LockOperation::Truncate, InjectedFailure::Io),
        (LockOperation::Seek, InjectedFailure::Io),
        (LockOperation::OwnerWrite, InjectedFailure::Io),
        (
            LockOperation::OwnerWrite,
            InjectedFailure::PartialOwnerWrite,
        ),
        (LockOperation::SyncFile, InjectedFailure::Io),
        (LockOperation::SyncRepositoryDirectory, InjectedFailure::Io),
    ];
    for (operation, injected) in cases {
        let outcome = run_acquire(
            fixture.repo(),
            FakeConfig {
                fail_at: Some((operation, injected)),
                ..FakeConfig::default()
            },
        );
        assert_exact_failure(outcome, operation, LockFailureKind::Io);
        fixture.assert_transaction_surface_unchanged();
    }
}

#[test]
fn post_lock_failure_leaves_corrupt_journal_and_allows_next_fake_acquire() {
    let fixture = RepoFixture::new();
    let outcome = run_acquire(
        fixture.repo(),
        FakeConfig {
            fail_at: Some((
                LockOperation::OwnerWrite,
                InjectedFailure::PartialOwnerWrite,
            )),
            ..FakeConfig::default()
        },
    );
    assert_exact_failure(outcome, LockOperation::OwnerWrite, LockFailureKind::Io);
    fixture.assert_transaction_surface_unchanged();

    let next = run_acquire(fixture.repo(), FakeConfig::default());
    next.result.expect("next acquire succeeds after unlock");
    assert_eq!(next.calls, success_prefix());
    drop(next._guard);
    fixture.assert_transaction_surface_unchanged();
}

// ---------------------------------------------------------------------------
// Real LockPort: same-process and filesystem edge cases
// ---------------------------------------------------------------------------

#[test]
fn real_acquire_succeeds_writes_owner_keeps_lock_file_and_leaves_journal() {
    let fixture = RepoFixture::new();
    let guard = acquire_real_lock(fixture.repo()).expect("real acquire");
    assert!(fixture.lock_path.is_file());
    let owner = std::fs::read_to_string(&fixture.lock_path).expect("owner bytes");
    let value: serde_json::Value = serde_json::from_str(owner.trim()).expect("owner json");
    assert_eq!(
        value["pid"].as_u64().expect("pid"),
        u64::from(std::process::id())
    );
    assert!(value["acquired_unix_nanos"].is_number());
    fixture.assert_transaction_surface_unchanged();
    drop(guard);
    // Persistent lock file must not be unlinked on release.
    assert!(fixture.lock_path.is_file());
    fixture.assert_lock_not_unlinked_if_present();
    fixture.assert_transaction_surface_unchanged();

    let again = acquire_real_lock(fixture.repo()).expect("reacquire after drop");
    drop(again);
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn real_same_process_contention_is_exact_try_lock_contention() {
    let fixture = RepoFixture::new();
    let held = acquire_real_lock(fixture.repo()).expect("holder");
    let failure = acquire_real_lock(fixture.repo()).expect_err("second acquire must contend");
    assert_eq!(failure.operation, LockOperation::TryLock);
    assert_eq!(failure.kind, LockFailureKind::Contention);
    fixture.assert_transaction_surface_unchanged();
    assert!(fixture.lock_path.is_file());
    drop(held);

    let recovered = acquire_real_lock(fixture.repo()).expect("acquire after release");
    drop(recovered);
    fixture.assert_transaction_surface_unchanged();
    assert!(fixture.lock_path.is_file());
}

#[test]
fn real_repository_symlink_is_rejected_before_lock() {
    let parent = TempDir::new().unwrap();
    let real_repo = parent.path().join("real");
    std::fs::create_dir_all(&real_repo).unwrap();
    let link = parent.path().join("link");
    std::os::unix::fs::symlink(&real_repo, &link).unwrap();

    let failure = acquire_real_lock(&link).expect_err("symlink repo");
    assert_eq!(failure.operation, LockOperation::RepositoryInspect);
    assert_eq!(failure.kind, LockFailureKind::Symlink);
    assert!(!real_repo.join(LOCK_FILE).exists());
}

#[test]
fn real_lock_path_symlink_is_rejected() {
    let fixture = RepoFixture::new();
    let target = fixture.repo().join("lock-target");
    std::fs::write(&target, b"x").unwrap();
    std::os::unix::fs::symlink(&target, &fixture.lock_path).unwrap();

    let failure = acquire_real_lock(fixture.repo()).expect_err("symlink lock path");
    assert_eq!(failure.operation, LockOperation::LockPathInspect);
    assert_eq!(failure.kind, LockFailureKind::Symlink);
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn real_lock_path_directory_is_nonregular() {
    let fixture = RepoFixture::new();
    std::fs::create_dir_all(&fixture.lock_path).unwrap();
    let failure = acquire_real_lock(fixture.repo()).expect_err("directory lock path");
    assert_eq!(failure.operation, LockOperation::LockPathInspect);
    assert_eq!(failure.kind, LockFailureKind::NonRegular);
    fixture.assert_transaction_surface_unchanged();
}

// ---------------------------------------------------------------------------
// Cross-process real lock via TCP READY/RELEASE handshake
//
// Helper process entry is the ignored exact test `lock_conformance_helper_main`,
// re-exec'd with SCHEMA_TOOL_LOCK_HELPER_* env vars. No alternate env-gated
// process entry path exists.
// ---------------------------------------------------------------------------

fn spawn_helper(mode: &str, repo: &Path, addr: &str) -> Child {
    let exe = std::env::current_exe().expect("current_exe");
    Command::new(exe)
        .args([
            "--exact",
            "artifact_transaction::lock::lock_conformance::lock_conformance_helper_main",
            "--nocapture",
            "--ignored",
        ])
        .env("SCHEMA_TOOL_LOCK_HELPER_REPO", repo)
        .env("SCHEMA_TOOL_LOCK_HELPER_ADDR", addr)
        .env("SCHEMA_TOOL_LOCK_HELPER_MODE", mode)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn helper")
}

#[test]
#[ignore = "lock helper entrypoint; invoked by cross-process tests via current_exe --exact"]
fn lock_conformance_helper_main() {
    let mode = std::env::var("SCHEMA_TOOL_LOCK_HELPER_MODE").expect("helper mode");
    let repo = PathBuf::from(std::env::var("SCHEMA_TOOL_LOCK_HELPER_REPO").expect("helper repo"));
    let addr = std::env::var("SCHEMA_TOOL_LOCK_HELPER_ADDR").expect("helper addr");

    let mut stream = TcpStream::connect(&addr).expect("helper connect");
    stream.set_read_timeout(Some(WATCHDOG)).unwrap();
    stream.set_write_timeout(Some(WATCHDOG)).unwrap();

    let guard = LockCoordinator::new(RealLockPort)
        .acquire(&repo)
        .expect("helper acquire");
    stream.write_all(READY).expect("helper READY");
    stream.flush().unwrap();

    match mode.as_str() {
        HELPER_MODE_HOLD => {
            let mut buf = vec![0u8; RELEASE.len()];
            stream.read_exact(&mut buf).expect("read RELEASE");
            assert_eq!(buf, RELEASE);
            drop(guard);
        }
        HELPER_MODE_DIE => {
            std::mem::forget(guard);
            // Process exit closes the FD; lock becomes available to the parent.
            std::process::exit(0);
        }
        other => panic!("unknown helper mode {other}"),
    }
}

fn accept_ready(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(false).expect("blocking accept");
    let (mut stream, _) = listener.accept().expect("accept helper");
    stream.set_read_timeout(Some(WATCHDOG)).unwrap();
    stream.set_write_timeout(Some(WATCHDOG)).unwrap();
    let mut buf = vec![0u8; READY.len()];
    stream.read_exact(&mut buf).expect("read READY");
    assert_eq!(buf, READY, "helper must signal READY after acquire");
    stream
}

fn wait_child_exit(child: &mut Child) {
    let deadline = Instant::now() + WATCHDOG;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                assert!(status.success(), "helper exit status {status}");
                return;
            }
            None if Instant::now() < deadline => {
                // Bounded wait without sleep-based lock polling: block on a short channel recv
                // only as a watchdog quantum. This is not lock-state polling.
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(20));
                    let _ = tx.send(());
                });
                let _ = rx.recv_timeout(Duration::from_millis(25));
            }
            None => {
                let _ = child.kill();
                panic!("helper did not exit before watchdog");
            }
        }
    }
}

#[test]
fn real_cross_process_holder_release_allows_reacquire() {
    let fixture = RepoFixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap().to_string();
    let mut child = spawn_helper(HELPER_MODE_HOLD, fixture.repo(), &addr);
    let mut stream = accept_ready(&listener);

    let contended = acquire_real_lock(fixture.repo()).expect_err("must contend while held");
    assert_eq!(contended.operation, LockOperation::TryLock);
    assert_eq!(contended.kind, LockFailureKind::Contention);
    fixture.assert_transaction_surface_unchanged();

    stream.write_all(RELEASE).expect("send RELEASE");
    stream.flush().unwrap();
    wait_child_exit(&mut child);

    let guard = acquire_real_lock(fixture.repo()).expect("reacquire after helper release");
    drop(guard);
    assert!(fixture.lock_path.is_file());
    fixture.assert_transaction_surface_unchanged();
}

#[test]
fn real_cross_process_holder_crash_releases_fd_lock() {
    let fixture = RepoFixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap().to_string();
    let mut child = spawn_helper(HELPER_MODE_DIE, fixture.repo(), &addr);
    let _ready = accept_ready(&listener);

    // Before the helper exits, contention must still be observed.
    let contended = acquire_real_lock(fixture.repo()).expect_err("must contend before helper exit");
    assert_eq!(contended.operation, LockOperation::TryLock);
    assert_eq!(contended.kind, LockFailureKind::Contention);

    wait_child_exit(&mut child);

    let guard = acquire_real_lock(fixture.repo()).expect("reacquire after helper death");
    drop(guard);
    assert!(fixture.lock_path.is_file());
    fixture.assert_transaction_surface_unchanged();
}
