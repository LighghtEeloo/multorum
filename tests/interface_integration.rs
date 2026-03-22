use clap::Parser;

use multorum::cli::{Cli, Command, WorkerCommand};
use multorum::mcp::McpServer;

#[test]
fn cli_provision_accepts_optional_worker_id() {
    let cli = Cli::try_parse_from([
        "multorum",
        "worker",
        "provision",
        "AuthImplementor",
        "--worker-id",
        "custom_worker_7",
    ])
    .unwrap();

    match cli.command {
        | Command::Worker { command: WorkerCommand::Provision { perspective, worker_id, .. } } => {
            assert_eq!(perspective.as_str(), "AuthImplementor");
            assert_eq!(worker_id.unwrap().as_str(), "custom_worker_7");
        }
        | command => panic!("unexpected command: {command:?}"),
    }
}

#[test]
fn orchestrator_mcp_provision_descriptor_exposes_optional_worker_id() {
    let server = McpServer::orchestrator();
    let provision = server
        .tools
        .iter()
        .find(|tool| tool.name == "provision_worker")
        .expect("missing provision_worker tool descriptor");

    assert!(provision.inputs.iter().any(|input| {
        input.name == "worker_id"
            && !input.required
            && input.description.contains("default perspective-based worker id")
    }));
}
