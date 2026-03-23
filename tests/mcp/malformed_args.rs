//! Argument extraction edge-case tests.
//!
//! Validates that malformed, mistyped, or unexpected arguments are handled
//! gracefully by the MCP dispatch layer rather than panicking.

use serde_json::json;

use multorum::mcp::transport::orchestrator::OrchestratorHandler;
use multorum::mcp::transport::worker::WorkerHandler;
use multorum::runtime::FsWorkerService;

use crate::support::repo::setup_repo;
use crate::support::result::{assert_tool_success, json_args};
use crate::support::worker::create_worker_runtime;

// ---------------------------------------------------------------------------
// Type mismatches on required fields
// ---------------------------------------------------------------------------

#[test]
fn string_where_integer_expected() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.dispatch("ack_inbox_message", json_args(json!({"sequence": "42"})));
    assert!(result.is_err(), "string for integer should be a protocol error");
}

#[test]
fn null_where_string_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let result = handler.dispatch("get_worker", json_args(json!({"worker_id": null})));
    assert!(result.is_err(), "null for required string should be a protocol error");
}

#[test]
fn boolean_where_string_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let result = handler.dispatch("get_worker", json_args(json!({"worker_id": true})));
    assert!(result.is_err(), "boolean for required string should be a protocol error");
}

#[test]
fn array_where_string_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let result = handler.dispatch("get_worker", json_args(json!({"worker_id": ["a"]})));
    assert!(result.is_err(), "array for required string should be a protocol error");
}

#[test]
fn integer_where_string_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let result = handler.dispatch("get_worker", json_args(json!({"worker_id": 42})));
    assert!(result.is_err(), "integer for required string should be a protocol error");
}

#[test]
fn negative_where_u64_expected() {
    let (_dir, svc) = setup_repo();
    let (_, worktree) = create_worker_runtime(&svc);
    let worker_svc = FsWorkerService::new(&worktree).unwrap();
    let handler = WorkerHandler::new(worker_svc);

    let result = handler.dispatch("ack_inbox_message", json_args(json!({"sequence": -1})));
    assert!(result.is_err(), "negative integer for u64 should be a protocol error");
}

// ---------------------------------------------------------------------------
// Optional field type mismatches (silently ignored)
// ---------------------------------------------------------------------------

#[test]
fn string_where_boolean_expected() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    // `overwriting_worktree` is optional boolean; a string should be treated as
    // absent (optional_bool returns None for non-bool values).
    let result = handler.dispatch(
        "create_worker",
        json_args(json!({"perspective": "AuthImplementor", "overwriting_worktree": "yes"})),
    );
    // Should still succeed — the bad optional is just ignored.
    let result = result.unwrap();
    assert_tool_success(&result);
}

#[test]
fn string_list_with_non_strings() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    // Create a worker first so merge_worker has something to reference,
    // but we only care that argument parsing doesn't panic.
    handler
        .dispatch(
            "create_worker",
            json_args(json!({"perspective": "AuthImplementor", "worker_id": "w1"})),
        )
        .unwrap();

    // `skip_checks` is StringList; non-string items should be filtered out.
    let result = handler
        .dispatch("merge_worker", json_args(json!({"worker_id": "w1", "skip_checks": [1, true]})));
    // merge_worker will fail for business-logic reasons (not committed), but
    // the argument parsing itself should not cause a protocol error.
    assert!(result.is_ok(), "non-string items in StringList should not cause protocol error");
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn extra_unknown_fields_ignored() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    let result = handler.dispatch("get_status", json_args(json!({"bonus": 123, "extra": "field"})));
    let result = result.unwrap();
    assert_tool_success(&result);
}

#[test]
fn empty_string_worker_id() {
    let (_dir, svc) = setup_repo();
    let handler = OrchestratorHandler::new(svc);

    // Empty string passes the required_str check but should fail WorkerId
    // parsing, producing a protocol error (invalid_params).
    let result = handler.dispatch("get_worker", json_args(json!({"worker_id": ""})));
    assert!(result.is_err(), "empty worker_id should be a protocol error");
}
