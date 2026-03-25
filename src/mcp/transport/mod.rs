//! MCP protocol transport over stdio.
//!
//! This module bridges the dependency-free MCP descriptor facade with a
//! real JSON-RPC transport backed by the `rmcp` crate. Two server
//! handlers are provided — one for the orchestrator surface and one for
//! the worker surface — each wiring MCP tool and resource requests to
//! the corresponding runtime service methods.

pub mod orchestrator;
pub mod worker;

use std::sync::Arc;

use rmcp::model::{
    Annotated, CallToolResult, Implementation, ListResourceTemplatesResult, ListResourcesResult,
    ListToolsResult, RawContent, RawResource, RawResourceTemplate, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate, ServerCapabilities, ServerInfo, Tool,
};
use serde::Serialize;
use serde_json::Value;

use crate::runtime::RuntimeError;

use super::dto::{
    ResourceDescriptor, ResourceTemplateDescriptor, ToolDescriptor, ToolInputDescriptor,
    ToolInputType,
};
use super::error::{McpErrorCode, McpToolError};

// ---------------------------------------------------------------------------
// Server info construction
// ---------------------------------------------------------------------------

/// Build an MCP `ServerInfo` for the given mode name.
fn server_info(name: &str) -> ServerInfo {
    let capabilities = ServerCapabilities::builder().enable_tools().enable_resources().build();
    ServerInfo::new(capabilities)
        .with_server_info(Implementation::new(name, env!("CARGO_PKG_VERSION")))
}

// ---------------------------------------------------------------------------
// Descriptor → rmcp type conversion
// ---------------------------------------------------------------------------

/// Convert a slice of [`ToolInputDescriptor`]s into a JSON Schema object
/// suitable for the `input_schema` field of an MCP [`Tool`].
fn input_schema(inputs: &[ToolInputDescriptor]) -> Arc<serde_json::Map<String, Value>> {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for input in inputs {
        let mut field = serde_json::Map::new();
        field.insert("description".into(), Value::String(input.description.into()));
        match input.kind {
            | ToolInputType::String => {
                field.insert("type".into(), Value::String("string".into()));
            }
            | ToolInputType::Integer => {
                field.insert("type".into(), Value::String("integer".into()));
            }
            | ToolInputType::Boolean => {
                field.insert("type".into(), Value::String("boolean".into()));
            }
            | ToolInputType::StringList => {
                field.insert("type".into(), Value::String("array".into()));
                field.insert("items".into(), serde_json::json!({ "type": "string" }));
            }
        }
        properties.insert(input.name.into(), Value::Object(field));
        if input.required {
            required.push(Value::String(input.name.into()));
        }
    }

    let mut schema = serde_json::Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".into(), Value::Array(required));
    }
    Arc::new(schema)
}

/// Convert one internal [`ToolDescriptor`] into an rmcp [`Tool`].
fn to_rmcp_tool(descriptor: &ToolDescriptor) -> Tool {
    Tool::new(descriptor.name, descriptor.description, input_schema(descriptor.inputs))
}

/// Convert one internal [`ResourceDescriptor`] into an rmcp [`Resource`].
fn to_rmcp_resource(descriptor: &ResourceDescriptor) -> Resource {
    let mut raw = RawResource::new(descriptor.uri, descriptor.uri);
    raw.description = Some(descriptor.description.to_string());
    raw.mime_type = Some("application/json".into());
    Annotated::new(raw, None)
}

/// Convert one internal [`ResourceTemplateDescriptor`] into an rmcp
/// [`ResourceTemplate`].
fn to_rmcp_resource_template(descriptor: &ResourceTemplateDescriptor) -> ResourceTemplate {
    let raw = RawResourceTemplate::new(descriptor.uri_template, descriptor.uri_template)
        .with_description(descriptor.description)
        .with_mime_type("application/json");
    Annotated::new(raw, None)
}

/// Convert a full tool descriptor list into rmcp [`ListToolsResult`].
fn list_tools_result(descriptors: &[ToolDescriptor]) -> ListToolsResult {
    ListToolsResult::with_all_items(descriptors.iter().map(to_rmcp_tool).collect())
}

/// Convert a full resource descriptor list into rmcp [`ListResourcesResult`].
fn list_resources_result(descriptors: &[ResourceDescriptor]) -> ListResourcesResult {
    ListResourcesResult::with_all_items(descriptors.iter().map(to_rmcp_resource).collect())
}

/// Convert a full resource template descriptor list into rmcp
/// [`ListResourceTemplatesResult`].
fn list_resource_templates_result(
    descriptors: &[ResourceTemplateDescriptor],
) -> ListResourceTemplatesResult {
    ListResourceTemplatesResult::with_all_items(
        descriptors.iter().map(to_rmcp_resource_template).collect(),
    )
}

// ---------------------------------------------------------------------------
// Result helpers
// ---------------------------------------------------------------------------

/// Dispatch a runtime operation and convert its result into a
/// [`CallToolResult`]. Runtime errors become tool-level errors (with
/// `is_error` set) rather than protocol-level errors.
fn dispatch_tool<T: Serialize>(
    result: Result<T, RuntimeError>,
) -> Result<CallToolResult, rmcp::ErrorData> {
    match result {
        | Ok(value) => {
            let json = serde_json::to_string_pretty(&value).map_err(|e| {
                rmcp::ErrorData::internal_error(
                    format!("failed to serialize tool result: {e}"),
                    None,
                )
            })?;
            Ok(CallToolResult::success(vec![Annotated::new(RawContent::text(json), None)]))
        }
        | Err(runtime_err) => {
            let mcp_err = McpToolError::from(runtime_err);
            let json = serde_json::json!({
                "code": mcp_err.code.as_str(),
                "message": mcp_err.message,
            });
            Ok(CallToolResult::error(vec![Annotated::new(
                RawContent::text(json.to_string()),
                None,
            )]))
        }
    }
}

