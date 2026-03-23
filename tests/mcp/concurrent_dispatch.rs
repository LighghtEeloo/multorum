//! Concurrent dispatch safety tests.
//!
//! These tests send multiple MCP requests concurrently through the duplex
//! wire transport, exercising rmcp's server event loop and the handler's
//! thread safety under real parallelism.

use rmcp::model::CallToolRequestParams;
use serde_json::json;

use crate::support::repo::setup_repo;
use crate::support::result::{assert_tool_success, json_args, tool_json};
use crate::support::wire::{orchestrator_duplex, worker_duplex};
use crate::support::worker::create_worker_runtime;

// ---------------------------------------------------------------------------
// Concurrent read operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_list_operations() {
    let (_dir, client) = orchestrator_duplex().await;
    let (tools, resources) = tokio::join!(client.list_all_tools(), client.list_all_resources());
    assert_eq!(tools.unwrap().len(), 16);
    assert_eq!(resources.unwrap().len(), 4);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn concurrent_tool_calls() {
    let (_dir, client) = orchestrator_duplex().await;
    let (perspectives, status) = tokio::join!(
        client.call_tool(CallToolRequestParams::new("list_perspectives")),
        client.call_tool(CallToolRequestParams::new("get_status")),
    );
    assert_tool_success(&perspectives.unwrap());
    assert_tool_success(&status.unwrap());
    client.cancel().await.unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent mutations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_create_different_workers() {
    let (_dir, client) = orchestrator_duplex().await;
    let (w1, w2) = tokio::join!(
        client.call_tool(CallToolRequestParams::new("create_worker").with_arguments(json_args(
            json!({"perspective": "AuthImplementor", "worker_id": "cw1"})
        )),),
        client.call_tool(CallToolRequestParams::new("create_worker").with_arguments(json_args(
            json!({"perspective": "AuthImplementor", "worker_id": "cw2"})
        )),),
    );
    assert_tool_success(&w1.unwrap());
    assert_tool_success(&w2.unwrap());

    let workers = client.call_tool(CallToolRequestParams::new("list_workers")).await.unwrap();
    assert_tool_success(&workers);
    assert_eq!(tool_json(&workers).as_array().unwrap().len(), 2);

    client.cancel().await.unwrap();
}

// ---------------------------------------------------------------------------
// Concurrent worker reads
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_worker_reads() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let client = worker_duplex(&worktree).await;

    let (contract, status) = tokio::join!(
        client.call_tool(CallToolRequestParams::new("get_contract")),
        client.call_tool(CallToolRequestParams::new("get_status")),
    );
    assert_tool_success(&contract.unwrap());
    assert_tool_success(&status.unwrap());

    client.cancel().await.unwrap();
}
