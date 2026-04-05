use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use toml::Value;

use multorum::runtime::{
    BundlePayload, CreateWorker, FsOrchestratorService, FsWorkerService, MessageKind,
    OrchestratorService, ReplyReference, RuntimeError, Sequence, SequenceFilter, WorkerService,
    WorkerState,
};
use multorum::schema::perspective::PerspectiveName;
use multorum::schema::rulebook::{CheckName, Rulebook};
use multorum::vcs::VcsError;

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

fn git_worktree_list(root: &Path) -> String {
    git(root, &["worktree", "list", "--porcelain"])
}

fn git_path(root: &Path, path: &str) -> PathBuf {
    let resolved = git(root, &["rev-parse", "--git-path", path]);
    let path = PathBuf::from(resolved);
    if path.is_absolute() { path } else { root.join(path) }
}

fn setup_repo() -> (TempDir, FsOrchestratorService, String) {
    setup_repo_with_rulebook(rulebook_toml())
}

fn setup_repo_with_rulebook(rulebook_toml: &str) -> (TempDir, FsOrchestratorService, String) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
    FsOrchestratorService::new(dir.path()).unwrap().rulebook_init().unwrap();
    fs::write(dir.path().join(".multorum/rulebook.toml"), rulebook_toml).unwrap();

    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Multorum Test"]);
    git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: initialize runtime fixture"]);

    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
    (dir, orchestrator, head)
}

fn short_commit(commit: &str) -> String {
    commit.chars().take(12).collect()
}

fn audit_entry_id(worker: &str, head_commit: &str) -> String {
    let head_prefix =
        head_commit.get(..6).expect("runtime fixture commits must be at least 6 characters long");
    format!("{worker}-{head_prefix}")
}

