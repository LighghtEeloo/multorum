//! Scenario-driven MCP integration tests.
//!
//! These tests exercise multi-step, stateful workflows that reflect
//! real-world MCP usage: bidding group lifecycles, blocker resolution
//! cycles, invalid state transitions, write-set enforcement, and
//! check pipeline execution.

use std::fs;
use std::path::Path;

use rmcp::model::CallToolRequestParams;
use serde_json::json;

use crate::support::repo::{git, setup_multi_perspective_repo};
use crate::support::result::{
    assert_tool_error_code, assert_tool_success, create_worker_args, json_args, merge_worker_args,
    send_commit_args, tool_json, tool_text,
};
use crate::support::wire::{orchestrator_duplex, worker_duplex};

// ===========================================================================
// Bidding group lifecycle
// ===========================================================================

/// Two workers from the same perspective form a bidding group. When one
/// is merged the sibling is automatically discarded.
#[tokio::test]
async fn bidding_group_sibling_discarded_on_merge() {
    let (dir, orch) = orchestrator_duplex().await;

    // Create two workers for the same perspective.
    let create_a = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("bg-a"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create_a);
    let worktree_a = tool_json(&create_a)["worktree_path"].as_str().unwrap().to_string();

    let create_b = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("bg-b"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create_b);

    // Worker A modifies its write-set file, commits, and submits.
    let wt_a = Path::new(&worktree_a);
    fs::write(wt_a.join("src/owned.rs"), "pub fn owned() -> i32 { 99 }\n").unwrap();
    git(wt_a, &["add", "src/owned.rs"]);
    git(wt_a, &["commit", "-m", "incr: bidding group winner"]);
    let head_a = git(wt_a, &["rev-parse", "HEAD"]);

    let worker_a = worker_duplex(wt_a).await;
    let commit = worker_a
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head_a)),
        )
        .await
        .unwrap();
    assert_tool_success(&commit);
    worker_a.cancel().await.unwrap();

    // Orchestrator merges worker A.
    let merge = orch
        .call_tool(
            CallToolRequestParams::new("merge_worker").with_arguments(merge_worker_args("bg-a")),
        )
        .await
        .unwrap();
    assert_tool_success(&merge);
    assert_eq!(tool_json(&merge)["state"], "merged");

    // Sibling worker B should now be discarded.
    let detail_b = orch
        .call_tool(
            CallToolRequestParams::new("get_worker")
                .with_arguments(json_args(json!({"worker": "bg-b"}))),
        )
        .await
        .unwrap();
    assert_tool_success(&detail_b);
    assert_eq!(tool_json(&detail_b)["state"], "discarded");

    // The merged file should be in the canonical workspace.
    assert_eq!(
        fs::read_to_string(dir.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 99 }\n"
    );

    orch.cancel().await.unwrap();
}

