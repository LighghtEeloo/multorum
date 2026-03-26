//! Wire-level MCP protocol tests.
//!
//! These tests connect an `OrchestratorHandler` or `WorkerHandler` to an
//! rmcp client via `tokio::io::duplex`, exercising the full JSON-RPC
//! framing, initialize handshake, and message round-trip.

use std::fs;
use std::path::Path;

use rmcp::model::{CallToolRequestParams, ReadResourceRequestParams};
use serde_json::json;

use crate::support::repo::{git, setup_repo};
use crate::support::result::{
    assert_tool_error, assert_tool_success, json_args, resource_text, tool_json,
};
use crate::support::wire::{orchestrator_duplex, worker_duplex};
use crate::support::worker::create_worker_runtime;

// ---------------------------------------------------------------------------
// Orchestrator wire tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn orchestrator_wire_server_info() {
    let (_dir, client) = orchestrator_duplex().await;
    // If we got here, the initialize handshake succeeded.
    // The client is connected and ready.
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn orchestrator_wire_list_tools() {
    let (_dir, client) = orchestrator_duplex().await;
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 16);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn orchestrator_wire_list_resources() {
    let (_dir, client) = orchestrator_duplex().await;
    let resources = client.list_all_resources().await.unwrap();
    assert_eq!(resources.len(), 3);
    let templates = client.list_all_resource_templates().await.unwrap();
    assert_eq!(templates.len(), 2);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn orchestrator_wire_call_tool_get_status() {
    let (_dir, client) = orchestrator_duplex().await;
    let result = client.call_tool(CallToolRequestParams::new("get_status")).await.unwrap();
    assert_tool_success(&result);
    let status = tool_json(&result);
    assert!(status["active_perspectives"].is_array());
    assert!(status["workers"].is_array());
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn orchestrator_wire_call_tool_with_args() {
    let (_dir, client) = orchestrator_duplex().await;
    let params = CallToolRequestParams::new("create_worker")
        .with_arguments(json_args(json!({"perspective": "AuthImplementor", "worker": "w1"})));
    let result = client.call_tool(params).await.unwrap();
    assert_tool_success(&result);
    let created = tool_json(&result);
    assert_eq!(created["worker"], "w1");
    assert_eq!(created["state"], "ACTIVE");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn orchestrator_wire_read_resource() {
    let (_dir, client) = orchestrator_duplex().await;
    let result = client
        .read_resource(ReadResourceRequestParams::new("multorum://orchestrator/status"))
        .await
        .unwrap();
    let text = resource_text(&result);
    let status: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(status["active_perspectives"].is_array());
    assert!(status["workers"].is_array());
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn orchestrator_wire_tool_error_propagates() {
    let (_dir, client) = orchestrator_duplex().await;
    let params = CallToolRequestParams::new("get_worker")
        .with_arguments(json_args(json!({"worker": "does-not-exist"})));
    let result = client.call_tool(params).await.unwrap();
    assert_tool_error(&result);
    let err = tool_json(&result);
    assert_eq!(err["code"], "unknown_worker");
    client.cancel().await.unwrap();
}

// ---------------------------------------------------------------------------
// Worker wire tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn worker_wire_list_tools() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let client = worker_duplex(&worktree).await;
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 6);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn worker_wire_contract_and_status() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let client = worker_duplex(&worktree).await;

    let contract = client.call_tool(CallToolRequestParams::new("get_contract")).await.unwrap();
    assert_tool_success(&contract);
    assert_eq!(tool_json(&contract)["perspective"], "AuthImplementor");

    let status = client.call_tool(CallToolRequestParams::new("get_status")).await.unwrap();
    assert_tool_success(&status);
    assert_eq!(tool_json(&status)["state"], "ACTIVE");

    client.cancel().await.unwrap();
}

// ---------------------------------------------------------------------------
// Full cross-handler workflow over wire
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wire_full_workflow() {
    let (dir, orch_client) = orchestrator_duplex().await;

    // Step 1: Create worker.
    let create_params = CallToolRequestParams::new("create_worker")
        .with_arguments(json_args(json!({"perspective": "AuthImplementor", "worker": "wf1"})));
    let create = orch_client.call_tool(create_params).await.unwrap();
    assert_tool_success(&create);
    let worktree = tool_json(&create)["worktree_path"].as_str().unwrap().to_string();

    // Step 2: Worker modifies files and commits.
    let worktree_path = Path::new(&worktree);
    fs::write(worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 42 }\n").unwrap();
    git(worktree_path, &["add", "src/owned.rs"]);
    git(worktree_path, &["commit", "-m", "incr: answer to everything"]);
    let head = git(worktree_path, &["rev-parse", "HEAD"]);

    // Step 3: Worker submits via wire.
    let worker_client = worker_duplex(worktree_path).await;
    let commit_params = CallToolRequestParams::new("send_commit")
        .with_arguments(json_args(json!({"head_commit": head})));
    let commit = worker_client.call_tool(commit_params).await.unwrap();
    assert_tool_success(&commit);
    worker_client.cancel().await.unwrap();

    // Step 4: Orchestrator merges via wire.
    let merge_params = CallToolRequestParams::new("merge_worker")
        .with_arguments(json_args(json!({"worker": "wf1"})));
    let merge = orch_client.call_tool(merge_params).await.unwrap();
    assert_tool_success(&merge);
    assert_eq!(tool_json(&merge)["state"], "MERGED");

    // Step 5: Verify merged file landed.
    assert_eq!(
        fs::read_to_string(dir.path().join("src/owned.rs")).unwrap(),
        "pub fn owned() -> i32 { 42 }\n"
    );

    orch_client.cancel().await.unwrap();
}
