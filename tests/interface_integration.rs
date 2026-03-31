use clap::Parser;

use multorum::cli::{Cli, Command, MethodologyRoleArg, RuntimeCommand, UtilCommand, WorkerCommand};
use multorum::mcp::McpServer;

#[test]
fn cli_init_parses_top_level_runtime_command() {
    let cli = Cli::try_parse_from(["multorum", "init"]).unwrap();

    match cli.command {
        | Command::Runtime(RuntimeCommand::Init) => {}
        | command => panic!("unexpected command: {command:?}"),
    }
}

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
        "--body-text",
        "Start with the auth rulebook.",
    ])
    .unwrap();

    match cli.command {
        | Command::Runtime(RuntimeCommand::Worker {
            command: WorkerCommand::Create { perspective, worker_id, overwriting_worktree, .. },
        }) => {
            assert_eq!(perspective, "AuthImplementor");
            assert_eq!(worker_id.unwrap(), "custom-worker-7");
            assert!(overwriting_worktree);
        }
        | command => panic!("unexpected command: {command:?}"),
    }
}

#[test]
fn cli_create_accepts_invalid_perspective_for_runtime_validation() {
    let cli = Cli::try_parse_from([
        "multorum",
        "worker",
        "create",
        "lowercase_bad",
        "--body-text",
        "Start with the auth rulebook.",
    ])
    .unwrap();

    match cli.command {
        | Command::Runtime(RuntimeCommand::Worker {
            command: WorkerCommand::Create { perspective, .. },
        }) => {
            assert_eq!(perspective, "lowercase_bad");
        }
        | command => panic!("unexpected command: {command:?}"),
    }
}

#[test]
fn cli_show_accepts_invalid_worker_for_runtime_validation() {
    let cli = Cli::try_parse_from(["multorum", "worker", "show", "!!!invalid"]).unwrap();

    match cli.command {
        | Command::Runtime(RuntimeCommand::Worker {
            command: WorkerCommand::Show { worker_id },
        }) => {
            assert_eq!(worker_id, "!!!invalid");
        }
        | command => panic!("unexpected command: {command:?}"),
    }
}

#[test]
fn cli_methodology_parses_under_util() {
    let cli = Cli::try_parse_from(["multorum", "util", "methodology", "worker"]).unwrap();

    match cli.command {
        | Command::Util { command: UtilCommand::Methodology { role } } => {
            assert_eq!(role, MethodologyRoleArg::Worker);
        }
        | command => panic!("unexpected command: {command:?}"),
    }
}

#[test]
fn cli_serve_orchestrator_takes_no_path_arguments() {
    let cli = Cli::try_parse_from(["multorum", "serve", "orchestrator"]).unwrap();

    match cli.command {
        | Command::Serve { command: multorum::cli::ServeCommand::Orchestrator } => {}
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
fn orchestrator_mcp_exposes_methodology_resource() {
    let server = McpServer::orchestrator();
    assert!(server.resources.iter().any(|resource| {
        resource.uri == "multorum://orchestrator/methodology"
            && resource.mime_type == "text/markdown"
    }));
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
        "--body-text",
        "Merged after reviewing worker evidence.",
    ])
    .unwrap();

    match cli.command {
        | Command::Runtime(RuntimeCommand::Worker {
            command: WorkerCommand::Merge { worker_id, skip_checks, .. },
        }) => {
            assert_eq!(worker_id, "custom-worker-7");
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

#[test]
fn worker_mcp_exposes_methodology_resource() {
    let server = McpServer::worker("worker-7".parse().unwrap());
    assert!(server.resources.iter().any(|resource| {
        resource.uri == "multorum://worker/methodology" && resource.mime_type == "text/markdown"
    }));
}

#[test]
fn cli_create_requires_a_body_source() {
    let err = Cli::try_parse_from(["multorum", "worker", "create", "AuthImplementor"]).unwrap_err();
    let rendered = err.to_string();
    assert!(rendered.contains("--body-text"));
    assert!(rendered.contains("--body-path"));
}

#[test]
fn cli_merge_requires_a_body_source() {
    let err = Cli::try_parse_from(["multorum", "worker", "merge", "custom-worker-7"]).unwrap_err();
    let rendered = err.to_string();
    assert!(rendered.contains("--body-text"));
    assert!(rendered.contains("--body-path"));
}
