//! MCP protocol transport over stdio.
//!
//! This module bridges the dependency-free MCP descriptor facade with a
//! real JSON-RPC transport backed by the `rmcp` crate. Two server
//! handlers are provided — one for the orchestrator surface and one for
//! the worker surface — each wiring MCP tool and resource requests to
//! the corresponding runtime service methods.

pub mod orchestrator;
pub mod worker;

use std::sync::{Arc, RwLock};

use rmcp::ErrorData;
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

/// Runtime binding state for one MCP handler.
///
/// The server defaults to the process working directory at startup.
/// The `set_working_directory` tool allows the client to rebind the
/// runtime to a different directory at any time.
#[derive(Debug, Clone)]
pub(crate) enum ServiceState<S> {
    /// Ready runtime service bound to a working directory.
    Ready(S),
    /// Service construction failed for the configured directory.
    Failed(McpToolError),
}

/// Thread-safe wrapper around [`ServiceState`] that allows runtime
/// rebinding via `set_working_directory`.
pub(crate) struct DeferredService<S> {
    pub(crate) state: RwLock<ServiceState<S>>,
}

impl<S> DeferredService<S> {
    /// Bind from the initial startup attempt (typically `from_current_dir`).
    fn from_startup_result(result: Result<S, RuntimeError>) -> Self {
        let state = match result {
            | Ok(service) => ServiceState::Ready(service),
            | Err(error) => ServiceState::Failed(McpToolError::from(error)),
        };
        Self { state: RwLock::new(state) }
    }

    /// Rebind the working directory by storing a new service
    /// construction result. Replaces any previous binding and returns
    /// the outcome so the caller can report it to the client.
    fn bind(&self, result: Result<S, RuntimeError>) -> Result<(), McpToolError> {
        let new_state = match result {
            | Ok(service) => ServiceState::Ready(service),
            | Err(error) => {
                let mcp_err = McpToolError::from(error);
                let ret = mcp_err.clone();
                *self.state.write().unwrap() = ServiceState::Failed(mcp_err);
                return Err(ret);
            }
        };
        *self.state.write().unwrap() = new_state;
        Ok(())
    }
}

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
    schema.insert("additionalProperties".into(), Value::Bool(false));
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
    raw.mime_type = Some(descriptor.mime_type.into());
    Annotated::new(raw, None)
}

/// Convert one internal [`ResourceTemplateDescriptor`] into an rmcp
/// [`ResourceTemplate`].
fn to_rmcp_resource_template(descriptor: &ResourceTemplateDescriptor) -> ResourceTemplate {
    let raw = RawResourceTemplate::new(descriptor.uri_template, descriptor.uri_template)
        .with_description(descriptor.description)
        .with_mime_type(descriptor.mime_type);
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
        | Err(runtime_err) => Ok(tool_error_result(&McpToolError::from(runtime_err))),
    }
}

/// Construct one tool-level MCP failure payload.
fn tool_error_result(error: &McpToolError) -> CallToolResult {
    let json = serde_json::json!({
        "code": error.code.as_str(),
        "message": error.message,
    });
    CallToolResult::error(vec![Annotated::new(RawContent::text(json.to_string()), None)])
}

/// Serialize a runtime result into a [`ReadResourceResult`] text body.
fn resource_success<T: Serialize>(
    uri: &str, value: &T,
) -> Result<ReadResourceResult, rmcp::ErrorData> {
    let json = serde_json::to_string_pretty(value).map_err(|e| {
        rmcp::ErrorData::internal_error(format!("failed to serialize resource: {e}"), None)
    })?;
    resource_text_success(uri, json, "application/json")
}

/// Return one text resource body with the given MIME type.
fn resource_text_success(
    uri: &str, text: String, mime_type: &str,
) -> Result<ReadResourceResult, rmcp::ErrorData> {
    Ok(ReadResourceResult::new(vec![ResourceContents::text(text, uri).with_mime_type(mime_type)]))
}

/// Convert one runtime error into MCP resource-read error data.
fn runtime_to_resource_error(error: RuntimeError) -> rmcp::ErrorData {
    let mcp = McpToolError::from(error);
    mcp_to_resource_error(&mcp)
}

