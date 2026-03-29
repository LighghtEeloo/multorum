//! Worker MCP server handler.
//!
//! Implements [`rmcp::ServerHandler`] by dispatching tool and resource
//! requests to a [`WorkerService`] instance.

use std::future::Future;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ListResourceTemplatesResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResult,
    ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler};

use crate::methodology::{MethodologyDocument, MethodologyRole};
use crate::runtime::{ForwardIntent, FsWorkerService, Sequence, WorkerService};

use super::{
    DeferredService, ServiceState, args_or_empty, dispatch_tool, extract_reply,
    extract_required_payload, extract_sequence_filter, list_resource_templates_result,
    list_resources_result, list_tools_result, mcp_to_resource_error, optional_bool, optional_str,
    required_str, required_u64, resource_success, resource_text_success, runtime_to_resource_error,
    server_info, tool_error_result, validate_tool_arguments,
};

/// MCP server handler for the worker surface.
///
/// The handler defaults to the process working directory at startup.
/// The `set_working_directory` tool allows the client to rebind the
/// runtime to a different worktree root at any time.
pub struct WorkerHandler {
    /// Runtime service, defaulting to cwd and rebindable via
    /// `set_working_directory`.
    service: DeferredService<FsWorkerService>,
    tools: ListToolsResult,
    resources: ListResourcesResult,
    resource_templates: ListResourceTemplatesResult,
}

impl Default for WorkerHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerHandler {
    /// Construct the handler with a pre-bound service.
    ///
    /// Note: This bypasses the default cwd binding and is intended
    /// for tests and programmatic embeddings that already hold a
    /// constructed service instance.
    pub fn with_service(service: FsWorkerService) -> Self {
        Self::from_startup_result(Ok(service))
    }

    /// Construct the handler, defaulting to the process working
    /// directory.
    pub fn new() -> Self {
        Self::from_startup_result(FsWorkerService::from_current_dir())
    }

    /// Construct the handler from an explicit startup result.
    fn from_startup_result(result: crate::runtime::Result<FsWorkerService>) -> Self {
        let tools = list_tools_result(&crate::mcp::tool::worker::descriptors());
        let resources = list_resources_result(&crate::mcp::resource::worker::descriptors());
        let resource_templates =
            list_resource_templates_result(&crate::mcp::resource::worker::templates());
        Self {
            service: DeferredService::from_startup_result(result),
            tools,
            resources,
            resource_templates,
        }
    }

    /// Dispatch one tool call to the runtime by name and JSON arguments.
    pub fn dispatch(
        &self, name: &str, args: serde_json::Map<String, serde_json::Value>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_tool_arguments(name, &args, &crate::mcp::tool::worker::descriptors())?;

        if name == "set_working_directory" {
            let path = required_str(&args, "path")?;
            return match self.service.bind(FsWorkerService::new(path)) {
                | Ok(()) => dispatch_tool(Ok(serde_json::json!({ "path": path }))),
                | Err(error) => Ok(tool_error_result(&error)),
            };
        }

        let guard = self.service.state.read().unwrap();
        let service = match &*guard {
            | ServiceState::Ready(service) => service,
            | ServiceState::Failed(error) => return Ok(tool_error_result(error)),
        };
        match name {
            | "get_contract" => dispatch_tool(service.contract()),
            | "read_inbox" => {
                let filter = extract_sequence_filter(&args)?;
                let include_body = optional_bool(&args, "include_body").unwrap_or(false);
                dispatch_tool(service.read_inbox(filter, include_body))
            }
            | "read_outbox" => {
                let filter = extract_sequence_filter(&args)?;
                let include_body = optional_bool(&args, "include_body").unwrap_or(false);
                dispatch_tool(service.read_outbox(filter, include_body))
            }
            | "ack_inbox_message" => {
                let sequence = required_u64(&args, "sequence")?;
                dispatch_tool(service.ack_inbox(Sequence(sequence)))
            }
            | "send_report" => {
                let head_commit = optional_str(&args, "head_commit").map(String::from);
                let forward_request = optional_str(&args, "forward_request")
                    .map(str::parse::<ForwardIntent>)
                    .transpose()
                    .map_err(|error| ErrorData::invalid_params(error, None))?;
                dispatch_tool(service.send_report(
                    head_commit,
                    forward_request,
                    extract_reply(&args),
                    extract_required_payload(&args)?,
                ))
            }
            | "send_commit" => {
                let head_commit = required_str(&args, "head_commit")?.to_string();
                dispatch_tool(service.send_commit(head_commit, extract_required_payload(&args)?))
            }
            | "get_status" => dispatch_tool(service.status()),
            | _ => Err(ErrorData::invalid_params(format!("unknown tool: {name}"), None)),
        }
    }

    /// Dispatch one resource read to the runtime by URI.
    pub fn read(&self, uri: &str) -> Result<ReadResourceResult, ErrorData> {
        if uri == "multorum://worker/methodology" {
            return resource_text_success(
                uri,
                MethodologyDocument::new(MethodologyRole::Worker).markdown().to_string(),
                "text/markdown",
            );
        }
        let guard = self.service.state.read().unwrap();
        let service = match &*guard {
            | ServiceState::Ready(service) => service,
            | ServiceState::Failed(error) => return Err(mcp_to_resource_error(error)),
        };
        match uri {
            | "multorum://worker/contract" => {
                let contract = service.contract().map_err(runtime_to_resource_error)?;
                resource_success(uri, &contract)
            }
            | "multorum://worker/inbox" => {
                let messages = service
                    .read_inbox(Default::default(), false)
                    .map_err(runtime_to_resource_error)?;
                resource_success(uri, &messages)
            }
            | "multorum://worker/status" => {
                let status = service.status().map_err(runtime_to_resource_error)?;
                resource_success(uri, &status)
            }
            | "multorum://worker/read-set"
            | "multorum://worker/write-set"
            | "multorum://worker/outbox"
            | "multorum://worker/transcript" => Err(ErrorData::resource_not_found(
                format!("resource not yet implemented: {uri}"),
                None,
            )),
            | _ => Err(ErrorData::resource_not_found(format!("unknown resource: {uri}"), None)),
        }
    }
}

impl ServerHandler for WorkerHandler {
    fn get_info(&self) -> ServerInfo {
        server_info("multorum-worker")
    }

    fn list_tools(
        &self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(self.tools.clone()))
    }

    fn call_tool(
        &self, request: CallToolRequestParams, _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        let args = args_or_empty(request.arguments);
        std::future::ready(self.dispatch(&request.name, args))
    }

    fn list_resources(
        &self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(self.resources.clone()))
    }

    fn list_resource_templates(
        &self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(self.resource_templates.clone()))
    }

    fn read_resource(
        &self, request: ReadResourceRequestParams, _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, ErrorData>> + Send + '_ {
        std::future::ready(self.read(&request.uri))
    }
}