/// Workers from different perspectives are independent and do not
/// discard each other on merge.
#[tokio::test]
async fn different_perspectives_independent_on_merge() {
    let (dir, svc) = setup_multi_perspective_repo();

    use multorum::mcp::transport::orchestrator::OrchestratorHandler;
    use multorum::mcp::transport::worker::WorkerHandler;
    use multorum::runtime::FsWorkerService;
    let orch = OrchestratorHandler::with_service(svc);

    // Create one worker per perspective.
    let create_auth = orch
        .dispatch("create_worker", create_worker_args("AuthImplementor", Some("auth-w")))
        .unwrap();
    assert_tool_success(&create_auth);
    let wt_auth = tool_json(&create_auth)["worktree_path"].as_str().unwrap().to_string();

    let create_data = orch
        .dispatch("create_worker", create_worker_args("DataImplementor", Some("data-w")))
        .unwrap();
    assert_tool_success(&create_data);
    let wt_data = tool_json(&create_data)["worktree_path"].as_str().unwrap().to_string();

    // Auth worker commits.
    fs::write(Path::new(&wt_auth).join("src/auth.rs"), "pub fn auth() -> i32 { 10 }\n").unwrap();
    git(Path::new(&wt_auth), &["add", "src/auth.rs"]);
    git(Path::new(&wt_auth), &["commit", "-m", "incr: auth work"]);
    let head_auth = git(Path::new(&wt_auth), &["rev-parse", "HEAD"]);

    let auth_worker = WorkerHandler::with_service(FsWorkerService::new(&wt_auth).unwrap());
    let commit_auth = auth_worker.dispatch("send_commit", send_commit_args(&head_auth)).unwrap();
    assert_tool_success(&commit_auth);

    // Merge auth worker.
    let merge_auth = orch.dispatch("merge_worker", merge_worker_args("auth-w")).unwrap();
    assert_tool_success(&merge_auth);
    assert_eq!(tool_json(&merge_auth)["state"], "merged");

    // Data worker should still be ACTIVE (different perspective).
    let detail_data = orch.dispatch("get_worker", json_args(json!({"worker": "data-w"}))).unwrap();
    assert_tool_success(&detail_data);
    assert_eq!(tool_json(&detail_data)["state"], "active");

    // Data worker can still commit and merge independently.
    fs::write(Path::new(&wt_data).join("src/data.rs"), "pub fn data() -> i32 { 20 }\n").unwrap();
    git(Path::new(&wt_data), &["add", "src/data.rs"]);
    git(Path::new(&wt_data), &["commit", "-m", "incr: data work"]);
    let head_data = git(Path::new(&wt_data), &["rev-parse", "HEAD"]);

    let data_worker = WorkerHandler::with_service(FsWorkerService::new(&wt_data).unwrap());
    let commit_data = data_worker.dispatch("send_commit", send_commit_args(&head_data)).unwrap();
    assert_tool_success(&commit_data);

    let merge_data = orch.dispatch("merge_worker", merge_worker_args("data-w")).unwrap();
    assert_tool_success(&merge_data);
    assert_eq!(tool_json(&merge_data)["state"], "merged");

    // Both changes should be present in the canonical workspace.
    assert_eq!(
        fs::read_to_string(dir.path().join("src/auth.rs")).unwrap(),
        "pub fn auth() -> i32 { 10 }\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("src/data.rs")).unwrap(),
        "pub fn data() -> i32 { 20 }\n"
    );
}

// ===========================================================================
// Blocker resolution cycle
// ===========================================================================

/// Full report -> resolve -> ack -> modify -> commit -> merge cycle.
#[tokio::test]
async fn blocker_report_resolve_then_commit() {
    let (dir, orch) = orchestrator_duplex().await;

    // Create worker.
    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("block-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    // Worker sends a blocker report.
    let report_body = wt.join("report.md");
    fs::write(&report_body, "Need API specification.\n").unwrap();

    let worker_client = worker_duplex(wt).await;
    let report = worker_client
        .call_tool(
            CallToolRequestParams::new("send_report")
                .with_arguments(json_args(json!({"body_path": report_body.to_str().unwrap()}))),
        )
        .await
        .unwrap();
    assert_tool_success(&report);

    // Worker should now be BLOCKED.
    let status = worker_client.call_tool(CallToolRequestParams::new("get_status")).await.unwrap();
    assert_eq!(tool_json(&status)["state"], "blocked");
    worker_client.cancel().await.unwrap();

    // Orchestrator reads the outbox.
    let outbox = orch
        .call_tool(
            CallToolRequestParams::new("read_worker_outbox")
                .with_arguments(json_args(json!({"worker": "block-w"}))),
        )
        .await
        .unwrap();
    assert_tool_success(&outbox);
    let messages = tool_json(&outbox);
    let messages = messages.as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["kind"], "report");
    let report_seq = messages[0]["sequence"].as_u64().unwrap();

    // Orchestrator acknowledges the report.
    let ack = orch
        .call_tool(CallToolRequestParams::new("ack_worker_outbox_message").with_arguments(
            json_args(json!({
                "worker": "block-w",
                "sequence": report_seq,
            })),
        ))
        .await
        .unwrap();
    assert_tool_success(&ack);

    // Orchestrator resolves with instructions.
    let resolve_body = dir.path().join("resolve.md");
    fs::write(&resolve_body, "Use the REST API documented at /docs/api.md.\n").unwrap();
    let resolve = orch
        .call_tool(CallToolRequestParams::new("resolve_worker").with_arguments(json_args(json!({
            "worker": "block-w",
            "body_path": resolve_body.to_str().unwrap(),
        }))))
        .await
        .unwrap();
    assert_tool_success(&resolve);

    // Worker reads inbox, acks the resolve, should be ACTIVE again.
    let worker_client2 = worker_duplex(wt).await;
    let inbox = worker_client2.call_tool(CallToolRequestParams::new("read_inbox")).await.unwrap();
    assert_tool_success(&inbox);
    let inbox_msgs = tool_json(&inbox);
    let inbox_msgs = inbox_msgs.as_array().unwrap();
    let resolve_msg = inbox_msgs.iter().find(|m| m["kind"] == "resolve").unwrap();
    let resolve_seq = resolve_msg["sequence"].as_u64().unwrap();

    let ack_inbox = worker_client2
        .call_tool(
            CallToolRequestParams::new("ack_inbox_message")
                .with_arguments(json_args(json!({"sequence": resolve_seq}))),
        )
        .await
        .unwrap();
    assert_tool_success(&ack_inbox);

    // Worker should be ACTIVE after acking the resolve.
    let status2 = worker_client2.call_tool(CallToolRequestParams::new("get_status")).await.unwrap();
    assert_eq!(tool_json(&status2)["state"], "active");

    // Worker modifies files, commits, and submits.
    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 777 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "-m", "incr: implement after resolve"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let commit = worker_client2
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head)),
        )
        .await
        .unwrap();
    assert_tool_success(&commit);
    worker_client2.cancel().await.unwrap();

    // Orchestrator merges.
    let merge = orch
        .call_tool(
            CallToolRequestParams::new("merge_worker").with_arguments(merge_worker_args("block-w")),
        )
        .await
        .unwrap();
    assert_tool_success(&merge);
    assert_eq!(tool_json(&merge)["state"], "merged");

    assert_eq!(
        fs::read_to_string(dir.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 777 }\n"
    );

    orch.cancel().await.unwrap();
}

