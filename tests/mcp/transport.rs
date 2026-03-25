//! Handler-level dispatch and resource read tests.

use std::fs;
use std::path::Path;

use rmcp::ServerHandler;
use serde_json::json;

use multorum::mcp::transport::orchestrator::OrchestratorHandler;
use multorum::mcp::transport::worker::WorkerHandler;
use multorum::runtime::{
    BundlePayload, CreateWorker, FsOrchestratorService, FsWorkerService, OrchestratorService,
    WorkerService,
};

use crate::support::repo::{git, setup_repo};
use crate::support::result::{
    assert_tool_error, assert_tool_success, empty_args, json_args, resource_json, tool_json,
};
use crate::support::worker::{create_worker_runtime, perspective};

// ===========================================================================
// Orchestrator handler -- server info and descriptor counts
// ===========================================================================

#[test]
fn orchestrator_server_info() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let info = handler.get_info();
    assert_eq!(info.server_info.name, "multorum-orchestrator");
}

#[test]
fn orchestrator_tool_descriptor_count() {
    assert_eq!(multorum::mcp::tool::orchestrator::descriptors().len(), 15);
}

#[test]
fn orchestrator_resource_descriptor_count() {
    assert_eq!(multorum::mcp::resource::orchestrator::descriptors().len(), 3);
}

#[test]
fn orchestrator_resource_template_descriptor_count() {
    assert_eq!(multorum::mcp::resource::orchestrator::templates().len(), 2);
}

// ===========================================================================
// Orchestrator handler -- no-argument tool dispatch
// ===========================================================================

#[test]
fn orchestrator_get_status() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.dispatch("get_status", empty_args()).unwrap();
    assert_tool_success(&result);
    let status = tool_json(&result);
    assert!(status["workers"].as_array().unwrap().is_empty());
}

#[test]
fn orchestrator_list_perspectives() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.dispatch("list_perspectives", empty_args()).unwrap();
    assert_tool_success(&result);
    let perspectives = tool_json(&result);
    let arr = perspectives.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "AuthImplementor");
}

#[test]
fn orchestrator_forward_perspective_requires_blocked_worker() {
    let (_dir, svc) = setup_repo();
    svc.create_worker(CreateWorker::new(perspective())).unwrap();
    let handler = OrchestratorHandler::new(svc);
    let result = handler
        .dispatch("forward_perspective", json_args(json!({"perspective": "AuthImplementor"})))
        .unwrap();
    assert_tool_error(&result);
    assert_eq!(tool_json(&result)["code"], "check_failed");
}

#[test]
fn orchestrator_list_workers_empty() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.dispatch("list_workers", empty_args()).unwrap();
    assert_tool_success(&result);
    assert!(tool_json(&result).as_array().unwrap().is_empty());
}

#[test]
fn orchestrator_validate_perspectives() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let result = handler
        .dispatch(
            "validate_perspectives",
            json_args(json!({"perspectives": ["AuthImplementor"]})),
        )
        .unwrap();
    assert_tool_success(&result);
    let json = tool_json(&result);
    assert_eq!(json["ok"], true);
    assert_eq!(json["perspectives"].as_array().unwrap().len(), 1);
    assert!(json["conflicts"].as_array().unwrap().is_empty());
}

#[test]
fn orchestrator_rulebook_init_on_bare_directory() {
    let dir = tempfile::tempdir().unwrap();
    let svc = FsOrchestratorService::new(dir.path()).unwrap();
    let handler = OrchestratorHandler::new(svc);

    let result = handler.dispatch("rulebook_init", empty_args()).unwrap();
    assert_tool_success(&result);
    let json = tool_json(&result);
    assert!(json["rulebook_path"].as_str().unwrap().contains("rulebook.toml"));
}

// ===========================================================================
// Orchestrator handler -- worker lifecycle tools
// ===========================================================================

