//! MCP result extraction and assertion helpers.

use rmcp::model::{CallToolResult, RawContent, ReadResourceResult, ResourceContents};
use serde_json::Value;

pub fn empty_args() -> serde_json::Map<String, Value> {
    serde_json::Map::new()
}

pub fn json_args(value: Value) -> serde_json::Map<String, Value> {
    value.as_object().unwrap().clone()
}

/// Build `create_worker` arguments with the required body text.
pub fn create_worker_args(
    perspective: &str, worker: Option<&str>,
) -> serde_json::Map<String, Value> {
    let mut args = json_args(
        serde_json::json!({"perspective": perspective, "body_text": "Bootstrap the worker."}),
    );
    if let Some(worker) = worker {
        args.insert("worker".to_string(), Value::String(worker.to_owned()));
    }
    args
}

/// Build `send_commit` arguments with the required body text.
pub fn send_commit_args(head_commit: &str) -> serde_json::Map<String, Value> {
    json_args(serde_json::json!({
        "head_commit": head_commit,
        "body_text": "Implemented the requested change.",
    }))
}

/// Build `merge_worker` arguments with the required body text.
pub fn merge_worker_args(worker: &str) -> serde_json::Map<String, Value> {
    json_args(serde_json::json!({
        "worker": worker,
        "body_text": "Merged after reviewing the worker submission.",
    }))
}

/// Extract the text payload from a successful tool call result.
pub fn tool_text(result: &CallToolResult) -> &str {
    assert!(!result.content.is_empty(), "tool result has no content");
    match &result.content[0].raw {
        | RawContent::Text(text) => &text.text,
        | _ => panic!("expected text content in tool result"),
    }
}

/// Parse the text payload of a tool result as JSON.
pub fn tool_json(result: &CallToolResult) -> Value {
    serde_json::from_str(tool_text(result)).expect("tool result text is not valid JSON")
}

/// Extract the text payload from a resource read result.
pub fn resource_text(result: &ReadResourceResult) -> &str {
    assert!(!result.contents.is_empty(), "resource result has no contents");
    match &result.contents[0] {
        | ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        | _ => panic!("expected text resource contents"),
    }
}

/// Parse the text payload of a resource result as JSON.
pub fn resource_json(result: &ReadResourceResult) -> Value {
    serde_json::from_str(resource_text(result)).expect("resource result text is not valid JSON")
}

pub fn assert_tool_success(result: &CallToolResult) {
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "expected tool success, got error: {}",
        tool_text(result),
    );
}

pub fn assert_tool_error(result: &CallToolResult) {
    assert_eq!(
        result.is_error,
        Some(true),
        "expected tool error, got success: {}",
        tool_text(result),
    );
}

/// Assert a tool error and return the parsed error code string.
pub fn assert_tool_error_code(result: &CallToolResult, expected_code: &str) {
    assert_tool_error(result);
    let err = tool_json(result);
    assert_eq!(err["code"], expected_code, "expected error code `{expected_code}`, got: {err}",);
}
