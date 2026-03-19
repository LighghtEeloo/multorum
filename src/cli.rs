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
//! Workers issue a single instruction back: `report`.

use clap::{Parser, Subcommand};

/// Multorum — multi-perspective codebase orchestration.
///
/// Infrastructure for managing multiple simultaneous perspectives on a
/// single codebase. See `DESIGN.md` for the full architecture reference.
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
    /// Create a sub-codebase for a perspective.
    ///
    /// Compiles the perspective's file sets, creates a git worktree at
    /// the pinned commit, installs the client-side write hook, and
    /// injects read-set guidance metadata. Transitions the worker to
    /// PROVISIONED, then immediately to ACTIVE.
    Provision {
        /// The perspective name to provision.
        perspective: String,
    },

    /// Unblock a worker after orchestrator resolution.
    ///
    /// Transitions the worker from BLOCKED to ACTIVE. The orchestrator
    /// must separately communicate resolution content to the worker.
    Resolve {
        /// The perspective name of the blocked worker.
        perspective: String,
    },

    /// Return a committed worker to active state for rework.
    ///
    /// Unfreezes the worktree so the worker can address problems
    /// identified by the orchestrator. Transitions from COMMITTED to
    /// ACTIVE.
    Revise {
        /// The perspective name of the committed worker.
        perspective: String,
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
    /// Signal that a worker is blocked.
    ///
    /// Transitions the worker to BLOCKED and notifies the orchestrator.
    /// The payload is opaque to Multorum — it is recorded and forwarded
    /// without interpretation.
    Report {
        /// The perspective name of the reporting worker.
        perspective: String,

        /// An optional message payload describing the blocker.
        #[arg(long)]
        message: Option<String>,
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
            | Self::Provision { perspective } => {
                todo!("provision: create sub-codebase for {perspective}")
            }
            | Self::Resolve { perspective } => {
                todo!("resolve: unblock worker {perspective}")
            }
            | Self::Revise { perspective } => {
                todo!("revise: return {perspective} to active")
            }
            | Self::Discard { perspective } => {
                todo!("discard: tear down {perspective}")
            }
            | Self::Integrate { perspective, skip_checks } => {
                let _ = skip_checks;
                todo!("integrate: run pre-merge pipeline for {perspective}")
            }
            | Self::Report { perspective, message } => {
                let _ = message;
                todo!("report: worker {perspective} is blocked")
            }
            | Self::Status => {
                todo!("status: query all worker states")
            }
        }
    }
}