#[test]
fn orchestrator_create_worker() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let result = handler
        .dispatch("create_worker", json_args(json!({"perspective": "AuthImplementor"})))
        .unwrap();
    assert_tool_success(&result);
    let json = tool_json(&result);
    assert_eq!(json["perspective"], "AuthImplementor");
    assert_eq!(json["state"], "ACTIVE");
    assert!(json["worktree_path"].is_string());
}

#[test]
fn orchestrator_create_worker_with_explicit_id() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let result = handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "custom-1"})),
        )
        .unwrap();
    assert_tool_success(&result);
    assert_eq!(tool_json(&result)["worker_id"], "custom-1");
}

#[test]
fn orchestrator_get_worker() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let create = handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();
    assert_tool_success(&create);

    let result = handler.dispatch("get_worker", json_args(json!({"worker_id": "w1"}))).unwrap();
    assert_tool_success(&result);
    let detail = tool_json(&result);
    assert_eq!(detail["worker_id"], "w1");
    assert_eq!(detail["state"], "ACTIVE");
}

#[test]
fn orchestrator_read_worker_outbox() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let create = handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();

    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let report_body = Path::new(&worktree).join("report.md");
    fs::write(&report_body, "Need clarification.\n").unwrap();
    worker_svc
        .send_report(
            None,
            multorum::runtime::ReplyReference::default(),
            BundlePayload { body_path: Some(report_body), ..BundlePayload::default() },
        )
        .unwrap();

    let result =
        handler.dispatch("read_worker_outbox", json_args(json!({"worker_id": "w1"}))).unwrap();
    assert_tool_success(&result);
    let outbox = tool_json(&result);
    let messages = outbox.as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["kind"], "report");
}

#[test]
fn orchestrator_ack_worker_outbox_message() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let create = handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();

    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let report_body = Path::new(&worktree).join("report.md");
    fs::write(&report_body, "Need clarification.\n").unwrap();
    worker_svc
        .send_report(
            None,
            multorum::runtime::ReplyReference::default(),
            BundlePayload { body_path: Some(report_body), ..BundlePayload::default() },
        )
        .unwrap();

    let outbox =
        handler.dispatch("read_worker_outbox", json_args(json!({"worker_id": "w1"}))).unwrap();
    let sequence = tool_json(&outbox).as_array().unwrap()[0]["sequence"].as_u64().unwrap();

    let result = handler
        .dispatch(
            "ack_worker_outbox_message",
            json_args(json!({"worker_id": "w1", "sequence": sequence})),
        )
        .unwrap();
    assert_tool_success(&result);
}

#[test]
fn orchestrator_list_workers_after_create() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    handler
        .dispatch("create_worker", json_args(json!({"perspective": "AuthImplementor"})))
        .unwrap();

    let result = handler.dispatch("list_workers", empty_args()).unwrap();
    assert_tool_success(&result);
    assert_eq!(tool_json(&result).as_array().unwrap().len(), 1);
}

#[test]
fn orchestrator_discard_worker() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();

    let result = handler.dispatch("discard_worker", json_args(json!({"worker_id": "w1"}))).unwrap();
    assert_tool_success(&result);
}

#[test]
fn orchestrator_delete_worker_after_discard() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();
    handler.dispatch("discard_worker", json_args(json!({"worker_id": "w1"}))).unwrap();

    let result = handler.dispatch("delete_worker", json_args(json!({"worker_id": "w1"}))).unwrap();
    assert_tool_success(&result);
    let json = tool_json(&result);
    assert!(json["deleted_workspace"].as_bool().unwrap());
}

