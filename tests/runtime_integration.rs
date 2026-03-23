use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;
use toml::Value;

use multorum::perspective::PerspectiveName;
use multorum::rulebook::Rulebook;
use multorum::runtime::{
    BundlePayload, CreateWorker, FsOrchestratorService, FsWorkerService, MessageKind,
    OrchestratorService, ReplyReference, RuntimeError, WorkerService, WorkerState,
};

fn perspective() -> PerspectiveName {
    PerspectiveName::new("AuthImplementor").unwrap()
}

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

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(root).output().unwrap();
    if !output.status.success() {
        panic!("git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn setup_repo() -> (TempDir, FsOrchestratorService, String) {
    setup_repo_with_rulebook(rulebook_toml())
}

fn setup_repo_with_rulebook(rulebook_toml: &str) -> (TempDir, FsOrchestratorService, String) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join(".multorum")).unwrap();
    fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
    fs::write(dir.path().join(".multorum/.gitignore"), "orchestrator/\nworktrees/\n").unwrap();
    fs::write(dir.path().join(".multorum/rulebook.toml"), rulebook_toml).unwrap();

    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Multorum Test"]);
    git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: initialize runtime fixture"]);

    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
    orchestrator.rulebook_install().unwrap();
    (dir, orchestrator, head)
}

fn short_commit(commit: &str) -> String {
    commit.chars().take(12).collect()
}

#[test]
fn rulebook_init_creates_default_committed_files() {
    let dir = tempfile::tempdir().unwrap();
    let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
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
    assert!(rulebook.fileset().definitions().is_empty());
    assert!(rulebook.perspective().declarations().is_empty());
    assert!(rulebook.check().pipeline().is_empty());
}

#[test]
fn rulebook_init_refuses_to_overwrite_existing_rulebook() {
    let dir = tempfile::tempdir().unwrap();
    let rulebook_path = dir.path().join(".multorum/rulebook.toml");
    fs::create_dir_all(rulebook_path.parent().unwrap()).unwrap();
    fs::write(&rulebook_path, "[check]\npipeline = []\n").unwrap();
    let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
    let canonical_rulebook_path = rulebook_path.canonicalize().unwrap();

    let error = orchestrator.rulebook_init().unwrap_err();

    assert!(matches!(error, RuntimeError::RulebookExists(path) if path == canonical_rulebook_path));
    assert_eq!(fs::read_to_string(rulebook_path).unwrap(), "[check]\npipeline = []\n");
}

#[test]
fn mailbox_flow_moves_payloads_and_transitions_worker_state() {
    let (repo, orchestrator, _) = setup_repo();
    let task_body = repo.path().join("task.md");
    fs::write(&task_body, "# initial task\n").unwrap();

    let provision = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_task(BundlePayload {
            body_path: Some(task_body.clone()),
            ..BundlePayload::default()
        }))
        .unwrap();
    assert_eq!(provision.state, WorkerState::Active);
    assert!(provision.worktree_path.is_absolute());
    assert!(
        provision
            .seeded_task_path
            .as_ref()
            .is_some_and(|path: &std::path::PathBuf| path.is_absolute())
    );
    assert!(!task_body.exists(), "task body should be moved into the runtime bundle");

    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Active);
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
            provision.worker_id.clone(),
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
fn merge_worker_cherry_picks_allowed_changes() {
    let (repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: update owned implementation"]);
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit(head_commit.clone(), BundlePayload::default()).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Committed);

    let merge = orchestrator.merge_worker(provision.worker_id.clone(), Vec::new()).unwrap();
    assert_eq!(merge.state, WorkerState::Merged);
    assert!(merge.ran_checks.is_empty());
    assert!(provision.worktree_path.exists(), "merged worktree should be preserved");
    assert_eq!(
        fs::read_to_string(repo.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 3 }\n"
    );
}