fn read_group_state_toml(dir: &Path, perspective: &str) -> Value {
    let path = dir
        .canonicalize()
        .unwrap()
        .join(format!(".multorum/orchestrator/group/{perspective}.toml"));
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn read_worker_state_toml(dir: &Path, worker_id: &str) -> Value {
    let path =
        dir.canonicalize().unwrap().join(format!(".multorum/orchestrator/worker/{worker_id}.toml"));
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
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
    assert_eq!(fs::read_to_string(&init.gitignore_path).unwrap(), "orchestrator/\ntr/\n");
    assert!(init.multorum_root.join("orchestrator").is_dir());
    assert!(init.multorum_root.join("audit").is_dir());
    assert!(init.multorum_root.join("orchestrator/group").is_dir());
    assert!(init.multorum_root.join("orchestrator/worker").is_dir());
    assert!(init.multorum_root.join("orchestrator/exclusion-set.txt").is_file());
    assert!(fs::read_dir(init.multorum_root.join("orchestrator/group")).unwrap().next().is_none());
    assert!(fs::read_dir(init.multorum_root.join("orchestrator/worker")).unwrap().next().is_none());
    assert_eq!(
        fs::read_to_string(init.multorum_root.join("orchestrator/exclusion-set.txt")).unwrap(),
        ""
    );
    assert!(init.multorum_root.join("tr").is_dir());
    assert!(init.warnings.is_empty());

    let rulebook = Rulebook::from_workspace_root(dir.path()).unwrap();
    assert!(rulebook.fileset().definitions().is_empty());
    assert!(rulebook.perspective().declarations().is_empty());
    assert!(rulebook.check().pipeline().is_empty());
    let status = orchestrator.status().unwrap();
    assert!(status.active_perspectives.is_empty());
    assert!(status.workers.is_empty());
}

#[test]
fn rulebook_init_repairs_runtime_surface_without_overwriting_existing_rulebook() {
    let dir = tempfile::tempdir().unwrap();
    let rulebook_path = dir.path().join(".multorum/rulebook.toml");
    fs::create_dir_all(rulebook_path.parent().unwrap()).unwrap();
    fs::write(&rulebook_path, "[check]\npipeline = []\n").unwrap();
    fs::write(dir.path().join(".multorum/.gitignore"), "orchestrator/\n").unwrap();
    let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();

    let init = orchestrator.rulebook_init().unwrap();

    assert_eq!(init.rulebook_path, rulebook_path.canonicalize().unwrap());
    assert_eq!(fs::read_to_string(&rulebook_path).unwrap(), "[check]\npipeline = []\n");
    assert_eq!(
        fs::read_to_string(dir.path().join(".multorum/.gitignore")).unwrap(),
        "orchestrator/\ntr/\n"
    );
    assert!(dir.path().join(".multorum/audit").is_dir());
    assert_eq!(
        fs::read_to_string(dir.path().join(".multorum/orchestrator/exclusion-set.txt")).unwrap(),
        ""
    );
    assert!(dir.path().join(".multorum/orchestrator/group").is_dir());
    assert!(dir.path().join(".multorum/orchestrator/worker").is_dir());
    assert!(dir.path().join(".multorum/tr").is_dir());
    assert_eq!(init.warnings.len(), 1);
    assert!(init.warnings[0].contains(".multorum/.gitignore"));

    orchestrator.rulebook_init().unwrap();
    assert_eq!(fs::read_to_string(&rulebook_path).unwrap(), "[check]\npipeline = []\n");
    assert!(
        fs::read_dir(dir.path().join(".multorum/orchestrator/group")).unwrap().next().is_none()
    );
    assert!(
        fs::read_dir(dir.path().join(".multorum/orchestrator/worker")).unwrap().next().is_none()
    );
}

#[test]
fn rulebook_init_installs_pre_commit_hook_after_git_init() {
    let dir = tempfile::tempdir().unwrap();
    let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();

    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Multorum Test"]);
    git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);

    orchestrator.rulebook_init().unwrap();

    let hook_path = git_path(dir.path(), "hooks/pre-commit");
    let hook = fs::read_to_string(&hook_path).unwrap();
    assert!(
        hook.contains("BEGIN MULTORUM HOOK"),
        "expected rulebook init to inject the shared pre-commit hook"
    );
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
    assert!(provision.created_task_path.is_absolute());
    assert!(!task_body.exists(), "task body should be moved into the runtime bundle");

    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Active);
    let contract = worker.contract().unwrap();
    assert!(contract.read_set_path.is_absolute());
    assert!(contract.write_set_path.is_absolute());
    let inbox = worker.read_inbox(Default::default(), false).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].kind, MessageKind::Task);
    assert!(inbox[0].bundle_path.is_absolute());
    worker.ack_inbox(inbox[0].sequence).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Active);

    let hint_body = repo.path().join("hint.md");
    fs::write(&hint_body, "New API detail: gracefully block if the schema drifts.\n").unwrap();
    orchestrator
        .hint_worker(
            provision.worker_id.clone(),
            ReplyReference { in_reply_to: Some(inbox[0].sequence) },
            BundlePayload { body_path: Some(hint_body.clone()), ..BundlePayload::default() },
        )
        .unwrap();
    assert!(!hint_body.exists(), "hint body should be moved into the inbox bundle");

    let hinted = worker
        .read_inbox(
            SequenceFilter::Range { from: Some(Sequence(inbox[0].sequence.0 + 1)), to: None },
            false,
        )
        .unwrap();
    assert_eq!(hinted.len(), 1);
    assert_eq!(hinted[0].kind, MessageKind::Hint);
    worker.ack_inbox(hinted[0].sequence).unwrap();
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

    let outbox =
        orchestrator.read_outbox(provision.worker_id.clone(), Default::default(), false).unwrap();
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0].kind, MessageKind::Report);
    assert!(!outbox[0].acknowledged);
    orchestrator.ack_outbox(provision.worker_id.clone(), outbox[0].sequence).unwrap();
    let acknowledged =
        orchestrator.read_outbox(provision.worker_id.clone(), Default::default(), false).unwrap();
    assert!(acknowledged[0].acknowledged);

    let resolve_body = repo.path().join("resolve.md");
    fs::write(&resolve_body, "Use the existing API shape.\n").unwrap();
    orchestrator
        .resolve_worker(
            provision.worker_id.clone(),
            ReplyReference { in_reply_to: Some(report.message.sequence) },
            BundlePayload { body_path: Some(resolve_body.clone()), ..BundlePayload::default() },
            true,
        )
        .unwrap();
    assert!(!resolve_body.exists(), "resolve body should be moved into the inbox bundle");

    let follow_up = worker
        .read_inbox(
            SequenceFilter::Range {
                from: hinted.last().map(|m| Sequence(m.sequence.0 + 1)),
                to: None,
            },
            false,
        )
        .unwrap();
    assert_eq!(follow_up.len(), 1);
    assert_eq!(follow_up[0].kind, MessageKind::Resolve);
    assert!(follow_up[0].bundle_path.is_absolute());
    worker.ack_inbox(follow_up[0].sequence).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Active);
}

