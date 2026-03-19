//! Git and process helpers for the filesystem runtime.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::rulebook::{CheckName, RULEBOOK_RELATIVE_PATH};
use crate::runtime::RuntimeError;

use super::RuntimeFileSystem;

impl RuntimeFileSystem {
    /// Ensure that the given commit exists in the worker repository.
    pub(crate) fn ensure_commit_exists(
        &self, worktree_root: &Path, commit: &str,
    ) -> Result<(), RuntimeError> {
        let mut command = self.git_command(worktree_root);
        command.arg("cat-file").arg("-e").arg(format!("{commit}^{{commit}}"));
        self.run_command(command, "verify commit").map(|_| ())
    }

    /// Return the repository HEAD commit for a workspace or worktree.
    pub(crate) fn git_head(&self, root: &Path) -> Result<String, RuntimeError> {
        let mut command = self.git_command(root);
        command.arg("rev-parse").arg("HEAD");
        Ok(self.run_command(command, "read HEAD")?.trim().to_owned())
    }

    /// Return all changed files between two commits.
    pub(crate) fn git_changed_files(
        &self, repo_root: &Path, from: &str, to: &str,
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
    pub(crate) fn cherry_pick(&self, commit: &str) -> Result<(), RuntimeError> {
        let mut command = self.git_command(self.workspace_root());
        command.arg("cherry-pick").arg(commit);
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
        &self, worktree_root: &Path, base_commit: &str,
    ) -> Result<(), RuntimeError> {
        if worktree_root.exists() {
            fs::remove_dir_all(worktree_root)?;
        }
        if let Some(parent) = worktree_root.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = self.git_command(self.workspace_root());
        command.arg("worktree").arg("add").arg("--detach").arg(worktree_root).arg(base_commit);
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

        Err(RuntimeError::CheckFailed(
            "canonical workspace has uncommitted tracked changes".to_owned(),
        ))
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

    pub(crate) fn git_show_rulebook(&self, commit: &str) -> Result<String, RuntimeError> {
        let mut command = self.git_command(self.workspace_root());
        command.arg("show").arg(format!("{commit}:{RULEBOOK_RELATIVE_PATH}"));
        self.run_command(command, "load rulebook")
    }

    pub(crate) fn git_list_files(&self, commit: &str) -> Result<Vec<PathBuf>, RuntimeError> {
        let mut command = self.git_command(self.workspace_root());
        command.arg("ls-tree").arg("-r").arg("--name-only").arg(commit);
        let output = self.run_command(command, "list commit files")?;
        Ok(output.lines().filter(|line| !line.trim().is_empty()).map(PathBuf::from).collect())
    }

    fn git_command(&self, cwd: &Path) -> Command {
        let mut command = Command::new("git");
        command.current_dir(cwd);
        command
    }

    fn run_command(&self, mut command: Command, action: &str) -> Result<String, RuntimeError> {
        let output = command.output()?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_owned()
        } else {
            stderr.trim().to_owned()
        };
        Err(RuntimeError::Git(format!("{action}: {details}")))
    }
}

fn absolutize_git_path(worktree_root: &Path, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() { candidate } else { worktree_root.join(candidate) }
}