/// Revise cycle: worker commits, orchestrator revises, worker re-commits.
#[tokio::test]
async fn revise_cycle_resubmit_and_merge() {
    let (dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("rev-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    // Worker submits first attempt.
    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 3 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "-m", "incr: first attempt"]);
    let head1 = git(wt, &["rev-parse", "HEAD"]);

    let w1 = worker_duplex(wt).await;
    let commit1 = w1
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head1)),
        )
        .await
        .unwrap();
    assert_tool_success(&commit1);
    assert_eq!(
        tool_json(&w1.call_tool(CallToolRequestParams::new("get_status")).await.unwrap())["state"],
        "committed"
    );
    w1.cancel().await.unwrap();

    // Orchestrator revises.
    let revise_body = dir.path().join("revise.md");
    fs::write(&revise_body, "Return value should be 42.\n").unwrap();
    let revise = orch
        .call_tool(CallToolRequestParams::new("revise_worker").with_arguments(json_args(json!({
            "worker": "rev-w",
            "body_path": revise_body.to_str().unwrap(),
        }))))
        .await
        .unwrap();
    assert_tool_success(&revise);

    // Worker acks the revise, goes back to ACTIVE.
    let w2 = worker_duplex(wt).await;
    let inbox = w2.call_tool(CallToolRequestParams::new("read_inbox")).await.unwrap();
    let inbox_msgs = tool_json(&inbox);
    let revise_msg = inbox_msgs.as_array().unwrap().iter().find(|m| m["kind"] == "revise").unwrap();
    let revise_seq = revise_msg["sequence"].as_u64().unwrap();

    let ack = w2
        .call_tool(
            CallToolRequestParams::new("ack_inbox_message")
                .with_arguments(json_args(json!({"sequence": revise_seq}))),
        )
        .await
        .unwrap();
    assert_tool_success(&ack);
    assert_eq!(
        tool_json(&w2.call_tool(CallToolRequestParams::new("get_status")).await.unwrap())["state"],
        "active"
    );

    // Worker re-commits with the corrected value, amending the
    // previous commit so the cherry-pick diff is relative to the base.
    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 42 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "--amend", "-m", "incr: correct return value"]);
    let head2 = git(wt, &["rev-parse", "HEAD"]);

    let commit2 = w2
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head2)),
        )
        .await
        .unwrap();
    assert_tool_success(&commit2);
    w2.cancel().await.unwrap();

    // Orchestrator merges the revised submission.
    let merge = orch
        .call_tool(
            CallToolRequestParams::new("merge_worker").with_arguments(merge_worker_args("rev-w")),
        )
        .await
        .unwrap();
    assert_tool_success(&merge);
    assert_eq!(tool_json(&merge)["state"], "merged");

    assert_eq!(
        fs::read_to_string(dir.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 42 }\n"
    );

    orch.cancel().await.unwrap();
}

// ===========================================================================
// Invalid state transitions
// ===========================================================================

