//! Repository scaffolding for integration tests.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use multorum::runtime::{FsOrchestratorService, OrchestratorService};

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

/// Rulebook with two disjoint perspectives and a check pipeline.
///
/// Each perspective has fully isolated read and write sets so that
/// both may have concurrent active workers without exclusion
/// conflicts:
///
/// - `AuthImplementor`: writes `src/auth.rs`, reads `src/auth_ref.rs`
/// - `DataImplementor`: writes `src/data.rs`, reads `src/data_ref.rs`
///
/// The check pipeline includes a skippable `lint` and a non-skippable
/// `build`.
fn multi_perspective_rulebook_toml() -> &'static str {
    r#"
        [fileset]
        AuthOwned.glob = "src/auth.rs"
        AuthRef.glob = "src/auth_ref.rs"
        DataOwned.glob = "src/data.rs"
        DataRef.glob = "src/data_ref.rs"

        [perspective.AuthImplementor]
        read = "AuthRef"
        write = "AuthOwned"

        [perspective.DataImplementor]
        read = "DataRef"
        write = "DataOwned"

        [check]
        pipeline = ["lint", "build"]

        [check.command]
        lint = "true"
        build = "true"

        [check.policy]
        lint = "skippable"
    "#
}

/// Set up a repo with two fully disjoint perspectives and a check
/// pipeline.
pub fn setup_multi_perspective_repo() -> (TempDir, FsOrchestratorService) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/auth.rs"), "pub fn auth() -> i32 { 1 }\n").unwrap();
    fs::write(dir.path().join("src/auth_ref.rs"), "pub fn auth_ref() -> i32 { 2 }\n").unwrap();
    fs::write(dir.path().join("src/data.rs"), "pub fn data() -> i32 { 3 }\n").unwrap();
    fs::write(dir.path().join("src/data_ref.rs"), "pub fn data_ref() -> i32 { 4 }\n").unwrap();
    FsOrchestratorService::new(dir.path()).unwrap().rulebook_init().unwrap();
    fs::write(dir.path().join(".multorum/rulebook.toml"), multi_perspective_rulebook_toml())
        .unwrap();

    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Multorum Test"]);
    git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: initialize multi-perspective fixture"]);

    let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
    (dir, orchestrator)
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