#[test]
fn same_perspective_can_spawn_multiple_workers_and_close_the_group_on_integration() {
    let (_repo, orchestrator, _) = setup_repo();
    let first = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let second = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    assert_ne!(first.worker_id, second.worker_id);
    assert_eq!(first.perspective, second.perspective);
    let status = orchestrator.status().unwrap();
    assert_eq!(status.active_perspectives.len(), 1);
    assert_eq!(status.active_perspectives[0].perspective, first.perspective);
    assert_eq!(orchestrator.list_workers().unwrap().len(), 2);
    assert_eq!(
        orchestrator.get_worker(first.worker_id.clone()).unwrap().worktree_path,
        first.worktree_path
    );
    assert_eq!(orchestrator.status().unwrap().workers.len(), 2);

    let worker = FsWorkerService::new(&first.worktree_path).unwrap();
    fs::write(first.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 5 }\n").unwrap();
    git(&first.worktree_path, &["add", "src/owned.rs"]);
    git(&first.worktree_path, &["commit", "-m", "incr: choose one bidder"]);
    let head_commit = git(&first.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit(head_commit, BundlePayload::default()).unwrap();
    orchestrator.merge_worker(first.worker_id.clone(), Vec::new()).unwrap();

    assert!(first.worktree_path.exists(), "merged worktree should be preserved");
    assert!(second.worktree_path.exists(), "discarded sibling worktree should be preserved");
    assert!(orchestrator.status().unwrap().workers.is_empty());
}

#[test]
fn create_worker_uses_explicit_worker_id_when_requested() {
    let (_repo, orchestrator, _) = setup_repo();
    let worker_id: multorum::runtime::WorkerId = "custom_worker_7".parse().unwrap();

    let provision = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();

    assert_eq!(provision.worker_id, worker_id);
    assert!(provision.worktree_path.ends_with(worker_id.as_str()));
    assert_eq!(orchestrator.get_worker(worker_id.clone()).unwrap().worker_id, worker_id);
}

#[test]
fn create_worker_rejects_duplicate_explicit_worker_id() {
    let (_repo, orchestrator, _) = setup_repo();
    let worker_id: multorum::runtime::WorkerId = "custom_worker_7".parse().unwrap();

    orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();

    let error = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap_err();
    assert!(matches!(error, RuntimeError::WorkerIdExists(actual) if actual == worker_id));
}

#[test]
fn create_worker_rejects_reused_explicit_worker_id_while_discarded_workspace_exists() {
    let (_repo, orchestrator, _) = setup_repo();
    let worker_id: multorum::runtime::WorkerId = "custom_worker_7".parse().unwrap();

    let first = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();
    orchestrator.discard_worker(worker_id.clone()).unwrap();
    assert!(first.worktree_path.exists());

    let error = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::ExistingWorkerWorkspace { worker_id: actual, state: WorkerState::Discarded, worktree_path }
            if actual == worker_id && worktree_path == first.worktree_path
    ));
}

#[test]
fn create_worker_reuses_explicit_worker_id_after_discard_when_overwriting() {
    let (_repo, orchestrator, _) = setup_repo();
    let worker_id: multorum::runtime::WorkerId = "custom_worker_7".parse().unwrap();

    let first = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();
    orchestrator.discard_worker(worker_id.clone()).unwrap();
    assert!(first.worktree_path.exists());

    let second = orchestrator
        .create_worker(
            CreateWorker::new(perspective())
                .with_worker_id(worker_id.clone())
                .with_overwriting_worktree(),
        )
        .unwrap();

    assert_eq!(second.worker_id, worker_id);
    assert_eq!(second.state, WorkerState::Active);
    assert_eq!(second.worktree_path, first.worktree_path);
    assert!(second.worktree_path.exists());
    assert_eq!(orchestrator.get_worker(worker_id).unwrap().state, WorkerState::Active);
}

#[test]
fn create_worker_rejects_reused_explicit_worker_id_while_merged_workspace_exists() {
    let (_repo, orchestrator, _) = setup_repo();
    let worker_id: multorum::runtime::WorkerId = "custom_worker_7".parse().unwrap();

    let first = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();
    let worker = FsWorkerService::new(&first.worktree_path).unwrap();
    fs::write(first.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 9 }\n").unwrap();
    git(&first.worktree_path, &["add", "src/owned.rs"]);
    git(&first.worktree_path, &["commit", "-m", "incr: finalize reused worker id"]);
    let head_commit = git(&first.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head_commit, BundlePayload::default()).unwrap();
    orchestrator.merge_worker(worker_id.clone(), Vec::new()).unwrap();
    assert!(first.worktree_path.exists());

    let error = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::ExistingWorkerWorkspace { worker_id: actual, state: WorkerState::Merged, worktree_path }
            if actual == worker_id && worktree_path == first.worktree_path
    ));
}