/// Merging an ACTIVE worker (no commit sent) returns `invalid_state`.
#[tokio::test]
async fn merge_active_worker_rejected() {
    let (_dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("st-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);

    let merge = orch
        .call_tool(
            CallToolRequestParams::new("merge_worker").with_arguments(merge_worker_args("st-w")),
        )
        .await
        .unwrap();
    assert_tool_error_code(&merge, "invalid_state");

    orch.cancel().await.unwrap();
}

/// Sending a second commit from a COMMITTED worker is rejected.
#[tokio::test]
async fn double_commit_rejected() {
    let (_dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("dc-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    // First commit.
    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 5 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "-m", "incr: first"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let w = worker_duplex(wt).await;
    let first = w
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head)),
        )
        .await
        .unwrap();
    assert_tool_success(&first);

    // Second commit attempt while still COMMITTED.
    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 6 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "-m", "incr: second"]);
    let head2 = git(wt, &["rev-parse", "HEAD"]);

    let second = w
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head2)),
        )
        .await
        .unwrap();
    assert_tool_error_code(&second, "invalid_state");

    w.cancel().await.unwrap();
    orch.cancel().await.unwrap();
}

/// Sending a report from a COMMITTED worker is rejected.
#[tokio::test]
async fn report_from_committed_worker_rejected() {
    let (_dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("rpt-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 7 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "-m", "incr: commit first"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let w = worker_duplex(wt).await;
    let commit = w
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head)),
        )
        .await
        .unwrap();
    assert_tool_success(&commit);

    // Try to send a report while COMMITTED.
    let report_body = wt.join("report.md");
    fs::write(&report_body, "Actually I have a question.\n").unwrap();
    let report = w
        .call_tool(
            CallToolRequestParams::new("send_report")
                .with_arguments(json_args(json!({"body_path": report_body.to_str().unwrap()}))),
        )
        .await
        .unwrap();
    assert_tool_error_code(&report, "invalid_state");

    w.cancel().await.unwrap();
    orch.cancel().await.unwrap();
}

