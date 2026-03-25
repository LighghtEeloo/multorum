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
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::bundle::{BODY_FILE_NAME, BundlePayload, BundleWriter};
use crate::runtime::{
    AuditEntry, MailboxMessageView, MultorumPaths, RulebookInit, RuntimeError, WorkerContractView,
    WorkerId, WorkerPaths,
    mailbox::{
        AckRef, BundleEnvelope, MailboxDirection, MessageKind, MessageRef, PublishedBundle,
        ReplyReference, Sequence,
    },
};
use crate::schema::perspective::CompiledPerspective;
use crate::schema::rulebook::{
    CheckName, CheckPolicy, CompiledRulebook, RULEBOOK_RELATIVE_PATH, Rulebook,
};
use crate::vcs::{CanonicalCommitHash, GitVcs, VersionControl};

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// Active rulebook projection stored under `.multorum/orchestrator/`.
///
/// The rulebook is always the one committed at `base_commit`. There is no
/// separate rulebook pin — the repository-wide rulebook is consistent with
/// the pinned base snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActiveRulebookRecord {
    /// Canonical commit pinning both the active rulebook and the base
    /// snapshot for newly created workers.
    pub base_commit: CanonicalCommitHash,
    /// Activation timestamp.
    pub activated_at: String,
}

/// Orchestrator-local projection for one live or historical worker.
///
/// `base_commit` pins the worker's code snapshot. The materialized
/// read/write-set files remain the authoritative worker boundary and may
/// be expanded by a later compatible rulebook install without changing
/// `base_commit`. The base pin changes only when the orchestrator
/// explicitly forwards the whole bidding group for this perspective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkerRecord {
    /// Unique worker identity.
    pub worker_id: WorkerId,
    /// Perspective currently held by the worker.
    pub perspective: crate::schema::perspective::PerspectiveName,
    /// Current lifecycle state.
    pub state: crate::runtime::WorkerState,
    /// Absolute path to the managed worktree.
    pub worktree_path: PathBuf,
    /// Canonical base commit pinning the worker's code snapshot.
    pub base_commit: CanonicalCommitHash,
    /// Canonical submitted worker commit when the worker is in `COMMITTED`.
    pub submitted_head_commit: Option<CanonicalCommitHash>,
}

/// Acknowledgement metadata written to mailbox `ack/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AckRecord {
    /// The acknowledged mailbox sequence number.
    pub(crate) sequence: Sequence,
    /// Monotonic timestamp recorded when the acknowledgement was written.
    pub(crate) acknowledged_at: String,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Protocol version written into persisted mailbox envelopes.