#[test]
fn worker_creation_without_payload_still_seeds_initial_task_bundle() {
    let (_repo, orchestrator, _) = setup_repo();

    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    assert!(provision.created_task_path.is_absolute());

    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();
    let inbox = worker.read_inbox(Default::default(), false).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].kind, MessageKind::Task);
    assert_eq!(inbox[0].sequence.0, 1);
}

#[test]
fn hint_worker_requires_active_state() {
    let (repo, orchestrator, _) = setup_repo();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&provision.worktree_path).unwrap();

    worker.send_report(None, ReplyReference::default(), BundlePayload::default()).unwrap();
    let hint_body = repo.path().join("hint.md");
    fs::write(&hint_body, "Please pause and report your current head.\n").unwrap();

    let error = orchestrator
        .hint_worker(
            provision.worker_id,
            ReplyReference::default(),
            BundlePayload { body_path: Some(hint_body.clone()), ..BundlePayload::default() },
        )
        .unwrap_err();

    assert!(hint_body.exists(), "hint body must remain in place after rejection");
    assert!(matches!(
        error,
        RuntimeError::InvalidState {
            operation: "publish hint bundle",
            expected: "ACTIVE",
            actual: WorkerState::Blocked,
        }
    ));
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

    let merge = orchestrator
        .merge_worker(provision.worker_id.clone(), Vec::new(), BundlePayload::default())
        .unwrap();
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
    orchestrator
        .merge_worker(first.worker_id.clone(), Vec::new(), BundlePayload::default())
        .unwrap();

    assert!(first.worktree_path.exists(), "merged worktree should be preserved");
    assert!(second.worktree_path.exists(), "discarded sibling worktree should be preserved");
    assert!(orchestrator.status().unwrap().workers.is_empty());
    let root = first.worktree_path.parent().unwrap().parent().unwrap().parent().unwrap();
    let group_state = read_group_state_toml(root, first.perspective.as_str());
    assert!(group_state["read_set"].as_array().unwrap().is_empty());
    assert!(group_state["write_set"].as_array().unwrap().is_empty());
}

#[test]
fn create_worker_uses_explicit_worker_id_when_requested() {
    let (_repo, orchestrator, _) = setup_repo();
    let worker_id: multorum::runtime::WorkerId = "custom-worker-7".parse().unwrap();

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
    let worker_id: multorum::runtime::WorkerId = "custom-worker-7".parse().unwrap();

    orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();

    let error = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap_err();
    assert!(matches!(error, RuntimeError::WorkerExists(actual) if actual == worker_id));
}

#[test]
fn create_worker_rejects_missing_perspective_even_with_live_group() {
    let (repo, orchestrator, _) = setup_repo();
    orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    fs::write(
        repo.path().join(".multorum/rulebook.toml"),
        r#"
            [fileset]
            Owned.glob = "src/owned.rs"
            Other.glob = "src/other.rs"

            [perspective]

            [check]
            pipeline = []
        "#,
    )
    .unwrap();

    let error = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap_err();
    assert!(matches!(error, RuntimeError::UnknownPerspective(name) if name == perspective()));
}

