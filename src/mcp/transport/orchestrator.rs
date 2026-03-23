//! Orchestrator MCP server handler.
//!
//! Implements [`rmcp::ServerHandler`] by dispatching tool and resource
//! requests to an [`OrchestratorService`] instance.

use std::future::Future;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ListResourceTemplatesResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResult,
    ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler};

use crate::runtime::{CreateWorker, FsOrchestratorService, OrchestratorService, WorkerId};

use super::{
    args_or_empty, dispatch_tool, extract_payload, extract_reply, list_resource_templates_result,
    list_resources_result, list_tools_result, optional_bool, optional_str, optional_string_list,
    optional_u64, required_str, resource_success, runtime_to_resource_error, server_info,
};

/// MCP server handler for the orchestrator surface.
pub struct OrchestratorHandler {
    service: FsOrchestratorService,
    tools: ListToolsResult,
    resources: ListResourcesResult,
    resource_templates: ListResourceTemplatesResult,
}

impl OrchestratorHandler {
    /// Construct the handler from a runtime orchestrator service.
    pub fn new(service: FsOrchestratorService) -> Self {
        let tools = list_tools_result(&crate::mcp::tool::orchestrator::descriptors());
        let resources = list_resources_result(&crate::mcp::resource::orchestrator::descriptors());
        let resource_templates =
            list_resource_templates_result(&crate::mcp::resource::orchestrator::templates());
        Self { service, tools, resources, resource_templates }
    }