#[test]
fn create_worker_reuses_explicit_worker_id_after_merge_when_overwriting() {
    let (_repo, orchestrator, _) = setup_repo();
    let worker_id: multorum::runtime::WorkerId = "custom_worker_7".parse().unwrap();

    let first = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();
    let worker = FsWorkerService::new(&first.worktree_path).unwrap();
    fs::write(first.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 9 }\n").unwrap();
    git(&first.worktree_path, &["add", "src/owned.rs"]);
    git(&first.worktree_path, &["commit", "-m", "incr: finalize reused worker id"]);
    let head_commit = git(&first.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head_commit, BundlePayload::default()).unwrap();
    orchestrator.merge_worker(worker_id.clone(), Vec::new()).unwrap();
    assert!(first.worktree_path.exists());

    let second = orchestrator
        .create_worker(
            CreateWorker::new(perspective())
                .with_worker_id(worker_id.clone())
                .with_overwriting_worktree(),
        )
        .unwrap();

    assert_eq!(second.worker_id, worker_id);
    assert_eq!(second.state, WorkerState::Active);
    assert_eq!(second.worktree_path, first.worktree_path);
    assert!(second.worktree_path.exists());
    assert_eq!(orchestrator.get_worker(worker_id).unwrap().state, WorkerState::Active);
}

#[test]
fn delete_worker_removes_workspace_after_discard() {
    let (_repo, orchestrator, _) = setup_repo();
    let created = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    orchestrator.discard_worker(created.worker_id.clone()).unwrap();
    assert!(created.worktree_path.exists());

    let deleted = orchestrator.delete_worker(created.worker_id.clone()).unwrap();

    assert_eq!(deleted.worker_id, created.worker_id);
    assert_eq!(deleted.state, WorkerState::Discarded);
    assert_eq!(deleted.worktree_path, created.worktree_path);
    assert!(deleted.deleted_workspace);
    assert!(!created.worktree_path.exists());

    let recreated = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(created.worker_id.clone()))
        .unwrap();
    assert_eq!(recreated.worker_id, created.worker_id);
    assert!(recreated.worktree_path.exists());
}

#[test]
fn delete_worker_rejects_live_worker() {
    let (_repo, orchestrator, _) = setup_repo();
    let created = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    let error = orchestrator.delete_worker(created.worker_id).unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::InvalidState {
            operation,
            expected,
            actual: WorkerState::Active,
        } if operation == "delete worker workspace" && expected == "MERGED or DISCARDED"
    ));
}

#[test]
fn rulebook_install_persists_canonical_head_commit() {
    let (repo, orchestrator, head) = setup_repo();

    let install = orchestrator.rulebook_install().unwrap();
    assert_eq!(install.active_commit.as_str(), head);

    let status = orchestrator.status().unwrap();
    assert_eq!(status.active_rulebook_commit.as_str(), head);

    let active_rulebook =
        fs::read_to_string(repo.path().join(".multorum/orchestrator/active-rulebook.toml"))
            .unwrap();
    assert!(active_rulebook.contains(&format!("base_commit = \"{head}\"")));
    assert!(!active_rulebook.contains("rulebook_commit"));
}