/// Serialize a runtime result into a [`ReadResourceResult`] text body.
fn resource_success<T: Serialize>(
    uri: &str, value: &T,
) -> Result<ReadResourceResult, rmcp::ErrorData> {
    let json = serde_json::to_string_pretty(value).map_err(|e| {
        rmcp::ErrorData::internal_error(format!("failed to serialize resource: {e}"), None)
    })?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(json, uri).with_mime_type("application/json"),
    ]))
}

/// Convert one runtime error into MCP resource-read error data.
fn runtime_to_resource_error(error: RuntimeError) -> rmcp::ErrorData {
    let mcp = McpToolError::from(error);
    let data = Some(serde_json::json!({
        "code": mcp.code.as_str(),
    }));
    match mcp.code {
        | McpErrorCode::UnknownPerspective
        | McpErrorCode::UnknownWorker
        | McpErrorCode::MessageNotFound
        | McpErrorCode::MissingWorkerRuntime => {
            rmcp::ErrorData::resource_not_found(mcp.message, data)
        }
        | McpErrorCode::Internal => rmcp::ErrorData::internal_error(mcp.message, data),
        | McpErrorCode::WorkerIdExists
        | McpErrorCode::InvalidState
        | McpErrorCode::AlreadyAcknowledged
        | McpErrorCode::RulebookConflict
        | McpErrorCode::CheckFailed
        | McpErrorCode::WriteSetViolation
        | McpErrorCode::MailboxConflict
        | McpErrorCode::Unimplemented => rmcp::ErrorData::invalid_params(mcp.message, data),
    }
}

// ---------------------------------------------------------------------------
// Argument extraction helpers
// ---------------------------------------------------------------------------

/// Extract a required string argument from the JSON arguments object.
fn required_str<'a>(
    args: &'a serde_json::Map<String, Value>, key: &str,
) -> Result<&'a str, rmcp::ErrorData> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        rmcp::ErrorData::invalid_params(format!("missing required field: {key}"), None)
    })
}

/// Extract an optional string argument.
fn optional_str<'a>(args: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Extract an optional u64 argument.
fn optional_u64(args: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

/// Extract an optional boolean argument.
fn optional_bool(args: &serde_json::Map<String, Value>, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

/// Extract an optional string list argument.
fn optional_string_list(args: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default()
}

/// Build a [`BundlePayload`] from common MCP tool arguments.
fn extract_payload(args: &serde_json::Map<String, Value>) -> crate::bundle::BundlePayload {
    let body_path = optional_str(args, "body").map(std::path::PathBuf::from);
    let artifacts = args
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(std::path::PathBuf::from).collect())
        .unwrap_or_default();
    crate::bundle::BundlePayload { body_text: None, body_path, artifacts }
}

/// Build a [`ReplyReference`] from common MCP tool arguments.
fn extract_reply(args: &serde_json::Map<String, Value>) -> crate::runtime::ReplyReference {
    crate::runtime::ReplyReference {
        in_reply_to: optional_u64(args, "reply_to").map(crate::runtime::Sequence),
    }
}

/// Return the default empty arguments when none are provided.
fn args_or_empty(args: Option<serde_json::Map<String, Value>>) -> serde_json::Map<String, Value> {
    args.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{input_schema, list_resource_templates_result};

    #[test]
    fn orchestrator_tool_schema_uses_typed_fields() {
        let descriptor = crate::mcp::tool::orchestrator::descriptors()
            .into_iter()
            .find(|descriptor| descriptor.name == "create_worker")
            .expect("create_worker descriptor should exist");

        let schema = input_schema(descriptor.inputs);
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("tool schema should expose properties");

        assert_eq!(properties["perspective"]["type"], "string");
        assert_eq!(properties["overwriting_worktree"]["type"], "boolean");
        assert_eq!(properties["artifacts"]["type"], "array");
        assert_eq!(properties["artifacts"]["items"]["type"], "string");
    }

    #[test]
    fn forward_perspective_tool_schema_uses_string_perspective() {
        let descriptor = crate::mcp::tool::orchestrator::descriptors()
            .into_iter()
            .find(|descriptor| descriptor.name == "forward_perspective")
            .expect("forward_perspective descriptor should exist");

        let schema = input_schema(descriptor.inputs);
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("tool schema should expose properties");

        assert_eq!(properties["perspective"]["type"], "string");
    }

    #[test]
    fn worker_tool_schema_uses_integer_fields() {
        let descriptor = crate::mcp::tool::worker::descriptors()
            .into_iter()
            .find(|descriptor| descriptor.name == "ack_inbox_message")
            .expect("ack_inbox_message descriptor should exist");

        let schema = input_schema(descriptor.inputs);
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("tool schema should expose properties");

        assert_eq!(properties["sequence"]["type"], "integer");
    }

    #[test]
    fn orchestrator_worker_detail_is_a_resource_template() {
        let resources = crate::mcp::resource::orchestrator::descriptors();
        assert!(
            resources.iter().all(|descriptor| !descriptor.uri.contains("{worker}")),
            "parameterized URIs should not be listed as concrete resources"
        );

        let templates =
            list_resource_templates_result(&crate::mcp::resource::orchestrator::templates());
        assert_eq!(templates.resource_templates.len(), 2);
        assert_eq!(
            templates.resource_templates[0].raw.uri_template,
            "multorum://orchestrator/workers/{worker}"
        );
        assert_eq!(
            templates.resource_templates[1].raw.uri_template,
            "multorum://orchestrator/workers/{worker}/outbox"
        );
    }
}
