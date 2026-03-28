//! Argument extraction edge-case tests.
//!
//! Validates that malformed, mistyped, or unexpected arguments are handled
//! gracefully by the MCP dispatch layer rather than panicking.

use serde_json::json;

use multorum::mcp::transport::orchestrator::OrchestratorHandler;
use multorum::mcp::transport::worker::WorkerHandler;
use multorum::runtime::FsWorkerService;

use crate::support::repo::setup_repo;
use crate::support::result::json_args;
use crate::support::worker::create_worker_runtime;

// ---------------------------------------------------------------------------
// Type mismatches on required fields
// ---------------------------------------------------------------------------

#[test]
fn string_where_integer_expected() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::with_service(worker_svc);

    let result = handler.dispatch("ack_inbox_message", json_args(json!({"sequence": "42"})));
    assert!(result.is_err(), "string for integer should be a protocol error");
}

#[test]
fn null_where_string_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    let result = handler.dispatch("get_worker", json_args(json!({"worker": null})));
    assert!(result.is_err(), "null for required string should be a protocol error");
}

#[test]
fn boolean_where_string_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    let result = handler.dispatch("get_worker", json_args(json!({"worker": true})));
    assert!(result.is_err(), "boolean for required string should be a protocol error");
}

#[test]
fn array_where_string_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    let result = handler.dispatch("get_worker", json_args(json!({"worker": ["a"]})));
    assert!(result.is_err(), "array for required string should be a protocol error");
}

#[test]
fn integer_where_string_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    let result = handler.dispatch("get_worker", json_args(json!({"worker": 42})));
    assert!(result.is_err(), "integer for required string should be a protocol error");
}

#[test]
fn negative_where_u64_expected() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::with_service(worker_svc);

    let result = handler.dispatch("ack_inbox_message", json_args(json!({"sequence": -1})));
    assert!(result.is_err(), "negative integer for u64 should be a protocol error");
}

// ---------------------------------------------------------------------------
// Optional field type mismatches
// ---------------------------------------------------------------------------

#[test]
fn string_where_boolean_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    let result = handler.dispatch(
        "create_worker",
        json_args(json!({
            "perspective": "AuthImplementor",
            "overwriting_worktree": "yes",
            "body_text": "Bootstrap the worker.",
        })),
    );
    assert!(result.is_err(), "optional boolean with wrong type should be a protocol error");
}

#[test]
fn string_list_with_non_strings() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    // Create a worker first so merge_worker has something to reference,
    // but we only care that argument parsing doesn't panic.
    handler
        .dispatch(
            "create_worker",
            json_args(json!({
                "perspective": "AuthImplementor",
                "worker": "w1",
                "body_text": "Bootstrap the worker.",
            })),
        )
        .unwrap();

    // `skip_checks` is StringList; non-string items should be rejected.
    let result = handler
        .dispatch("merge_worker", json_args(json!({"worker": "w1", "skip_checks": [1, true]})));
    assert!(result.is_err(), "non-string items in StringList should be a protocol error");
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn extra_unknown_fields_rejected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    let result = handler.dispatch("get_status", json_args(json!({"bonus": 123, "extra": "field"})));
    assert!(result.is_err(), "unknown fields should be a protocol error");
}

#[test]
fn empty_string_worker() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    // Empty string passes the required_str check but should fail WorkerId
    // parsing, producing a protocol error (invalid_params).
    let result = handler.dispatch("get_worker", json_args(json!({"worker": ""})));
    assert!(result.is_err(), "empty worker should be a protocol error");
}

#[test]
fn create_worker_requires_body_source() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);

    let result =
        handler.dispatch("create_worker", json_args(json!({"perspective": "AuthImplementor"})));
    assert!(result.is_err(), "missing body source should be a protocol error");
}

#[test]
fn send_commit_requires_body_source() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::with_service(worker_svc);

    let result = handler.dispatch("send_commit", json_args(json!({"head_commit": "deadbeef"})));
    assert!(result.is_err(), "missing body source should be a protocol error");
}