#[test]
fn orchestrator_resolve_worker() {
    let (dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let create = handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();

    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let report_body = Path::new(&worktree).join("report.md");
    fs::write(&report_body, "Need clarification.\n").unwrap();
    worker_svc
        .send_report(
            None,
            multorum::runtime::ReplyReference::default(),
            BundlePayload { body_path: Some(report_body), ..BundlePayload::default() },
        )
        .unwrap();

    let resolve_body = dir.path().join("resolve.md");
    fs::write(&resolve_body, "Use the existing API.\n").unwrap();
    let result = handler
        .dispatch(
            "resolve_worker",
            json_args(json!({
                "worker_id": "w1",
                "body": resolve_body.to_str().unwrap(),
            })),
        )
        .unwrap();
    assert_tool_success(&result);
    assert!(!resolve_body.exists(), "body should be moved into .multorum storage");
}

#[test]
fn orchestrator_revise_worker() {
    let (dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let create = handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();

    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    fs::write(Path::new(&worktree).join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n").unwrap();
    git(Path::new(&worktree), &["add", "src/owned.rs"]);
    git(Path::new(&worktree), &["commit", "-m", "incr: update owned"]);
    let head = git(Path::new(&worktree), &["rev-parse", "HEAD"]);
    worker_svc.send_commit(head, BundlePayload::default()).unwrap();

    let revise_body = dir.path().join("revise.md");
    fs::write(&revise_body, "Please adjust the return value.\n").unwrap();
    let result = handler
        .dispatch(
            "revise_worker",
            json_args(json!({
                "worker_id": "w1",
                "body": revise_body.to_str().unwrap(),
            })),
        )
        .unwrap();
    assert_tool_success(&result);
    assert!(!revise_body.exists(), "body should be moved into .multorum storage");
}

#[test]
fn orchestrator_merge_worker() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let create = handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();

    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    fs::write(Path::new(&worktree).join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n").unwrap();
    git(Path::new(&worktree), &["add", "src/owned.rs"]);
    git(Path::new(&worktree), &["commit", "-m", "incr: update owned"]);
    let head = git(Path::new(&worktree), &["rev-parse", "HEAD"]);
    worker_svc.send_commit(head, BundlePayload::default()).unwrap();

    let result = handler.dispatch("merge_worker", json_args(json!({"worker_id": "w1"}))).unwrap();
    assert_tool_success(&result);
    let merge = tool_json(&result);
    assert_eq!(merge["state"], "MERGED");
}

// ===========================================================================
// Orchestrator handler -- error cases
// ===========================================================================

#[test]
fn orchestrator_unknown_tool_returns_protocol_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.dispatch("nonexistent_tool", empty_args());
    assert!(result.is_err(), "unknown tool should return protocol-level error");
}

#[test]
fn orchestrator_missing_required_arg_returns_protocol_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.dispatch("get_worker", empty_args());
    assert!(result.is_err(), "missing worker_id should return protocol-level error");
}

#[test]
fn orchestrator_nonexistent_worker_returns_tool_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result =
        handler.dispatch("get_worker", json_args(json!({"worker_id": "does-not-exist"}))).unwrap();
    assert_tool_error(&result);
    let err = tool_json(&result);
    assert_eq!(err["code"], "unknown_worker");
}

#[test]
fn orchestrator_invalid_perspective_returns_protocol_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result =
        handler.dispatch("create_worker", json_args(json!({"perspective": "lowercase_bad"})));
    assert!(result.is_err(), "invalid perspective name should return protocol-level error");
}

#[test]
fn orchestrator_delete_active_worker_returns_tool_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();

    let result = handler.dispatch("delete_worker", json_args(json!({"worker_id": "w1"}))).unwrap();
    assert_tool_error(&result);
    let err = tool_json(&result);
    assert_eq!(err["code"], "invalid_state");
}

// ===========================================================================
// Orchestrator handler -- resource reads
// ===========================================================================

#[test]
fn orchestrator_resource_status() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.read("multorum://orchestrator/status").unwrap();
    let status = resource_json(&result);
    assert!(status["active_perspectives"].as_array().unwrap().is_empty());
    assert!(status["workers"].as_array().unwrap().is_empty());
}

#[test]
fn orchestrator_resource_perspectives() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.read("multorum://orchestrator/perspectives").unwrap();
    let perspectives = resource_json(&result);
    assert_eq!(perspectives.as_array().unwrap().len(), 1);
}

