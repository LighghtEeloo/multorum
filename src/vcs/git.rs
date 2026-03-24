//! Git-backed [`super::VersionControl`] implementation.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::runtime::RuntimeError;

use super::{CanonicalCommitHash, VersionControl};

const WORKER_EXCLUDE_ENTRIES: [&str; 6] = [
    ".multorum/contract.toml",
    ".multorum/read-set.txt",
    ".multorum/write-set.txt",
    ".multorum/inbox/",
    ".multorum/outbox/",
    ".multorum/artifacts/",
];

/// Git backend for Multorum repository operations.
#[derive(Debug, Default, Clone, Copy)]
pub struct GitVcs;

impl GitVcs {
    /// Construct the Git backend.
    pub fn new() -> Self {
        Self
    }

    fn git_command(&self, cwd: &Path) -> Command {
        let mut command = Command::new("git");
        command.current_dir(cwd);
        command
    }

    fn run_command(
        &self, mut command: Command, action: &'static str,
    ) -> Result<String, RuntimeError> {
        let cwd =
            command.get_current_dir().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let output = command.output()?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned());
        }

        Err(RuntimeError::Vcs {
            backend: self.backend_name(),
            action,
            cwd,
            details: command_failure_details(&output.stdout, &output.stderr),
        })
    }

    /// Return whether Git still tracks `worktree_root` as one of the
    /// repository's attached worktrees.
    ///
    /// Note: `multorum worker delete` must clear Git's administrative
    /// worktree entry even when the directory vanished on disk.
    fn is_registered_worktree(
        &self, workspace_root: &Path, worktree_root: &Path,
    ) -> Result<bool, RuntimeError> {
        let mut command = self.git_command(workspace_root);
        command.arg("worktree").arg("list").arg("--porcelain");
        let output = self.run_command(command, "list worktrees")?;
        let expected = normalize_worktree_path(workspace_root, worktree_root);
        Ok(output
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .any(|entry| normalize_worktree_path(workspace_root, Path::new(entry)) == expected))
    }

    fn install_worker_exclude(&self, worktree_root: &Path) -> Result<(), RuntimeError> {
        let mut command = self.git_command(worktree_root);
        command.arg("rev-parse").arg("--git-path").arg("info/exclude");
        let output = self.run_command(command, "resolve local exclude path")?;
        let exclude_path = absolutize_git_path(worktree_root, output.trim());
        if let Some(parent) = exclude_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut lines = if exclude_path.exists() {
            fs::read_to_string(&exclude_path)?.lines().map(str::to_owned).collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        for entry in WORKER_EXCLUDE_ENTRIES {
            if !lines.iter().any(|line| line == entry) {
                lines.push(entry.to_owned());
            }
        }

        fs::write(exclude_path, lines.join("\n") + "\n")?;
        Ok(())
    }

    /// Install the shared pre-commit hook.
    ///
    /// Since Git worktrees share the main repository's hooks directory,
    /// a single script handles both the worker write-set check and the
    /// orchestrator exclusion-set check. The hook detects which context
    /// it runs in by probing the files present in the working directory.
    fn install_pre_commit_hook(&self, repo_root: &Path) -> Result<(), RuntimeError> {
        let mut command = self.git_command(repo_root);
        command.arg("rev-parse").arg("--git-path").arg("hooks/pre-commit");
        let output = self.run_command(command, "resolve pre-commit hook path")?;
        let hook_path = absolutize_git_path(repo_root, output.trim());
        if let Some(parent) = hook_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let script = r#"#!/bin/sh
set -eu

# --- Worker write-set guard ---
# In a worker worktree, every staged path must be inside the write set.
write_set=".multorum/write-set.txt"
if [ -f "$write_set" ]; then
    allowed=''
    while IFS= read -r path; do
        [ -n "$path" ] || continue
        allowed="$allowed
$path"
    done < "$write_set"

    git diff --cached --name-only --diff-filter=ACDMRTUXB | while IFS= read -r path; do
        [ -n "$path" ] || continue
        if ! printf '%s\n' "$allowed" | grep -Fxq "$path"; then
            printf 'multorum: staged path outside write set: %s\n' "$path" >&2
            exit 1
        fi
    done
fi

# --- Orchestrator exclusion-set guard ---
# In the canonical workspace, no staged path may appear in the exclusion set.
exclusion_set=".multorum/orchestrator/exclusion-set.txt"
if [ -f "$exclusion_set" ]; then
    blocked=''
    while IFS= read -r path; do
        [ -n "$path" ] || continue
        blocked="$blocked
$path"
    done < "$exclusion_set"

    if [ -n "$blocked" ]; then
        git diff --cached --name-only --diff-filter=ACDMRTUXB | while IFS= read -r path; do
            [ -n "$path" ] || continue
            if printf '%s\n' "$blocked" | grep -Fxq "$path"; then
                printf 'multorum: staged path in orchestrator exclusion set: %s\n' "$path" >&2
                exit 1
            fi
        done
    fi
fi
"#;

        fs::write(&hook_path, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&hook_path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&hook_path, permissions)?;
        }
        Ok(())
    }
}

