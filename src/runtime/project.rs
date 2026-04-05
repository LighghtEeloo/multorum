//! Detect the managed Multorum project bound to one filesystem path.
//!
//! The runtime must first decide whether the current repository view is
//! the canonical orchestrator workspace or one managed worker worktree.
//! This module centralizes that decision so CLI and server startup do
//! not guess from partial path heuristics.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::schema::rulebook::Rulebook;
use crate::vcs::{GitVcs, VersionControl};

use super::{RuntimeError, WorkerPaths};

/// Runtime role discovered for one managed repository view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeRole {
    /// Canonical workspace that owns `.multorum/orchestrator/`.
    Orchestrator,
    /// Managed worker worktree under `.multorum/tr/<worker-id>/`.
    Worker,
}

impl RuntimeRole {
    /// Stable lowercase role name for diagnostics.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            | Self::Orchestrator => "orchestrator",
            | Self::Worker => "worker",
        }
    }
}

/// Managed project context discovered from one filesystem path.
#[derive(Debug, Clone)]
pub(crate) struct CurrentProject {
    /// Repository root that owns the input path.
    repo_root: PathBuf,
    /// Canonical Multorum workspace root.
    workspace_root: PathBuf,
    /// Runtime role for the discovered repository view.
    role: RuntimeRole,
}

/// Resolved workspace root and warnings for `multorum init`.
///
/// `multorum init` must work even when the repository does not yet
/// satisfy the strict managed-role detection used by the rest of the
/// runtime. This type carries the initialization target and any
/// non-fatal findings discovered while selecting that target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InitWorkspaceTarget {
    workspace_root: PathBuf,
    warnings: Vec<String>,
}

impl InitWorkspaceTarget {
    /// Build one resolved init target with accumulated warnings.
    pub(crate) fn new(workspace_root: PathBuf, warnings: Vec<String>) -> Self {
        Self { workspace_root, warnings }
    }

    /// Canonical workspace root that `multorum init` should initialize.
    pub(crate) fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Non-fatal findings observed during target resolution.
    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

impl CurrentProject {
    /// Discover the managed project for the current working directory.
    pub(crate) fn from_current_dir() -> Result<Self, RuntimeError> {
        let cwd = std::env::current_dir()?;
        Self::with_vcs(&cwd, Arc::new(GitVcs::new()))
    }

    /// Resolve the workspace that `multorum init` should initialize for
    /// the current directory.
    ///
    /// Note: Unlike [`Self::from_current_dir`], this path intentionally
    /// does not reject unmanaged or ambiguous marker states. It selects
    /// the best available workspace root and reports unusual findings as
    /// warnings so initialization can repair the committed surface.
    pub(crate) fn init_target_from_current_dir() -> Result<InitWorkspaceTarget, RuntimeError> {
        let cwd = std::env::current_dir()?;
        Self::init_target_with_vcs(&cwd, Arc::new(GitVcs::new()))
    }

    /// Resolve the workspace that `multorum init` should initialize for
    /// one path with an explicit VCS backend.
    pub(crate) fn init_target_with_vcs(
        path: &Path, vcs: Arc<dyn VersionControl>,
    ) -> Result<InitWorkspaceTarget, RuntimeError> {
        let repo_root = vcs.repository_root(path).canonicalize()?;
        let has_rulebook = Rulebook::rulebook_path(&repo_root).exists();
        let worker_paths = WorkerPaths::new(repo_root.clone());
        let has_worker_contract = worker_paths.contract().exists();
        let mut warnings = Vec::new();

        let workspace_root = match worker_paths.workspace_root() {
            | Ok(workspace_root) => {
                if has_rulebook {
                    warnings.push(format!(
                        "detected worker-style repository `{}` that also has `.multorum/rulebook.toml`; using workspace root `{}` for initialization",
                        repo_root.display(),
                        workspace_root.display()
                    ));
                } else if !has_worker_contract {
                    warnings.push(format!(
                        "detected worker-style repository `{}` without `.multorum/contract.toml`; using workspace root `{}` for initialization",
                        repo_root.display(),
                        workspace_root.display()
                    ));
                } else {
                    warnings.push(format!(
                        "detected worker worktree `{}`; using workspace root `{}` for initialization",
                        repo_root.display(),
                        workspace_root.display()
                    ));
                }
                workspace_root
            }
            | Err(_) => {
                if has_worker_contract && has_rulebook {
                    warnings.push(format!(
                        "found both `.multorum/contract.toml` and `.multorum/rulebook.toml` at `{}`; initialization will continue at this repository root",
                        repo_root.display()
                    ));
                } else if has_worker_contract {
                    warnings.push(format!(
                        "found `.multorum/contract.toml` at repository root `{}` without worker layout markers; initialization will continue at this repository root",
                        repo_root.display()
                    ));
                }
                repo_root.clone()
            }
        };

        if WorkerPaths::new(workspace_root.clone()).contract().exists() {
            warnings.push(format!(
                "found worker contract marker at workspace root `{}`; runtime role markers are ambiguous",
                workspace_root.display()
            ));
        }

        Ok(InitWorkspaceTarget::new(workspace_root, warnings))
    }