#[test]
fn orchestrator_resource_workers() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.read("multorum://orchestrator/workers").unwrap();
    assert!(resource_json(&result).as_array().unwrap().is_empty());
}

#[test]
fn orchestrator_resource_worker_detail() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();

    let result = handler.read("multorum://orchestrator/workers/w1").unwrap();
    let detail = resource_json(&result);
    assert_eq!(detail["worker_id"], "w1");
    assert_eq!(detail["state"], "ACTIVE");
}

#[test]
fn orchestrator_resource_unimplemented_returns_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();

    for sub in &["contract", "transcript", "checks"] {
        let uri = format!("multorum://orchestrator/workers/w1/{sub}");
        let result = handler.read(&uri);
        assert!(result.is_err(), "resource {uri} should return not-implemented error");
    }
}

#[test]
fn orchestrator_resource_worker_outbox() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let create = handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();

    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let report_body = Path::new(&worktree).join("report.md");
    fs::write(&report_body, "Need clarification.\n").unwrap();
    worker_svc
        .send_report(
            None,
            multorum::runtime::ReplyReference::default(),
            BundlePayload { body_path: Some(report_body), ..BundlePayload::default() },
        )
        .unwrap();

    let result = handler.read("multorum://orchestrator/workers/w1/outbox").unwrap();
    let outbox = resource_json(&result);
    let messages = outbox.as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["kind"], "report");
}

#[test]
fn orchestrator_resource_unknown_returns_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.read("multorum://orchestrator/nonexistent");
    assert!(result.is_err());
}

#[test]
fn orchestrator_resource_invalid_worker_id_returns_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.read("multorum://orchestrator/workers/!!!invalid");
    assert!(result.is_err());
}

#[test]
fn orchestrator_resource_unknown_worker_sub_resource_returns_error() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);
    let result = handler.read("multorum://orchestrator/workers/w1/nonexistent");
    assert!(result.is_err());
}

// ===========================================================================
// Worker handler -- server info and descriptor counts
// ===========================================================================

#[test]
fn worker_server_info() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);
    let info = handler.get_info();
    assert_eq!(info.server_info.name, "multorum-worker");
}

#[test]
fn worker_tool_descriptor_count() {
    assert_eq!(multorum::mcp::tool::worker::descriptors().len(), 6);
}

#[test]
fn worker_resource_descriptor_count() {
    assert_eq!(multorum::mcp::resource::worker::descriptors().len(), 3);
}

#[test]
fn worker_resource_template_descriptor_count() {
    assert!(multorum::mcp::resource::worker::templates().is_empty());
}

// ===========================================================================
// Worker handler -- tool dispatch
// ===========================================================================

#[test]
fn worker_get_contract() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.dispatch("get_contract", empty_args()).unwrap();
    assert_tool_success(&result);
    let contract = tool_json(&result);
    assert_eq!(contract["perspective"], "AuthImplementor");
    assert!(contract["base_commit"].is_string());
}

#[test]
fn worker_read_inbox() {
    let (_dir, svc) = setup_repo();

    use multorum::runtime::OrchestratorService;
    let task_body = tempfile::NamedTempFile::new().unwrap();
    fs::write(task_body.path(), "# initial task\n").unwrap();
    let provision = svc
        .create_worker(CreateWorker::new(perspective()).with_task(BundlePayload {
            body_path: Some(task_body.path().to_path_buf()),
            ..BundlePayload::default()
        }))
        .unwrap();

    let worker_svc = FsWorkerService::new(&provision.worktree_path).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.dispatch("read_inbox", empty_args()).unwrap();
    assert_tool_success(&result);
    let inbox = tool_json(&result);
    let messages = inbox.as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["kind"], "task");
}

#[test]
fn worker_read_inbox_with_after() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.dispatch("read_inbox", json_args(json!({"after": 999}))).unwrap();
    assert_tool_success(&result);
    assert!(tool_json(&result).as_array().unwrap().is_empty());
}