    /// Dispatch one tool call to the runtime by name and JSON arguments.
    pub fn dispatch(
        &self, name: &str, args: serde_json::Map<String, serde_json::Value>,
    ) -> Result<CallToolResult, ErrorData> {
        match name {
            | "rulebook_init" => dispatch_tool(self.service.rulebook_init()),
            | "rulebook_validate" => dispatch_tool(self.service.rulebook_validate()),
            | "rulebook_install" => dispatch_tool(self.service.rulebook_install()),
            | "rulebook_uninstall" => dispatch_tool(self.service.rulebook_uninstall()),
            | "list_perspectives" => dispatch_tool(self.service.list_perspectives()),
            | "list_workers" => dispatch_tool(self.service.list_workers()),
            | "get_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker_id")?)?;
                dispatch_tool(self.service.get_worker(worker_id))
            }
            | "read_worker_outbox" => {
                let worker_id = parse_worker_id(required_str(&args, "worker_id")?)?;
                let after = optional_u64(&args, "after").map(crate::runtime::Sequence);
                dispatch_tool(self.service.read_outbox(worker_id, after))
            }
            | "ack_worker_outbox_message" => {
                let worker_id = parse_worker_id(required_str(&args, "worker_id")?)?;
                let sequence = required_u64(&args, "sequence")?;
                dispatch_tool(
                    self.service.ack_outbox(worker_id, crate::runtime::Sequence(sequence)),
                )
            }
            | "create_worker" => {
                let perspective = parse_perspective(required_str(&args, "perspective")?)?;
                let mut request = CreateWorker::new(perspective);
                if let Some(id) = optional_str(&args, "worker_id") {
                    request = request.with_worker_id(parse_worker_id(id)?);
                }
                if optional_bool(&args, "overwriting_worktree").unwrap_or(false) {
                    request = request.with_overwriting_worktree();
                }
                let payload = extract_payload(&args);
                if !payload.is_empty() {
                    request = request.with_task(payload);
                }
                dispatch_tool(self.service.create_worker(request))
            }
            | "resolve_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker_id")?)?;
                dispatch_tool(self.service.resolve_worker(
                    worker_id,
                    extract_reply(&args),
                    extract_payload(&args),
                ))
            }
            | "revise_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker_id")?)?;
                dispatch_tool(self.service.revise_worker(
                    worker_id,
                    extract_reply(&args),
                    extract_payload(&args),
                ))
            }
            | "discard_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker_id")?)?;
                dispatch_tool(self.service.discard_worker(worker_id))
            }
            | "delete_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker_id")?)?;
                dispatch_tool(self.service.delete_worker(worker_id))
            }
            | "merge_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker_id")?)?;
                let skip_checks = optional_string_list(&args, "skip_checks");
                dispatch_tool(self.service.merge_worker(worker_id, skip_checks))
            }
            | "get_status" => dispatch_tool(self.service.status()),
            | _ => Err(ErrorData::invalid_params(format!("unknown tool: {name}"), None)),
        }
    }

    /// Dispatch one resource read to the runtime by URI.
    pub fn read(&self, uri: &str) -> Result<ReadResourceResult, ErrorData> {
        match uri {
            | "multorum://orchestrator/status" => {
                let status = self.service.status().map_err(runtime_to_resource_error)?;
                resource_success(uri, &status)
            }
            | "multorum://orchestrator/rulebook/active" => {
                let status = self.service.status().map_err(runtime_to_resource_error)?;
                resource_success(
                    uri,
                    &serde_json::json!({
                        "active_rulebook_commit": status.active_rulebook_commit,
                    }),
                )
            }
            | "multorum://orchestrator/perspectives" => {
                let perspectives =
                    self.service.list_perspectives().map_err(runtime_to_resource_error)?;
                resource_success(uri, &perspectives)
            }
            | "multorum://orchestrator/workers" => {
                let workers = self.service.list_workers().map_err(runtime_to_resource_error)?;
                resource_success(uri, &workers)
            }
            | _ if uri.starts_with("multorum://orchestrator/workers/") => {
                self.read_worker_resource(uri)
            }
            | _ => Err(ErrorData::resource_not_found(format!("unknown resource: {uri}"), None)),
        }
    }

    /// Handle parameterised worker resource URIs.
    fn read_worker_resource(&self, uri: &str) -> Result<ReadResourceResult, ErrorData> {
        let path = uri.strip_prefix("multorum://orchestrator/workers/").unwrap_or("");

        let (worker_id_str, sub) = match path.find('/') {
            | Some(pos) => (&path[..pos], Some(&path[pos + 1..])),
            | None => (path, None),
        };

        let worker_id = worker_id_str
            .parse::<WorkerId>()
            .map_err(|e| ErrorData::invalid_params(format!("invalid worker id: {e}"), None))?;

        match sub {
            | None => {
                let detail =
                    self.service.get_worker(worker_id).map_err(runtime_to_resource_error)?;
                resource_success(uri, &detail)
            }
            | Some("outbox") => {
                let messages =
                    self.service.read_outbox(worker_id, None).map_err(runtime_to_resource_error)?;
                resource_success(uri, &messages)
            }
            | Some("contract" | "transcript" | "checks") => Err(ErrorData::resource_not_found(
                format!("resource not yet implemented: {uri}"),
                None,
            )),
            | Some(other) => Err(ErrorData::resource_not_found(
                format!("unknown worker sub-resource: {other}"),
                None,
            )),
        }
    }
}

impl ServerHandler for OrchestratorHandler {
    fn get_info(&self) -> ServerInfo {
        server_info("multorum-orchestrator")
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

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

fn parse_worker_id(s: &str) -> Result<WorkerId, ErrorData> {
    s.parse().map_err(|e| ErrorData::invalid_params(format!("invalid worker id: {e}"), None))
}

fn parse_perspective(s: &str) -> Result<crate::perspective::PerspectiveName, ErrorData> {
    s.parse().map_err(|e| ErrorData::invalid_params(format!("invalid perspective name: {e}"), None))
}

/// Extract a required u64 argument.
fn required_u64(
    args: &serde_json::Map<String, serde_json::Value>, key: &str,
) -> Result<u64, ErrorData> {
    args.get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ErrorData::invalid_params(format!("missing required field: {key}"), None))
}