    /// Discover the managed project for one path with an explicit VCS backend.
    pub(crate) fn with_vcs(
        path: &Path, vcs: Arc<dyn VersionControl>,
    ) -> Result<Self, RuntimeError> {
        let repo_root = vcs.repository_root(path).canonicalize()?;
        let has_rulebook = Rulebook::rulebook_path(&repo_root).exists();
        let worker_paths = WorkerPaths::new(repo_root.clone());
        let worker_workspace_root = worker_paths.workspace_root().ok();
        let has_worker_contract = worker_paths.contract().exists();

        tracing::trace!(
            input_path = %path.display(),
            repo_root = %repo_root.display(),
            has_rulebook,
            has_worker_contract,
            inside_worker_layout = worker_workspace_root.is_some(),
            "detecting current multorum project"
        );

        match (worker_workspace_root, has_worker_contract, has_rulebook) {
            | (Some(workspace_root), true, _) => {
                Ok(Self { repo_root, workspace_root, role: RuntimeRole::Worker })
            }
            | (Some(_), false, _) => Err(RuntimeError::MissingWorkerRuntime(repo_root.clone())),
            | (None, false, true) => Ok(Self {
                repo_root: repo_root.clone(),
                workspace_root: repo_root,
                role: RuntimeRole::Orchestrator,
            }),
            | (None, true, true) => Err(RuntimeError::AmbiguousRuntimeRole {
                repo_root,
                details: "found a worker contract at a workspace root",
            }),
            | (None, true, false) => Err(RuntimeError::AmbiguousRuntimeRole {
                repo_root,
                details: "found a worker contract outside `.multorum/tr/<worker>`",
            }),
            | (None, false, false) => Err(RuntimeError::UnmanagedProject(repo_root)),
        }
    }

    /// Repository root that owns the current path.
    pub(crate) fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Canonical workspace root for the current managed project.
    pub(crate) fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Runtime role for the current managed project.
    pub(crate) fn role(&self) -> RuntimeRole {
        self.role
    }

    /// Require the current path to resolve to the orchestrator workspace.
    pub(crate) fn orchestrator_workspace_root(&self) -> Result<&Path, RuntimeError> {
        match self.role {
            | RuntimeRole::Orchestrator => Ok(self.workspace_root()),
            | RuntimeRole::Worker => Err(RuntimeError::RuntimeRoleMismatch {
                expected: RuntimeRole::Orchestrator.as_str(),
                actual: RuntimeRole::Worker.as_str(),
                repo_root: self.repo_root.clone(),
            }),
        }
    }

