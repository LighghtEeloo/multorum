//! Git and process helpers for the filesystem runtime.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::rulebook::{CheckName, RULEBOOK_RELATIVE_PATH};
use crate::runtime::{CanonicalCommitHash, RuntimeError};

use super::RuntimeFileSystem;

impl RuntimeFileSystem {
    /// Resolve one user-facing revision to the canonical commit hash
    /// stored by Multorum.
    pub(crate) fn resolve_commit(
        &self, repo_root: &Path, revision: &str, operation: &'static str,
    ) -> Result<CanonicalCommitHash, RuntimeError> {
        let mut command = self.git_command(repo_root);
        command.arg("rev-parse").arg("--verify").arg(format!("{revision}^{{commit}}"));
        let output = command.output()?;
        if output.status.success() {
            let resolved = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            tracing::debug!(
                root = %repo_root.display(),
                revision,
                resolved_commit = %resolved,
                "resolved git revision to canonical commit"
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

    /// Return the repository HEAD commit for a workspace or worktree.
    pub(crate) fn git_head(&self, root: &Path) -> Result<CanonicalCommitHash, RuntimeError> {
        self.resolve_commit(root, "HEAD", "read HEAD")
    }

    /// Return all changed files between two commits.
    pub(crate) fn git_changed_files(
        &self, repo_root: &Path, from: &CanonicalCommitHash, to: &CanonicalCommitHash,
    ) -> Result<std::collections::BTreeSet<PathBuf>, RuntimeError> {
        let mut command = self.git_command(repo_root);
        command.arg("diff").arg("--name-only").arg(format!("{from}..{to}"));
        let output = self.run_command(command, "diff commits")?;
        Ok(output.lines().filter(|line| !line.trim().is_empty()).map(PathBuf::from).collect())
    }

    /// Run one shell-based rulebook check in a worktree.
    pub(crate) fn run_check(
        &self, worktree_root: &Path, name: &CheckName, command_text: &str,
    ) -> Result<(), RuntimeError> {
        tracing::debug!(
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

    /// Cherry-pick one worker commit into the canonical workspace.
    pub(crate) fn cherry_pick(&self, commit: &CanonicalCommitHash) -> Result<(), RuntimeError> {
        let mut command = self.git_command(self.workspace_root());
        command.arg("cherry-pick").arg(commit.as_str());
        self.run_command(command, "cherry-pick worker commit").map(|_| ())
    }

    /// Remove a managed worktree.
    pub(crate) fn remove_worktree(&self, worktree_root: &Path) -> Result<(), RuntimeError> {
        if !worktree_root.exists() {
            return Ok(());
        }

        let mut command = self.git_command(self.workspace_root());
        command.arg("worktree").arg("remove").arg("--force").arg(worktree_root);
        self.run_command(command, "remove worktree").map(|_| ())
    }

    /// Create a detached worktree rooted at the pinned base commit.
    pub(crate) fn add_worktree(
        &self, worktree_root: &Path, base_commit: &CanonicalCommitHash,
    ) -> Result<(), RuntimeError> {
        if worktree_root.exists() {
            fs::remove_dir_all(worktree_root)?;
        }
        if let Some(parent) = worktree_root.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = self.git_command(self.workspace_root());
        command
            .arg("worktree")
            .arg("add")
            .arg("--detach")
            .arg(worktree_root)
            .arg(base_commit.as_str());
        self.run_command(command, "create worktree").map(|_| ())
    }

    /// Refuse integration when the canonical workspace already has
    /// unrelated tracked modifications.
    pub(crate) fn ensure_clean_workspace(&self) -> Result<(), RuntimeError> {
        let mut command = self.git_command(self.workspace_root());
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

    pub(crate) fn install_worker_exclude(&self, worktree_root: &Path) -> Result<(), RuntimeError> {
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

        for entry in [
            ".multorum/contract.toml",
            ".multorum/read-set.txt",
            ".multorum/write-set.txt",
            ".multorum/inbox/",
            ".multorum/outbox/",
            ".multorum/artifacts/",
        ] {
            if !lines.iter().any(|line| line == entry) {
                lines.push(entry.to_owned());
            }
        }

        fs::write(exclude_path, lines.join("\n") + "\n")?;
        Ok(())
    }

    pub(crate) fn install_pre_commit_hook(&self, worktree_root: &Path) -> Result<(), RuntimeError> {
        let mut command = self.git_command(worktree_root);
        command.arg("rev-parse").arg("--git-path").arg("hooks/pre-commit");
        let output = self.run_command(command, "resolve pre-commit hook path")?;
        let hook_path = absolutize_git_path(worktree_root, output.trim());
        if let Some(parent) = hook_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let script = r#"#!/bin/sh
set -eu

write_set=".multorum/write-set.txt"
if [ ! -f "$write_set" ]; then
    exit 0
fi

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

    pub(crate) fn git_show_rulebook(
        &self, commit: &CanonicalCommitHash,
    ) -> Result<String, RuntimeError> {
        let mut command = self.git_command(self.workspace_root());
        command.arg("show").arg(format!("{commit}:{RULEBOOK_RELATIVE_PATH}"));
        self.run_command(command, "load rulebook")
    }

    pub(crate) fn git_list_files(
        &self, commit: &CanonicalCommitHash,
    ) -> Result<Vec<PathBuf>, RuntimeError> {
        let mut command = self.git_command(self.workspace_root());
        command.arg("ls-tree").arg("-r").arg("--name-only").arg(commit.as_str());
        let output = self.run_command(command, "list commit files")?;
        Ok(output.lines().filter(|line| !line.trim().is_empty()).map(PathBuf::from).collect())
    }

    fn git_command(&self, cwd: &Path) -> Command {
        let mut command = Command::new("git");
        command.current_dir(cwd);
        command
    }

    fn run_command(
        &self, mut command: Command, action: &'static str,
    ) -> Result<String, RuntimeError> {
        let cwd = command
            .get_current_dir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.workspace_root().to_path_buf());
        let output = command.output()?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned());
        }

        Err(RuntimeError::Git {
            action,
            cwd,
            details: command_failure_details(&output.stdout, &output.stderr),
        })
    }
}

fn absolutize_git_path(worktree_root: &Path, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() { candidate } else { worktree_root.join(candidate) }
}

fn command_failure_details(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let stdout = String::from_utf8_lossy(stdout);
    if stderr.trim().is_empty() { stdout.trim().to_owned() } else { stderr.trim().to_owned() }
}
