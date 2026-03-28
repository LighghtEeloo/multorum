//! Command-line interface for the Multorum orchestrator.
//!
//! Every state transition in Multorum is the result of an explicit
//! orchestrator instruction issued through this CLI. Multorum is
//! purely reactive — it never acts on its own initiative.
//!
//! The command surface follows the runtime model directly:
//!
//! - `init` bootstraps `.multorum/` and the default committed artifacts.
//! - `perspective` inspects and validates declared roles from the current rulebook.
//! - `worker` addresses orchestrator-side operations on concrete workers.
//! - `local` addresses worker-local operations from inside a worker
//!   worktree.
//! - `methodology` prints the high-level role guidance shipped with the
//!   binary so agents can bootstrap themselves without repository-local
//!   skill files.

use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

use crate::{
    methodology::{MethodologyDocument, MethodologyRole},
    runtime::{
        self, CreateWorker, FsOrchestratorService, FsWorkerService, OrchestratorService, WorkerId,
        WorkerService,
    },
    schema::perspective::PerspectiveName,
};

/// Multorum — multi-perspective conflict-free codebase orchestration.
///
/// Infrastructure for managing multiple simultaneous perspectives on a
/// single codebase.
#[derive(Debug, Parser)]
#[command(name = "multorum", version, about)]
pub struct Cli {
    /// The orchestrator instruction to execute.
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Parse command-line arguments and execute the instruction.
    pub fn run() {
        let cli = Self::parse();
        match cli.command {
            | Command::Methodology { role } => {
                println!("{}", MethodologyDocument::new(role.into()).markdown());
            }
            | Command::Util { command } => command.execute(),
            | Command::Serve { command } => command.execute(),
            | Command::Runtime(RuntimeCommand::Init) => {
                if let Err(error) = RuntimeCommand::execute_init_from_current_dir() {
                    eprintln!("error: {error}");
                    std::process::exit(1);
                }
            }
            | Command::Runtime(command) => {
                let services = match CliServices::from_current_dir() {
                    | Ok(services) => services,
                    | Err(error) => {
                        eprintln!("error: {error}");
                        std::process::exit(1);
                    }
                };
                if let Err(error) = command.execute(&services) {
                    eprintln!("error: {error}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Runtime service container used by the CLI frontend.
///
#[derive(Debug)]
pub struct CliServices {
    runtime: CliRuntime,
}

/// Instantiated runtime surface for the current repository.
#[derive(Debug)]
enum CliRuntime {
    /// Orchestrator-facing service bound to the canonical workspace.
    Orchestrator(FsOrchestratorService),
    /// Worker-facing service bound to one managed worktree.
    Worker(FsWorkerService),
}

impl CliServices {
    /// Build CLI services from the current directory.
    pub fn from_current_dir() -> runtime::Result<Self> {
        let project = runtime::project::CurrentProject::from_current_dir()?;
        let runtime = match project.role() {
            | runtime::project::RuntimeRole::Orchestrator => CliRuntime::Orchestrator(
                FsOrchestratorService::new(project.orchestrator_workspace_root()?.to_path_buf())?,
            ),
            | runtime::project::RuntimeRole::Worker => {
                CliRuntime::Worker(FsWorkerService::new(project.worker_repo_root()?.to_path_buf())?)
            }
        };
        Ok(Self { runtime })
    }

    /// Borrow the orchestrator service for commands that require it.
    pub fn orchestrator(&self) -> runtime::Result<&FsOrchestratorService> {
        match &self.runtime {
            | CliRuntime::Orchestrator(service) => Ok(service),
            | CliRuntime::Worker(_) => Err(runtime::RuntimeError::RuntimeRoleMismatch {
                expected: "orchestrator",
                actual: "worker",
                repo_root: runtime::project::CurrentProject::from_current_dir()?
                    .repo_root()
                    .to_path_buf(),
            }),
        }
    }

    /// Construct the worker service for the current worktree.
    pub fn worker(&self) -> runtime::Result<&FsWorkerService> {
        match &self.runtime {
            | CliRuntime::Worker(service) => Ok(service),
            | CliRuntime::Orchestrator(_) => Err(runtime::RuntimeError::RuntimeRoleMismatch {
                expected: "worker",
                actual: "orchestrator",
                repo_root: runtime::project::CurrentProject::from_current_dir()?
                    .repo_root()
                    .to_path_buf(),
            }),
        }
    }
}

/// Shared payload options for commands that publish mailbox bundles.
///
/// `--body-text` and `--body-path` are mutually exclusive.
#[derive(Debug, Clone, Args)]
pub struct BundlePayloadArgs {
    /// Inline Markdown body text for the bundle's `body.md`.
    ///
    /// Mutually exclusive with `--body-path`.
    #[arg(long = "body-text", value_name = "TEXT", conflicts_with = "body_path")]
    pub body_text: Option<String>,

    /// Existing Markdown file to move into the bundle's `body.md`.
    ///
    /// On successful publication, Multorum consumes the path and stores
    /// the moved file under its managed `.multorum/` runtime state.
    /// Mutually exclusive with `--body-text`.
    #[arg(long = "body-path", value_name = "FILE", conflicts_with = "body_text")]
    pub body_path: Option<PathBuf>,

    /// Files to move under the bundle's `artifacts/` directory.
    ///
    /// On successful publication, Multorum consumes each path and keeps
    /// the moved artifact under `.multorum/`.
    #[arg(long = "artifact", value_name = "FILE")]
    pub artifacts: Vec<PathBuf>,
}

impl BundlePayloadArgs {
    /// Convert CLI payload arguments into runtime bundle payload.
    pub fn into_runtime(self) -> crate::bundle::BundlePayload {
        crate::bundle::BundlePayload {
            body_text: self.body_text,
            body_path: self.body_path,
            artifacts: self.artifacts,
        }
    }
}

/// Shared sequence filtering options for mailbox read commands.
///
/// `--exact` is mutually exclusive with `--from` and `--to`.
#[derive(Debug, Clone, Args)]
pub struct SequenceFilterArgs {
    /// Inclusive lower-bound sequence number.
    #[arg(long = "from", value_name = "SEQUENCE", conflicts_with = "exact")]
    pub from: Option<u64>,

    /// Inclusive upper-bound sequence number.
    #[arg(long = "to", value_name = "SEQUENCE", conflicts_with = "exact")]
    pub to: Option<u64>,

    /// Return exactly one message by sequence number.
    #[arg(long = "exact", value_name = "SEQUENCE", conflicts_with_all = ["from", "to"])]
    pub exact: Option<u64>,
}

impl SequenceFilterArgs {
    /// Convert CLI filter arguments into runtime sequence filter.
    pub fn into_runtime(self) -> runtime::SequenceFilter {
        if let Some(seq) = self.exact {
            runtime::SequenceFilter::Exact(runtime::Sequence(seq))
        } else {
            runtime::SequenceFilter::Range {
                from: self.from.map(runtime::Sequence),
                to: self.to.map(runtime::Sequence),
            }
        }
    }
}

/// Shared reply metadata for mailbox bundles that answer earlier messages.
#[derive(Debug, Clone, Args)]
pub struct ReplyReferenceArgs {
    /// Sequence number of the message this bundle answers.
    #[arg(long = "reply-to", value_name = "SEQUENCE")]
    pub in_reply_to: Option<u64>,
}

impl ReplyReferenceArgs {
    /// Convert CLI reply metadata into runtime reply metadata.
    pub fn into_runtime(self) -> runtime::ReplyReference {
        runtime::ReplyReference { in_reply_to: self.in_reply_to.map(runtime::Sequence) }
    }
}

/// Top-level CLI commands.
///
/// Runtime commands (`init`, `perspective`, `worker`, etc.) require a
/// Multorum repository and are flattened into the top-level namespace.
/// Utility commands are self-contained and never need runtime services.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print high-level methodology for one runtime role.
    ///
    /// This command is self-contained and does not require a managed
    /// repository. It is intended for agents that need bootstrap
    /// guidance before they begin calling the runtime surface.
    Methodology {
        /// Role whose methodology should be printed.
        role: MethodologyRoleArg,
    },

    /// Runtime commands that operate on a Multorum repository.
    #[command(flatten)]
    Runtime(RuntimeCommand),

    /// Start an MCP server on stdio.
    Serve {
        #[command(subcommand)]
        command: ServeCommand,
    },

    /// Shell utilities.
    Util {
        #[command(subcommand)]
        command: UtilCommand,
    },
}

/// MCP server mode selection.
#[derive(Debug, Subcommand)]
pub enum ServeCommand {
    /// Start the orchestrator MCP server.
    ///
    /// Exposes orchestrator tools and resources over stdio using the
    /// Model Context Protocol. The server defaults to the process
    /// working directory; clients may call `set_working_directory`
    /// to rebind the runtime to a different workspace root.
    Orchestrator,

    /// Start a worker MCP server.
    ///
    /// Exposes worker tools and resources over stdio using the Model
    /// Context Protocol. The server defaults to the process working
    /// directory; clients may call `set_working_directory` to rebind
    /// the runtime to a different worktree root.
    Worker,
}

/// CLI selector for one shipped methodology document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MethodologyRoleArg {
    /// Print the orchestrator methodology.
    Orchestrator,
    /// Print the worker methodology.
    Worker,
}

impl From<MethodologyRoleArg> for MethodologyRole {
    fn from(value: MethodologyRoleArg) -> Self {
        match value {
            | MethodologyRoleArg::Orchestrator => MethodologyRole::Orchestrator,
            | MethodologyRoleArg::Worker => MethodologyRole::Worker,
        }
    }
}

/// Commands that require a Multorum repository and runtime services.
#[derive(Debug, Subcommand)]
pub enum RuntimeCommand {
    /// Initialize `.multorum/` with the default committed artifacts.
    ///
    /// Creates `.multorum/rulebook.toml` from the checked-in default
    /// template, ensures `.multorum/.gitignore` ignores the runtime
    /// directories, prepares the local orchestrator runtime
    /// directories, and creates empty `group/` and `worker/` state
    /// folders under `.multorum/orchestrator/`.
    Init,

    /// Inspect and validate perspectives from the current rulebook.
    Perspective {
        #[command(subcommand)]
        command: PerspectiveCommand,
    },

    /// Operate on orchestrator-visible workers.
    Worker {
        #[command(subcommand)]
        command: WorkerCommand,
    },

    /// Operate on the current worker worktree.
    Local {
        #[command(subcommand)]
        command: LocalCommand,
    },

    /// Return the full orchestrator status snapshot.
    Status,
}

/// Shell utility commands.
#[derive(Debug, Subcommand)]
pub enum UtilCommand {
    /// Emit shell completions to stdout.
    ///
    /// Source the output in your shell profile to enable tab completion.
    /// For example: `source <(multorum util completion bash)`.
    Completion {
        /// Target shell.
        shell: Shell,
    },
}

/// Perspective inspection commands.
#[derive(Debug, Subcommand)]
pub enum PerspectiveCommand {
    /// List compiled perspectives from the current rulebook.
    List,

    /// Validate a set of perspectives for conflict-freedom.
    ///
    /// Checks the named perspectives against each other and against
    /// active bidding groups. With `--no-live`, active groups are
    /// ignored.
    Validate {
        /// Perspectives to check.
        perspectives: Vec<PerspectiveName>,

        /// Skip checking against active bidding groups.
        #[arg(long)]
        no_live: bool,
    },

    /// Forward one blocked bidding group to HEAD.
    Forward {
        /// Perspective whose live bidding group should move forward.
        perspective: PerspectiveName,
    },
}

/// Orchestrator-side worker commands.
#[derive(Debug, Subcommand)]
pub enum WorkerCommand {
    /// Create a new worker workspace from one perspective.
    Create {
        /// Perspective to instantiate.
        perspective: PerspectiveName,

        /// Optional runtime worker identity chosen by the orchestrator.
        ///
        /// When omitted, Multorum allocates a default perspective-based
        /// identity automatically.
        #[arg(long = "worker", value_name = "WORKER")]
        worker_id: Option<WorkerId>,

        /// Replace an existing finalized workspace for the same
        /// explicit worker.
        #[arg(long = "overwriting-worktree")]
        overwriting_worktree: bool,

        /// Optional payload for the initial `task` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,
    },

    /// List active workers.
    List,

    /// Show one worker in detail.
    Show {
        /// Worker identity to inspect.
        worker_id: WorkerId,
    },

    /// List messages sent by a worker to the orchestrator.
    Outbox {
        /// Worker identity whose outbox should be read.
        worker_id: WorkerId,

        #[command(flatten)]
        filter: SequenceFilterArgs,

        /// Include full body.md content for each message.
        #[arg(long)]
        body: bool,
    },

    /// List messages sent by the orchestrator to a worker.
    Inbox {
        /// Worker identity whose inbox should be read.
        worker_id: WorkerId,

        #[command(flatten)]
        filter: SequenceFilterArgs,

        /// Include full body.md content for each message.
        #[arg(long)]
        body: bool,
    },

    /// Acknowledge one worker outbox message.
    Ack {
        /// Worker identity whose outbox owns the message.
        worker_id: WorkerId,

        /// Sequence number to acknowledge.
        sequence: u64,
    },

    /// Publish a `resolve` bundle to a blocked worker inbox.
    Resolve {
        /// Worker identity to resolve.
        worker_id: WorkerId,

        /// Optional payload for the `resolve` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,

        /// Optional reply metadata for the `resolve` bundle.
        #[command(flatten)]
        reply: ReplyReferenceArgs,
    },

    /// Publish an advisory `hint` bundle to an active worker inbox.
    ///
    /// Use this to pass new project information or ask the worker to
    /// gracefully block itself by sending a report.
    Hint {
        /// Worker identity to notify.
        worker_id: WorkerId,

        /// Optional payload for the `hint` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,

        /// Optional reply metadata for the `hint` bundle.
        #[command(flatten)]
        reply: ReplyReferenceArgs,
    },

    /// Publish a `revise` bundle to a committed worker inbox.
    Revise {
        /// Worker identity to revise.
        worker_id: WorkerId,

        /// Optional payload for the `revise` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,

        /// Optional reply metadata for the `revise` bundle.
        #[command(flatten)]
        reply: ReplyReferenceArgs,
    },

    /// Finalize a worker without integration.
    Discard {
        /// Worker identity to discard.
        worker_id: WorkerId,
    },

    /// Delete one finalized worker workspace.
    Delete {
        /// Worker identity whose workspace should be deleted.
        worker_id: WorkerId,
    },

    /// Run the pre-merge pipeline and merge one worker.
    Merge {
        /// Worker identity to merge.
        worker_id: WorkerId,

        /// Checks to skip based on trusted worker evidence.
        #[arg(long = "skip-check", value_name = "CHECK")]
        skip_checks: Vec<String>,

        /// Optional audit rationale payload.
        ///
        /// Prefer self-contained findings in this payload instead of
        /// references to worker outbox paths, because worker runtime
        /// state may be deleted after merge confirmation.
        #[command(flatten)]
        payload: BundlePayloadArgs,
    },
}

/// Worker-local commands.
#[derive(Debug, Subcommand)]
pub enum LocalCommand {
    /// Load the worker contract for the current worktree.
    Contract,

    /// Return the projected worker status for the current worktree.
    Status,

    /// List messages sent by the orchestrator to this worker.
    Inbox {
        #[command(flatten)]
        filter: SequenceFilterArgs,

        /// Include full body.md content for each message.
        #[arg(long)]
        body: bool,
    },

    /// List messages sent by this worker to the orchestrator.
    Outbox {
        #[command(flatten)]
        filter: SequenceFilterArgs,

        /// Include full body.md content for each message.
        #[arg(long)]
        body: bool,
    },

    /// Acknowledge one inbox message.
    Ack {
        /// Sequence number to acknowledge.
        sequence: u64,
    },

    /// Publish a blocker report from the current worktree.
    Report {
        /// Optional git commit hash relevant to the report.
        #[arg(long = "head-commit", value_name = "COMMIT")]
        head_commit: Option<String>,

        /// Optional reply metadata for the `report` bundle.
        #[command(flatten)]
        reply: ReplyReferenceArgs,

        /// Optional payload for the `report` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,
    },

    /// Publish a completed worker submission from the current worktree.
    Commit {
        /// The git commit hash submitted by the worker.
        #[arg(long = "head-commit", value_name = "COMMIT")]
        head_commit: String,

        /// Optional payload for the `commit` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,
    },
}

impl ServeCommand {
    /// Start an MCP server on stdio in the selected mode.
    ///
    /// The server defaults to the process working directory. Clients
    /// may call `set_working_directory` to rebind to a different path.
    pub fn execute(self) {
        use crate::mcp::transport::{orchestrator::OrchestratorHandler, worker::WorkerHandler};

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap_or_else(
            |e| {
                eprintln!("error: failed to start async runtime: {e}");
                std::process::exit(1);
            },
        );

        async fn run_server<H: rmcp::ServerHandler>(handler: H) {
            let transport = rmcp::transport::io::stdio();
            match rmcp::serve_server(handler, transport).await {
                | Ok(running) => {
                    let _ = running.waiting().await;
                }
                | Err(e) => {
                    eprintln!("error: MCP server failed to initialize: {e}");
                    std::process::exit(1);
                }
            }
        }

        rt.block_on(async {
            match self {
                | Self::Orchestrator => run_server(OrchestratorHandler::new()).await,
                | Self::Worker => run_server(WorkerHandler::new()).await,
            }
        });
    }
}

impl UtilCommand {
    /// Execute one utility command.
    ///
    /// Utility commands are self-contained and never need runtime services.
    pub fn execute(self) {
        match self {
            | Self::Completion { shell } => {
                let mut cmd = Cli::command();
                clap_complete::generate(shell, &mut cmd, "multorum", &mut std::io::stdout());
            }
        }
    }
}

impl RuntimeCommand {
    /// Execute `multorum init` from any repository state.
    ///
    /// This path intentionally bypasses strict managed-role detection so
    /// it can bootstrap unmanaged repositories and repair ambiguous
    /// marker layouts.
    pub fn execute_init_from_current_dir() -> runtime::Result<()> {
        let target = runtime::project::CurrentProject::init_target_from_current_dir()?;
        let service = runtime::FsOrchestratorService::new(target.workspace_root().to_path_buf())?;
        let mut result = service.rulebook_init()?;
        result.warnings.extend(target.warnings().iter().cloned());
        for warning in &result.warnings {
            eprintln!("warning: {warning}");
        }
        println!("{result:#?}");
        Ok(())
    }

    /// Execute one runtime command against the given services.
    pub fn execute(self, services: &CliServices) -> runtime::Result<()> {
        match self {
            | Self::Init => {
                Self::execute_init_from_current_dir()?;
            }
            | Self::Perspective { command } => command.execute(services)?,
            | Self::Worker { command } => command.execute(services)?,
            | Self::Local { command } => command.execute(services)?,
            | Self::Status => {
                let result = services.orchestrator()?.status()?;
                println!("{result:#?}");
            }
        }
        Ok(())
    }
}

impl PerspectiveCommand {
    /// Execute one perspective inspection command.
    pub fn execute(self, services: &CliServices) -> runtime::Result<()> {
        match self {
            | Self::List => {
                let result = services.orchestrator()?.list_perspectives()?;
                println!("{result:#?}");
            }
            | Self::Validate { perspectives, no_live } => {
                let result =
                    services.orchestrator()?.validate_perspectives(perspectives, no_live)?;
                println!("{result:#?}");
            }
            | Self::Forward { perspective } => {
                let result = services.orchestrator()?.forward_perspective(perspective)?;
                println!("{result:#?}");
            }
        }
        Ok(())
    }
}

impl WorkerCommand {
    /// Execute one orchestrator-side worker command.
    pub fn execute(self, services: &CliServices) -> runtime::Result<()> {
        match self {
            | Self::Create { perspective, worker_id, overwriting_worktree, payload } => {
                let task =
                    (!payload.clone().into_runtime().is_empty()).then(|| payload.into_runtime());
                let mut request = CreateWorker::new(perspective);
                if let Some(worker_id) = worker_id {
                    request = request.with_worker_id(worker_id);
                }
                if overwriting_worktree {
                    request = request.with_overwriting_worktree();
                }
                if let Some(task) = task {
                    request = request.with_task(task);
                }
                let result = services.orchestrator()?.create_worker(request)?;
                println!("{result:#?}");
            }
            | Self::List => {
                let result = services.orchestrator()?.list_workers()?;
                println!("{result:#?}");
            }
            | Self::Show { worker_id } => {
                let result = services.orchestrator()?.get_worker(worker_id)?;
                println!("{result:#?}");
            }
            | Self::Outbox { worker_id, filter, body } => {
                let result = services
                    .orchestrator()?
                    .read_outbox(worker_id, filter.into_runtime(), body)?;
                println!("{result:#?}");
            }
            | Self::Inbox { worker_id, filter, body } => {
                let result = services
                    .orchestrator()?
                    .read_inbox(worker_id, filter.into_runtime(), body)?;
                println!("{result:#?}");
            }
            | Self::Ack { worker_id, sequence } => {
                let result =
                    services.orchestrator()?.ack_outbox(worker_id, runtime::Sequence(sequence))?;
                println!("{result:#?}");
            }
            | Self::Resolve { worker_id, payload, reply } => {
                let result = services.orchestrator()?.resolve_worker(
                    worker_id,
                    reply.into_runtime(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Hint { worker_id, payload, reply } => {
                let result = services.orchestrator()?.hint_worker(
                    worker_id,
                    reply.into_runtime(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Revise { worker_id, payload, reply } => {
                let result = services.orchestrator()?.revise_worker(
                    worker_id,
                    reply.into_runtime(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Discard { worker_id } => {
                let result = services.orchestrator()?.discard_worker(worker_id)?;
                println!("{result:#?}");
            }
            | Self::Delete { worker_id } => {
                let result = services.orchestrator()?.delete_worker(worker_id)?;
                println!("{result:#?}");
            }
            | Self::Merge { worker_id, skip_checks, payload } => {
                let result = services.orchestrator()?.merge_worker(
                    worker_id,
                    skip_checks,
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
        }
        Ok(())
    }
}

impl LocalCommand {
    /// Execute one worker-local command.
    pub fn execute(self, services: &CliServices) -> runtime::Result<()> {
        let worker = services.worker()?;
        match self {
            | Self::Contract => {
                let result = worker.contract()?;
                println!("{result:#?}");
            }
            | Self::Status => {
                let result = worker.status()?;
                println!("{result:#?}");
            }
            | Self::Inbox { filter, body } => {
                let result = worker.read_inbox(filter.into_runtime(), body)?;
                println!("{result:#?}");
            }
            | Self::Outbox { filter, body } => {
                let result = worker.read_outbox(filter.into_runtime(), body)?;
                println!("{result:#?}");
            }
            | Self::Ack { sequence } => {
                let result = worker.ack_inbox(runtime::Sequence(sequence))?;
                println!("{result:#?}");
            }
            | Self::Report { head_commit, reply, payload } => {
                let result = worker.send_report(
                    head_commit,
                    reply.into_runtime(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Commit { head_commit, payload } => {
                let result = worker.send_commit(head_commit, payload.into_runtime())?;
                println!("{result:#?}");
            }
        }
        Ok(())
    }
}