const PROTOCOL_VERSION: u32 = 1;

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

    pub(crate) fn read_path_list(path: &Path) -> Result<BTreeSet<PathBuf>, RuntimeError> {
        if !path.exists() {
            return Ok(BTreeSet::new());
        }
        Ok(fs::read_to_string(path)?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(PathBuf::from)
            .collect())
    }

    // -----------------------------------------------------------------------
    // Rulebook state
    // -----------------------------------------------------------------------

    /// Initialize the committed `.multorum/` project surface.
    pub(crate) fn initialize_rulebook(&self) -> Result<RulebookInit, RuntimeError> {
        let multorum_root = self.paths.multorum_root();
        let gitignore_path = self.paths.multorum_gitignore();
        let rulebook_path = Rulebook::rulebook_path(self.workspace_root());

        if rulebook_path.exists() {
            return Err(RuntimeError::RulebookExists(rulebook_path));
        }

        fs::create_dir_all(&multorum_root)?;
        fs::create_dir_all(self.paths.orchestrator().root())?;
        fs::create_dir_all(multorum_root.join("tr"))?;

        self.ensure_multorum_gitignore()?;
        fs::write(&rulebook_path, Rulebook::default_template())?;
        tracing::info!(
            multorum_root = %multorum_root.display(),
            rulebook_path = %rulebook_path.display(),
            gitignore_path = %gitignore_path.display(),
            "initialized rulebook workspace"
        );

        Ok(RulebookInit { multorum_root, rulebook_path, gitignore_path })
    }

    /// Load the active rulebook projection.
    pub(crate) fn load_active_rulebook(&self) -> Result<ActiveRulebookRecord, RuntimeError> {
        let path = self.paths.orchestrator().active_rulebook();
        if !path.exists() {
            return Err(RuntimeError::MissingActiveRulebook);
        }
        Self::read_toml(&path)
    }

    /// Persist the active rulebook projection.
    pub(crate) fn store_active_rulebook(
        &self, record: &ActiveRulebookRecord,
    ) -> Result<(), RuntimeError> {
        let orchestrator = self.paths.orchestrator();
        let orchestrator_root = orchestrator.root();
        fs::create_dir_all(orchestrator.workers_dir())?;
        fs::create_dir_all(orchestrator_root.join("audit"))?;
        Self::write_toml(&orchestrator.active_rulebook(), record)
    }

    /// Remove the active rulebook projection.
    pub(crate) fn remove_active_rulebook(&self) -> Result<(), RuntimeError> {
        let path = self.paths.orchestrator().active_rulebook();
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Load and compile a rulebook at one pinned commit.
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

    /// Load the active rulebook projection and its compiled rulebook.
    pub(crate) fn load_active_compiled_rulebook(
        &self,
    ) -> Result<(ActiveRulebookRecord, CompiledRulebook), RuntimeError> {
        let active = self.load_active_rulebook()?;
        let compiled = self.load_compiled_rulebook(&active.base_commit)?;
        Ok((active, compiled))
    }

    // -----------------------------------------------------------------------
    // Worker records
    // -----------------------------------------------------------------------

    /// Load one worker projection.
    pub(crate) fn load_worker_record(
        &self, worker_id: &WorkerId,
    ) -> Result<WorkerRecord, RuntimeError> {
        let path = self.paths.orchestrator().worker_state(worker_id);
        if !path.exists() {
            return Err(RuntimeError::UnknownWorker(worker_id.to_string()));
        }
        Self::read_toml(&path)
    }

    /// Persist one worker projection.
    pub(crate) fn store_worker_record(&self, record: &WorkerRecord) -> Result<(), RuntimeError> {
        let path = self.paths.orchestrator().worker_state(&record.worker_id);
        Self::write_toml(&path, record)
    }

    /// Delete one worker projection file.
    pub(crate) fn delete_worker_record(&self, worker_id: &WorkerId) -> Result<bool, RuntimeError> {
        let path = self.paths.orchestrator().worker_state(worker_id);
        if path.exists() {
            fs::remove_file(&path)?;
            tracing::trace!(path = %path.display(), "deleted worker state file");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Return all known worker projections.
    pub(crate) fn list_worker_records(&self) -> Result<Vec<WorkerRecord>, RuntimeError> {
        let workers_root = self.paths.orchestrator().workers_dir();
        if !workers_root.exists() {
            return Ok(Vec::new());
        }

        let mut workers = Vec::new();
        for entry in fs::read_dir(&workers_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(extension) = path.extension() else {
                continue;
            };
            if extension == "toml" {
                workers.push(Self::read_toml(&path)?);
            }
        }
        workers.sort_by(|left: &WorkerRecord, right: &WorkerRecord| {
            left.worker_id.cmp(&right.worker_id)
        });
        Ok(workers)
    }

    // -----------------------------------------------------------------------
    // Worker contract and boundary materialization
    // -----------------------------------------------------------------------

    /// Load the worker contract view from a worker worktree.
    ///
    /// The contract file pins worker identity and base snapshot. The
    /// referenced read/write-set files remain separately materialized so
    /// `rulebook install` may expand them for a live worker without
    /// rewriting the stable contract metadata. The contract file itself
    /// changes only when a whole bidding group is explicitly forwarded.
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
        &self, record: &WorkerRecord, perspective: &CompiledPerspective,
    ) -> Result<(), RuntimeError> {
        let worker_paths = WorkerPaths::new(record.worktree_path.clone());

        fs::create_dir_all(worker_paths.inbox_new())?;
        fs::create_dir_all(worker_paths.inbox_ack())?;
        fs::create_dir_all(worker_paths.outbox_new())?;
        fs::create_dir_all(worker_paths.outbox_ack())?;
        fs::create_dir_all(worker_paths.artifacts())?;

        self.write_worker_contract(record)?;
        self.materialize_worker_boundary(record, perspective)?;

        self.vcs().install_worker_runtime_support(worker_paths.worktree_root())?;
        Ok(())
    }

    /// Refresh the materialized boundary for one live worker.
    ///
    /// This rewrites only the read/write-set files. The worker keeps its
    /// pinned base snapshot and runtime identity.
    pub(crate) fn refresh_worker_boundary(
        &self, record: &WorkerRecord, perspective: &CompiledPerspective,
    ) -> Result<(), RuntimeError> {
        self.materialize_worker_boundary(record, perspective)
    }

    /// Refresh the worker contract after one explicit base-forwarding
    /// operation.
    pub(crate) fn refresh_worker_contract(
        &self, record: &WorkerRecord,
    ) -> Result<(), RuntimeError> {
        self.write_worker_contract(record)
    }

    fn write_worker_contract(&self, record: &WorkerRecord) -> Result<(), RuntimeError> {
        let worker_paths = WorkerPaths::new(record.worktree_path.clone());
        let contract = WorkerContractView {
            worker_id: record.worker_id.clone(),
            perspective: record.perspective.clone(),
            base_commit: record.base_commit.clone(),
            read_set_path: worker_paths.read_set(),
            write_set_path: worker_paths.write_set(),
        };
        Self::write_toml(&worker_paths.contract(), &contract)
    }

    fn materialize_worker_boundary(
        &self, record: &WorkerRecord, perspective: &CompiledPerspective,
    ) -> Result<(), RuntimeError> {
        let worker_paths = WorkerPaths::new(record.worktree_path.clone());
        Self::write_path_list(&worker_paths.read_set(), perspective.read())?;
        Self::write_path_list(&worker_paths.write_set(), perspective.write())?;
        tracing::trace!(
            worker_id = %record.worker_id,
            perspective = %record.perspective,
            read_count = perspective.read().len(),
            write_count = perspective.write().len(),
            "materialized worker boundary"
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Exclusion set
    // -----------------------------------------------------------------------

    /// Recompute and persist the orchestrator exclusion set.
    ///
    /// The exclusion set is the union of every active bidding group's
    /// read and write sets. It must be rewritten after any lifecycle
    /// transition that changes the set of active workers (create,
    /// merge, discard).
    pub(crate) fn rewrite_exclusion_set(&self) -> Result<(), RuntimeError> {
        let mut exclusion = BTreeSet::<PathBuf>::new();
        for record in self.list_worker_records()? {
            if !record.state.is_live() {
                continue;
            }
            let worker_paths = self.worker_paths(&record.worker_id);
            exclusion.extend(Self::read_path_list(&worker_paths.read_set())?);
            exclusion.extend(Self::read_path_list(&worker_paths.write_set())?);
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
        &self, record: &WorkerRecord, head_commit: &CanonicalCommitHash,
        changed_files: &BTreeSet<PathBuf>, ran_checks: &[String], skipped_checks: &[String],
        payload: BundlePayload,
    ) -> Result<(), RuntimeError> {
        let audit_dir = self.paths.orchestrator().audit();
        let entry_dir = audit_dir.join(record.worker_id.as_str());
        fs::create_dir_all(&entry_dir)?;

        let (rationale_body, rationale_artifacts) = if !payload.is_empty() {
            let written = BundleWriter::write(&entry_dir, payload)?;
            (written.body_path, written.artifact_paths)
        } else {
            (None, Vec::new())
        };

        let entry = AuditEntry {
            worker_id: record.worker_id.clone(),
            perspective: record.perspective.clone(),
            base_commit: record.base_commit.clone(),
            head_commit: head_commit.clone(),
            changed_files: changed_files.iter().cloned().collect(),
            ran_checks: ran_checks.to_vec(),
            skipped_checks: skipped_checks.to_vec(),
            merged_at: timestamp_now(),
            rationale_body,
            rationale_artifacts,
        };
        Self::write_toml(&self.paths.orchestrator().audit_entry(&record.worker_id), &entry)?;
        tracing::info!(worker_id = %record.worker_id, "wrote audit entry");
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
            created_at: timestamp_now(),
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
                created_at: envelope.created_at.clone(),
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

        let ack = AckRecord { sequence, acknowledged_at: timestamp_now() };
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

/// Return a monotonic string timestamp.
pub(crate) fn timestamp_now() -> String {
    let now =
        SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after unix epoch");
    format!("{}.{}", now.as_secs(), now.subsec_nanos())
}
