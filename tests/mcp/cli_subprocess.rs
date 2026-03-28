//! CLI subprocess smoke tests.
//!
//! Spawns the real `multorum serve orchestrator` binary, sends JSON-RPC
//! messages over stdin/stdout, and validates the protocol handshake and
//! basic tool listing.

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Send a JSON-RPC message as a newline-delimited JSON line.
async fn send_jsonrpc(child: &mut Child, msg: &Value) {
    let stdin = child.stdin.as_mut().expect("stdin not piped");
    let line = serde_json::to_string(msg).unwrap();
    stdin.write_all(line.as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();
    stdin.flush().await.unwrap();
}

/// Read one JSON-RPC response line from stdout.
async fn read_jsonrpc(stdout: &mut BufReader<tokio::process::ChildStdout>) -> Value {
    let mut line = String::new();
    stdout.read_line(&mut line).await.unwrap();
    serde_json::from_str(line.trim()).expect("stdout line is not valid JSON")
}

/// Perform the MCP initialize handshake and return the server info response.
async fn handshake(
    child: &mut Child, stdout: &mut BufReader<tokio::process::ChildStdout>,
) -> Value {
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {
                "name": "multorum-test",
                "version": "0.0.0"
            }
        }
    });
    send_jsonrpc(child, &init_request).await;
    let response = read_jsonrpc(stdout).await;
    assert_eq!(response["id"], 1);
    assert!(response["result"].is_object(), "initialize should return a result");

    // Send initialized notification.
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    send_jsonrpc(child, &initialized).await;

    response
}

fn spawn_orchestrator(launch_cwd: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_multorum"))
        .args(["serve", "orchestrator"])
        .current_dir(launch_cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn multorum binary")
}

fn spawn_worker(launch_cwd: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_multorum"))
        .args(["serve", "worker"])
        .current_dir(launch_cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn multorum binary")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cli_orchestrator_handshake() {
    let launch_cwd = tempfile::tempdir().unwrap();
    let mut child = spawn_orchestrator(launch_cwd.path());
    let child_stdout = child.stdout.take().unwrap();
    let mut stdout = BufReader::new(child_stdout);

    let response = handshake(&mut child, &mut stdout).await;
    let server_info = &response["result"]["serverInfo"];
    assert_eq!(server_info["name"], "multorum-orchestrator");

    // Clean shutdown.
    drop(child.stdin.take());
    let status = child.wait().await.unwrap();
    assert!(status.success(), "process should exit cleanly after stdin close");
}

#[tokio::test]
async fn cli_orchestrator_list_tools() {
    let launch_cwd = tempfile::tempdir().unwrap();
    let mut child = spawn_orchestrator(launch_cwd.path());
    let child_stdout = child.stdout.take().unwrap();
    let mut stdout = BufReader::new(child_stdout);

    handshake(&mut child, &mut stdout).await;

    // Send tools/list request.
    let list_tools = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    send_jsonrpc(&mut child, &list_tools).await;
    let response = read_jsonrpc(&mut stdout).await;
    assert_eq!(response["id"], 2);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 18);

    let list_resources = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "resources/list"
    });
    send_jsonrpc(&mut child, &list_resources).await;
    let response = read_jsonrpc(&mut stdout).await;
    assert_eq!(response["id"], 3);
    let resources = response["result"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 4);

    drop(child.stdin.take());
    let status = child.wait().await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn cli_orchestrator_clean_shutdown() {
    let launch_cwd = tempfile::tempdir().unwrap();
    let mut child = spawn_orchestrator(launch_cwd.path());
    let child_stdout = child.stdout.take().unwrap();
    let mut stdout = BufReader::new(child_stdout);

    handshake(&mut child, &mut stdout).await;

    // Close stdin to signal EOF.
    drop(child.stdin.take());
    let status = child.wait().await.unwrap();
    assert!(status.success(), "process should exit with code 0 after stdin close");
}

#[tokio::test]
async fn cli_worker_invalid_cwd_defers_error_until_tool_call() {
    let launch_cwd = tempfile::tempdir().unwrap();
    let mut child = spawn_worker(launch_cwd.path());
    let child_stdout = child.stdout.take().unwrap();
    let mut stdout = BufReader::new(child_stdout);

    let response = handshake(&mut child, &mut stdout).await;
    let server_info = &response["result"]["serverInfo"];
    assert_eq!(server_info["name"], "multorum-worker");

    let list_tools = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    send_jsonrpc(&mut child, &list_tools).await;
    let response = read_jsonrpc(&mut stdout).await;
    assert_eq!(response["id"], 2);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 8);

    let read_methodology = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "resources/read",
        "params": {
            "uri": "multorum://worker/methodology"
        }
    });
    send_jsonrpc(&mut child, &read_methodology).await;
    let response = read_jsonrpc(&mut stdout).await;
    assert_eq!(response["id"], 3);
    let methodology = response["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(methodology.contains("# Multorum Worker Methodology"));

    let get_status = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "get_status",
            "arguments": {}
        }
    });
    send_jsonrpc(&mut child, &get_status).await;
    let response = read_jsonrpc(&mut stdout).await;
    assert_eq!(response["id"], 4);
    assert_eq!(response["result"]["isError"], true);

    drop(child.stdin.take());
    let status = child.wait().await.unwrap();
    assert!(status.success(), "process should stay alive until stdin close");
}
