//! Storage helpers shared by the runtime entry points.
//!
//! The runtime model is intentionally filesystem-first: `.multorum/`
//! stores the authoritative control plane, worker contract, compiled
//! file sets, and mailbox bundles. This module centralizes that on-disk
//! layout and the small amount of version-control orchestration needed
//! to create worktrees, delete finalized workspaces, and integrate
//! submitted commits.
//!
//! The orchestrator persists runtime state as one file per bidding
//! group under `.multorum/orchestrator/group/` and one file per worker
//! under `.multorum/orchestrator/worker/`.

use super::timestamp::Timestamp;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tempfile::TempDir;

use crate::bundle::{BODY_FILE_NAME, BundlePayload, BundleWriter};
use crate::runtime::{
    AuditEntry, MailboxMessageView, MultorumPaths, RulebookInit, RuntimeError, WorkerContractView,
    WorkerId, WorkerPaths,
    mailbox::{
        AckRef, BundleEnvelope, MailboxDirection, MessageKind, MessageRef, ProtocolVersion,
        PublishedBundle, ReplyReference, Sequence, SequenceFilter,
    },
};
use crate::schema::rulebook::{
    CheckName, CheckPolicy, CompiledRulebook, RULEBOOK_RELATIVE_PATH, Rulebook,
};
use crate::vcs::{CanonicalCommitHash, GitVcs, VersionControl};

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// One persisted bidding-group record stored under
/// `.multorum/orchestrator/group/<Perspective>.toml`.
///
/// A group forms when the first worker for a perspective is created.
/// Its base commit and boundary are locked at formation. Subsequent
/// workers for the same perspective join the existing group and share
/// its base commit and boundary.
///
/// Note: When the group has no live workers left (all members are
/// `MERGED` or `DISCARDED`), Multorum clears the materialized boundary.
/// The group record itself stays on disk until `worker delete`
/// removes the final worker record for that perspective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GroupStateRecord {
    /// Perspective governing this group.
    pub perspective: crate::schema::perspective::PerspectiveName,
    /// Base commit pinning the group's code snapshot.
    pub base_commit: CanonicalCommitHash,
    /// Compiled read set at group formation.
    pub read_set: BTreeSet<PathBuf>,
    /// Compiled write set at group formation.
    pub write_set: BTreeSet<PathBuf>,
}

/// One persisted worker record stored under
/// `.multorum/orchestrator/worker/<worker>.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkerStateRecord {
    /// Unique worker identity.
    #[serde(rename = "worker")]
    pub worker_id: WorkerId,
    /// Perspective governing this worker.
    pub perspective: crate::schema::perspective::PerspectiveName,
    /// Current lifecycle state.
    pub state: crate::runtime::WorkerState,
    /// Absolute path to the managed worktree.
    pub worktree_path: PathBuf,
    /// Canonical submitted worker commit when the worker is in `COMMITTED`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_head_commit: Option<CanonicalCommitHash>,
}

/// Orchestrator runtime state reconstructed from persisted group and
/// worker records.
///
/// Note: The runtime still operates on a joined in-memory aggregate so
/// service code can reason in terms of bidding groups. Persistence is
/// split only at the filesystem boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StateFile {
    /// Bidding groups, each containing its member workers.
    pub groups: Vec<BiddingGroupRecord>,
}

/// One bidding group with its compiled boundary and member workers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BiddingGroupRecord {
    /// Perspective governing this group.
    pub perspective: crate::schema::perspective::PerspectiveName,
    /// Base commit pinning the group's code snapshot.
    pub base_commit: CanonicalCommitHash,
    /// Compiled read set at group formation.
    pub read_set: BTreeSet<PathBuf>,
    /// Compiled write set at group formation.
    pub write_set: BTreeSet<PathBuf>,
    /// Workers in this group.
    pub workers: Vec<WorkerEntry>,
}

impl BiddingGroupRecord {
    /// Whether the group has at least one live (non-finalized) worker.
    pub fn has_live_workers(&self) -> bool {
        self.workers.iter().any(|w| w.state.is_live())
    }

    /// Find a worker entry by id.
    pub fn find_worker(&self, worker_id: &WorkerId) -> Option<&WorkerEntry> {
        self.workers.iter().find(|w| w.worker_id == *worker_id)
    }

    /// Find a mutable worker entry by id.
    pub fn find_worker_mut(&mut self, worker_id: &WorkerId) -> Option<&mut WorkerEntry> {
        self.workers.iter_mut().find(|w| w.worker_id == *worker_id)
    }

    /// Clear the materialized boundary for a finalized group.
    ///
    /// An empty boundary marks "no active ownership" for this group.
    pub fn clear_boundary(&mut self) {
        self.read_set.clear();
        self.write_set.clear();
    }
}

/// One worker within a bidding group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerEntry {
    /// Unique worker identity.
    pub worker_id: WorkerId,
    /// Current lifecycle state.
    pub state: crate::runtime::WorkerState,
    /// Absolute path to the managed worktree.
    pub worktree_path: PathBuf,
    /// Canonical submitted worker commit when the worker is in `COMMITTED`.
    pub submitted_head_commit: Option<CanonicalCommitHash>,
}

