use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum TreeNode {
    Directory,
    File(Vec<u8>),
    Symlink(Vec<u8>),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct TreeSnapshot(pub(super) BTreeMap<Vec<u8>, TreeNode>);

impl TreeSnapshot {
    pub(super) fn capture(root: &Path) -> Self {
        let mut entries = BTreeMap::new();
        capture_directory(root, root, &mut entries).expect("capture repository snapshot");
        Self(entries)
    }

    pub(super) fn get_file(&self, path: &str) -> Option<&[u8]> {
        match self.0.get(path.as_bytes()) {
            Some(TreeNode::File(bytes)) => Some(bytes),
            _ => None,
        }
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        self.0.contains_key(path.as_bytes())
    }

    pub(super) fn insert_directory(&mut self, path: &str) {
        self.0.insert(path.as_bytes().to_vec(), TreeNode::Directory);
    }

    pub(super) fn insert_file(&mut self, path: &str, bytes: Vec<u8>) {
        self.0
            .insert(path.as_bytes().to_vec(), TreeNode::File(bytes));
    }

    pub(super) fn remove_path(&mut self, path: &str) {
        let prefix = format!("{path}/").into_bytes();
        self.0.retain(|candidate, _| {
            candidate.as_slice() != path.as_bytes() && !candidate.starts_with(&prefix)
        });
    }

    pub(super) fn rename_path(&mut self, source: &str, destination: &str) {
        let source_prefix = format!("{source}/").into_bytes();
        let mut moved = Vec::new();
        self.0.retain(|path, node| {
            if path.as_slice() == source.as_bytes() || path.starts_with(&source_prefix) {
                let suffix = &path[source.len()..];
                let mut destination_path = destination.as_bytes().to_vec();
                destination_path.extend_from_slice(suffix);
                moved.push((destination_path, node.clone()));
                false
            } else {
                true
            }
        });
        self.0.extend(moved);
    }
}

fn capture_directory(
    root: &Path,
    directory: &Path,
    output: &mut BTreeMap<Vec<u8>, TreeNode>,
) -> std::io::Result<()> {
    let mut children = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by(|left, right| os_bytes(&left.file_name()).cmp(os_bytes(&right.file_name())));
    for child in children {
        let path = child.path();
        let relative = path.strip_prefix(root).expect("snapshot path below root");
        let key = path_bytes(relative);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            output.insert(key, TreeNode::Symlink(path_bytes(&fs::read_link(&path)?)));
        } else if metadata.is_dir() {
            output.insert(key, TreeNode::Directory);
            capture_directory(root, &path, output)?;
        } else if metadata.is_file() {
            output.insert(key, TreeNode::File(fs::read(&path)?));
        } else {
            output.insert(key, TreeNode::Other);
        }
    }
    Ok(())
}

fn path_bytes(path: &Path) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, component) in path.components().enumerate() {
        if index > 0 {
            output.push(b'/');
        }
        output.extend_from_slice(component.as_os_str().as_bytes());
    }
    output
}

fn os_bytes(value: &OsStr) -> &[u8] {
    value.as_bytes()
}

#[test]
fn snapshot_is_recursive_exact_and_does_not_follow_symlinks() {
    use super::test_temp::create_test_temp_dir;
    use std::os::unix::fs::symlink;
    let temp = create_test_temp_dir("artifact-snapshot-");
    fs::create_dir_all(temp.path().join("a/empty")).unwrap();
    fs::write(temp.path().join("a/file"), [0, 1, 255]).unwrap();
    symlink(PathBuf::from("a/file"), temp.path().join("link")).unwrap();
    let snapshot = TreeSnapshot::capture(temp.path());
    assert!(snapshot.contains("a"));
    assert!(snapshot.contains("a/empty"));
    assert_eq!(snapshot.get_file("a/file"), Some([0, 1, 255].as_slice()));
    assert_eq!(
        snapshot.0.get(b"link".as_slice()),
        Some(&TreeNode::Symlink(b"a/file".to_vec()))
    );
}
