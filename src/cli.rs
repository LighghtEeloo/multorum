//! Command-line interface for the Multorum orchestrator.
//!
//! Every state transition in Multorum is the result of an explicit
//! orchestrator instruction issued through this CLI. Multorum is
//! purely reactive — it never acts on its own initiative.
//!
//! The instruction set is grouped into four categories:
//!
//! - **Rulebook** — `rulebook switch`, `rulebook validate`
//! - **Worker lifecycle** — `provision`, `resolve`, `revise`, `discard`
//! - **Integration** — `integrate`
//! - **Query** — `status`
//!
//! Worker-originated instructions (`commit` and `report`) are represented
//! here as CLI commands so the same typed runtime service layer can back
//! both sides of the mailbox protocol. In the mailbox-based design,
//! these commands publish message bundles into the worker outbox, while
//! `resolve` and `revise` publish bundles into the worker inbox.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::{
    perspective::PerspectiveName,
    runtime::{
        self,
        service::{
            FilesystemOrchestratorService, FilesystemWorkerService, OrchestratorService,
            WorkerService,
        },
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
    orchestrator: FilesystemOrchestratorService,
}

impl CliServices {
    /// Build CLI services from the current directory.
    pub fn from_current_dir() -> runtime::Result<Self> {
        Ok(Self { orchestrator: FilesystemOrchestratorService::from_current_dir()? })
    }