impl StateFile {
    /// Reconstruct joined runtime state from persisted group and worker
    /// records.
    ///
    /// Note: Every worker must reference an existing group record.
    /// Multorum fails fast instead of guessing a partial topology from
    /// orphaned files.
    fn from_records(
        groups: Vec<GroupStateRecord>, workers: Vec<WorkerStateRecord>,
    ) -> Result<Self, RuntimeError> {
        let mut grouped =
            BTreeMap::<crate::schema::perspective::PerspectiveName, BiddingGroupRecord>::new();
        let mut seen_workers = BTreeSet::<WorkerId>::new();
        for group in groups {
            let previous = grouped.insert(
                group.perspective.clone(),
                BiddingGroupRecord {
                    perspective: group.perspective,
                    base_commit: group.base_commit,
                    read_set: group.read_set,
                    write_set: group.write_set,
                    workers: Vec::new(),
                },
            );
            if previous.is_some() {
                return Err(RuntimeError::CheckFailed(
                    "duplicate persisted group state for one perspective".to_owned(),
                ));
            }
        }

        for worker in workers {
            if !seen_workers.insert(worker.worker_id.clone()) {
                return Err(RuntimeError::CheckFailed(format!(
                    "duplicate persisted worker state for `{}`",
                    worker.worker_id
                )));
            }
            let group = grouped.get_mut(&worker.perspective).ok_or_else(|| {
                RuntimeError::CheckFailed(format!(
                    "worker `{}` references missing group `{}`",
                    worker.worker_id, worker.perspective
                ))
            })?;
            group.workers.push(WorkerEntry {
                worker_id: worker.worker_id,
                state: worker.state,
                worktree_path: worker.worktree_path,
                submitted_head_commit: worker.submitted_head_commit,
            });
        }

        let mut groups = grouped.into_values().collect::<Vec<_>>();
        for group in &mut groups {
            group.workers.sort_by(|left, right| left.worker_id.cmp(&right.worker_id));
        }
        Ok(Self { groups })
    }

    /// Convert the joined runtime view into persisted group and worker
    /// records.
    fn into_records(self) -> (Vec<GroupStateRecord>, Vec<WorkerStateRecord>) {
        let mut group_records = Vec::with_capacity(self.groups.len());
        let mut worker_records = Vec::new();

        for group in self.groups {
            group_records.push(GroupStateRecord {
                perspective: group.perspective.clone(),
                base_commit: group.base_commit.clone(),
                read_set: group.read_set.clone(),
                write_set: group.write_set.clone(),
            });
            worker_records.extend(group.workers.into_iter().map(|worker| WorkerStateRecord {
                worker_id: worker.worker_id,
                perspective: group.perspective.clone(),
                state: worker.state,
                worktree_path: worker.worktree_path,
                submitted_head_commit: worker.submitted_head_commit,
            }));
        }

        group_records.sort_by(|left, right| left.perspective.cmp(&right.perspective));
        worker_records.sort_by(|left, right| left.worker_id.cmp(&right.worker_id));
        (group_records, worker_records)
    }

    /// Find the live bidding group for a perspective.
    ///
    /// A "live" group is one with at least one non-finalized worker.
    pub fn find_live_group(
        &self, perspective: &crate::schema::perspective::PerspectiveName,
    ) -> Option<&BiddingGroupRecord> {
        self.groups.iter().find(|g| g.perspective == *perspective && g.has_live_workers())
    }

    /// Find a mutable reference to the live bidding group for a perspective.
    pub fn find_live_group_mut(
        &mut self, perspective: &crate::schema::perspective::PerspectiveName,
    ) -> Option<&mut BiddingGroupRecord> {
        self.groups.iter_mut().find(|g| g.perspective == *perspective && g.has_live_workers())
    }

    /// Find the group and worker entry for a given worker id.
    pub fn find_worker(&self, worker_id: &WorkerId) -> Option<(&BiddingGroupRecord, &WorkerEntry)> {
        for group in &self.groups {
            if let Some(worker) = group.find_worker(worker_id) {
                return Some((group, worker));
            }
        }
        None
    }

    /// Find mutable references to the group and worker entry for a
    /// given worker id.
    pub fn find_worker_group_mut(
        &mut self, worker_id: &WorkerId,
    ) -> Option<&mut BiddingGroupRecord> {
        self.groups.iter_mut().find(|g| g.find_worker(worker_id).is_some())
    }

    /// Remove one worker entry by id and garbage-collect empty groups.
    ///
    /// Note: Reusing an explicit worker id must evict any finalized entry
    /// before the new worker is inserted, otherwise lookups can keep
    /// resolving the stale finalized record first.
    pub fn remove_worker(&mut self, worker_id: &WorkerId) -> bool {
        let mut removed = false;
        for group in &mut self.groups {
            let before = group.workers.len();
            group.workers.retain(|worker| worker.worker_id != *worker_id);
            removed |= group.workers.len() != before;
        }
        self.gc_empty_groups();
        removed
    }

    /// All groups that still have at least one live worker.
    pub fn live_groups(&self) -> impl Iterator<Item = &BiddingGroupRecord> {
        self.groups.iter().filter(|g| g.has_live_workers())
    }

    /// Remove groups that have no remaining workers at all.
    pub fn gc_empty_groups(&mut self) {
        self.groups.retain(|g| !g.workers.is_empty());
    }
}