impl VersionControl for GitVcs {
    fn backend_name(&self) -> &'static str {
        "git"
    }

    fn repository_root(&self, path: &Path) -> PathBuf {
        let mut current = Some(path);
        while let Some(candidate) = current {
            if candidate.join(".git").exists() {
                return candidate.to_path_buf();
            }
            current = candidate.parent();
        }
        path.to_path_buf()
    }

    fn resolve_commit(
        &self, repo_root: &Path, revision: &str, operation: &'static str,
    ) -> Result<CanonicalCommitHash, RuntimeError> {
        let mut command = self.git_command(repo_root);
        command.arg("rev-parse").arg("--verify").arg(format!("{revision}^{{commit}}"));
        let output = command.output()?;
        if output.status.success() {
            let resolved = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            tracing::debug!(
                backend = self.backend_name(),
                root = %repo_root.display(),
                revision,
                resolved_commit = %resolved,
                "resolved repository revision to canonical commit"
            );
            return Ok(CanonicalCommitHash::new(resolved));
        }

        let details = command_failure_details(&output.stdout, &output.stderr);
        Err(RuntimeError::CommitNotFound {
            operation,
            worktree_root: repo_root.to_path_buf(),
            commit: revision.to_owned(),
            details,
        })
    }

    fn head_commit(&self, repo_root: &Path) -> Result<CanonicalCommitHash, RuntimeError> {
        self.resolve_commit(repo_root, "HEAD", "read HEAD")
    }

    fn changed_files(
        &self, repo_root: &Path, from: &CanonicalCommitHash, to: &CanonicalCommitHash,
    ) -> Result<BTreeSet<PathBuf>, RuntimeError> {
        let mut command = self.git_command(repo_root);
        command.arg("diff").arg("--name-only").arg(format!("{from}..{to}"));
        let output = self.run_command(command, "diff commits")?;
        Ok(output.lines().filter(|line| !line.trim().is_empty()).map(PathBuf::from).collect())
    }

    fn create_worktree(
        &self, workspace_root: &Path, worktree_root: &Path, base_commit: &CanonicalCommitHash,
    ) -> Result<(), RuntimeError> {
        if worktree_root.exists() {
            fs::remove_dir_all(worktree_root)?;
        }
        if let Some(parent) = worktree_root.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = self.git_command(workspace_root);
        command
            .arg("worktree")
            .arg("add")
            .arg("--detach")
            .arg(worktree_root)
            .arg(base_commit.as_str());
        self.run_command(command, "create worktree").map(|_| ())
    }

    fn remove_worktree(
        &self, workspace_root: &Path, worktree_root: &Path,
    ) -> Result<bool, RuntimeError> {
        if !self.is_registered_worktree(workspace_root, worktree_root)? {
            tracing::debug!(
                backend = self.backend_name(),
                root = %workspace_root.display(),
                worktree_root = %worktree_root.display(),
                "skipping worktree removal because git has no matching registration"
            );
            return Ok(false);
        }

        let mut command = self.git_command(workspace_root);
        command.arg("worktree").arg("remove").arg("--force").arg(worktree_root);
        self.run_command(command, "remove worktree")?;
        Ok(true)
    }

    fn ensure_clean_workspace(&self, workspace_root: &Path) -> Result<(), RuntimeError> {
        let mut command = self.git_command(workspace_root);
        command.arg("status").arg("--porcelain").arg("--untracked-files=no");
        let output = self.run_command(command, "read workspace status")?;
        if output.trim().is_empty() {
            return Ok(());
        }

        let changed_paths = output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        Err(RuntimeError::CheckFailed(format!(
            "canonical workspace has uncommitted tracked changes: {changed_paths}"
        )))
    }

    fn integrate_commit(
        &self, workspace_root: &Path, commit: &CanonicalCommitHash,
    ) -> Result<(), RuntimeError> {
        let mut command = self.git_command(workspace_root);
        command.arg("cherry-pick").arg(commit.as_str());
        self.run_command(command, "integrate submitted commit").map(|_| ())
    }

    fn install_worker_runtime_support(&self, worktree_root: &Path) -> Result<(), RuntimeError> {
        self.install_worker_exclude(worktree_root)?;
        self.install_pre_commit_hook(worktree_root)
    }

    fn install_orchestrator_hook(&self, workspace_root: &Path) -> Result<(), RuntimeError> {
        self.install_pre_commit_hook(workspace_root)
    }

    fn show_file_at_commit(
        &self, workspace_root: &Path, commit: &CanonicalCommitHash, path: &Path,
    ) -> Result<String, RuntimeError> {
        let mut command = self.git_command(workspace_root);
        let relative_path = path.to_string_lossy().replace('\\', "/");
        command.arg("show").arg(format!("{commit}:{relative_path}"));
        self.run_command(command, "load committed file")
    }

    fn list_files_at_commit(
        &self, workspace_root: &Path, commit: &CanonicalCommitHash,
    ) -> Result<Vec<PathBuf>, RuntimeError> {
        let mut command = self.git_command(workspace_root);
        command.arg("ls-tree").arg("-r").arg("--name-only").arg(commit.as_str());
        let output = self.run_command(command, "list commit files")?;
        Ok(output.lines().filter(|line| !line.trim().is_empty()).map(PathBuf::from).collect())
    }
}

fn absolutize_git_path(worktree_root: &Path, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() { candidate } else { worktree_root.join(candidate) }
}

fn normalize_worktree_path(workspace_root: &Path, worktree_root: &Path) -> PathBuf {
    if worktree_root.is_absolute() {
        worktree_root.to_path_buf()
    } else {
        workspace_root.join(worktree_root)
    }
}

fn command_failure_details(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let stdout = String::from_utf8_lossy(stdout);
    if stderr.trim().is_empty() { stdout.trim().to_owned() } else { stderr.trim().to_owned() }
}