#[test]
fn create_worker_rejects_reused_explicit_worker_id_while_discarded_workspace_exists() {
    let (_repo, orchestrator, _) = setup_repo();
    let worker_id: multorum::runtime::WorkerId = "custom-worker-7".parse().unwrap();

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
    let worker_id: multorum::runtime::WorkerId = "custom-worker-7".parse().unwrap();

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
    let worker_id: multorum::runtime::WorkerId = "custom-worker-7".parse().unwrap();

    let first = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();
    let worker = FsWorkerService::new(&first.worktree_path).unwrap();
    fs::write(first.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 9 }\n").unwrap();
    git(&first.worktree_path, &["add", "src/owned.rs"]);
    git(&first.worktree_path, &["commit", "-m", "incr: finalize reused worker id"]);
    let head_commit = git(&first.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head_commit, BundlePayload::default()).unwrap();
    orchestrator.merge_worker(worker_id.clone(), Vec::new(), BundlePayload::default()).unwrap();
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
    let worker_id: multorum::runtime::WorkerId = "custom-worker-7".parse().unwrap();

    let first = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(worker_id.clone()))
        .unwrap();
    let worker = FsWorkerService::new(&first.worktree_path).unwrap();
    fs::write(first.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 9 }\n").unwrap();
    git(&first.worktree_path, &["add", "src/owned.rs"]);
    git(&first.worktree_path, &["commit", "-m", "incr: finalize reused worker id"]);
    let head_commit = git(&first.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head_commit, BundlePayload::default()).unwrap();
    orchestrator.merge_worker(worker_id.clone(), Vec::new(), BundlePayload::default()).unwrap();
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
    let (repo, orchestrator, _) = setup_repo();
    let created = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    orchestrator.discard_worker(created.worker_id.clone()).unwrap();
    assert!(created.worktree_path.exists());
    assert!(
        git_worktree_list(repo.path()).contains(created.worktree_path.to_string_lossy().as_ref())
    );

    let deleted = orchestrator.delete_worker(created.worker_id.clone()).unwrap();

    assert_eq!(deleted.worker_id, created.worker_id);
    assert_eq!(deleted.state, WorkerState::Discarded);
    assert_eq!(deleted.worktree_path, created.worktree_path);
    assert!(deleted.deleted_workspace);
    // Worker entry removed from persisted worker state during delete.
    assert!(!created.worktree_path.exists());
    assert!(
        !git_worktree_list(repo.path()).contains(created.worktree_path.to_string_lossy().as_ref())
    );

    let recreated = orchestrator
        .create_worker(CreateWorker::new(perspective()).with_worker_id(created.worker_id.clone()))
        .unwrap();
    assert_eq!(recreated.worker_id, created.worker_id);
    assert!(recreated.worktree_path.exists());
}

#[test]
fn delete_worker_rewrites_exclusion_set_projection() {
    let (repo, orchestrator, _) = setup_repo();
    let created = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    orchestrator.discard_worker(created.worker_id.clone()).unwrap();
    assert!(read_exclusion_set(repo.path()).is_empty());

    let exclusion_path =
        repo.path().canonicalize().unwrap().join(".multorum/orchestrator/exclusion-set.txt");
    fs::write(&exclusion_path, "src/owned.rs\n").unwrap();
    assert!(!read_exclusion_set(repo.path()).is_empty());

    orchestrator.delete_worker(created.worker_id).unwrap();
    assert!(read_exclusion_set(repo.path()).is_empty());
}

#[test]
fn delete_worker_clears_git_worktree_registration_after_manual_directory_removal() {
    let (repo, orchestrator, _) = setup_repo();
    let created = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    orchestrator.discard_worker(created.worker_id.clone()).unwrap();
    fs::remove_dir_all(&created.worktree_path).unwrap();
    assert!(!created.worktree_path.exists());
    assert!(
        git_worktree_list(repo.path()).contains(created.worktree_path.to_string_lossy().as_ref())
    );

    let deleted = orchestrator.delete_worker(created.worker_id.clone()).unwrap();

    assert!(deleted.deleted_workspace);
    // Worker entry removed from persisted worker state during delete.
    assert!(
        !git_worktree_list(repo.path()).contains(created.worktree_path.to_string_lossy().as_ref())
    );
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

    // Verify the canonical commit hash is stored in persisted worker state.
    let root = provision.worktree_path.parent().unwrap().parent().unwrap().parent().unwrap();
    let worker_state = read_worker_state_toml(root, provision.worker_id.as_str());
    assert_eq!(worker_state["submitted_head_commit"].as_str(), Some(head_commit.as_str()));

    let integration = orchestrator
        .merge_worker(provision.worker_id.clone(), Vec::new(), BundlePayload::default())
        .unwrap();
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

    let integration = orchestrator
        .merge_worker(provision.worker_id.clone(), Vec::new(), BundlePayload::default())
        .unwrap();
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
    let error = orchestrator
        .merge_worker(provision.worker_id.clone(), Vec::new(), BundlePayload::default())
        .unwrap_err();
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

    let error = orchestrator
        .merge_worker(provision.worker_id.clone(), Vec::new(), BundlePayload::default())
        .unwrap_err();
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
            Owned.glob = "src/owned.rs"
            Other.glob = "src/other.rs"

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
        .merge_worker(
            provision.worker_id.clone(),
            vec![CheckName::new("test").unwrap()],
            BundlePayload::default(),
        )
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
            Owned.glob = "src/owned.rs"
            Other.glob = "src/other.rs"

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
    let merge = orchestrator
        .merge_worker(
            provision.worker_id.clone(),
            vec![CheckName::new("test").unwrap()],
            BundlePayload::default(),
        )
        .unwrap();

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
        RuntimeError::Vcs(VcsError::CommitNotFound {
            operation,
            worktree_root,
            commit,
            ..
        }) if operation == "verify submitted worker commit"
            && worktree_root == provision.worktree_path
            && commit == "deadbeef"
    ));
}

