//! CLI subprocess smoke tests.
//!
//! Spawns the real `multorum serve orchestrator` binary, sends JSON-RPC
//! messages over stdin/stdout, and validates the protocol handshake and
//! basic tool listing.

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use crate::support::repo::setup_repo;

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

fn spawn_orchestrator(dir: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_multorum"))
        .args(["serve", "orchestrator"])
        .current_dir(dir)
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
    let (dir, _svc) = setup_repo();
    let mut child = spawn_orchestrator(dir.path());
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
    let (dir, _svc) = setup_repo();
    let mut child = spawn_orchestrator(dir.path());
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
    assert_eq!(tools.len(), 15);

    drop(child.stdin.take());
    let status = child.wait().await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn cli_orchestrator_clean_shutdown() {
    let (dir, _svc) = setup_repo();
    let mut child = spawn_orchestrator(dir.path());
    let child_stdout = child.stdout.take().unwrap();
    let mut stdout = BufReader::new(child_stdout);

    handshake(&mut child, &mut stdout).await;

    // Close stdin to signal EOF.
    drop(child.stdin.take());
    let status = child.wait().await.unwrap();
    assert!(status.success(), "process should exit with code 0 after stdin close");
}