/// Resolving a worker that is not BLOCKED returns `invalid_state`.
#[tokio::test]
async fn resolve_active_worker_rejected() {
    let (dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("res-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);

    let resolve_body = dir.path().join("resolve.md");
    fs::write(&resolve_body, "You should be blocked first.\n").unwrap();
    let resolve = orch
        .call_tool(CallToolRequestParams::new("resolve_worker").with_arguments(json_args(json!({
            "worker": "res-w",
            "body_path": resolve_body.to_str().unwrap(),
        }))))
        .await
        .unwrap();
    assert_tool_error_code(&resolve, "invalid_state");

    orch.cancel().await.unwrap();
}

/// Revising a worker that is not COMMITTED returns `invalid_state`.
#[tokio::test]
async fn revise_active_worker_rejected() {
    let (dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("reva-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);

    let revise_body = dir.path().join("revise.md");
    fs::write(&revise_body, "Please redo.\n").unwrap();
    let revise = orch
        .call_tool(CallToolRequestParams::new("revise_worker").with_arguments(json_args(json!({
            "worker": "reva-w",
            "body_path": revise_body.to_str().unwrap(),
        }))))
        .await
        .unwrap();
    assert_tool_error_code(&revise, "invalid_state");

    orch.cancel().await.unwrap();
}

/// Deleting an ACTIVE worker (not finalized) returns `invalid_state`.
#[tokio::test]
async fn delete_active_worker_rejected() {
    let (_dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("del-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);

    let delete = orch
        .call_tool(
            CallToolRequestParams::new("delete_worker")
                .with_arguments(json_args(json!({"worker": "del-w"}))),
        )
        .await
        .unwrap();
    assert_tool_error_code(&delete, "invalid_state");

    orch.cancel().await.unwrap();
}

// ===========================================================================
// Write-set enforcement
// ===========================================================================

/// A worker that modifies a file outside its write set is rejected at
/// merge time with `write_set_violation`.
#[tokio::test]
async fn write_set_violation_on_merge() {
    let (_dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("ws-w"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    // Worker modifies a file outside its write set (src/other.rs belongs
    // to the read set, not the write set). Use --no-verify to bypass
    // the pre-commit hook so we can test the server-side write-set
    // enforcement in the merge path.
    fs::write(wt.join("src/other.rs"), "pub fn other() -> i32 { 999 }\n").unwrap();
    git(wt, &["add", "src/other.rs"]);
    git(wt, &["commit", "--no-verify", "-m", "incr: touch read-only file"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let w = worker_duplex(wt).await;
    let commit = w
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head)),
        )
        .await
        .unwrap();
    assert_tool_success(&commit);
    w.cancel().await.unwrap();

    // Merge should fail with write_set_violation.
    let merge = orch
        .call_tool(
            CallToolRequestParams::new("merge_worker").with_arguments(merge_worker_args("ws-w")),
        )
        .await
        .unwrap();
    assert_tool_error_code(&merge, "write_set_violation");

    // Verify the error message mentions the violating file.
    let err_msg = tool_text(&merge);
    assert!(err_msg.contains("other.rs"), "error should mention other.rs, got: {err_msg}");

    orch.cancel().await.unwrap();
}

/// A worker that modifies only write-set files merges successfully.
#[tokio::test]
async fn write_set_respected_merge_succeeds() {
    let (_dir, orch) = orchestrator_duplex().await;

    let create = orch
        .call_tool(
            CallToolRequestParams::new("create_worker")
                .with_arguments(create_worker_args("AuthImplementor", Some("ws-ok"))),
        )
        .await
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    // Only touch the allowed file.
    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 100 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "-m", "incr: update owned only"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let w = worker_duplex(wt).await;
    let commit = w
        .call_tool(
            CallToolRequestParams::new("send_commit").with_arguments(send_commit_args(&head)),
        )
        .await
        .unwrap();
    assert_tool_success(&commit);
    w.cancel().await.unwrap();

    let merge = orch
        .call_tool(
            CallToolRequestParams::new("merge_worker").with_arguments(merge_worker_args("ws-ok")),
        )
        .await
        .unwrap();
    assert_tool_success(&merge);
    assert_eq!(tool_json(&merge)["state"], "merged");

    orch.cancel().await.unwrap();
}

// ===========================================================================
// Check pipeline
// ===========================================================================

/// Merge with a failing check returns `check_failed`.
#[tokio::test]
async fn check_pipeline_failure_blocks_merge() {
    use multorum::mcp::transport::orchestrator::OrchestratorHandler;
    use multorum::mcp::transport::worker::WorkerHandler;
    use multorum::runtime::FsWorkerService;

    // Set up a repo with a check that will fail.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
    use multorum::runtime::{FsOrchestratorService, OrchestratorService};
    FsOrchestratorService::new(dir.path()).unwrap().rulebook_init().unwrap();
    fs::write(
        dir.path().join(".multorum/rulebook.toml"),
        r#"
            [fileset]
            Owned.path = "src/owned.rs"
            Other.path = "src/other.rs"

            [perspective.AuthImplementor]
            read = "Other"
            write = "Owned"

            [check]
            pipeline = ["lint"]

            [check.command]
            lint = "exit 1"

            [check.policy]
            lint = "skippable"
        "#,
    )
    .unwrap();

    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Multorum Test"]);
    git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: failing check fixture"]);

    let svc = FsOrchestratorService::new(dir.path()).unwrap();

    let orch = OrchestratorHandler::with_service(svc);

    let create = orch
        .dispatch("create_worker", create_worker_args("AuthImplementor", Some("ck-w")))
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 50 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "-m", "incr: check test"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let worker = WorkerHandler::with_service(FsWorkerService::new(wt).unwrap());
    let commit = worker.dispatch("send_commit", send_commit_args(&head)).unwrap();
    assert_tool_success(&commit);

    // Merge without skipping -- should fail.
    let merge = orch.dispatch("merge_worker", merge_worker_args("ck-w")).unwrap();
    assert_tool_error_code(&merge, "check_failed");
}

/// Merge with `skip_checks` for a skippable check succeeds.
#[tokio::test]
async fn check_pipeline_skip_succeeds() {
    use multorum::mcp::transport::orchestrator::OrchestratorHandler;
    use multorum::mcp::transport::worker::WorkerHandler;
    use multorum::runtime::FsWorkerService;

    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
    use multorum::runtime::{FsOrchestratorService, OrchestratorService};
    FsOrchestratorService::new(dir.path()).unwrap().rulebook_init().unwrap();
    fs::write(
        dir.path().join(".multorum/rulebook.toml"),
        r#"
            [fileset]
            Owned.path = "src/owned.rs"
            Other.path = "src/other.rs"

            [perspective.AuthImplementor]
            read = "Other"
            write = "Owned"

            [check]
            pipeline = ["lint"]

            [check.command]
            lint = "exit 1"

            [check.policy]
            lint = "skippable"
        "#,
    )
    .unwrap();

    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Multorum Test"]);
    git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: skip check fixture"]);

    let svc = FsOrchestratorService::new(dir.path()).unwrap();

    let orch = OrchestratorHandler::with_service(svc);

    let create = orch
        .dispatch("create_worker", create_worker_args("AuthImplementor", Some("sk-w")))
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    fs::write(wt.join("src/owned.rs"), "pub fn owned() -> i32 { 60 }\n").unwrap();
    git(wt, &["add", "src/owned.rs"]);
    git(wt, &["commit", "-m", "incr: skip check test"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let worker = WorkerHandler::with_service(FsWorkerService::new(wt).unwrap());
    let commit = worker.dispatch("send_commit", send_commit_args(&head)).unwrap();
    assert_tool_success(&commit);

    // Merge with skip_checks = ["lint"] -- should succeed.
    let merge = orch
        .dispatch(
            "merge_worker",
            json_args(json!({
                "worker": "sk-w",
                "skip_checks": ["lint"],
                "body_text": "Merged after reviewing skipped lint evidence.",
            })),
        )
        .unwrap();
    assert_tool_success(&merge);
    let merge_json = tool_json(&merge);
    assert_eq!(merge_json["state"], "merged");
    assert!(
        merge_json["skipped_checks"].as_array().unwrap().iter().any(|c| c == "lint"),
        "lint should appear in skipped_checks"
    );
}

/// Skipping a non-skippable check returns `check_failed`.
#[tokio::test]
async fn skip_non_skippable_check_rejected() {
    use multorum::mcp::transport::orchestrator::OrchestratorHandler;
    use multorum::mcp::transport::worker::WorkerHandler;
    use multorum::runtime::FsWorkerService;

    let (_dir, svc) = setup_multi_perspective_repo();
    let orch = OrchestratorHandler::with_service(svc);

    let create = orch
        .dispatch("create_worker", create_worker_args("AuthImplementor", Some("ns-w")))
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    fs::write(wt.join("src/auth.rs"), "pub fn auth() -> i32 { 70 }\n").unwrap();
    git(wt, &["add", "src/auth.rs"]);
    git(wt, &["commit", "-m", "incr: non-skippable test"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let worker = WorkerHandler::with_service(FsWorkerService::new(wt).unwrap());
    let commit = worker.dispatch("send_commit", send_commit_args(&head)).unwrap();
    assert_tool_success(&commit);

    // "build" has policy=always in the multi-perspective fixture.
    let merge = orch
        .dispatch(
            "merge_worker",
            json_args(json!({
                "worker": "ns-w",
                "skip_checks": ["build"],
                "body_text": "Attempted merge while requesting a forbidden skip.",
            })),
        )
        .unwrap();
    assert_tool_error_code(&merge, "check_failed");
    let err_text = tool_text(&merge);
    assert!(err_text.contains("not skippable"), "error should mention non-skippable: {err_text}");
}

/// Passing checks run successfully and appear in `ran_checks`.
#[tokio::test]
async fn check_pipeline_passing_reports_ran_checks() {
    use multorum::mcp::transport::orchestrator::OrchestratorHandler;
    use multorum::mcp::transport::worker::WorkerHandler;
    use multorum::runtime::FsWorkerService;

    let (_dir, svc) = setup_multi_perspective_repo();
    let orch = OrchestratorHandler::with_service(svc);

    let create = orch
        .dispatch("create_worker", create_worker_args("AuthImplementor", Some("rc-w")))
        .unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();
    let wt = Path::new(&worktree);

    fs::write(wt.join("src/auth.rs"), "pub fn auth() -> i32 { 80 }\n").unwrap();
    git(wt, &["add", "src/auth.rs"]);
    git(wt, &["commit", "-m", "incr: passing checks test"]);
    let head = git(wt, &["rev-parse", "HEAD"]);

    let worker = WorkerHandler::with_service(FsWorkerService::new(wt).unwrap());
    let commit = worker.dispatch("send_commit", send_commit_args(&head)).unwrap();
    assert_tool_success(&commit);

    // Both lint and build are `true` in the multi-perspective fixture.
    let merge = orch.dispatch("merge_worker", merge_worker_args("rc-w")).unwrap();
    assert_tool_success(&merge);
    let merge_json = tool_json(&merge);
    assert_eq!(merge_json["state"], "merged");

    let ran = merge_json["ran_checks"].as_array().unwrap();
    assert_eq!(ran.len(), 2);
    assert!(ran.iter().any(|c| c == "lint"));
    assert!(ran.iter().any(|c| c == "build"));
    assert!(merge_json["skipped_checks"].as_array().unwrap().is_empty());
}