// --- Exclusion-set and runtime-state behavior ---

fn read_exclusion_set(dir: &Path) -> BTreeSet<PathBuf> {
    let path = dir.canonicalize().unwrap().join(".multorum/orchestrator/exclusion-set.txt");
    if !path.exists() {
        return BTreeSet::new();
    }
    fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

#[test]
fn exclusion_set_tracks_active_worker_boundaries() {
    let (dir, orchestrator, _head) = setup_repo();

    // With no live workers the exclusion set is empty.
    assert!(read_exclusion_set(dir.path()).is_empty());

    // Creating a worker adds its read and write sets to the exclusion set.
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let exclusion = read_exclusion_set(dir.path());
    assert!(exclusion.contains(&PathBuf::from("src/owned.rs")), "write set file missing");
    assert!(exclusion.contains(&PathBuf::from("src/other.rs")), "read set file missing");

    // Discarding the worker clears the exclusion set.
    orchestrator.discard_worker(result.worker_id).unwrap();
    assert!(read_exclusion_set(dir.path()).is_empty());
}

#[test]
fn discard_worker_accepts_blocked_state_and_clears_exclusion_set() {
    let (dir, orchestrator, _head) = setup_repo();
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    let worker = FsWorkerService::new(&result.worktree_path).unwrap();

    worker.send_report(None, ReplyReference::default(), BundlePayload::default()).unwrap();
    assert_eq!(worker.status().unwrap().state, WorkerState::Blocked);
    assert!(!read_exclusion_set(dir.path()).is_empty());

    let discarded = orchestrator.discard_worker(result.worker_id.clone()).unwrap();
    assert_eq!(discarded.worker_id, result.worker_id);
    assert_eq!(discarded.state, WorkerState::Discarded);
    assert_eq!(worker.status().unwrap().state, WorkerState::Discarded);
    assert!(read_exclusion_set(dir.path()).is_empty());
    let worker_state = read_worker_state_toml(dir.path(), result.worker_id.as_str());
    assert_eq!(worker_state["state"].as_str().unwrap(), "discarded");
    let group_state = read_group_state_toml(dir.path(), result.perspective.as_str());
    assert!(group_state["read_set"].as_array().unwrap().is_empty());
    assert!(group_state["write_set"].as_array().unwrap().is_empty());
}

#[test]
fn exclusion_set_clears_after_merge() {
    let (dir, orchestrator, _head) = setup_repo();

    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    assert!(!read_exclusion_set(dir.path()).is_empty());

    // Commit a change in the worker worktree so we can merge.
    let worker = FsWorkerService::new(&result.worktree_path).unwrap();
    fs::write(result.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 42 }\n").unwrap();
    git(&result.worktree_path, &["add", "src/owned.rs"]);
    git(&result.worktree_path, &["commit", "-m", "feat: update owned"]);
    let head = git(&result.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head, BundlePayload::default()).unwrap();

    orchestrator.merge_worker(result.worker_id, vec![], BundlePayload::default()).unwrap();
    assert!(read_exclusion_set(dir.path()).is_empty());
}

#[test]
fn merge_accepts_empty_worker_commit_with_non_empty_submission_payload() {
    let (dir, orchestrator, _head) = setup_repo();
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    let worker = FsWorkerService::new(&result.worktree_path).unwrap();
    let evidence = result.worktree_path.join("analysis-evidence.txt");
    fs::write(&evidence, "analysis completed: no code changes required\n").unwrap();

    git(
        &result.worktree_path,
        &["commit", "--allow-empty", "-m", "docs: analysis-only worker submission"],
    );
    let head = git(&result.worktree_path, &["rev-parse", "HEAD"]);

    worker
        .send_commit(
            head.clone(),
            BundlePayload {
                body_text: Some("analysis-only result with no code diff".to_owned()),
                body_path: None,
                artifacts: vec![evidence.clone()],
            },
        )
        .unwrap();
    assert!(
        !evidence.exists(),
        "path-backed artifact should be moved into the worker outbox bundle"
    );

    let merged = orchestrator
        .merge_worker(result.worker_id.clone(), vec![], BundlePayload::default())
        .unwrap();
    assert_eq!(merged.state, WorkerState::Merged);
    assert_eq!(
        git(dir.path(), &["log", "-1", "--format=%s"]),
        "docs: analysis-only worker submission"
    );

    let audit_toml_path = dir.path().canonicalize().unwrap().join(format!(
        ".multorum/audit/{}/entry.toml",
        audit_entry_id(result.worker_id.as_str(), &head)
    ));
    let entry: toml::Value =
        toml::from_str(&fs::read_to_string(&audit_toml_path).unwrap()).unwrap();
    let changed = entry["changed_files"].as_array().unwrap();
    assert!(changed.is_empty(), "empty worker commit should record no changed files");
}

#[test]
fn merge_writes_audit_entry() {
    let (dir, orchestrator, _head) = setup_repo();
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    let worker = FsWorkerService::new(&result.worktree_path).unwrap();
    fs::write(result.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 99 }\n").unwrap();
    git(&result.worktree_path, &["add", "src/owned.rs"]);
    git(&result.worktree_path, &["commit", "-m", "feat: update owned"]);
    let head = git(&result.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head.clone(), BundlePayload::default()).unwrap();

    let rationale = BundlePayload {
        body_text: Some("Worker updated owned.rs with improved logic.".to_owned()),
        body_path: None,
        artifacts: vec![],
    };
    orchestrator.merge_worker(result.worker_id.clone(), vec![], rationale).unwrap();

    // Verify the audit TOML entry exists and contains expected fields.
    let audit_toml_path = dir.path().canonicalize().unwrap().join(format!(
        ".multorum/audit/{}/entry.toml",
        audit_entry_id(result.worker_id.as_str(), &head)
    ));
    assert!(audit_toml_path.exists(), "audit entry TOML missing");
    let entry: toml::Value =
        toml::from_str(&fs::read_to_string(&audit_toml_path).unwrap()).unwrap();
    assert_eq!(entry["worker"].as_str().unwrap(), result.worker_id.as_str());
    assert_eq!(entry["perspective"].as_str().unwrap(), "AuthImplementor");
    let changed = entry["changed_files"].as_array().unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].as_str().unwrap(), "src/owned.rs");

    // Verify the rationale body was written.
    let body_path = dir.path().canonicalize().unwrap().join(format!(
        ".multorum/audit/{}/body.md",
        audit_entry_id(result.worker_id.as_str(), &head)
    ));
    assert!(body_path.exists(), "audit rationale body missing");
    assert!(fs::read_to_string(&body_path).unwrap().contains("improved logic"));
}

