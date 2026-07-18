use super::fault_fs::PartialEffect;
use super::scenario::{InitialRoot, Scenario, TRANSACTION_ID};
use super::snapshot::{TreeNode, TreeSnapshot};
use crate::artifact_transaction::protocol::{
    LogicalPath, Operation, OperationBoundary, OperationEvent, TransactionPhase, TreeOwner,
    WritePurpose,
};
use serde::Serialize;

const ROOT: &str = "out/generated";
const STAGE: &str = "out/.generated.schema-tool-stage-conformance-fixed-id";
const BACKUP: &str = "out/.generated.schema-tool-backup-conformance-fixed-id";
const DISCARD: &str = "out/.generated.schema-tool-rollback-discard-conformance-fixed-id";
const FORMAL_JOURNAL: &str = ".schema-tool-generate-transaction.json";
const JOURNAL_TEMP: &str = ".schema-tool-generate-transaction.json.tmp-conformance-fixed-id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExpectedArtifactVersion {
    Old,
    Absent,
    New,
}

#[derive(Clone)]
pub(super) struct ReferenceModel {
    initial_root: InitialRoot,
    snapshot: TreeSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ReferenceProtocolState {
    pub(super) formal_phase: Option<TransactionPhase>,
    pub(super) pending_journal_phase: Option<TransactionPhase>,
    pub(super) temp_exists: bool,
    pub(super) commit_crossed: bool,
}

#[derive(Clone)]
pub(super) struct ReferenceState {
    model: ReferenceModel,
    protocol: ReferenceProtocolState,
}

#[derive(Serialize)]
struct ReferenceJournal<'a> {
    version: u32,
    transaction_id: &'a str,
    root: &'a str,
    stage: &'a str,
    backup: &'a str,
    rollback_discard: &'a str,
    original_exists: bool,
    phase: ReferencePhase,
}

#[derive(Serialize)]
#[serde(tag = "name", content = "state", rename_all = "snake_case")]
enum ReferencePhase {
    Preparing,
    Prepared,
    RollingBack(ReferenceRollbackState),
    Committed,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ReferenceRollbackState {
    Started,
    NewMovedToDiscard,
    OriginalRestored,
}

impl ReferenceState {
    pub(super) fn new(scenario: &Scenario) -> Self {
        Self {
            model: ReferenceModel::new(scenario),
            protocol: ReferenceProtocolState {
                formal_phase: None,
                pending_journal_phase: None,
                temp_exists: false,
                commit_crossed: false,
            },
        }
    }

    pub(super) fn initial_root(&self) -> InitialRoot {
        self.model.initial_root
    }

    pub(super) fn snapshot(&self) -> &TreeSnapshot {
        self.model.snapshot()
    }

    pub(super) fn protocol(&self) -> &ReferenceProtocolState {
        &self.protocol
    }

    pub(super) fn apply_event(&mut self, event: &OperationEvent) {
        self.model.apply_event(event);
        if event.target.boundary != OperationBoundary::AfterSuccess {
            return;
        }
        match &event.target.site.operation {
            Operation::CreateFile {
                target: LogicalPath::JournalTemp,
            } => self.protocol.temp_exists = true,
            Operation::WriteBytes {
                purpose: WritePurpose::JournalTemp,
                journal_target_phase: Some(phase),
                ..
            } => self.protocol.pending_journal_phase = Some(*phase),
            Operation::Rename {
                purpose: crate::artifact_transaction::protocol::RenamePurpose::PublishJournal,
                ..
            } => {
                let phase = self
                    .protocol
                    .pending_journal_phase
                    .expect("reference journal publication has a modeled target phase");
                self.protocol.formal_phase = Some(phase);
                self.protocol.temp_exists = false;
                if phase == TransactionPhase::Committed {
                    self.protocol.commit_crossed = true;
                }
            }
            Operation::RemoveFile {
                target: LogicalPath::JournalTemp,
            } => self.protocol.temp_exists = false,
            Operation::RemoveFile {
                target: LogicalPath::FormalJournal,
            } => self.protocol.formal_phase = None,
            _ => {}
        }
    }

    pub(super) fn assert_matches(&self, actual: &TreeSnapshot) {
        self.model.assert_matches(actual);
    }

