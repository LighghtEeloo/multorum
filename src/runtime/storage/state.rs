//! Persisted state and local runtime materialization for the storage runtime.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

use crate::bundle::{BundlePayload, BundleWriter};
use crate::runtime::{
    AuditEntry, RulebookInit, RuntimeError, WorkerContractView, WorkerId, WorkerPaths,
};
use crate::schema::perspective::CompiledPerspective;
use crate::schema::rulebook::{CompiledRulebook, RULEBOOK_RELATIVE_PATH, Rulebook};
use crate::vcs::CanonicalCommitHash;

use super::{ActiveRulebookRecord, RuntimeFs, WorkerRecord};

const MULTORUM_GITIGNORE_ENTRIES: [&str; 2] = ["orchestrator/", "tr/"];

impl RuntimeFs {
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

    /// Load the worker contract view from a worker worktree.
    ///
    /// The contract file pins worker identity and base snapshot. The
    /// referenced read/write-set files remain separately materialized so
    /// `rulebook install` may expand them for a live worker without
    /// rewriting the stable contract metadata.
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

        let contract = WorkerContractView {
            worker_id: record.worker_id.clone(),
            perspective: record.perspective.clone(),
            base_commit: record.base_commit.clone(),
            read_set_path: worker_paths.read_set(),
            write_set_path: worker_paths.write_set(),
        };
        Self::write_toml(&worker_paths.contract(), &contract)?;
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

    /// Recompute and persist the orchestrator exclusion set.
    ///
    /// The exclusion set is the union of every active bidding group's
    /// read and write sets. It must be rewritten after any lifecycle
    /// transition that changes the set of active workers (create,
    /// merge, discard).
    pub(crate) fn rewrite_exclusion_set(&self) -> Result<(), RuntimeError> {
        let mut exclusion = BTreeSet::<PathBuf>::new();
        for record in self.list_worker_records()? {
            if !super::is_live_worker_state(record.state) {
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
            merged_at: super::timestamp_now(),
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
}