/// Acknowledgement metadata written to mailbox `ack/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AckRecord {
    /// The acknowledged mailbox sequence number.
    pub(crate) sequence: Sequence,
    /// Timestamp recorded when the acknowledgement was written.
    pub(crate) acknowledged_at: Timestamp,
}

/// One staged runtime path waiting to be promoted into place.
///
/// Note: Merge-time promotion may need to roll back both files and
/// directories, so this helper treats both uniformly as rename targets.
#[derive(Debug)]
struct PreparedPromotion {
    staged_path: PathBuf,
    final_path: PathBuf,
    backup_path: PathBuf,
    had_original: bool,
    promoted: bool,
}

impl PreparedPromotion {
    /// Construct one staged promotion under the merge staging root.
    fn new(
        staged_path: PathBuf, final_path: PathBuf, staging_root: &Path, backup_name: &str,
    ) -> Self {
        Self {
            had_original: final_path.exists(),
            promoted: false,
            backup_path: staging_root.join(backup_name),
            staged_path,
            final_path,
        }
    }

    /// Promote the staged path into its final runtime location.
    fn promote(&mut self) -> Result<(), RuntimeError> {
        if let Some(parent) = self.final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = self.backup_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if self.had_original {
            fs::rename(&self.final_path, &self.backup_path)?;
        }
        if let Err(error) = fs::rename(&self.staged_path, &self.final_path) {
            if self.had_original && self.backup_path.exists() {
                fs::rename(&self.backup_path, &self.final_path)?;
            }
            return Err(RuntimeError::Io(error));
        }
        self.promoted = true;
        Ok(())
    }

    /// Restore the original runtime path after a failed merge.
    fn rollback(&mut self) -> Result<(), RuntimeError> {
        if self.promoted && self.final_path.exists() {
            fs::rename(&self.final_path, &self.staged_path)?;
            self.promoted = false;
        }
        if self.had_original && self.backup_path.exists() {
            fs::rename(&self.backup_path, &self.final_path)?;
        }
        Ok(())
    }
}

/// Pre-merge verification results collected before the canonical branch is
/// mutated.
///
/// Bundles the four values produced by the verification phase so that
/// [`RuntimeFs::prepare_merge_artifacts`] does not exceed the argument limit
/// and callers can access individual fields after the call.
pub(crate) struct MergeVerification {
    /// Resolved head commit submitted by the worker.
    pub(crate) head_commit: CanonicalCommitHash,
    /// Files changed by the worker relative to the base commit.
    pub(crate) changed_files: BTreeSet<PathBuf>,
    /// Checks that executed during integration.
    pub(crate) ran_checks: Vec<String>,
    /// Checks skipped due to trusted evidence.
    pub(crate) skipped_checks: Vec<String>,
}

/// Merge-time runtime artifacts staged in the system temporary directory.
///
/// The staged directory is invisible to readers until promotion. This
/// lets `merge_worker` validate audit payloads and prepare the final
/// runtime projection before the canonical branch is mutated.
#[derive(Debug)]
pub(crate) struct StagedMergeArtifacts {
    promotions: Vec<PreparedPromotion>,
    staging_dir: Option<TempDir>,
}

impl StagedMergeArtifacts {
    /// Promote every staged runtime artifact into place.
    pub(crate) fn promote(&mut self) -> Result<(), RuntimeError> {
        for idx in 0..self.promotions.len() {
            if let Err(error) = self.promotions[idx].promote() {
                let rollback_result = self.rollback_range(idx + 1);
                return Err(rollback_result.err().unwrap_or(error));
            }
        }
        Ok(())
    }

    /// Roll back every already-promoted runtime artifact.
    pub(crate) fn rollback(&mut self) -> Result<(), RuntimeError> {
        self.rollback_range(self.promotions.len())
    }

    fn rollback_range(&mut self, promoted: usize) -> Result<(), RuntimeError> {
        for promotion in self.promotions[..promoted].iter_mut().rev() {
            promotion.rollback()?;
        }
        Ok(())
    }

