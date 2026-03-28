//! In-memory duplex transport helpers for wire-level MCP tests.

use std::path::Path;

use rmcp::ServiceExt;
use tempfile::TempDir;

use multorum::mcp::transport::orchestrator::OrchestratorHandler;
use multorum::mcp::transport::worker::WorkerHandler;
use multorum::runtime::FsWorkerService;

use super::repo::setup_repo;

/// Connect an `OrchestratorHandler` to an in-memory rmcp client via duplex.
///
/// Returns the tempdir (must be held alive), and the client running service.
/// The server runs in a background tokio task.
pub async fn orchestrator_duplex() -> (TempDir, rmcp::service::RunningService<rmcp::RoleClient, ()>)
{
    let (dir, svc) = setup_repo();
    let handler = OrchestratorHandler::with_service(svc);
    let (server_io, client_io) = tokio::io::duplex(65536);
    tokio::spawn(async move {
        let server = handler.serve(server_io).await.expect("server failed to start");
        server.waiting().await.expect("server stopped unexpectedly");
    });
    let client = ().serve(client_io).await.expect("client failed to connect");
    (dir, client)
}

/// Connect a `WorkerHandler` to an in-memory rmcp client via duplex.
///
/// The caller must keep the orchestrator tempdir alive.
pub async fn worker_duplex(worktree: &Path) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let worker_svc = FsWorkerService::new(worktree).unwrap();
    let handler = WorkerHandler::with_service(worker_svc);
    let (server_io, client_io) = tokio::io::duplex(65536);
    tokio::spawn(async move {
        let server = handler.serve(server_io).await.expect("worker server failed to start");
        server.waiting().await.expect("worker server stopped unexpectedly");
    });
    ().serve(client_io).await.expect("worker client failed to connect")
}