#[test]
fn send_commit_canonicalizes_symbolic_revision_before_storage_and_integration() {
    let (_repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: update owned implementation"]);
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit("HEAD".to_owned(), BundlePayload::default()).unwrap();

    let worker_state = fs::read_to_string(
        provision
            .worktree_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("orchestrator/workers")
            .join(provision.worker_id.as_str())
            .join("state.toml"),
    )
    .unwrap();
    let worker_state: Value = toml::from_str(&worker_state).unwrap();
    assert_eq!(worker_state["submitted_head_commit"].as_str(), Some(head_commit.as_str()));

    let integration = orchestrator.merge_worker(provision.worker_id.clone(), Vec::new()).unwrap();
    assert_eq!(integration.state, WorkerState::Merged);
}

#[test]
fn send_commit_canonicalizes_short_hash_before_storage_and_integration() {
    let (_repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: update owned implementation"]);
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit(short_commit(&head_commit), BundlePayload::default()).unwrap();

    let outbox_envelope = fs::read_to_string(
        provision.worktree_path.join(".multorum/outbox/new/0001-commit/envelope.toml"),
    )
    .unwrap();
    let outbox_envelope: Value = toml::from_str(&outbox_envelope).unwrap();
    assert_eq!(outbox_envelope["head_commit"].as_str(), Some(head_commit.as_str()));

    let integration = orchestrator.merge_worker(provision.worker_id.clone(), Vec::new()).unwrap();
    assert_eq!(integration.state, WorkerState::Merged);
}

#[test]
fn send_report_canonicalizes_optional_head_commit_before_storage() {
    let (_repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: update owned implementation"]);
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    let report = worker
        .send_report(
            Some(short_commit(&head_commit)),
            ReplyReference::default(),
            BundlePayload::default(),
        )
        .unwrap();

    let envelope = fs::read_to_string(report.bundle_path.join("envelope.toml")).unwrap();
    let envelope: Value = toml::from_str(&envelope).unwrap();
    assert_eq!(envelope["head_commit"].as_str(), Some(head_commit.as_str()));
}

#[test]
fn merge_rejects_paths_outside_the_compiled_write_set() {
    let (_repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();
    let base_commit = worker.contract().unwrap().base_commit;

    fs::write(provision.worktree_path.join("src/other.rs"), "pub fn other() -> i32 { 99 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/other.rs"]);
    git(
        &provision.worktree_path,
        &["commit", "--no-verify", "-m", "incr: modify unauthorized file"],
    );
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit(head_commit.clone(), BundlePayload::default()).unwrap();
    let error = orchestrator.merge_worker(provision.worker_id.clone(), Vec::new()).unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::WriteSetViolation {
            worker_id: _,
            perspective: actual_perspective,
            base_commit: actual_base_commit,
            head_commit: actual_head_commit,
            violations,
        } if actual_perspective == perspective()
            && actual_base_commit == base_commit
            && actual_head_commit.as_str() == head_commit
            && violations == vec![Path::new("src/other.rs").to_path_buf()]
    ));
}

#[test]
fn merge_rejects_when_worker_head_moves_after_submission() {
    let (_repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: update owned implementation"]);
    let submitted_head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(submitted_head_commit.clone(), BundlePayload::default()).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 4 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: move worker head after submission"]);
    let current_head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    let error = orchestrator.merge_worker(provision.worker_id.clone(), Vec::new()).unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::WorkerHeadMismatch {
            worker_id: _,
            submitted_head_commit: actual_submitted_head_commit,
            current_head_commit: actual_current_head_commit,
        } if actual_submitted_head_commit.as_str() == submitted_head_commit
            && actual_current_head_commit.as_str() == current_head_commit
    ));
}

#[test]
fn merge_rejects_skip_request_for_check_without_policy_override() {
    let (_repo, orchestrator, _) = setup_repo_with_rulebook(
        r#"
            [fileset]
            Owned.path = "src/owned.rs"
            Other.path = "src/other.rs"

            [perspective.AuthImplementor]
            read = "Other"
            write = "Owned"

            [check]
            pipeline = ["test"]

            [check.command]
            test = "true"
        "#,
    );
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 7 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: prepare skip policy test"]);
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit(head_commit, BundlePayload::default()).unwrap();
    let error = orchestrator
        .merge_worker(provision.worker_id.clone(), vec!["test".to_owned()])
        .unwrap_err();

    assert!(
        matches!(error, RuntimeError::CheckFailed(message) if message == "check `test` is not skippable")
    );
}

#[test]
fn merge_accepts_skip_request_for_explicit_skippable_check() {
    let (_repo, orchestrator, _) = setup_repo_with_rulebook(
        r#"
            [fileset]
            Owned.path = "src/owned.rs"
            Other.path = "src/other.rs"

            [perspective.AuthImplementor]
            read = "Other"
            write = "Owned"

            [check]
            pipeline = ["test"]

            [check.command]
            test = "false"

            [check.policy]
            test = "skippable"
        "#,
    );
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    fs::write(provision.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 8 }\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/owned.rs"]);
    git(&provision.worktree_path, &["commit", "-m", "incr: skip explicit skippable check"]);
    let head_commit = git(&provision.worktree_path, &["rev-parse", "HEAD"]);

    worker.send_commit(head_commit, BundlePayload::default()).unwrap();
    let merge =
        orchestrator.merge_worker(provision.worker_id.clone(), vec!["test".to_owned()]).unwrap();

    assert_eq!(merge.state, WorkerState::Merged);
    assert!(merge.ran_checks.is_empty());
    assert_eq!(merge.skipped_checks, vec!["test"]);
}

#[test]
fn send_commit_reports_missing_commit_with_worktree_context() {
    let (_repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    let error = worker.send_commit("deadbeef".to_owned(), BundlePayload::default()).unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::CommitNotFound {
            operation,
            worktree_root,
            commit,
            ..
        } if operation == "verify submitted worker commit"
            && worktree_root == provision.worktree_path
            && commit == "deadbeef"
    ));
}