    /// Remove merge staging after the runtime transaction has finished.
    ///
    /// Note: Cleanup happens after the canonical commit is finalized, so
    /// failures here must not change the merge result.
    pub(crate) fn cleanup(mut self) {
        if let Some(staging_dir) = self.staging_dir.take() {
            let staging_path = staging_dir.path().to_path_buf();
            if let Err(error) = staging_dir.close() {
                tracing::warn!(
                    path = %staging_path.display(),
                    error = %error,
                    "failed to clean up merge staging"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Protocol version written into persisted mailbox envelopes.
const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(1);

/// Canonical mailbox envelope file name within one bundle directory.
const ENVELOPE_FILE_NAME: &str = "envelope.toml";

/// Canonical acknowledgement file extension for mailbox bundles.
const ACK_EXTENSION: &str = "ack";

/// Gitignore entries for runtime-only directories.
const MULTORUM_GITIGNORE_ENTRIES: [&str; 2] = ["orchestrator/", "tr/"];

// ---------------------------------------------------------------------------
// RuntimeFs — core
// ---------------------------------------------------------------------------

/// Storage access rooted at the canonical workspace.
#[derive(Debug, Clone)]
pub(crate) struct RuntimeFs {
    paths: MultorumPaths,
    vcs: Arc<dyn VersionControl>,
}

impl RuntimeFs {
    /// Build runtime helpers for the canonical workspace root.
    pub(crate) fn new(workspace_root: impl Into<PathBuf>) -> Result<Self, RuntimeError> {
        Self::with_vcs(workspace_root, Arc::new(GitVcs::new()))
    }

    /// Build runtime helpers for the canonical workspace root with one
    /// explicit repository backend.
    pub(crate) fn with_vcs(
        workspace_root: impl Into<PathBuf>, vcs: Arc<dyn VersionControl>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self { paths: MultorumPaths::new_canonical(workspace_root.into())?, vcs })
    }

    /// The canonical workspace root.
    pub(crate) fn workspace_root(&self) -> &Path {
        self.paths.workspace_root()
    }

    /// Deterministic worktree-local runtime paths for one worker.
    pub(crate) fn worker_paths(&self, worker_id: &WorkerId) -> WorkerPaths {
        self.paths.worker(worker_id)
    }

    /// Repository backend bound to the current workspace.
    pub(crate) fn vcs(&self) -> &dyn VersionControl {
        self.vcs.as_ref()
    }

    /// Run one shell-based rulebook check in a worktree.
    pub(crate) fn run_check(
        &self, worktree_root: &Path, name: &CheckName, command_text: &str,
    ) -> Result<(), RuntimeError> {
        tracing::trace!(
            check = %name,
            command = command_text,
            root = %worktree_root.display(),
            "running pre-merge check"
        );

        let output =
            Command::new("sh").arg("-lc").arg(command_text).current_dir(worktree_root).output()?;

        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_owned()
        } else {
            stderr.trim().to_owned()
        };
        Err(RuntimeError::CheckFailed(format!("{name}: {details}")))
    }

    // -----------------------------------------------------------------------
    // TOML / path-list helpers
    // -----------------------------------------------------------------------

    pub(crate) fn read_toml<T>(path: &Path) -> Result<T, RuntimeError>
    where
        T: DeserializeOwned,
    {
        let contents = fs::read_to_string(path)?;
        Ok(toml::from_str(&contents)?)
    }

    pub(crate) fn write_toml<T>(path: &Path, value: &T) -> Result<(), RuntimeError>
    where
        T: Serialize,
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string(value)?)?;
        Ok(())
    }

    fn write_path_list(path: &Path, paths: &BTreeSet<PathBuf>) -> Result<(), RuntimeError> {
        let mut file = File::create(path)?;
        for entry in paths {
            writeln!(file, "{}", entry.display())?;
        }
        Ok(())
    }

    fn load_state_records<T>(root: &Path) -> Result<Vec<T>, RuntimeError>
    where
        T: DeserializeOwned,
    {
        let mut entries = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            entries.push(Self::read_toml(&path)?);
        }
        Ok(entries)
    }

