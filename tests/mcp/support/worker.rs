//! Worker creation helpers for integration tests.

use multorum::perspective::PerspectiveName;
use multorum::runtime::{CreateWorker, FsOrchestratorService, OrchestratorService};

pub fn perspective() -> PerspectiveName {
    PerspectiveName::new("AuthImplementor").unwrap()
}

/// Create a worker via the runtime (not MCP) and return the worktree path.
pub fn create_worker_runtime(
    orchestrator: &FsOrchestratorService,
) -> (multorum::runtime::WorkerId, std::path::PathBuf) {
    let result = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
    (result.worker_id, result.worktree_path)
}