    pub(super) fn expected_artifact_version(&self) -> ExpectedArtifactVersion {
        if self.protocol.commit_crossed {
            ExpectedArtifactVersion::New
        } else if self.model.initial_root == InitialRoot::Existing {
            ExpectedArtifactVersion::Old
        } else {
            ExpectedArtifactVersion::Absent
        }
    }

    pub(super) fn original_safely_restored(&self) -> bool {
        if self.model.initial_root == InitialRoot::Existing {
            self.model.snapshot.contains(ROOT) && !self.model.snapshot.contains(BACKUP)
        } else {
            !self.model.snapshot.contains(ROOT)
        }
    }

    pub(super) fn assert_terminal(&self, scenario: &Scenario, actual: &TreeSnapshot) {
        self.model
            .assert_terminal(scenario, actual, self.expected_artifact_version());
    }
}

impl ReferenceModel {
    pub(super) fn new(scenario: &Scenario) -> Self {
        Self::from_snapshot(
            scenario.initial_root,
            TreeSnapshot::capture(scenario.repo()),
        )
    }

    pub(super) fn from_snapshot(initial_root: InitialRoot, snapshot: TreeSnapshot) -> Self {
        Self {
            initial_root,
            snapshot,
        }
    }

    pub(super) fn snapshot(&self) -> &TreeSnapshot {
        &self.snapshot
    }

    pub(super) fn apply_event(&mut self, event: &OperationEvent) {
        if event.target.boundary == OperationBoundary::AfterSuccess {
            self.apply_success(&event.target.site.operation);
        }
    }

    pub(super) fn apply_partial(&mut self, operation: &Operation, effect: PartialEffect) {
        match effect {
            PartialEffect::WritePrefix { bytes } => {
                let (path, content) = self.write_content(operation);
                self.snapshot
                    .insert_file(&path, content[..content.len().min(bytes)].to_vec());
            }
            PartialEffect::CopyPrefix { bytes } => {
                let Operation::CopyFile {
                    source,
                    destination,
                } = operation
                else {
                    panic!("CopyPrefix applied to {operation:?}")
                };
                let source = logical_path(source);
                let destination = logical_path(destination);
                let content = self
                    .snapshot
                    .get_file(&source)
                    .unwrap_or_else(|| panic!("model copy source missing: {source}"));
                self.snapshot
                    .insert_file(&destination, content[..content.len().min(bytes)].to_vec());
            }
            PartialEffect::RemoveFileThenError => {
                let Operation::RemoveFile { target } = operation else {
                    panic!("RemoveFileThenError applied to {operation:?}")
                };
                self.snapshot.remove_path(&logical_path(target));
            }
            PartialEffect::RemoveTreeFirstLexicalEntry => {
                let Operation::RemoveTree { target, .. } = operation else {
                    panic!("RemoveTreeFirstLexicalEntry applied to {operation:?}")
                };
                self.remove_first_lexical_entry(&logical_path(target));
            }
        }
    }

    fn apply_success(&mut self, operation: &Operation) {
        match operation {
            Operation::CreateDir { target } => {
                self.snapshot.insert_directory(&logical_path(target));
            }
            Operation::CreateFile { target } | Operation::Truncate { target } => {
                self.snapshot.insert_file(&logical_path(target), Vec::new());
            }
            Operation::WriteBytes { .. } => {
                let (path, bytes) = self.write_content(operation);
                self.snapshot.insert_file(&path, bytes);
            }
            Operation::CopyFile {
                source,
                destination,
            } => {
                let source = logical_path(source);
                let destination = logical_path(destination);
                let bytes = self
                    .snapshot
                    .get_file(&source)
                    .unwrap_or_else(|| panic!("model copy source missing: {source}"))
                    .to_vec();
                self.snapshot.insert_file(&destination, bytes);
            }
            Operation::Rename {
                source,
                destination,
                ..
            } => self
                .snapshot
                .rename_path(&logical_path(source), &logical_path(destination)),
            Operation::RemoveFile { target } | Operation::RemoveTree { target, .. } => {
                self.snapshot.remove_path(&logical_path(target));
            }
            Operation::SyncFile { .. } | Operation::SyncDir { .. } => {}
        }
    }

    fn write_content(&self, operation: &Operation) -> (String, Vec<u8>) {
        let Operation::WriteBytes {
            purpose,
            target,
            journal_target_phase,
        } = operation
        else {
            panic!("write effect applied to {operation:?}")
        };
        let bytes = match purpose {
            WritePurpose::Artifact => artifact_bytes(target),
            WritePurpose::JournalTemp => reference_journal_bytes(
                journal_target_phase.expect("journal write has target phase"),
                self.initial_root,
            ),
        };
        (logical_path(target), bytes)
    }

