//! Shared temporary-directory root resolution for conformance tests.
//!
//! Prefer an explicit `TMPDIR` when the harness sets one (e.g. large matrices on a
//! data volume). Otherwise fall back to the process default temporary directory.
//! Never hard-code host paths such as `/mnt/data` or `/tmp`.

use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Resolve the directory under which conformance fixtures should be created.
///
/// - If `TMPDIR` is set, use that value (even when empty is not expected; empty
///   `OsString` is treated as unset by `var_os` only when the variable is absent).
/// - Otherwise use [`std::env::temp_dir`].
pub(super) fn test_temp_root() -> PathBuf {
    match std::env::var_os("TMPDIR") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::temp_dir(),
    }
}

/// Create an auto-cleaned [`TempDir`] under [`test_temp_root`] with the given prefix.
pub(super) fn create_test_temp_dir(prefix: &str) -> TempDir {
    let root = test_temp_root();
    std::fs::create_dir_all(&root)
        .unwrap_or_else(|error| panic!("create test temp root {}: {error}", root.display()));
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(&root)
        .unwrap_or_else(|error| {
            panic!(
                "create tempdir with prefix {prefix:?} under {}: {error}",
                root.display()
            )
        })
}

/// True when `path` is the same directory as (or is rooted at) `root` after
/// lexical normalization. Used only by the light resolver self-test.
fn path_is_under(path: &Path, root: &Path) -> bool {
    let path = path.components().collect::<Vec<_>>();
    let root = root.components().collect::<Vec<_>>();
    path.starts_with(&root)
}

#[test]
fn temp_root_fallback_uses_tmpdir_when_set_and_process_temp_when_unset() {
    // Light unit test only: verifies resolver selection. Does not create a heavy
    // matrix fixture tree, so it is safe under `env -u TMPDIR`.
    let process_temp = std::env::temp_dir();
    match std::env::var_os("TMPDIR") {
        Some(dir) => {
            let expected = PathBuf::from(dir);
            assert_eq!(
                test_temp_root(),
                expected,
                "TMPDIR must win over process temp when set"
            );
            let temp = create_test_temp_dir("artifact-temp-root-set-");
            assert!(
                path_is_under(temp.path(), &expected),
                "created temp {} must live under TMPDIR {}",
                temp.path().display(),
                expected.display()
            );
            // TempDir drops and removes the directory automatically.
            let path = temp.path().to_path_buf();
            drop(temp);
            assert!(!path.exists(), "TempDir must auto-clean {}", path.display());
        }
        None => {
            assert_eq!(
                test_temp_root(),
                process_temp,
                "unset TMPDIR must fall back to std::env::temp_dir()"
            );
            let temp = create_test_temp_dir("artifact-temp-root-fallback-");
            assert!(
                path_is_under(temp.path(), &process_temp),
                "created temp {} must live under process temp {}",
                temp.path().display(),
                process_temp.display()
            );
            let path = temp.path().to_path_buf();
            drop(temp);
            assert!(!path.exists(), "TempDir must auto-clean {}", path.display());
        }
    }
}
