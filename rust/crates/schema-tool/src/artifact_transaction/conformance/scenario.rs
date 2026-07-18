use super::test_temp::create_test_temp_dir;
use crate::codegen::{ArtifactKind, ArtifactPlan, GeneratedArtifact};
use crate::manifest::GenerationTarget;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

pub(super) const TRANSACTION_ID: &str = "conformance-fixed-id";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum InitialRoot {
    Existing,
    Absent,
}

pub(super) struct Scenario {
    temp: TempDir,
    pub(super) initial_root: InitialRoot,
    pub(super) plan: ArtifactPlan,
}

impl Scenario {
    pub(super) fn create(initial_root: InitialRoot) -> Self {
        let temp = create_test_temp_dir("artifact-transaction-conformance-");
        fs::create_dir_all(temp.path().join("out")).expect("create root parent");
        fs::write(temp.path().join("unrelated.txt"), b"unrelated").expect("write unrelated");
        fs::create_dir(temp.path().join("empty-unrelated")).expect("create unrelated empty dir");
        fs::write(temp.path().join(".schema-tool-generate.lock"), b"owner\n")
            .expect("write persistent lock");
        if initial_root == InitialRoot::Existing {
            let root = temp.path().join("out/generated");
            fs::create_dir_all(root.join("nested/empty")).expect("create initial tree");
            fs::write(root.join("same.txt"), b"old same\n").expect("write old artifact");
            fs::write(root.join("nested/same.txt"), b"old nested\n")
                .expect("write nested artifact");
            fs::write(root.join("unplanned.txt"), b"preserve\n").expect("write unplanned artifact");
        }
        let plan = ArtifactPlan::try_new(
            vec![
                GeneratedArtifact::new(
                    GenerationTarget::Rust,
                    ArtifactKind::Types,
                    "out/generated/same.txt",
                    "new same",
                    vec![],
                ),
                GeneratedArtifact::new(
                    GenerationTarget::Rust,
                    ArtifactKind::Typed,
                    "out/generated/nested/same.txt",
                    "new nested",
                    vec![],
                ),
                GeneratedArtifact::new(
                    GenerationTarget::Rust,
                    ArtifactKind::Catalog,
                    "out/generated/nested/new.txt",
                    "brand new",
                    vec![],
                ),
            ],
            ["out/generated".into()],
        )
        .expect("build conformance plan");
        Self {
            temp,
            initial_root,
            plan,
        }
    }

    pub(super) fn repo(&self) -> &Path {
        self.temp.path()
    }
}