#[test]
fn merge_writes_audit_entry_without_rationale() {
    let (dir, orchestrator, _head) = setup_repo();
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    let worker = FsWorkerService::new(&result.worktree_path).unwrap();
    fs::write(result.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 7 }\n").unwrap();
    git(&result.worktree_path, &["add", "src/owned.rs"]);
    git(&result.worktree_path, &["commit", "-m", "feat: update owned"]);
    let head = git(&result.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head.clone(), BundlePayload::default()).unwrap();

    orchestrator.merge_worker(result.worker_id.clone(), vec![], BundlePayload::default()).unwrap();

    // Audit entry exists even without rationale.
    let audit_toml_path = dir.path().canonicalize().unwrap().join(format!(
        ".multorum/audit/{}/entry.toml",
        audit_entry_id(result.worker_id.as_str(), &head)
    ));
    assert!(audit_toml_path.exists(), "audit entry TOML missing");
    let entry: toml::Value =
        toml::from_str(&fs::read_to_string(&audit_toml_path).unwrap()).unwrap();
    assert!(
        entry.get("rationale_body").is_none(),
        "rationale_body should be absent when no payload is supplied"
    );
}

#[test]
fn merge_rejects_existing_audit_entry_id_without_integrating_the_worker_commit() {
    let (dir, orchestrator, base_head) = setup_repo();
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    let worker = FsWorkerService::new(&result.worktree_path).unwrap();
    fs::write(result.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 17 }\n").unwrap();
    git(&result.worktree_path, &["add", "src/owned.rs"]);
    git(&result.worktree_path, &["commit", "-m", "incr: stage audit id collision"]);
    let head = git(&result.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head.clone(), BundlePayload::default()).unwrap();

    let entry_id = audit_entry_id(result.worker_id.as_str(), &head);
    let audit_entry_path = dir.path().join(format!(".multorum/audit/{entry_id}/entry.toml"));
    fs::create_dir_all(audit_entry_path.parent().unwrap()).unwrap();
    fs::write(&audit_entry_path, "worker = \"existing\"\n").unwrap();

    let error = orchestrator
        .merge_worker(result.worker_id.clone(), vec![], BundlePayload::default())
        .unwrap_err();

    match error {
        | RuntimeError::CheckFailed(message) => {
            assert!(message.contains("audit entry id already exists"));
            assert!(message.contains(&entry_id));
        }
        | other => panic!("expected CheckFailed for audit id collision, got: {other:?}"),
    }
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), base_head);
    assert_eq!(worker.status().unwrap().state, WorkerState::Committed);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 1 }\n"
    );
    assert!(audit_entry_path.exists(), "pre-existing audit entry must be preserved");
}

