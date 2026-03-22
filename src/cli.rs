//! Command-line interface for the Multorum orchestrator.
//!
//! Every state transition in Multorum is the result of an explicit
//! orchestrator instruction issued through this CLI. Multorum is
//! purely reactive — it never acts on its own initiative.
//!
//! The command surface follows the runtime model directly:
//!
//! - `rulebook` manages committed configuration.
//! - `perspective` inspects declared roles from the active rulebook.
//! - `bidding-group` inspects active competing groups.
//! - `worker` addresses orchestrator-side operations on concrete workers.
//! - `local` addresses worker-local operations from inside a worker
//!   worktree.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::{
    perspective::PerspectiveName,
    runtime::{
        self, FsOrchestratorService, FsWorkerService, OrchestratorService, WorkerId, WorkerService,
    },
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
        let services = match CliServices::from_current_dir() {
            | Ok(services) => services,
            | Err(error) => {
                eprintln!("error: {error}");
                std::process::exit(1);
            }
        };
        if let Err(error) = cli.command.execute(&services) {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}

/// Runtime service container used by the CLI frontend.
///
#[derive(Debug)]
pub struct CliServices {
    orchestrator: FsOrchestratorService,
}

impl CliServices {
    /// Build CLI services from the current directory.
    pub fn from_current_dir() -> runtime::Result<Self> {
        Ok(Self { orchestrator: FsOrchestratorService::from_current_dir()? })
    }

    /// Construct the worker service for the current worktree.
    pub fn worker(&self) -> runtime::Result<FsWorkerService> {
        FsWorkerService::from_current_dir()
    }
}

/// Shared payload options for commands that publish mailbox bundles.
///
/// The stub CLI models bundle contents as filesystem paths so the command
/// surface matches the file-based protocol in `DESIGN.md`.
#[derive(Debug, Clone, Args)]
pub struct BundlePayloadArgs {
    /// Optional Markdown body file to move into `body.md`.
    ///
    /// On successful publication, Multorum consumes the path and stores
    /// the moved file under its managed `.multorum/` runtime state.
    #[arg(long, value_name = "FILE")]
    pub body: Option<PathBuf>,

    /// Files to move under the bundle's `artifacts/` directory.
    ///
    /// On successful publication, Multorum consumes each path and keeps
    /// the moved artifact under `.multorum/`.
    #[arg(long = "artifact", value_name = "FILE")]
    pub artifacts: Vec<PathBuf>,
}

