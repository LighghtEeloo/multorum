use clap::Parser;

use multorum::cli::{Cli, Command, RuntimeCommand, WorkerCommand};
use multorum::mcp::McpServer;

#[test]
fn cli_create_accepts_optional_worker_id() {
    let cli = Cli::try_parse_from([
        "multorum",
        "worker",
        "create",
        "AuthImplementor",
        "--worker",
        "custom-worker-7",
        "--overwriting-worktree",
    ])
    .unwrap();

    match cli.command {
        | Command::Runtime(RuntimeCommand::Worker {
            command: WorkerCommand::Create { perspective, worker_id, overwriting_worktree, .. },
        }) => {
            assert_eq!(perspective.as_str(), "AuthImplementor");
            assert_eq!(worker_id.unwrap().as_str(), "custom-worker-7");
            assert!(overwriting_worktree);
        }
        | command => panic!("unexpected command: {command:?}"),
    }
}

#[test]
fn orchestrator_mcp_create_descriptor_exposes_optional_worker_id() {
    let server = McpServer::orchestrator();
    let create = server
        .tools
        .iter()
        .find(|tool| tool.name == "create_worker")
        .expect("missing create_worker tool descriptor");

    assert!(create.inputs.iter().any(|input| {
        input.name == "worker"
            && !input.required
            && input.description.contains("default perspective-based identity")
    }));
    assert!(create.inputs.iter().any(|input| {
        input.name == "overwriting_worktree"
            && !input.required
            && input.description.contains("finalized workspace")
    }));
}

#[test]
fn orchestrator_mcp_delete_descriptor_requires_worker_id() {
    let server = McpServer::orchestrator();
    let delete = server
        .tools
        .iter()
        .find(|tool| tool.name == "delete_worker")
        .expect("missing delete_worker tool descriptor");

    assert_eq!(delete.inputs.len(), 1);
    assert_eq!(delete.inputs[0].name, "worker");
    assert!(delete.inputs[0].required);
}

#[test]
fn cli_merge_accepts_worker_id_and_skip_checks() {
    let cli = Cli::try_parse_from([
        "multorum",
        "worker",
        "merge",
        "custom-worker-7",
        "--skip-check",
        "unit",
    ])
    .unwrap();

    match cli.command {
        | Command::Runtime(RuntimeCommand::Worker {
            command: WorkerCommand::Merge { worker_id, skip_checks, .. },
        }) => {
            assert_eq!(worker_id.as_str(), "custom-worker-7");
            assert_eq!(skip_checks, vec!["unit"]);
        }
        | command => panic!("unexpected command: {command:?}"),
    }
}

#[test]
fn orchestrator_mcp_merge_descriptor_uses_merge_name() {
    let server = McpServer::orchestrator();
    let merge = server
        .tools
        .iter()
        .find(|tool| tool.name == "merge_worker")
        .expect("missing merge_worker tool descriptor");

    assert!(merge.inputs.iter().any(|input| {
        input.name == "worker" && input.required && input.description.contains("merge")
    }));
}
