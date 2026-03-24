//! Repository scaffolding for integration tests.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use multorum::runtime::{FsOrchestratorService, OrchestratorService};

fn rulebook_toml() -> &'static str {
    r#"
        [fileset]
        Owned.path = "src/owned.rs"
        Other.path = "src/other.rs"

        [perspective.AuthImplementor]
        read = "Other"
        write = "Owned"

        [check]
        pipeline = []
    "#
}

pub fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(root).output().unwrap();
    if !output.status.success() {
        panic!("git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub fn setup_repo() -> (TempDir, FsOrchestratorService) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join(".multorum")).unwrap();
    fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
    fs::write(dir.path().join(".multorum/.gitignore"), "orchestrator/\ntr/\n").unwrap();
    fs::write(dir.path().join(".multorum/rulebook.toml"), rulebook_toml()).unwrap();

    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Multorum Test"]);
    git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: initialize runtime fixture"]);

    let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
    orchestrator.rulebook_install().unwrap();
    (dir, orchestrator)
}
