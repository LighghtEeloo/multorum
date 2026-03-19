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
//! here as CLI commands because the current implementation is a stub.
//! In the mailbox-based design, these commands publish message bundles
//! into the worker outbox, while `resolve` and `revise` publish bundles
//! into the worker inbox.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Multorum — multi-perspective codebase orchestration.
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
        cli.command.execute();
    }
}

/// Shared payload options for commands that publish mailbox bundles.
///
/// The stub CLI models bundle contents as filesystem paths so the command
/// surface matches the file-based protocol in `DESIGN.md`.
#[derive(Debug, Clone, Args)]
pub struct BundlePayload {
    /// Optional Markdown body file to copy into `body.md`.
    #[arg(long, value_name = "FILE")]
    pub body: Option<PathBuf>,

    /// Files to attach under the bundle's `artifacts/` directory.
    #[arg(long = "artifact", value_name = "FILE")]
    pub artifacts: Vec<PathBuf>,
}

/// Shared reply metadata for mailbox bundles that answer earlier messages.
#[derive(Debug, Clone, Args)]
pub struct ReplyReference {
    /// Sequence number of the message this bundle answers.
    #[arg(long = "reply-to", value_name = "SEQUENCE")]
    pub in_reply_to: Option<u64>,
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
        perspective: String,

        /// Optional payload for the initial `task` bundle.
        #[command(flatten)]
        payload: BundlePayload,
    },

    /// Unblock a worker after orchestrator resolution.
    ///
    /// Publishes a `resolve` bundle into the worker inbox. Once the
    /// bundle is acknowledged, the worker transitions from BLOCKED to
    /// ACTIVE.
    Resolve {
        /// The perspective name of the blocked worker.
        perspective: String,

        /// Optional payload for the `resolve` bundle.
        #[command(flatten)]
        payload: BundlePayload,

        /// Optional reply metadata for the `resolve` bundle.
        #[command(flatten)]
        reply: ReplyReference,
    },

    /// Return a committed worker to active state for rework.
    ///
    /// Publishes a `revise` bundle into the worker inbox. Once the
    /// bundle is acknowledged, the worker transitions from COMMITTED
    /// to ACTIVE and resumes with the preserved worktree.
    Revise {
        /// The perspective name of the committed worker.
        perspective: String,

        /// Optional payload for the `revise` bundle.
        #[command(flatten)]
        payload: BundlePayload,

        /// Optional reply metadata for the `revise` bundle.
        #[command(flatten)]
        reply: ReplyReference,
    },

    /// Tear down a worker's worktree without integrating.
    ///
    /// Valid from ACTIVE or COMMITTED states. The work is abandoned
    /// and the worktree is released. Transitions to DISCARDED.
    Discard {
        /// The perspective name to discard.
        perspective: String,
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
        perspective: String,

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
        perspective: String,

        /// The git commit hash submitted by the worker.
        #[arg(long = "head-commit", value_name = "COMMIT")]
        head_commit: String,

        /// Optional payload for the `commit` bundle.
        #[command(flatten)]
        payload: BundlePayload,
    },

    /// Signal that a worker is blocked.
    ///
    /// Publishes a `report` bundle into the worker outbox. Once
    /// accepted, Multorum transitions the worker to BLOCKED. The
    /// payload is opaque to Multorum — it is recorded without
    /// interpretation.
    Report {
        /// The perspective name of the reporting worker.
        perspective: String,

        /// Optional git commit hash relevant to the report.
        #[arg(long = "head-commit", value_name = "COMMIT")]
        head_commit: Option<String>,

        /// Optional payload for the `report` bundle.
        #[command(flatten)]
        payload: BundlePayload,
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
    pub fn execute(self) {
        match self {
            | Self::Switch { commit } => {
                todo!("rulebook switch: activate rulebook at {commit}")
            }
            | Self::Validate { commit } => {
                todo!("rulebook validate: dry-run validation at {commit}")
            }
        }
    }
}

impl Command {
    /// Execute the orchestrator instruction.
    pub fn execute(self) {
        match self {
            | Self::Rulebook { command } => command.execute(),
            | Self::Provision { perspective, payload } => {
                let _ = payload;
                todo!("provision: create sub-codebase and optional task bundle for {perspective}")
            }
            | Self::Resolve { perspective, payload, reply } => {
                let _ = (payload, reply);
                todo!("resolve: publish inbox bundle for {perspective}")
            }
            | Self::Revise { perspective, payload, reply } => {
                let _ = (payload, reply);
                todo!("revise: publish inbox bundle for {perspective}")
            }
            | Self::Discard { perspective } => {
                todo!("discard: tear down {perspective}")
            }
            | Self::Integrate { perspective, skip_checks } => {
                let _ = skip_checks;
                todo!("integrate: run pre-merge pipeline for {perspective}")
            }
            | Self::Commit { perspective, head_commit, payload } => {
                let _ = (head_commit, payload);
                todo!("commit: publish outbox submission bundle for {perspective}")
            }
            | Self::Report { perspective, head_commit, payload } => {
                let _ = (head_commit, payload);
                todo!("report: publish outbox blocker bundle for {perspective}")
            }
            | Self::Status => {
                todo!("status: query all worker states")
            }
        }
    }
}
