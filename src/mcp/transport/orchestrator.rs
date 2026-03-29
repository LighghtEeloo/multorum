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

use crate::methodology::{MethodologyDocument, MethodologyRole};
use crate::runtime::{CreateWorker, FsOrchestratorService, OrchestratorService, WorkerId};

use super::{
    DeferredService, ServiceState, args_or_empty, dispatch_tool, extract_reply,
    extract_required_payload, extract_sequence_filter, list_resource_templates_result,
    list_resources_result, list_tools_result, mcp_to_resource_error, optional_bool, optional_str,
    optional_string_list, required_str, required_u64, resource_success, resource_text_success,
    runtime_to_resource_error, server_info, tool_error_result, validate_tool_arguments,
};

/// MCP server handler for the orchestrator surface.
///
/// The handler defaults to the process working directory at startup.
/// The `set_working_directory` tool allows the client to rebind the
/// runtime to a different workspace root at any time.
pub struct OrchestratorHandler {
    /// Runtime service, defaulting to cwd and rebindable via
    /// `set_working_directory`.
    service: DeferredService<FsOrchestratorService>,
    tools: ListToolsResult,
    resources: ListResourcesResult,
    resource_templates: ListResourceTemplatesResult,
}

impl Default for OrchestratorHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl OrchestratorHandler {
    /// Construct the handler with a pre-bound service.
    ///
    /// Note: This bypasses the default cwd binding and is intended
    /// for tests and programmatic embeddings that already hold a
    /// constructed service instance.
    pub fn with_service(service: FsOrchestratorService) -> Self {
        Self::from_startup_result(Ok(service))
    }

    /// Construct the handler, defaulting to the process working
    /// directory.
    pub fn new() -> Self {
        Self::from_startup_result(FsOrchestratorService::from_current_dir())
    }