impl BundlePayloadArgs {
    /// Convert CLI payload arguments into runtime bundle payload.
    pub fn into_runtime(self) -> runtime::BundlePayload {
        runtime::BundlePayload { body_text: None, body_path: self.body, artifacts: self.artifacts }
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
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the active rulebook.
    Rulebook {
        #[command(subcommand)]
        command: RulebookCommand,
    },

    /// Inspect compiled perspectives from the active rulebook.
    Perspective {
        #[command(subcommand)]
        command: PerspectiveCommand,
    },

    /// Inspect active bidding groups.
    BiddingGroup {
        #[command(subcommand)]
        command: BiddingGroupCommand,
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

/// Perspective inspection commands.
#[derive(Debug, Subcommand)]
pub enum PerspectiveCommand {
    /// List compiled perspectives from the active rulebook.
    List,
}

/// Active bidding-group inspection commands.
#[derive(Debug, Subcommand)]
pub enum BiddingGroupCommand {
    /// List active bidding groups.
    List,
}

/// Orchestrator-side worker commands.
#[derive(Debug, Subcommand)]
pub enum WorkerCommand {
    /// Provision a new worker from one perspective.
    Provision {
        /// Perspective to instantiate.
        perspective: PerspectiveName,

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

    /// Tear down a worker worktree without integration.
    Discard {
        /// Worker identity to discard.
        worker_id: WorkerId,
    },

    /// Run the pre-merge pipeline and integrate one worker.
    Integrate {
        /// Worker identity to integrate.
        worker_id: WorkerId,

        /// Checks to skip based on trusted worker evidence.
        #[arg(long = "skip-check", value_name = "CHECK")]
        skip_checks: Vec<String>,
    },
}

/// Worker-local commands.
#[derive(Debug, Subcommand)]
pub enum LocalCommand {
    /// Load the immutable worker contract for the current worktree.
    Contract,

    /// Return the projected worker status for the current worktree.
    Status,

    /// List inbox messages after an optional sequence.
    Inbox {
        /// Only return messages after this sequence number.
        #[arg(long = "after", value_name = "SEQUENCE")]
        after: Option<u64>,
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

/// Rulebook management subcommands.
///
/// Accessed via `multorum rulebook <subcommand>`.
#[derive(Debug, Subcommand)]
pub enum RulebookCommand {
    /// Initialize `.multorum/` with the default committed artifacts.
    ///
    /// Creates `.multorum/rulebook.toml` from the checked-in default
    /// template, ensures `.multorum/.gitignore` ignores the runtime
    /// directories, and prepares the local orchestrator runtime
    /// directories. The command refuses to overwrite an existing
    /// committed rulebook.
    Init,

    /// Validate and activate a new rulebook version.
    ///
    /// Compiles the target rulebook and checks that no active
    /// bidding-group boundary conflicts with any candidate boundary in
    /// the new rulebook. If valid, the new rulebook becomes active.
    Switch {
        /// The git commit hash pinning the rulebook to activate.
        commit: String,
    },

    /// Dry-run rulebook validation without making changes.
    ///
    /// Performs the same validation as `rulebook switch` but does not
    /// activate. Useful to test whether a switch is currently possible.
    Validate {
        /// The git commit hash pinning the rulebook to validate.
        commit: String,
    },
}

impl RulebookCommand {
    /// Execute the rulebook instruction.
    pub fn execute(self, services: &CliServices) -> runtime::Result<()> {
        match self {
            | Self::Init => {
                let result = services.orchestrator.rulebook_init()?;
                println!("{result:#?}");
            }
            | Self::Switch { commit } => {
                let result = services.orchestrator.rulebook_switch(commit)?;
                println!("{result:#?}");
            }
            | Self::Validate { commit } => {
                let result = services.orchestrator.rulebook_validate(commit)?;
                println!("{result:#?}");
            }
        }
        Ok(())
    }
}

impl Command {
    /// Execute the orchestrator instruction.
    pub fn execute(self, services: &CliServices) -> runtime::Result<()> {
        match self {
            | Self::Rulebook { command } => command.execute(services)?,
            | Self::Perspective { command } => command.execute(services)?,
            | Self::BiddingGroup { command } => command.execute(services)?,
            | Self::Worker { command } => command.execute(services)?,
            | Self::Local { command } => command.execute(services)?,
            | Self::Status => {
                let result = services.orchestrator.status()?;
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
                let result = services.orchestrator.list_perspectives()?;
                println!("{result:#?}");
            }
        }
        Ok(())
    }
}

impl BiddingGroupCommand {
    /// Execute one bidding-group inspection command.
    pub fn execute(self, services: &CliServices) -> runtime::Result<()> {
        match self {
            | Self::List => {
                let result = services.orchestrator.list_bidding_groups()?;
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
            | Self::Provision { perspective, payload } => {
                let task =
                    (!payload.clone().into_runtime().is_empty()).then(|| payload.into_runtime());
                let result = services.orchestrator.provision_worker(perspective, task)?;
                println!("{result:#?}");
            }
            | Self::List => {
                let result = services.orchestrator.list_workers()?;
                println!("{result:#?}");
            }
            | Self::Show { worker_id } => {
                let result = services.orchestrator.get_worker(worker_id)?;
                println!("{result:#?}");
            }
            | Self::Resolve { worker_id, payload, reply } => {
                let result = services.orchestrator.resolve_worker(
                    worker_id,
                    reply.into_runtime(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Revise { worker_id, payload, reply } => {
                let result = services.orchestrator.revise_worker(
                    worker_id,
                    reply.into_runtime(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Discard { worker_id } => {
                let result = services.orchestrator.discard_worker(worker_id)?;
                println!("{result:#?}");
            }
            | Self::Integrate { worker_id, skip_checks } => {
                let result = services.orchestrator.integrate_worker(worker_id, skip_checks)?;
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
            | Self::Inbox { after } => {
                let result = worker.read_inbox(after.map(runtime::Sequence))?;
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