/// Convert one MCP-visible tool error into resource-read error data.
fn mcp_to_resource_error(mcp: &McpToolError) -> rmcp::ErrorData {
    let data = Some(serde_json::json!({
        "code": mcp.code.as_str(),
    }));
    match mcp.code {
        | McpErrorCode::UnknownPerspective
        | McpErrorCode::UnknownWorker
        | McpErrorCode::MessageNotFound
        | McpErrorCode::MissingWorkerRuntime => {
            rmcp::ErrorData::resource_not_found(mcp.message.clone(), data)
        }
        | McpErrorCode::Internal => rmcp::ErrorData::internal_error(mcp.message.clone(), data),
        | McpErrorCode::WorkerExists
        | McpErrorCode::InvalidState
        | McpErrorCode::AlreadyAcknowledged
        | McpErrorCode::BiddingGroupConflict
        | McpErrorCode::CheckFailed
        | McpErrorCode::WriteSetViolation
        | McpErrorCode::MailboxConflict
        | McpErrorCode::Unimplemented => rmcp::ErrorData::invalid_params(mcp.message.clone(), data),
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

/// Extract a required u64 argument from the JSON arguments object.
fn required_u64(args: &serde_json::Map<String, Value>, key: &str) -> Result<u64, rmcp::ErrorData> {
    args.get(key).and_then(Value::as_u64).ok_or_else(|| {
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

/// Parse mutually exclusive sequence filter arguments (`from`/`to` vs `exact`).
fn extract_sequence_filter(
    args: &serde_json::Map<String, Value>,
) -> Result<crate::runtime::SequenceFilter, ErrorData> {
    let from = optional_u64(args, "from").map(crate::runtime::Sequence);
    let to = optional_u64(args, "to").map(crate::runtime::Sequence);
    let exact = optional_u64(args, "exact").map(crate::runtime::Sequence);

    if exact.is_some() && (from.is_some() || to.is_some()) {
        return Err(ErrorData::invalid_params("exact is mutually exclusive with from/to", None));
    }

    Ok(match exact {
        | Some(seq) => crate::runtime::SequenceFilter::Exact(seq),
        | None => crate::runtime::SequenceFilter::Range { from, to },
    })
}

/// Extract an optional string list argument.
fn optional_string_list(args: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default()
}

/// Build a [`BundlePayload`] from common MCP tool arguments.
///
/// MCP callers may provide either inline Markdown via `body_text` or a
/// path-backed body via `body_path`. The path-backed form transfers file
/// ownership into Multorum-managed bundle storage on successful
/// publication.
fn extract_payload(args: &serde_json::Map<String, Value>) -> crate::bundle::BundlePayload {
    let body_text = optional_str(args, "body_text").map(str::to_owned);
    let body_path = optional_str(args, "body_path").map(std::path::PathBuf::from);
    let artifacts = args
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(std::path::PathBuf::from).collect())
        .unwrap_or_default();
    crate::bundle::BundlePayload { body_text, body_path, artifacts }
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

/// Validate one tool argument object against the declared MCP descriptor.
///
/// This keeps the MCP dispatch contract aligned with the CLI contract:
/// unknown fields are rejected and provided fields must match their typed
/// schema entry even when optional.
fn validate_tool_arguments(
    tool: &str, args: &serde_json::Map<String, Value>, descriptors: &[ToolDescriptor],
) -> Result<(), rmcp::ErrorData> {
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.name == tool)
        .ok_or_else(|| rmcp::ErrorData::invalid_params(format!("unknown tool: {tool}"), None))?;

    for key in args.keys() {
        let known = descriptor.inputs.iter().any(|input| input.name == key.as_str());
        if !known {
            return Err(rmcp::ErrorData::invalid_params(
                format!("unknown field for {tool}: {key}"),
                None,
            ));
        }
    }

    for input in descriptor.inputs {
        let Some(value) = args.get(input.name) else {
            if input.required {
                return Err(rmcp::ErrorData::invalid_params(
                    format!("missing required field: {}", input.name),
                    None,
                ));
            }
            continue;
        };
        if !value_matches_input(value, input.kind) {
            return Err(rmcp::ErrorData::invalid_params(
                format!(
                    "invalid field type for `{}`: expected {}",
                    input.name,
                    expected_type_label(input.kind)
                ),
                None,
            ));
        }
    }

    Ok(())
}

/// Return whether a JSON value matches one declared tool input type.
fn value_matches_input(value: &Value, kind: ToolInputType) -> bool {
    match kind {
        | ToolInputType::String => value.is_string(),
        | ToolInputType::Integer => value.is_u64(),
        | ToolInputType::Boolean => value.is_boolean(),
        | ToolInputType::StringList => {
            value.as_array().is_some_and(|items| items.iter().all(Value::is_string))
        }
    }
}

/// Return a user-facing expected type label for one input descriptor type.
fn expected_type_label(kind: ToolInputType) -> &'static str {
    match kind {
        | ToolInputType::String => "string",
        | ToolInputType::Integer => "non-negative integer",
        | ToolInputType::Boolean => "boolean",
        | ToolInputType::StringList => "array of strings",
    }
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
        assert_eq!(schema["additionalProperties"], false);
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
