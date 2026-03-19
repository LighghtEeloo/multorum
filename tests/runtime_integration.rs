use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use multorum::perspective::PerspectiveName;
use multorum::rulebook::Rulebook;
use multorum::runtime::{
    BundlePayload, MessageKind, ReplyReference, RuntimeError, WorkerState,
    service::{
        FilesystemOrchestratorService, FilesystemWorkerService, OrchestratorService, WorkerService,
    },
};

fn perspective() -> PerspectiveName {
    PerspectiveName::new("AuthImplementor").unwrap()
}

fn rulebook_toml() -> &'static str {
    r#"
        [filesets]
        Owned.path = "src/owned.rs"
        Other.path = "src/other.rs"

        [perspectives.AuthImplementor]
        read = "Other"
        write = "Owned"

        [checks]
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

fn setup_repo() -> (TempDir, FilesystemOrchestratorService, String) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join(".multorum")).unwrap();
    fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
    fs::write(dir.path().join(".multorum/.gitignore"), "orchestrator/\nworktrees/\n").unwrap();
    fs::write(dir.path().join(".multorum/rulebook.toml"), rulebook_toml()).unwrap();

    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Multorum Test"]);
    git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: initialize runtime fixture"]);

    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let orchestrator = FilesystemOrchestratorService::new(dir.path()).unwrap();
    orchestrator.rulebook_switch(head.clone()).unwrap();
    (dir, orchestrator, head)
}

#[test]
fn rulebook_init_creates_default_committed_files() {
    let dir = tempfile::tempdir().unwrap();
    let orchestrator = FilesystemOrchestratorService::new(dir.path()).unwrap();
    let canonical_root = dir.path().canonicalize().unwrap();

    let init = orchestrator.rulebook_init().unwrap();

    assert_eq!(init.multorum_root, canonical_root.join(".multorum"));
    assert_eq!(init.rulebook_path, canonical_root.join(".multorum/rulebook.toml"));
    assert_eq!(init.gitignore_path, canonical_root.join(".multorum/.gitignore"));
    assert_eq!(fs::read_to_string(&init.rulebook_path).unwrap(), Rulebook::default_template());
    assert_eq!(fs::read_to_string(&init.gitignore_path).unwrap(), "orchestrator/\nworktrees/\n");
    assert!(init.multorum_root.join("orchestrator").is_dir());
    assert!(init.multorum_root.join("worktrees").is_dir());

    let rulebook = Rulebook::from_workspace_root(dir.path()).unwrap();
    assert!(rulebook.filesets().definitions().is_empty());
    assert!(rulebook.perspectives().declarations().is_empty());
    assert!(rulebook.checks().pipeline().is_empty());
}

#[test]
fn rulebook_init_refuses_to_overwrite_existing_rulebook() {
    let dir = tempfile::tempdir().unwrap();
    let rulebook_path = dir.path().join(".multorum/rulebook.toml");
    fs::create_dir_all(rulebook_path.parent().unwrap()).unwrap();
    fs::write(&rulebook_path, "[checks]\npipeline = []\n").unwrap();
    let orchestrator = FilesystemOrchestratorService::new(dir.path()).unwrap();
    let canonical_rulebook_path = rulebook_path.canonicalize().unwrap();

    let error = orchestrator.rulebook_init().unwrap_err();

    assert!(matches!(error, RuntimeError::RulebookExists(path) if path == canonical_rulebook_path));
    assert_eq!(fs::read_to_string(rulebook_path).unwrap(), "[checks]\npipeline = []\n");
}

#[test]
fn mailbox_flow_moves_payloads_and_transitions_worker_state() {
    let (repo, orchestrator, _) = setup_repo();
    let task_body = repo.path().join("task.md");
    fs::write(&task_body, "# initial task\n").unwrap();

    let provision = orchestrator
        .provision_worker(
            perspective(),
            Some(BundlePayload { body_path: Some(task_body.clone()), ..BundlePayload::default() }),
        )
        .unwrap();
    assert_eq!(provision.state, WorkerState::Provisioned);
    assert!(provision.worktree_path.is_absolute());
    assert!(provision.seeded_task_path.as_ref().is_some_and(|path| path.is_absolute()));
    assert!(!task_body.exists(), "task body should be moved into the runtime bundle");

    let worker = FilesystemWorkerService::new(&provision.worktree_path).unwrap();
    let contract = worker.contract().unwrap();
    assert!(contract.read_set_path.is_absolute());
    assert!(contract.write_set_path.is_absolute());
    let inbox = worker.read_inbox(None).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].kind, MessageKind::Task);
    assert!(inbox[0].bundle_path.is_absolute());
    worker.ack_inbox(inbox[0].sequence).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Active);

    let report_body = provision.worktree_path.join("report.md");
    fs::write(&report_body, "Need clarification.\n").unwrap();
    let report = worker
        .send_report(
            None,
            ReplyReference::default(),
            BundlePayload { body_path: Some(report_body.clone()), ..BundlePayload::default() },
        )
        .unwrap();
    assert!(report.bundle_path.is_absolute());
    assert!(!report_body.exists(), "report body should be moved into the outbox bundle");
    assert_eq!(worker.status().unwrap().state, WorkerState::Blocked);

    let resolve_body = repo.path().join("resolve.md");
    fs::write(&resolve_body, "Use the existing API shape.\n").unwrap();
    orchestrator
        .resolve_worker(
            perspective(),
            ReplyReference { in_reply_to: Some(report.message.sequence) },
            BundlePayload { body_path: Some(resolve_body.clone()), ..BundlePayload::default() },
        )
        .unwrap();
    assert!(!resolve_body.exists(), "resolve body should be moved into the inbox bundle");

    let follow_up = worker.read_inbox(inbox.last().map(|message| message.sequence)).unwrap();
    assert_eq!(follow_up.len(), 1);
    assert_eq!(follow_up[0].kind, MessageKind::Resolve);
    assert!(follow_up[0].bundle_path.is_absolute());
    worker.ack_inbox(follow_up[0].sequence).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Active);
}

#[test]
fn integrate_worker_cherry_picks_allowed_changes() {
    let (repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.provision_worker(perspective(), None).unwrap();
    let worker = FilesystemWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: update owned implementation"]);
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit(head_commit.clone(), BundlePayload::default()).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Committed);

    let integration = orchestrator.integrate_worker(perspective(), Vec::new()).unwrap();
    assert_eq!(integration.state, WorkerState::Integrated);
    assert!(integration.ran_checks.is_empty());
    assert!(!provision.worktree_path.exists(), "integrated worktree should be removed");
    assert_eq!(
        fs::read_to_string(repo.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 3 }\n"
    );
}

#[test]
fn integrate_rejects_paths_outside_the_compiled_write_set() {
    let (_repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.provision_worker(perspective(), None).unwrap();
    let worker = FilesystemWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/other.rs"), "pub fn other() -> i32 { 99 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/other.rs"]);
    git(
        &provision.worktree_path,
        &["commit", "--no-verify", "-m", "incr: modify unauthorized file"],
    );
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit(head_commit, BundlePayload::default()).unwrap();
    let error = orchestrator.integrate_worker(perspective(), Vec::new()).unwrap_err();
    assert!(matches!(error, RuntimeError::WriteSetViolation));
}
