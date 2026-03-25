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

use crate::runtime::{FsWorkerService, Sequence, WorkerService};

use super::{
    args_or_empty, dispatch_tool, extract_payload, extract_reply, list_resource_templates_result,
    list_resources_result, list_tools_result, optional_str, optional_u64, required_str,
    required_u64, resource_success, runtime_to_resource_error, server_info,
    validate_tool_arguments,
};

/// MCP server handler for the worker surface.
pub struct WorkerHandler {
    service: FsWorkerService,
    tools: ListToolsResult,
    resources: ListResourcesResult,
    resource_templates: ListResourceTemplatesResult,
}

impl WorkerHandler {
    /// Construct the handler from a runtime worker service.
    pub fn new(service: FsWorkerService) -> Self {
        let tools = list_tools_result(&crate::mcp::tool::worker::descriptors());
        let resources = list_resources_result(&crate::mcp::resource::worker::descriptors());
        let resource_templates =
            list_resource_templates_result(&crate::mcp::resource::worker::templates());
        Self { service, tools, resources, resource_templates }
    }

    /// Dispatch one tool call to the runtime by name and JSON arguments.
    pub fn dispatch(
        &self, name: &str, args: serde_json::Map<String, serde_json::Value>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_tool_arguments(name, &args, &crate::mcp::tool::worker::descriptors())?;
        match name {
            | "get_contract" => dispatch_tool(self.service.contract()),
            | "read_inbox" => {
                let after = optional_u64(&args, "after").map(Sequence);
                dispatch_tool(self.service.read_inbox(after))
            }
            | "ack_inbox_message" => {
                let sequence = required_u64(&args, "sequence")?;
                dispatch_tool(self.service.ack_inbox(Sequence(sequence)))
            }
            | "send_report" => {
                let head_commit = optional_str(&args, "head_commit").map(String::from);
                dispatch_tool(self.service.send_report(
                    head_commit,
                    extract_reply(&args),
                    extract_payload(&args),
                ))
            }
            | "send_commit" => {
                let head_commit = required_str(&args, "head_commit")?.to_string();
                dispatch_tool(self.service.send_commit(head_commit, extract_payload(&args)))
            }
            | "get_status" => dispatch_tool(self.service.status()),
            | _ => Err(ErrorData::invalid_params(format!("unknown tool: {name}"), None)),
        }
    }

    /// Dispatch one resource read to the runtime by URI.
    pub fn read(&self, uri: &str) -> Result<ReadResourceResult, ErrorData> {
        match uri {
            | "multorum://worker/contract" => {
                let contract = self.service.contract().map_err(runtime_to_resource_error)?;
                resource_success(uri, &contract)
            }
            | "multorum://worker/inbox" => {
                let messages = self.service.read_inbox(None).map_err(runtime_to_resource_error)?;
                resource_success(uri, &messages)
            }
            | "multorum://worker/status" => {
                let status = self.service.status().map_err(runtime_to_resource_error)?;
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