    fn remove_stale_state_files(
        root: &Path, expected: &BTreeSet<PathBuf>,
    ) -> Result<(), RuntimeError> {
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            if !expected.contains(&path) {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    fn write_state_snapshot(
        &self, group_root: &Path, worker_root: &Path, state: StateFile,
    ) -> Result<(), RuntimeError> {
        fs::create_dir_all(group_root)?;
        fs::create_dir_all(worker_root)?;

        let (groups, workers) = state.into_records();
        let orchestrator_paths = self.paths.orchestrator();
        let expected_group_paths = groups
            .iter()
            .map(|group| {
                if group_root == orchestrator_paths.group_root().as_path() {
                    orchestrator_paths.group_state(&group.perspective)
                } else {
                    group_root.join(format!("{}.toml", group.perspective))
                }
            })
            .collect::<BTreeSet<_>>();
        let expected_worker_paths = workers
            .iter()
            .map(|worker| {
                if worker_root == orchestrator_paths.worker_root().as_path() {
                    orchestrator_paths.worker_state(&worker.worker_id)
                } else {
                    worker_root.join(format!("{}.toml", worker.worker_id))
                }
            })
            .collect::<BTreeSet<_>>();

        Self::remove_stale_state_files(group_root, &expected_group_paths)?;
        Self::remove_stale_state_files(worker_root, &expected_worker_paths)?;

        for group in groups {
            let path = if group_root == orchestrator_paths.group_root().as_path() {
                orchestrator_paths.group_state(&group.perspective)
            } else {
                group_root.join(format!("{}.toml", group.perspective))
            };
            Self::write_toml(&path, &group)?;
        }

        for worker in workers {
            let path = if worker_root == orchestrator_paths.worker_root().as_path() {
                orchestrator_paths.worker_state(&worker.worker_id)
            } else {
                worker_root.join(format!("{}.toml", worker.worker_id))
            };
            Self::write_toml(&path, &worker)?;
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Rulebook initialization
    // -----------------------------------------------------------------------

    /// Initialize or repair the committed `.multorum/` project surface.
    pub(crate) fn initialize_rulebook(&self) -> Result<RulebookInit, RuntimeError> {
        let multorum_root = self.paths.multorum_root();
        let gitignore_path = self.paths.multorum_gitignore();
        let rulebook_path = Rulebook::rulebook_path(self.workspace_root());
        let orchestrator_paths = self.paths.orchestrator();
        let mut warnings = Vec::new();

        fs::create_dir_all(&multorum_root)?;
        fs::create_dir_all(orchestrator_paths.root())?;
        fs::create_dir_all(orchestrator_paths.group_root())?;
        fs::create_dir_all(orchestrator_paths.worker_root())?;
        fs::create_dir_all(self.paths.audit())?;
        fs::create_dir_all(multorum_root.join("tr"))?;

        let added_gitignore_entries = self.ensure_multorum_gitignore()?;
        if !added_gitignore_entries.is_empty() {
            warnings.push(format!(
                "added missing entries to `.multorum/.gitignore`: {}",
                added_gitignore_entries.join(", ")
            ));
        }
        if !rulebook_path.exists() {
            fs::write(&rulebook_path, Rulebook::default_template())?;
        }

        let exclusion_path = orchestrator_paths.exclusion_set();
        if !exclusion_path.exists() {
            let state = self.load_state()?;
            self.rewrite_exclusion_set(&state)?;
        }
        tracing::info!(
            multorum_root = %multorum_root.display(),
            rulebook_path = %rulebook_path.display(),
            gitignore_path = %gitignore_path.display(),
            "initialized rulebook workspace"
        );

        Ok(RulebookInit { multorum_root, rulebook_path, gitignore_path, warnings })
    }

    // -----------------------------------------------------------------------
    // State directories
    // -----------------------------------------------------------------------

    /// Load the orchestrator group and worker state directories.
    pub(crate) fn load_state(&self) -> Result<StateFile, RuntimeError> {
        let orchestrator_paths = self.paths.orchestrator();
        let group_root = orchestrator_paths.group_root();
        let worker_root = orchestrator_paths.worker_root();
        if !group_root.is_dir() || !worker_root.is_dir() {
            return Err(RuntimeError::MissingOrchestratorState);
        }
        let groups = Self::load_state_records::<GroupStateRecord>(&group_root)?;
        let workers = Self::load_state_records::<WorkerStateRecord>(&worker_root)?;
        StateFile::from_records(groups, workers)
    }

    /// Persist the orchestrator group and worker state directories.
    pub(crate) fn store_state(&self, state: &StateFile) -> Result<(), RuntimeError> {
        let orchestrator_paths = self.paths.orchestrator();
        self.write_state_snapshot(
            &orchestrator_paths.group_root(),
            &orchestrator_paths.worker_root(),
            state.clone(),
        )
    }

    // -----------------------------------------------------------------------
    // Rulebook compilation
    // -----------------------------------------------------------------------

    /// Load and compile the rulebook from the working tree.
    ///
    /// Reads `rulebook.toml` from disk and compiles perspective
    /// boundaries against the current working tree files.
    pub(crate) fn load_working_tree_rulebook(&self) -> Result<CompiledRulebook, RuntimeError> {
        let rulebook_path = Rulebook::rulebook_path(self.workspace_root());
        let rulebook_text = fs::read_to_string(&rulebook_path)?;
        let rulebook = Rulebook::from_toml_str(&rulebook_text)?;
        rulebook.compile_for_root(self.workspace_root()).map_err(RuntimeError::from)
    }

    /// Load and compile a rulebook at one pinned commit.
    ///
    /// Used at merge time to recover the check pipeline from the
    /// rulebook that was current when the bidding group formed.
    pub(crate) fn load_compiled_rulebook(
        &self, commit: &CanonicalCommitHash,
    ) -> Result<CompiledRulebook, RuntimeError> {
        let rulebook_text = self.vcs().show_file_at_commit(
            self.workspace_root(),
            commit,
            Path::new(RULEBOOK_RELATIVE_PATH),
        )?;
        let files = self.vcs().list_files_at_commit(self.workspace_root(), commit)?;
        let rulebook = Rulebook::from_toml_str(&rulebook_text)?;
        rulebook.compile(&files).map_err(RuntimeError::from)
    }

    // -----------------------------------------------------------------------
    // Worker contract and boundary materialization
    // -----------------------------------------------------------------------

    /// Load the worker contract view from a worker worktree.
    pub(crate) fn load_worker_contract(
        &self, worktree_root: &Path,
    ) -> Result<WorkerContractView, RuntimeError> {
        let path = WorkerPaths::new(worktree_root.to_path_buf()).contract();
        if !path.exists() {
            return Err(RuntimeError::MissingWorkerRuntime(worktree_root.display().to_string()));
        }
        Self::read_toml(&path)
    }

    /// Prepare the worker-local runtime surface.
    pub(crate) fn prepare_worker_runtime(
        &self, worker: &WorkerEntry, group: &BiddingGroupRecord,
    ) -> Result<(), RuntimeError> {
        let worker_paths = WorkerPaths::new(worker.worktree_path.clone());

        fs::create_dir_all(worker_paths.inbox_new())?;
        fs::create_dir_all(worker_paths.inbox_ack())?;
        fs::create_dir_all(worker_paths.outbox_new())?;
        fs::create_dir_all(worker_paths.outbox_ack())?;

        self.write_worker_contract(worker, group)?;
        self.materialize_worker_boundary(worker, group)?;

        self.vcs().install_worker_runtime_support(worker_paths.worktree_root())?;
        Ok(())
    }

    /// Refresh the materialized boundary for one worker from its group.
    pub(crate) fn refresh_worker_boundary(
        &self, worker: &WorkerEntry, group: &BiddingGroupRecord,
    ) -> Result<(), RuntimeError> {
        self.materialize_worker_boundary(worker, group)
    }

    /// Refresh the worker contract after a base-forwarding operation.
    pub(crate) fn refresh_worker_contract(
        &self, worker: &WorkerEntry, group: &BiddingGroupRecord,
    ) -> Result<(), RuntimeError> {
        self.write_worker_contract(worker, group)
    }

    fn write_worker_contract(
        &self, worker: &WorkerEntry, group: &BiddingGroupRecord,
    ) -> Result<(), RuntimeError> {
        let worker_paths = WorkerPaths::new(worker.worktree_path.clone());
        let contract = WorkerContractView {
            worker_id: worker.worker_id.clone(),
            perspective: group.perspective.clone(),
            base_commit: group.base_commit.clone(),
            read_set_path: worker_paths.read_set(),
            write_set_path: worker_paths.write_set(),
        };
        Self::write_toml(&worker_paths.contract(), &contract)
    }

    fn materialize_worker_boundary(
        &self, worker: &WorkerEntry, group: &BiddingGroupRecord,
    ) -> Result<(), RuntimeError> {
        let worker_paths = WorkerPaths::new(worker.worktree_path.clone());
        Self::write_path_list(&worker_paths.read_set(), &group.read_set)?;
        Self::write_path_list(&worker_paths.write_set(), &group.write_set)?;
        tracing::trace!(
            worker_id = %worker.worker_id,
            perspective = %group.perspective,
            read_count = group.read_set.len(),
            write_count = group.write_set.len(),
            "materialized worker boundary"
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Exclusion set
    // -----------------------------------------------------------------------

    fn collect_exclusion_set(state: &StateFile) -> BTreeSet<PathBuf> {
        let mut exclusion = BTreeSet::<PathBuf>::new();
        for group in state.live_groups() {
            exclusion.extend(group.read_set.iter().cloned());
            exclusion.extend(group.write_set.iter().cloned());
        }
        exclusion
    }

    /// Recompute and persist the orchestrator exclusion set from group
    /// and worker state.
    ///
    /// The exclusion set is the union of every live bidding group's
    /// read and write sets. It must be rewritten after every persisted
    /// state update so the projection always matches runtime state.
    pub(crate) fn rewrite_exclusion_set(&self, state: &StateFile) -> Result<(), RuntimeError> {
        let exclusion = Self::collect_exclusion_set(state);
        let path = self.paths.orchestrator().exclusion_set();
        Self::write_path_list(&path, &exclusion)?;
        tracing::trace!(count = exclusion.len(), "rewrote orchestrator exclusion set");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Audit
    // -----------------------------------------------------------------------

    /// Stage the runtime artifacts that must become visible when a merge
    /// transaction commits.
    ///
    /// Note: This prepares audit state, persisted group/worker state,
    /// and the exclusion set before canonical integration starts.
    /// Promotion still happens later, after the worker patch has been
    /// applied in no-commit mode.
    ///
    /// Note: Merge staging is allocated with `tempfile` in the system
    /// temp directory so transient artifacts do not pollute
    /// `.multorum/orchestrator/`.
    pub(crate) fn prepare_merge_artifacts(
        &self, updated_state: &StateFile, worker: &WorkerEntry, group: &BiddingGroupRecord,
        verification: &MergeVerification, payload: BundlePayload,
    ) -> Result<StagedMergeArtifacts, RuntimeError> {
        let MergeVerification { head_commit, changed_files, ran_checks, skipped_checks } =
            verification;
        payload.validate()?;

        fs::create_dir_all(self.paths.audit())?;
        let orchestrator_paths = self.paths.orchestrator();
        let staging_dir = tempfile::Builder::new()
            .prefix(&format!("multorum-merge-{}-", worker.worker_id.as_str()))
            .tempdir()?;
        let staging_root = staging_dir.path();

        let staged_group_root = staging_root.join("group");
        let staged_worker_root = staging_root.join("worker");
        self.write_state_snapshot(&staged_group_root, &staged_worker_root, updated_state.clone())?;

        let staged_exclusion_path = staging_root.join("exclusion-set.txt");
        let exclusion = Self::collect_exclusion_set(updated_state);
        Self::write_path_list(&staged_exclusion_path, &exclusion)?;

        let audit_entry_id = self.paths.audit_entry_id(&worker.worker_id, head_commit)?;
        let final_audit_root = self.paths.audit_entry_root(&audit_entry_id);
        let final_audit_entry = self.paths.audit_entry(&audit_entry_id);
        if final_audit_root.exists() {
            return Err(RuntimeError::CheckFailed(format!(
                "audit entry id already exists: {audit_entry_id}"
            )));
        }
        let final_bundle_root = final_audit_root.clone();
        let mut rationale_body = None;
        let mut rationale_artifacts = Vec::new();
        let mut promotions = Vec::new();

        if !payload.is_empty() {
            let staged_bundle_root = staging_root.join("audit-bundle");
            fs::create_dir_all(&staged_bundle_root)?;
            let written = BundleWriter::write(&staged_bundle_root, payload)?;
            if written.body_path.is_some() {
                rationale_body = Some(final_bundle_root.join(BODY_FILE_NAME));
            }
            rationale_artifacts = written
                .artifact_paths
                .iter()
                .map(|path| final_bundle_root.join("artifacts").join(path.file_name().unwrap()))
                .collect();
            promotions.push(PreparedPromotion::new(
                staged_bundle_root,
                final_bundle_root,
                staging_root,
                "backup-audit-bundle",
            ));
        }

        let entry = AuditEntry {
            worker_id: worker.worker_id.clone(),
            perspective: group.perspective.clone(),
            base_commit: group.base_commit.clone(),
            head_commit: head_commit.clone(),
            changed_files: changed_files.iter().cloned().collect(),
            ran_checks: ran_checks.to_vec(),
            skipped_checks: skipped_checks.to_vec(),
            merged_at: Timestamp::now(),
            rationale_body,
            rationale_artifacts,
        };
        let staged_audit_entry = staging_root.join("audit-entry.toml");
        Self::write_toml(&staged_audit_entry, &entry)?;

        promotions.push(PreparedPromotion::new(
            staged_audit_entry,
            final_audit_entry,
            staging_root,
            "backup-audit-entry.toml",
        ));
        promotions.push(PreparedPromotion::new(
            staged_group_root,
            orchestrator_paths.group_root(),
            staging_root,
            "backup-group",
        ));
        promotions.push(PreparedPromotion::new(
            staged_worker_root,
            orchestrator_paths.worker_root(),
            staging_root,
            "backup-worker",
        ));
        promotions.push(PreparedPromotion::new(
            staged_exclusion_path,
            orchestrator_paths.exclusion_set(),
            staging_root,
            "backup-exclusion-set.txt",
        ));

        Ok(StagedMergeArtifacts { promotions, staging_dir: Some(staging_dir) })
    }

    fn ensure_multorum_gitignore(&self) -> Result<Vec<&'static str>, RuntimeError> {
        let gitignore_path = self.paths.multorum_gitignore();
        let existed = gitignore_path.exists();
        let mut added_entries = Vec::new();
        let mut lines = if existed {
            fs::read_to_string(&gitignore_path)?.lines().map(str::to_owned).collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        for entry in MULTORUM_GITIGNORE_ENTRIES {
            if !lines.iter().any(|line| line == entry) {
                lines.push(entry.to_owned());
                if existed {
                    added_entries.push(entry);
                }
            }
        }

        fs::write(gitignore_path, lines.join("\n") + "\n")?;
        Ok(added_entries)
    }

    // -----------------------------------------------------------------------
    // Mailbox persistence
    // -----------------------------------------------------------------------

    /// Publish a mailbox bundle and transfer any path-backed payloads.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn publish_bundle(
        &self, worktree_root: &Path, direction: MailboxDirection, kind: MessageKind,
        worker_id: &WorkerId, perspective: &crate::schema::perspective::PerspectiveName,
        reply: ReplyReference, head_commit: Option<CanonicalCommitHash>, payload: BundlePayload,
    ) -> Result<PublishedBundle, RuntimeError> {
        let worker_paths = WorkerPaths::new(worktree_root.to_path_buf());
        let new_root = worker_paths.mailbox_new(direction);
        fs::create_dir_all(&new_root)?;

        let sequence = self.next_sequence(&new_root)?;
        let temp_dir = new_root.join(format!(".tmp-{}-{}", kind.slug(), sequence.0));
        let final_dir = new_root.join(format!("{:04}-{}", sequence.0, kind.slug()));

        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir)?;
        }

        fs::create_dir_all(&temp_dir)?;

        let envelope = BundleEnvelope {
            protocol: PROTOCOL_VERSION,
            worker_id: worker_id.clone(),
            perspective: perspective.clone(),
            kind,
            sequence,
            created_at: Timestamp::now(),
            in_reply_to: reply.in_reply_to,
            head_commit,
        };
        Self::write_toml(&temp_dir.join(ENVELOPE_FILE_NAME), &envelope)?;

        // Mailbox bundles always contain a body.md, even if empty.
        let written = BundleWriter::write(&temp_dir, payload)?;
        if written.body_path.is_none() {
            fs::write(temp_dir.join(BODY_FILE_NAME), "")?;
        }

        fs::rename(&temp_dir, &final_dir)?;

        Ok(PublishedBundle {
            message: MessageRef { worker_id: worker_id.clone(), kind, sequence },
            bundle_path: final_dir,
        })
    }

    /// Read mailbox bundles matching a sequence filter.
    ///
    /// When `include_body` is true, each returned view includes the full
    /// `body.md` content. Otherwise only the first-line summary is set.
    pub(crate) fn list_mailbox_messages(
        &self, worktree_root: &Path, worker_id: &WorkerId, direction: MailboxDirection,
        filter: SequenceFilter, include_body: bool,
    ) -> Result<Vec<MailboxMessageView>, RuntimeError> {
        let worker_paths = WorkerPaths::new(worktree_root.to_path_buf());
        let new_root = worker_paths.mailbox_new(direction);
        let ack_root = worker_paths.mailbox_ack(direction);
        if !new_root.exists() {
            return Ok(Vec::new());
        }

        let mut messages = Vec::new();
        for entry in fs::read_dir(&new_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let bundle_path = entry.path();
            if bundle_path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".tmp-"))
            {
                continue;
            }

            let envelope: BundleEnvelope = Self::read_toml(&bundle_path.join(ENVELOPE_FILE_NAME))?;
            if !filter.matches(envelope.sequence) {
                continue;
            }

            let acknowledged = ack_root.join(Self::ack_file_name(envelope.sequence)).exists();
            let body = if include_body {
                fs::read_to_string(bundle_path.join(BODY_FILE_NAME)).ok()
            } else {
                None
            };
            messages.push(MailboxMessageView {
                worker_id: worker_id.clone(),
                perspective: envelope.perspective.clone(),
                direction,
                kind: envelope.kind,
                sequence: envelope.sequence,
                created_at: envelope.created_at,
                acknowledged,
                head_commit: envelope.head_commit.clone(),
                summary: self.bundle_summary(&bundle_path, &envelope.kind),
                body,
                bundle_path,
            });
        }

        messages.sort_by_key(|message| message.sequence);
        Ok(messages)
    }

    /// Acknowledge one inbox or outbox bundle.
    pub(crate) fn acknowledge_message(
        &self, worktree_root: &Path, direction: MailboxDirection, sequence: Sequence,
    ) -> Result<AckRef, RuntimeError> {
        let worker_paths = WorkerPaths::new(worktree_root.to_path_buf());
        let bundle_dir =
            self.find_bundle_by_sequence(&worker_paths.mailbox_new(direction), sequence)?;
        let envelope: BundleEnvelope = Self::read_toml(&bundle_dir.join(ENVELOPE_FILE_NAME))?;
        let ack_root = worker_paths.mailbox_ack(direction);
        fs::create_dir_all(&ack_root)?;
        let ack_path = ack_root.join(Self::ack_file_name(sequence));
        if ack_path.exists() {
            return Err(RuntimeError::AlreadyAcknowledged);
        }

        let ack = AckRecord { sequence, acknowledged_at: Timestamp::now() };
        let mut file = OpenOptions::new().write(true).create_new(true).open(&ack_path)?;
        file.write_all(toml::to_string(&ack)?.as_bytes())?;

        Ok(AckRef {
            message: MessageRef { worker_id: envelope.worker_id, kind: envelope.kind, sequence },
        })
    }

    fn next_sequence(&self, new_root: &Path) -> Result<Sequence, RuntimeError> {
        let mut max = 0_u64;
        if new_root.exists() {
            for entry in fs::read_dir(new_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy().to_string();
                let Some(prefix) = name.split('-').next() else {
                    continue;
                };
                if let Ok(parsed) = prefix.parse::<u64>() {
                    max = max.max(parsed);
                }
            }
        }
        Ok(Sequence(max + 1))
    }

    fn bundle_summary(&self, bundle_path: &Path, kind: &MessageKind) -> String {
        let body_path = bundle_path.join(BODY_FILE_NAME);
        if let Ok(body) = fs::read_to_string(body_path)
            && let Some(line) = body.lines().map(str::trim).find(|line| !line.is_empty())
        {
            return line.to_owned();
        }
        kind.slug().to_owned()
    }

    fn find_bundle_by_sequence(
        &self, new_root: &Path, sequence: Sequence,
    ) -> Result<PathBuf, RuntimeError> {
        if !new_root.exists() {
            return Err(RuntimeError::MessageNotFound);
        }

        for entry in fs::read_dir(new_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let bundle_path = entry.path();
            let envelope_path = bundle_path.join(ENVELOPE_FILE_NAME);
            if !envelope_path.exists() {
                continue;
            }
            let envelope: BundleEnvelope = Self::read_toml(&envelope_path)?;
            if envelope.sequence == sequence {
                return Ok(bundle_path);
            }
        }

        Err(RuntimeError::MessageNotFound)
    }

    /// The acknowledgement file name for one sequence number.
    fn ack_file_name(sequence: Sequence) -> String {
        format!("{:04}.{}", sequence.0, ACK_EXTENSION)
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Validate that a skip request only targets skippable declared checks.
///
/// Checks omitted from the optional `[check.policy]` table default to
/// `always`, so skip requests for them are rejected here.
pub(super) fn validate_skip_request(
    rulebook: &CompiledRulebook, skip_checks: &[String],
) -> Result<BTreeSet<CheckName>, RuntimeError> {
    let mut accepted = BTreeSet::new();
    for requested in skip_checks {
        let name = CheckName::new(requested)
            .map_err(|_| RuntimeError::CheckFailed(format!("unknown check `{requested}`")))?;
        let Some(decl) = rulebook.check().get(&name) else {
            return Err(RuntimeError::CheckFailed(format!("unknown check `{requested}`")));
        };
        if decl.policy() != CheckPolicy::Skippable {
            return Err(RuntimeError::CheckFailed(format!("check `{requested}` is not skippable")));
        }
        accepted.insert(name);
    }
    Ok(accepted)
}
