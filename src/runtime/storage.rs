//! Storage helpers shared by the runtime entry points.
//!
//! The runtime model is intentionally filesystem-first: `.multorum/`
//! stores the authoritative control plane, worker contract, compiled
//! file sets, and mailbox bundles. This module centralizes that on-disk
//! layout and the small amount of version-control orchestration needed
//! to create worktrees, delete finalized workspaces, and integrate
//! submitted commits.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use super::timestamp::Timestamp;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::bundle::{BODY_FILE_NAME, BundlePayload, BundleWriter};
use crate::runtime::{
    AuditEntry, MailboxMessageView, MultorumPaths, RulebookInit, RuntimeError, WorkerContractView,
    WorkerId, WorkerPaths,
    mailbox::{
        AckRef, BundleEnvelope, MailboxDirection, MessageKind, MessageRef, ProtocolVersion,
        PublishedBundle, ReplyReference, Sequence,
    },
};
use crate::schema::rulebook::{
    CheckName, CheckPolicy, CompiledRulebook, RULEBOOK_RELATIVE_PATH, Rulebook,
};
use crate::vcs::{CanonicalCommitHash, GitVcs, VersionControl};

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// Orchestrator runtime state stored at `.multorum/orchestrator/state.toml`.
///
/// This is the single source of truth for all bidding groups and workers.
/// Each group entry carries the perspective name, base commit, and compiled
/// boundary. Each worker entry within a group carries the worker,
/// lifecycle state, and submitted head commit where applicable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StateFile {
    /// Bidding groups, each containing its member workers.
    #[serde(default)]
    pub groups: Vec<BiddingGroupRecord>,
}

/// One bidding group with its compiled boundary and member workers.
///
/// A group forms when the first worker for a perspective is created.
/// Its base commit and boundary are locked at formation. Subsequent
/// workers for the same perspective join the existing group and share
/// its base commit and boundary.
///
/// Note: When the group has no live workers left (all members are
/// `MERGED` or `DISCARDED`), Multorum clears the materialized boundary.
/// The historical group membership stays in `state.toml` until
/// `worker delete` removes the final member record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkerEntry {
    /// Unique worker identity.
    #[serde(rename = "worker")]
    pub worker_id: WorkerId,
    /// Current lifecycle state.
    pub state: crate::runtime::WorkerState,
    /// Absolute path to the managed worktree.
    pub worktree_path: PathBuf,
    /// Canonical submitted worker commit when the worker is in `COMMITTED`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_head_commit: Option<CanonicalCommitHash>,
}

impl StateFile {
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

    // -----------------------------------------------------------------------
    // Rulebook initialization
    // -----------------------------------------------------------------------

    /// Initialize the committed `.multorum/` project surface.
    pub(crate) fn initialize_rulebook(&self) -> Result<RulebookInit, RuntimeError> {
        let multorum_root = self.paths.multorum_root();
        let gitignore_path = self.paths.multorum_gitignore();
        let rulebook_path = Rulebook::rulebook_path(self.workspace_root());
        let orchestrator_paths = self.paths.orchestrator();

        if rulebook_path.exists() {
            return Err(RuntimeError::RulebookExists(rulebook_path));
        }

        fs::create_dir_all(&multorum_root)?;
        fs::create_dir_all(orchestrator_paths.root())?;
        fs::create_dir_all(orchestrator_paths.audit())?;
        fs::create_dir_all(multorum_root.join("tr"))?;

        self.ensure_multorum_gitignore()?;
        fs::write(&rulebook_path, Rulebook::default_template())?;
        let state = StateFile::default();
        self.store_state(&state)?;
        self.rewrite_exclusion_set(&state)?;
        tracing::info!(
            multorum_root = %multorum_root.display(),
            rulebook_path = %rulebook_path.display(),
            gitignore_path = %gitignore_path.display(),
            "initialized rulebook workspace"
        );

        Ok(RulebookInit { multorum_root, rulebook_path, gitignore_path })
    }

    // -----------------------------------------------------------------------
    // State file
    // -----------------------------------------------------------------------

    /// Load the orchestrator state file.
    pub(crate) fn load_state(&self) -> Result<StateFile, RuntimeError> {
        let path = self.paths.orchestrator().state();
        if !path.exists() {
            return Err(RuntimeError::MissingOrchestratorState);
        }
        Self::read_toml(&path)
    }

    /// Persist the orchestrator state file.
    pub(crate) fn store_state(&self, state: &StateFile) -> Result<(), RuntimeError> {
        let path = self.paths.orchestrator().state();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Self::write_toml(&path, state)
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
        fs::create_dir_all(worker_paths.artifacts())?;

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

    /// Recompute and persist the orchestrator exclusion set from `state.toml`.
    ///
    /// The exclusion set is the union of every live bidding group's
    /// read and write sets. It must be rewritten after every persisted
    /// `state.toml` update so the projection always matches runtime state.
    pub(crate) fn rewrite_exclusion_set(&self, state: &StateFile) -> Result<(), RuntimeError> {
        let mut exclusion = BTreeSet::<PathBuf>::new();
        for group in state.live_groups() {
            exclusion.extend(group.read_set.iter().cloned());
            exclusion.extend(group.write_set.iter().cloned());
        }
        let path = self.paths.orchestrator().exclusion_set();
        Self::write_path_list(&path, &exclusion)?;
        tracing::trace!(count = exclusion.len(), "rewrote orchestrator exclusion set");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Audit
    // -----------------------------------------------------------------------

    /// Write an audit entry for a successfully merged worker.
    ///
    /// The rationale payload body and artifacts are moved into the audit
    /// directory alongside the TOML entry, using the same bundle I/O as
    /// mailbox publishing.
    pub(crate) fn write_audit_entry(
        &self, worker: &WorkerEntry, group: &BiddingGroupRecord, head_commit: &CanonicalCommitHash,
        changed_files: &BTreeSet<PathBuf>, ran_checks: &[String], skipped_checks: &[String],
        payload: BundlePayload,
    ) -> Result<(), RuntimeError> {
        let audit_dir = self.paths.orchestrator().audit();
        let entry_dir = audit_dir.join(worker.worker_id.as_str());
        fs::create_dir_all(&entry_dir)?;

        let (rationale_body, rationale_artifacts) = if !payload.is_empty() {
            let written = BundleWriter::write(&entry_dir, payload)?;
            (written.body_path, written.artifact_paths)
        } else {
            (None, Vec::new())
        };

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
        Self::write_toml(&self.paths.orchestrator().audit_entry(&worker.worker_id), &entry)?;
        tracing::info!(worker_id = %worker.worker_id, "wrote audit entry");
        Ok(())
    }

    fn ensure_multorum_gitignore(&self) -> Result<(), RuntimeError> {
        let gitignore_path = self.paths.multorum_gitignore();
        let mut lines = if gitignore_path.exists() {
            fs::read_to_string(&gitignore_path)?.lines().map(str::to_owned).collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        for entry in MULTORUM_GITIGNORE_ENTRIES {
            if !lines.iter().any(|line| line == entry) {
                lines.push(entry.to_owned());
            }
        }

        fs::write(gitignore_path, lines.join("\n") + "\n")?;
        Ok(())
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

    /// Read mailbox bundles after an optional sequence threshold.
    pub(crate) fn list_mailbox_messages(
        &self, worktree_root: &Path, worker_id: &WorkerId, direction: MailboxDirection,
        after: Option<Sequence>,
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
            if let Some(after) = after
                && envelope.sequence <= after
            {
                continue;
            }

            let acknowledged = ack_root.join(Self::ack_file_name(envelope.sequence)).exists();
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