#[test]
fn worker_ack_inbox_message() {
    let (_dir, svc) = setup_repo();

    use multorum::runtime::OrchestratorService;
    let task_body = tempfile::NamedTempFile::new().unwrap();
    fs::write(task_body.path(), "# initial task\n").unwrap();
    let provision = svc
        .create_worker(CreateWorker::new(perspective()).with_task(BundlePayload {
            body_path: Some(task_body.path().to_path_buf()),
            ..BundlePayload::default()
        }))
        .unwrap();

    let worker_svc = FsWorkerService::new(&provision.worktree_path).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let inbox_result = handler.dispatch("read_inbox", empty_args()).unwrap();
    let sequence = tool_json(&inbox_result).as_array().unwrap()[0]["sequence"].as_u64().unwrap();

    let result =
        handler.dispatch("ack_inbox_message", json_args(json!({"sequence": sequence}))).unwrap();
    assert_tool_success(&result);
}

#[test]
fn worker_send_report() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();

    let inbox = worker_svc.read_inbox(None).unwrap();
    for msg in &inbox {
        worker_svc.ack_inbox(msg.sequence).unwrap();
    }

    let handler = WorkerHandler::new(worker_svc);
    let report_body = worktree.join("report.md");
    fs::write(&report_body, "Blocked on design question.\n").unwrap();
    let result = handler
        .dispatch("send_report", json_args(json!({"body": report_body.to_str().unwrap()})))
        .unwrap();
    assert_tool_success(&result);
    assert!(!report_body.exists(), "report body should be moved into .multorum storage");
}

#[test]
fn worker_send_report_accepts_inline_body_text() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();

    let inbox = worker_svc.read_inbox(None).unwrap();
    for msg in &inbox {
        worker_svc.ack_inbox(msg.sequence).unwrap();
    }

    let handler = WorkerHandler::new(worker_svc);
    let result = handler
        .dispatch(
            "send_report",
            json_args(
                json!({"body_text": "Blocked on design question.\nNeed orchestrator input."}),
            ),
        )
        .unwrap();
    assert_tool_success(&result);
    let body =
        fs::read_to_string(worktree.join(".multorum/outbox/new/0001-report/body.md")).unwrap();
    assert!(body.contains("Blocked on design question."));
}

#[test]
fn worker_send_commit() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    fs::write(worktree.join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n").unwrap();
    git(&worktree, &["add", "src/owned.rs"]);
    git(&worktree, &["commit", "-m", "incr: update owned"]);
    let head = git(&worktree, &["rev-parse", "HEAD"]);

    let result = handler.dispatch("send_commit", json_args(json!({"head_commit": head}))).unwrap();
    assert_tool_success(&result);
}

#[test]
fn worker_send_commit_accepts_inline_body_text() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    fs::write(worktree.join("src/owned.rs"), "pub fn owned() -> i32 { 7 }\n").unwrap();
    git(&worktree, &["add", "src/owned.rs"]);
    git(&worktree, &["commit", "-m", "incr: update owned"]);
    let head = git(&worktree, &["rev-parse", "HEAD"]);

    let result = handler
        .dispatch(
            "send_commit",
            json_args(json!({
                "head_commit": head,
                "body_text": "Implemented the requested owned.rs update.\nNo known limitations."
            })),
        )
        .unwrap();
    assert_tool_success(&result);
    let body =
        fs::read_to_string(worktree.join(".multorum/outbox/new/0001-commit/body.md")).unwrap();
    assert!(body.contains("Implemented the requested owned.rs update."));
}

#[test]
fn worker_get_status() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.dispatch("get_status", empty_args()).unwrap();
    assert_tool_success(&result);
    let status = tool_json(&result);
    assert_eq!(status["state"], "ACTIVE");
}

// ===========================================================================
// Worker handler -- error cases
// ===========================================================================

