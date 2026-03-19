//! Persisted state and local runtime materialization for the filesystem runtime.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

use crate::perspective::{CompiledPerspective, PerspectiveName};
use crate::rulebook::{CompiledRulebook, Rulebook};
use crate::runtime::{RuntimeError, WorkerContractView};

use super::{ActiveRulebookRecord, RuntimeFileSystem, STATE_FILE_NAME, WorkerRecord};

impl RuntimeFileSystem {
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
        fs::create_dir_all(orchestrator_root.join("workers"))?;
        fs::create_dir_all(orchestrator_root.join("audit"))?;
        Self::write_toml(&orchestrator.active_rulebook(), record)
    }

    /// Load and compile a rulebook at one git commit.
    pub(crate) fn load_compiled_rulebook(
        &self, commit: &str,
    ) -> Result<CompiledRulebook, RuntimeError> {
        let rulebook_text = self.git_show_rulebook(commit)?;
        let files = self.git_list_files(commit)?;
        let rulebook = Rulebook::from_toml_str(&rulebook_text)?;
        rulebook.compile(&files).map_err(RuntimeError::from)
    }

    /// Load the active rulebook projection and its compiled rulebook.
    pub(crate) fn load_active_compiled_rulebook(
        &self,
    ) -> Result<(ActiveRulebookRecord, CompiledRulebook), RuntimeError> {
        let active = self.load_active_rulebook()?;
        let compiled = self.load_compiled_rulebook(&active.rulebook_commit)?;
        Ok((active, compiled))
    }

    /// Load one worker projection.
    pub(crate) fn load_worker_record(
        &self, perspective: &PerspectiveName,
    ) -> Result<WorkerRecord, RuntimeError> {
        let path = self.paths.orchestrator().worker_state(perspective);
        if !path.exists() {
            return Err(RuntimeError::UnknownPerspective(perspective.to_string()));
        }
        Self::read_toml(&path)
    }

    /// Persist one worker projection.
    pub(crate) fn store_worker_record(&self, record: &WorkerRecord) -> Result<(), RuntimeError> {
        let dir = self.paths.orchestrator().worker(&record.perspective);
        fs::create_dir_all(&dir)?;
        Self::write_toml(&dir.join(STATE_FILE_NAME), record)
    }

    /// Return all known worker projections.
    pub(crate) fn list_worker_records(&self) -> Result<Vec<WorkerRecord>, RuntimeError> {
        let workers_root = self.paths.orchestrator().workers();
        if !workers_root.exists() {
            return Ok(Vec::new());
        }

        let mut workers = Vec::new();
        for entry in fs::read_dir(&workers_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let state_path = entry.path().join(STATE_FILE_NAME);
            if state_path.exists() {
                workers.push(Self::read_toml(&state_path)?);
            }
        }
        workers.sort_by(|left: &WorkerRecord, right: &WorkerRecord| {
            left.perspective.cmp(&right.perspective)
        });
        Ok(workers)
    }

    /// Load the immutable worker contract from a provisioned worktree.
    pub(crate) fn load_worker_contract(
        &self, worktree_root: &Path,
    ) -> Result<WorkerContractView, RuntimeError> {
        let path = crate::runtime::WorkerPaths::new(worktree_root.to_path_buf()).contract();
        if !path.exists() {
            return Err(RuntimeError::MissingWorkerRuntime(worktree_root.display().to_string()));
        }
        Self::read_toml(&path)
    }

    /// Prepare the worker-local runtime surface.
    pub(crate) fn prepare_worker_runtime(
        &self, record: &WorkerRecord, perspective: &CompiledPerspective,
    ) -> Result<(), RuntimeError> {
        let worker_paths = crate::runtime::WorkerPaths::new(record.worktree_path.clone());

        fs::create_dir_all(worker_paths.inbox_new())?;
        fs::create_dir_all(worker_paths.inbox_ack())?;
        fs::create_dir_all(worker_paths.outbox_new())?;
        fs::create_dir_all(worker_paths.outbox_ack())?;
        fs::create_dir_all(worker_paths.artifacts())?;

        let contract = WorkerContractView {
            perspective: record.perspective.clone(),
            rulebook_commit: record.rulebook_commit.clone(),
            base_commit: record.base_commit.clone(),
            read_set_path: worker_paths.read_set(),
            write_set_path: worker_paths.write_set(),
        };
        Self::write_toml(&worker_paths.contract(), &contract)?;
        Self::write_path_list(&worker_paths.read_set(), perspective.read())?;
        Self::write_path_list(&worker_paths.write_set(), perspective.write())?;

        self.install_worker_exclude(worker_paths.worktree_root())?;
        self.install_pre_commit_hook(worker_paths.worktree_root())?;
        Ok(())
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
}