    fn remove_first_lexical_entry(&mut self, root: &str) {
        let prefix = format!("{root}/");
        let first = self
            .snapshot
            .0
            .keys()
            .filter_map(|path| std::str::from_utf8(path).ok())
            .filter_map(|path| path.strip_prefix(&prefix))
            .map(|relative| relative.split('/').next().expect("non-empty relative"))
            .min()
            .map(|name| format!("{root}/{name}"));
        if let Some(first) = first {
            self.snapshot.remove_path(&first);
        } else {
            self.snapshot.remove_path(root);
        }
    }

    pub(super) fn assert_matches(&self, actual: &TreeSnapshot) {
        assert_eq!(&self.snapshot, actual, "reference state diverged");
    }

    pub(super) fn assert_terminal(
        &self,
        _scenario: &Scenario,
        snapshot: &TreeSnapshot,
        expected: ExpectedArtifactVersion,
    ) {
        match expected {
            ExpectedArtifactVersion::Old => {
                assert_eq!(self.initial_root, InitialRoot::Existing);
                assert_eq!(
                    snapshot.get_file("out/generated/same.txt"),
                    Some(b"old same\n".as_slice())
                );
                assert_eq!(
                    snapshot.get_file("out/generated/nested/same.txt"),
                    Some(b"old nested\n".as_slice())
                );
                assert_eq!(
                    snapshot.get_file("out/generated/unplanned.txt"),
                    Some(b"preserve\n".as_slice())
                );
                assert!(!snapshot.contains("out/generated/nested/new.txt"));
            }
            ExpectedArtifactVersion::Absent => {
                assert_eq!(self.initial_root, InitialRoot::Absent);
                assert!(!snapshot.contains(ROOT));
            }
            ExpectedArtifactVersion::New => {
                assert_eq!(
                    snapshot.get_file("out/generated/same.txt"),
                    Some(b"new same\n".as_slice())
                );
                assert_eq!(
                    snapshot.get_file("out/generated/nested/same.txt"),
                    Some(b"new nested\n".as_slice())
                );
                assert_eq!(
                    snapshot.get_file("out/generated/nested/new.txt"),
                    Some(b"brand new\n".as_slice())
                );
                if self.initial_root == InitialRoot::Existing {
                    assert_eq!(
                        snapshot.get_file("out/generated/unplanned.txt"),
                        Some(b"preserve\n".as_slice())
                    );
                }
            }
        }
        assert_eq!(
            snapshot.get_file("unrelated.txt"),
            Some(b"unrelated".as_slice())
        );
        assert!(snapshot.contains("empty-unrelated"));
        assert!(snapshot.contains(".schema-tool-generate.lock"));
        for path in snapshot.0.keys() {
            let path = std::str::from_utf8(path).expect("scenario paths are UTF-8");
            assert!(
                ![FORMAL_JOURNAL, STAGE, BACKUP, DISCARD]
                    .iter()
                    .any(|prefix| path.starts_with(prefix)),
                "unexpected transaction residue: {path}"
            );
        }
    }
}

fn logical_path(path: &LogicalPath) -> String {
    match path {
        LogicalPath::Repo => String::new(),
        LogicalPath::Root => ROOT.into(),
        LogicalPath::Stage => STAGE.into(),
        LogicalPath::Backup => BACKUP.into(),
        LogicalPath::Discard => DISCARD.into(),
        LogicalPath::FormalJournal => FORMAL_JOURNAL.into(),
        LogicalPath::JournalTemp => JOURNAL_TEMP.into(),
        LogicalPath::TreeEntry { owner, relative } => format!(
            "{}/{}",
            match owner {
                TreeOwner::Root => ROOT,
                TreeOwner::Stage => STAGE,
            },
            relative.as_str()
        ),
    }
}

fn artifact_bytes(target: &LogicalPath) -> Vec<u8> {
    let LogicalPath::TreeEntry {
        owner: TreeOwner::Stage,
        relative,
    } = target
    else {
        panic!("artifact write target is not a stage entry: {target:?}")
    };
    match relative.as_str() {
        "same.txt" => b"new same\n".to_vec(),
        "nested/same.txt" => b"new nested\n".to_vec(),
        "nested/new.txt" => b"brand new\n".to_vec(),
        path => panic!("unknown artifact path in reference model: {path}"),
    }
}

fn reference_journal_bytes(phase: TransactionPhase, initial: InitialRoot) -> Vec<u8> {
    let phase = match phase {
        TransactionPhase::Preparing => ReferencePhase::Preparing,
        TransactionPhase::Prepared => ReferencePhase::Prepared,
        TransactionPhase::Committed => ReferencePhase::Committed,
        TransactionPhase::RollingBack(state) => ReferencePhase::RollingBack(match state {
            crate::artifact_transaction::protocol::RollbackState::Started => {
                ReferenceRollbackState::Started
            }
            crate::artifact_transaction::protocol::RollbackState::NewMovedToDiscard => {
                ReferenceRollbackState::NewMovedToDiscard
            }
            crate::artifact_transaction::protocol::RollbackState::OriginalRestored => {
                ReferenceRollbackState::OriginalRestored
            }
        }),
    };
    serde_json::to_vec_pretty(&ReferenceJournal {
        version: 2,
        transaction_id: TRANSACTION_ID,
        root: ROOT,
        stage: STAGE,
        backup: BACKUP,
        rollback_discard: DISCARD,
        original_exists: initial == InitialRoot::Existing,
        phase,
    })
    .expect("serialize independent reference journal")
}

pub(super) fn assert_snapshot_has_real_partial_effect(before: &TreeSnapshot, after: &TreeSnapshot) {
    assert_ne!(
        before, after,
        "partial fault must have a real filesystem effect"
    );
    assert!(after
        .0
        .values()
        .all(|node| !matches!(node, TreeNode::Other)));
}

#[test]
fn independent_reference_journal_has_explicit_v2_byte_fixtures() {
    let fixtures = [
        (
            TransactionPhase::Preparing,
            InitialRoot::Existing,
            "{\n  \"version\": 2,\n  \"transaction_id\": \"conformance-fixed-id\",\n  \"root\": \"out/generated\",\n  \"stage\": \"out/.generated.schema-tool-stage-conformance-fixed-id\",\n  \"backup\": \"out/.generated.schema-tool-backup-conformance-fixed-id\",\n  \"rollback_discard\": \"out/.generated.schema-tool-rollback-discard-conformance-fixed-id\",\n  \"original_exists\": true,\n  \"phase\": {\n    \"name\": \"preparing\"\n  }\n}",
        ),
        (
            TransactionPhase::RollingBack(
                crate::artifact_transaction::protocol::RollbackState::OriginalRestored,
            ),
            InitialRoot::Absent,
            "{\n  \"version\": 2,\n  \"transaction_id\": \"conformance-fixed-id\",\n  \"root\": \"out/generated\",\n  \"stage\": \"out/.generated.schema-tool-stage-conformance-fixed-id\",\n  \"backup\": \"out/.generated.schema-tool-backup-conformance-fixed-id\",\n  \"rollback_discard\": \"out/.generated.schema-tool-rollback-discard-conformance-fixed-id\",\n  \"original_exists\": false,\n  \"phase\": {\n    \"name\": \"rolling_back\",\n    \"state\": \"original_restored\"\n  }\n}",
        ),
    ];
    for (phase, initial, bytes) in fixtures {
        assert_eq!(reference_journal_bytes(phase, initial), bytes.as_bytes());
        let value: serde_json::Value = serde_json::from_slice(bytes.as_bytes()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 8);
        assert_eq!(value["version"], 2);
        assert!(value["phase"].is_object());
    }
}

#[test]
fn independent_reference_journal_matches_successful_publications() {
    use super::fault_fs::FaultingRealFs;
    use super::oracle::ModelCheckingObserver;
    use crate::artifact_transaction::ArtifactTransaction;

    for initial in [InitialRoot::Existing, InitialRoot::Absent] {
        let scenario = Scenario::create(initial);
        let observer = ModelCheckingObserver::new(&scenario, None);
        let mut engine = ArtifactTransaction::test_engine(FaultingRealFs::no_fault(), observer);
        ArtifactTransaction::install_detailed_with_id(
            scenario.repo(),
            &scenario.plan,
            TRANSACTION_ID,
            &mut engine,
        )
        .unwrap();
    }
}