    /// Require the current path to resolve to a managed worker worktree.
    pub(crate) fn worker_repo_root(&self) -> Result<&Path, RuntimeError> {
        match self.role {
            | RuntimeRole::Worker => Ok(self.repo_root()),
            | RuntimeRole::Orchestrator => Err(RuntimeError::RuntimeRoleMismatch {
                expected: RuntimeRole::Worker.as_str(),
                actual: RuntimeRole::Orchestrator.as_str(),
                repo_root: self.repo_root.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;

    use tempfile::TempDir;

    use crate::runtime::{CreateWorker, FsOrchestratorService, OrchestratorService};
    use crate::schema::perspective::PerspectiveName;
    use crate::vcs::GitVcs;

    use super::*;

    fn perspective() -> PerspectiveName {
        PerspectiveName::new("AuthImplementor").unwrap()
    }

    fn rulebook_toml() -> &'static str {
        r#"
            [fileset]
            Owned.glob = "src/owned.rs"
            Other.glob = "src/other.rs"

            [perspective.AuthImplementor]
            read = "Other"
            write = "Owned"

            [check]
            pipeline = []
        "#
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git").args(args).current_dir(root).output().unwrap();
        if !output.status.success() {
            panic!("git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr));
        }
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn setup_repo() -> (TempDir, FsOrchestratorService) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
        fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
        FsOrchestratorService::new(dir.path()).unwrap().rulebook_init().unwrap();
        fs::write(dir.path().join(".multorum/rulebook.toml"), rulebook_toml()).unwrap();

        git(dir.path(), &["init"]);
        git(dir.path(), &["config", "user.name", "Multorum Test"]);
        git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: initialize runtime fixture"]);

        let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
        (dir, orchestrator)
    }

    #[test]
    fn discovers_orchestrator_from_workspace_subdirectory() {
        let (dir, _orchestrator) = setup_repo();

        let project =
            CurrentProject::with_vcs(&dir.path().join("src"), Arc::new(GitVcs::new())).unwrap();

        assert_eq!(project.role(), RuntimeRole::Orchestrator);
        assert_eq!(project.repo_root(), dir.path().canonicalize().unwrap().as_path());
        assert_eq!(project.workspace_root(), dir.path().canonicalize().unwrap().as_path());
    }

    #[test]
    fn discovers_worker_from_worktree_subdirectory() {
        let (dir, orchestrator) = setup_repo();
        let worker = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

        let project =
            CurrentProject::with_vcs(&worker.worktree_path.join("src"), Arc::new(GitVcs::new()))
                .unwrap();

        assert_eq!(project.role(), RuntimeRole::Worker);
        assert_eq!(project.repo_root(), worker.worktree_path.canonicalize().unwrap().as_path());
        assert_eq!(project.workspace_root(), dir.path().canonicalize().unwrap().as_path());
    }

    #[test]
    fn rejects_unmanaged_repository() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        git(dir.path(), &["init"]);

        let error =
            CurrentProject::with_vcs(&dir.path().join("src"), Arc::new(GitVcs::new())).unwrap_err();

        assert!(
            matches!(error, RuntimeError::UnmanagedProject(path) if path == dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn rejects_ambiguous_runtime_role_markers() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".multorum")).unwrap();
        fs::write(dir.path().join(".multorum/rulebook.toml"), "[check]\npipeline = []\n").unwrap();
        fs::write(dir.path().join(".multorum/contract.toml"), "").unwrap();
        git(dir.path(), &["init"]);

        let error = CurrentProject::with_vcs(dir.path(), Arc::new(GitVcs::new())).unwrap_err();

        assert!(matches!(
            error,
            RuntimeError::AmbiguousRuntimeRole { ref repo_root, .. }
                if repo_root == &dir.path().canonicalize().unwrap()
        ));
    }

    #[test]
    fn init_target_accepts_unmanaged_repository() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        git(dir.path(), &["init"]);

        let target =
            CurrentProject::init_target_with_vcs(dir.path(), Arc::new(GitVcs::new())).unwrap();

        assert_eq!(target.workspace_root(), dir.path().canonicalize().unwrap().as_path());
        assert!(target.warnings().is_empty());
    }

    #[test]
    fn init_target_resolves_worker_to_workspace_and_warns() {
        let (dir, orchestrator) = setup_repo();
        let worker = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

        let target =
            CurrentProject::init_target_with_vcs(&worker.worktree_path, Arc::new(GitVcs::new()))
                .unwrap();

        assert_eq!(target.workspace_root(), dir.path().canonicalize().unwrap().as_path());
        assert!(!target.warnings().is_empty());
    }

    #[test]
    fn init_target_warns_on_ambiguous_root_markers() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".multorum")).unwrap();
        fs::write(dir.path().join(".multorum/rulebook.toml"), "[check]\npipeline = []\n").unwrap();
        fs::write(dir.path().join(".multorum/contract.toml"), "").unwrap();
        git(dir.path(), &["init"]);

        let target =
            CurrentProject::init_target_with_vcs(dir.path(), Arc::new(GitVcs::new())).unwrap();

        assert_eq!(target.workspace_root(), dir.path().canonicalize().unwrap().as_path());
        assert_eq!(target.warnings().len(), 2);
        assert!(target.warnings()[0].contains("both"));
    }
}