#[test]
fn merge_rejects_invalid_audit_payload_without_integrating_the_worker_commit() {
    let (dir, orchestrator, base_head) = setup_repo();
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    let worker = FsWorkerService::new(&result.worktree_path).unwrap();
    fs::write(result.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 13 }\n").unwrap();
    git(&result.worktree_path, &["add", "src/owned.rs"]);
    git(&result.worktree_path, &["commit", "-m", "incr: stage invalid audit payload"]);
    let head = git(&result.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head.clone(), BundlePayload::default()).unwrap();

    let exclusion_before = read_exclusion_set(dir.path());
    let rationale_body = dir.path().join("rationale.md");
    fs::write(&rationale_body, "This file should stay in place.\n").unwrap();
    let error = orchestrator
        .merge_worker(
            result.worker_id.clone(),
            vec![],
            BundlePayload {
                body_text: Some("inline body".to_owned()),
                body_path: Some(rationale_body.clone()),
                artifacts: vec![],
            },
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::Bundle(_)));
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), base_head);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 1 }\n"
    );
    assert_eq!(worker.status().unwrap().state, WorkerState::Committed);
    assert_eq!(read_exclusion_set(dir.path()), exclusion_before);
    assert!(rationale_body.exists(), "invalid audit payload must not be consumed");
    assert!(
        !dir.path()
            .join(format!(
                ".multorum/audit/{}/entry.toml",
                audit_entry_id(result.worker_id.as_str(), &head)
            ))
            .exists(),
        "audit entry must not be visible after merge rejection"
    );
}

#[test]
fn merge_rejects_duplicate_audit_artifact_names_without_integrating_the_worker_commit() {
    let (dir, orchestrator, base_head) = setup_repo();
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    let worker = FsWorkerService::new(&result.worktree_path).unwrap();
    fs::write(result.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 21 }\n").unwrap();
    git(&result.worktree_path, &["add", "src/owned.rs"]);
    git(&result.worktree_path, &["commit", "-m", "incr: stage duplicate artifacts"]);
    let head = git(&result.worktree_path, &["rev-parse", "HEAD"]);
    worker.send_commit(head.clone(), BundlePayload::default()).unwrap();

    let exclusion_before = read_exclusion_set(dir.path());
    let artifact_a = dir.path().join("audit-a/log.txt");
    let artifact_b = dir.path().join("audit-b/log.txt");
    fs::create_dir_all(artifact_a.parent().unwrap()).unwrap();
    fs::create_dir_all(artifact_b.parent().unwrap()).unwrap();
    fs::write(&artifact_a, "artifact a\n").unwrap();
    fs::write(&artifact_b, "artifact b\n").unwrap();

    let error = orchestrator
        .merge_worker(
            result.worker_id.clone(),
            vec![],
            BundlePayload {
                body_text: None,
                body_path: None,
                artifacts: vec![artifact_a.clone(), artifact_b.clone()],
            },
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::Bundle(_)));
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), base_head);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 1 }\n"
    );
    assert_eq!(worker.status().unwrap().state, WorkerState::Committed);
    assert_eq!(read_exclusion_set(dir.path()), exclusion_before);
    assert!(artifact_a.exists(), "duplicate artifact validation must not consume sources");
    assert!(artifact_b.exists(), "duplicate artifact validation must not consume sources");
    assert!(
        !dir.path()
            .join(format!(
                ".multorum/audit/{}/entry.toml",
                audit_entry_id(result.worker_id.as_str(), &head)
            ))
            .exists(),
        "audit entry must not be visible after merge rejection"
    );
}