#[test]
fn worker_unknown_tool_returns_protocol_error() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);
    let result = handler.dispatch("nonexistent_tool", empty_args());
    assert!(result.is_err());
}

#[test]
fn worker_missing_required_sequence_returns_protocol_error() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);
    let result = handler.dispatch("ack_inbox_message", empty_args());
    assert!(result.is_err());
}

#[test]
fn worker_missing_required_head_commit_returns_protocol_error() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);
    let result = handler.dispatch("send_commit", empty_args());
    assert!(result.is_err());
}

#[test]
fn worker_invalid_commit_returns_tool_error() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);
    let result =
        handler.dispatch("send_commit", json_args(json!({"head_commit": "deadbeef"}))).unwrap();
    assert_tool_error(&result);
}

#[test]
fn worker_ack_nonexistent_sequence_returns_tool_error() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);
    let result =
        handler.dispatch("ack_inbox_message", json_args(json!({"sequence": 9999}))).unwrap();
    assert_tool_error(&result);
}

// ===========================================================================
// Worker handler -- resource reads
// ===========================================================================

#[test]
fn worker_resource_contract() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.read("multorum://worker/contract").unwrap();
    let contract = resource_json(&result);
    assert_eq!(contract["perspective"], "AuthImplementor");
}

#[test]
fn worker_resource_inbox() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.read("multorum://worker/inbox").unwrap();
    let inbox = resource_json(&result);
    assert!(inbox.is_array());
}

#[test]
fn worker_resource_status() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.read("multorum://worker/status").unwrap();
    let status = resource_json(&result);
    assert_eq!(status["state"], "ACTIVE");
}

#[test]
fn worker_resource_unimplemented_returns_error() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    for uri in &[
        "multorum://worker/read-set",
        "multorum://worker/write-set",
        "multorum://worker/outbox",
        "multorum://worker/transcript",
    ] {
        let result = handler.read(uri);
        assert!(result.is_err(), "resource {uri} should return not-implemented error");
    }
}

#[test]
fn worker_resource_unknown_returns_error() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.read("multorum://worker/nonexistent");
    assert!(result.is_err());
}

// ===========================================================================
// Full cross-handler workflow
// ===========================================================================

#[test]
fn full_workflow_create_commit_merge_via_mcp() {
    let (repo, svc) = setup_repo();
    let orch = OrchestratorHandler::new(svc);

    let create = orch
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "mcp-w1"})),
        )
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();

    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let worker = WorkerHandler::new(worker_svc);

    let contract = worker.dispatch("get_contract", empty_args()).unwrap();
    assert_tool_success(&contract);
    assert_eq!(tool_json(&contract)["perspective"], "AuthImplementor");

    let status = worker.dispatch("get_status", empty_args()).unwrap();
    assert_eq!(tool_json(&status)["state"], "ACTIVE");

    fs::write(Path::new(&worktree).join("src/owned.rs"), "pub fn owned() -> i32 { 42 }\n").unwrap();
    git(Path::new(&worktree), &["add", "src/owned.rs"]);
    git(Path::new(&worktree), &["commit", "-m", "incr: answer to everything"]);
    let head = git(Path::new(&worktree), &["rev-parse", "HEAD"]);

    let commit = worker.dispatch("send_commit", json_args(json!({"head_commit": head}))).unwrap();
    assert_tool_success(&commit);

    let status = worker.dispatch("get_status", empty_args()).unwrap();
    assert_eq!(tool_json(&status)["state"], "COMMITTED");

    let merge = orch.dispatch("merge_worker", json_args(json!({"worker_id": "mcp-w1"}))).unwrap();
    assert_tool_success(&merge);
    assert_eq!(tool_json(&merge)["state"], "MERGED");

    assert_eq!(
        fs::read_to_string(repo.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 42 }\n"
    );

    let workers = orch.dispatch("list_workers", empty_args()).unwrap();
    assert!(tool_json(&workers).as_array().unwrap().is_empty());
}