    /// Construct the worker service for the current worktree.
    pub fn worker(&self) -> runtime::Result<FilesystemWorkerService> {
        FilesystemWorkerService::from_current_dir()
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

/// The orchestrator instruction set.
///
/// Each variant corresponds to one instruction described in
/// `DESIGN.md` §10 "The Orchestrator Instruction Set".
#[derive(Debug, Subcommand)]
pub enum Command {
    // ── Rulebook instructions ───────────────────────────────────
    //
    /// Manage the active rulebook.
    Rulebook {
        #[command(subcommand)]
        command: RulebookCommand,
    },

    // ── Worker lifecycle instructions ───────────────────────────
    //
    /// Create a sub-codebase with a perspective.
    ///
    /// Compiles the perspective's file sets, creates a git worktree at
    /// the pinned commit, installs the client-side write hook,
    /// materializes the worker-local runtime files, and injects
    /// read-set guidance metadata. The orchestrator may also seed the
    /// worker inbox with an initial `task` bundle.
    Provision {
        /// The perspective name to provision.
        perspective: PerspectiveName,

        /// Optional payload for the initial `task` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,
    },

    /// Unblock a worker after orchestrator resolution.
    ///
    /// Publishes a `resolve` bundle into the worker inbox. Once the
    /// bundle is acknowledged, the worker transitions from BLOCKED to
    /// ACTIVE.
    Resolve {
        /// The perspective name of the blocked worker.
        perspective: PerspectiveName,

        /// Optional payload for the `resolve` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,

        /// Optional reply metadata for the `resolve` bundle.
        #[command(flatten)]
        reply: ReplyReferenceArgs,
    },

    /// Return a committed worker to active state for rework.
    ///
    /// Publishes a `revise` bundle into the worker inbox. Once the
    /// bundle is acknowledged, the worker transitions from COMMITTED
    /// to ACTIVE and resumes with the preserved worktree.
    Revise {
        /// The perspective name of the committed worker.
        perspective: PerspectiveName,

        /// Optional payload for the `revise` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,

        /// Optional reply metadata for the `revise` bundle.
        #[command(flatten)]
        reply: ReplyReferenceArgs,
    },

    /// Tear down a worker's worktree without integrating.
    ///
    /// Valid from ACTIVE or COMMITTED states. The work is abandoned
    /// and the worktree is released. Transitions to DISCARDED.
    Discard {
        /// The perspective name to discard.
        perspective: PerspectiveName,
    },

    // ── Integration instructions ────────────────────────────────
    //
    /// Run the pre-merge pipeline and integrate a worker's commit.
    ///
    /// Gate 1 (file set check) always runs. Gate 2 (user-defined checks)
    /// runs according to check policies and any evidence-based skip
    /// instructions from the orchestrator.
    Integrate {
        /// The perspective name to integrate.
        perspective: PerspectiveName,

        /// Checks to skip based on trusted worker evidence.
        ///
        /// The orchestrator may instruct Multorum to skip specific
        /// `skippable` checks when the worker has submitted trusted
        /// evidence. The file set check cannot be skipped.
        #[arg(long = "skip-check", value_name = "CHECK")]
        skip_checks: Vec<String>,
    },

    // ── Worker-facing instructions ──────────────────────────────
    //
    /// Submit the worker's task as complete.
    ///
    /// Publishes a `commit` bundle into the worker outbox. Once
    /// accepted, Multorum freezes the worktree and transitions the
    /// worker from ACTIVE to COMMITTED. The orchestrator then decides
    /// whether to `integrate`, `revise`, or `discard` the submission.
    Commit {
        /// The perspective name of the committing worker.
        perspective: PerspectiveName,

        /// The git commit hash submitted by the worker.
        #[arg(long = "head-commit", value_name = "COMMIT")]
        head_commit: String,

        /// Optional payload for the `commit` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,
    },

    /// Signal that a worker is blocked.
    ///
    /// Publishes a `report` bundle into the worker outbox. Once
    /// accepted, Multorum transitions the worker to BLOCKED. The
    /// payload is opaque to Multorum — it is recorded without
    /// interpretation.
    Report {
        /// The perspective name of the reporting worker.
        perspective: PerspectiveName,

        /// Optional git commit hash relevant to the report.
        #[arg(long = "head-commit", value_name = "COMMIT")]
        head_commit: Option<String>,

        /// Optional payload for the `report` bundle.
        #[command(flatten)]
        payload: BundlePayloadArgs,
    },

    // ── Query instructions ──────────────────────────────────────
    //
    /// Query the current state of all workers.
    ///
    /// Returns the active rulebook commit hash, the state of each
    /// worker, and a summary of any blocked workers awaiting resolution.
    Status,
}

/// Rulebook management subcommands.
///
/// Accessed via `multorum rulebook <subcommand>`.
#[derive(Debug, Subcommand)]
pub enum RulebookCommand {
    /// Validate and activate a new rulebook version.
    ///
    /// Compiles the target rulebook and checks that no file held by an
    /// active worker's write set conflicts with the new rulebook. If
    /// valid, the new rulebook becomes active.
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
            | Self::Provision { perspective, payload } => {
                let task =
                    (!payload.clone().into_runtime().is_empty()).then(|| payload.into_runtime());
                let result = services.orchestrator.provision_worker(perspective, task)?;
                println!("{result:#?}");
            }
            | Self::Resolve { perspective, payload, reply } => {
                let result = services.orchestrator.resolve_worker(
                    perspective,
                    reply.into_runtime(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Revise { perspective, payload, reply } => {
                let result = services.orchestrator.revise_worker(
                    perspective,
                    reply.into_runtime(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Discard { perspective } => {
                let result = services.orchestrator.discard_worker(perspective)?;
                println!("{result:#?}");
            }
            | Self::Integrate { perspective, skip_checks } => {
                let result = services.orchestrator.integrate_worker(perspective, skip_checks)?;
                println!("{result:#?}");
            }
            | Self::Commit { perspective, head_commit, payload } => {
                let worker = services.worker()?;
                let contract = worker.contract()?;
                if contract.perspective != perspective {
                    return Err(runtime::RuntimeError::PerspectiveMismatch {
                        expected: perspective.to_string(),
                        found: contract.perspective.to_string(),
                    });
                }
                let result = worker.send_commit(head_commit, payload.into_runtime())?;
                println!("{result:#?}");
            }
            | Self::Report { perspective, head_commit, payload } => {
                let worker = services.worker()?;
                let contract = worker.contract()?;
                if contract.perspective != perspective {
                    return Err(runtime::RuntimeError::PerspectiveMismatch {
                        expected: perspective.to_string(),
                        found: contract.perspective.to_string(),
                    });
                }
                let result = worker.send_report(
                    head_commit,
                    runtime::ReplyReference::default(),
                    payload.into_runtime(),
                )?;
                println!("{result:#?}");
            }
            | Self::Status => {
                let result = services.orchestrator.status()?;
                println!("{result:#?}");
            }
        }
        Ok(())
    }
}