#[test]
fn orchestrator_hook_rejects_commit_touching_excluded_file() {
    let (dir, orchestrator, _head) = setup_repo();
    let root = dir.path().canonicalize().unwrap();

    // Create a worker so its boundary populates the exclusion set.
    orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    assert!(!read_exclusion_set(dir.path()).is_empty());

    let hook_path = root.join(".git/hooks/pre-commit");
    assert!(hook_path.exists(), "expected orchestrator pre-commit hook to be installed");
    let excl_path = root.join(".multorum/orchestrator/exclusion-set.txt");
    assert!(excl_path.exists(), "expected exclusion-set file to be materialized");

    // The hook should now be installed. Stage a change to an excluded file
    // in the orchestrator workspace and try to commit.
    fs::write(root.join("src/owned.rs"), "// orchestrator edit\n").unwrap();
    git(&root, &["add", "src/owned.rs"]);
    let output = Command::new("git")
        .args(["commit", "-m", "should be rejected"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "commit touching excluded file should have been rejected by hook"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("exclusion set"),
        "hook error should mention exclusion set, got: {stderr}"
    );
}

#[test]
fn orchestrator_hook_allows_commit_outside_exclusion_set() {
    let (dir, orchestrator, _head) = setup_repo();
    let root = dir.path().canonicalize().unwrap();

    // Create a worker — exclusion set covers src/owned.rs and src/other.rs.
    orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    // Create and commit a file outside the exclusion set.
    fs::write(root.join("README.md"), "# hello\n").unwrap();
    git(&root, &["add", "README.md"]);
    let output = Command::new("git")
        .args(["commit", "-m", "add readme"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "commit outside exclusion set should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn worker_and_orchestrator_share_one_pre_commit_hook() {
    let (dir, orchestrator, _head) = setup_repo();
    let root = dir.path().canonicalize().unwrap();
    let provision = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();

    let orchestrator_hook = git_path(&root, "hooks/pre-commit");
    let worker_hook = git_path(&provision.worktree_path, "hooks/pre-commit");
    assert_eq!(
        orchestrator_hook.canonicalize().unwrap(),
        worker_hook.canonicalize().unwrap(),
        "worker and orchestrator must resolve the same pre-commit hook path"
    );

    fs::write(provision.worktree_path.join("src/other.rs"), "// worker edit outside write set\n")
        .unwrap();
    git(&provision.worktree_path, &["add", "src/other.rs"]);
    let worker_commit = Command::new("git")
        .args(["commit", "-m", "should be rejected"])
        .current_dir(&provision.worktree_path)
        .output()
        .unwrap();
    assert!(
        !worker_commit.status.success(),
        "worker commit outside write set should have been rejected by hook"
    );
    let worker_stderr = String::from_utf8_lossy(&worker_commit.stderr);
    assert!(
        worker_stderr.contains("outside write set"),
        "worker hook error should mention write set, got: {worker_stderr}"
    );

    fs::write(root.join("src/owned.rs"), "// orchestrator edit in exclusion set\n").unwrap();
    git(&root, &["add", "src/owned.rs"]);
    let orchestrator_commit = Command::new("git")
        .args(["commit", "-m", "should be rejected"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        !orchestrator_commit.status.success(),
        "orchestrator commit in exclusion set should have been rejected by hook"
    );
    let orchestrator_stderr = String::from_utf8_lossy(&orchestrator_commit.stderr);
    assert!(
        orchestrator_stderr.contains("exclusion set"),
        "orchestrator hook error should mention exclusion set, got: {orchestrator_stderr}"
    );
}