    /// Construct the handler from an explicit startup result.
    fn from_startup_result(result: crate::runtime::Result<FsOrchestratorService>) -> Self {
        let tools = list_tools_result(&crate::mcp::tool::orchestrator::descriptors());
        let resources = list_resources_result(&crate::mcp::resource::orchestrator::descriptors());
        let resource_templates =
            list_resource_templates_result(&crate::mcp::resource::orchestrator::templates());
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
        validate_tool_arguments(name, &args, &crate::mcp::tool::orchestrator::descriptors())?;

        if name == "set_working_directory" {
            let path = required_str(&args, "path")?;
            return match self.service.bind(FsOrchestratorService::new(path)) {
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
            | "rulebook_init" => dispatch_tool(service.rulebook_init()),
            | "list_perspectives" => dispatch_tool(service.list_perspectives()),
            | "validate_perspectives" => {
                let perspectives = optional_string_list(&args, "perspectives")
                    .into_iter()
                    .map(|s| parse_perspective(&s))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let no_live = optional_bool(&args, "no_live").unwrap_or(false);
                dispatch_tool(service.validate_perspectives(perspectives, no_live))
            }
            | "forward_perspective" => {
                let perspective = parse_perspective(required_str(&args, "perspective")?)?;
                dispatch_tool(service.forward_perspective(perspective))
            }
            | "list_workers" => dispatch_tool(service.list_workers()),
            | "get_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                dispatch_tool(service.get_worker(worker_id))
            }
            | "read_worker_outbox" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                let filter = extract_sequence_filter(&args)?;
                let include_body = optional_bool(&args, "include_body").unwrap_or(false);
                dispatch_tool(service.read_outbox(worker_id, filter, include_body))
            }
            | "read_worker_inbox" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                let filter = extract_sequence_filter(&args)?;
                let include_body = optional_bool(&args, "include_body").unwrap_or(false);
                dispatch_tool(service.read_inbox(worker_id, filter, include_body))
            }
            | "ack_worker_outbox_message" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                let sequence = required_u64(&args, "sequence")?;
                dispatch_tool(service.ack_outbox(worker_id, crate::runtime::Sequence(sequence)))
            }
            | "create_worker" => {
                let perspective = parse_perspective(required_str(&args, "perspective")?)?;
                let mut request = CreateWorker::new(perspective);
                if let Some(id) = optional_str(&args, "worker") {
                    request = request.with_worker_id(parse_worker_id(id)?);
                }
                if optional_bool(&args, "overwriting_worktree").unwrap_or(false) {
                    request = request.with_overwriting_worktree();
                }
                if optional_bool(&args, "no_auto_forward").unwrap_or(false) {
                    request = request.without_auto_forward();
                }
                request = request.with_task(extract_required_payload(&args)?);
                dispatch_tool(service.create_worker(request))
            }
            | "resolve_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                dispatch_tool(service.resolve_worker(
                    worker_id,
                    extract_reply(&args),
                    extract_required_payload(&args)?,
                    !optional_bool(&args, "no_auto_forward").unwrap_or(false),
                ))
            }
            | "hint_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                dispatch_tool(service.hint_worker(
                    worker_id,
                    extract_reply(&args),
                    extract_required_payload(&args)?,
                ))
            }
            | "revise_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                dispatch_tool(service.revise_worker(
                    worker_id,
                    extract_reply(&args),
                    extract_required_payload(&args)?,
                ))
            }
            | "discard_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                dispatch_tool(service.discard_worker(worker_id))
            }
            | "delete_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                dispatch_tool(service.delete_worker(worker_id))
            }
            | "merge_worker" => {
                let worker_id = parse_worker_id(required_str(&args, "worker")?)?;
                let skip_checks = optional_string_list(&args, "skip_checks");
                let audit_payload = extract_required_payload(&args)?;
                dispatch_tool(service.merge_worker(worker_id, skip_checks, audit_payload))
            }
            | "get_status" => dispatch_tool(service.status()),
            | _ => Err(ErrorData::invalid_params(format!("unknown tool: {name}"), None)),
        }
    }

    /// Dispatch one resource read to the runtime by URI.
    pub fn read(&self, uri: &str) -> Result<ReadResourceResult, ErrorData> {
        if uri == "multorum://orchestrator/methodology" {
            return resource_text_success(
                uri,
                MethodologyDocument::new(MethodologyRole::Orchestrator).markdown().to_string(),
                "text/markdown",
            );
        }
        let guard = self.service.state.read().unwrap();
        let service = match &*guard {
            | ServiceState::Ready(service) => service,
            | ServiceState::Failed(error) => return Err(mcp_to_resource_error(error)),
        };
        match uri {
            | "multorum://orchestrator/status" => {
                let status = service.status().map_err(runtime_to_resource_error)?;
                resource_success(uri, &status)
            }
            | "multorum://orchestrator/perspectives" => {
                let perspectives =
                    service.list_perspectives().map_err(runtime_to_resource_error)?;
                resource_success(uri, &perspectives)
            }
            | "multorum://orchestrator/workers" => {
                let workers = service.list_workers().map_err(runtime_to_resource_error)?;
                resource_success(uri, &workers)
            }
            | _ if uri.starts_with("multorum://orchestrator/workers/") => {
                self.read_worker_resource(service, uri)
            }
            | _ => Err(ErrorData::resource_not_found(format!("unknown resource: {uri}"), None)),
        }
    }

    /// Handle parameterised worker resource URIs.
    fn read_worker_resource(
        &self, service: &FsOrchestratorService, uri: &str,
    ) -> Result<ReadResourceResult, ErrorData> {
        let path = uri.strip_prefix("multorum://orchestrator/workers/").unwrap_or("");

        let (worker_id_str, sub) = match path.find('/') {
            | Some(pos) => (&path[..pos], Some(&path[pos + 1..])),
            | None => (path, None),
        };

        let worker_id = worker_id_str
            .parse::<WorkerId>()
            .map_err(|e| ErrorData::invalid_params(format!("invalid worker: {e}"), None))?;

        match sub {
            | None => {
                let detail = service.get_worker(worker_id).map_err(runtime_to_resource_error)?;
                resource_success(uri, &detail)
            }
            | Some("outbox") => {
                let messages = service
                    .read_outbox(worker_id, Default::default(), false)
                    .map_err(runtime_to_resource_error)?;
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
    s.parse().map_err(|e| ErrorData::invalid_params(format!("invalid worker: {e}"), None))
}

fn parse_perspective(s: &str) -> Result<crate::schema::perspective::PerspectiveName, ErrorData> {
    s.parse().map_err(|e| ErrorData::invalid_params(format!("invalid perspective name: {e}"), None))
}
