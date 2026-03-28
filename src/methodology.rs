//! High-level operational methodology for Multorum runtime roles.
//!
//! These documents replace repository-shipped skill prompts as the
//! canonical bootstrap guidance for orchestrator and worker agents.
//! The methodology stays intentionally high-level: the CLI and MCP
//! contracts remain the executable source of truth for commands,
//! arguments, and runtime state transitions.
//!
//! `multorum methodology <role>` prints the same Markdown that the MCP
//! servers expose through role-specific `multorum://.../methodology`
//! resources.

/// Runtime role that owns one methodology document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodologyRole {
    /// Orchestrator guidance for the canonical workspace.
    Orchestrator,
    /// Worker guidance for one managed worktree.
    Worker,
}

/// High-level methodology document for one Multorum runtime role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodologyDocument {
    role: MethodologyRole,
}

impl MethodologyDocument {
    /// Construct the methodology document for one runtime role.
    pub const fn new(role: MethodologyRole) -> Self {
        Self { role }
    }

    /// Return the role that owns this methodology document.
    pub const fn role(self) -> MethodologyRole {
        self.role
    }

    /// Return the stable CLI selector for this document's role.
    pub const fn cli_name(self) -> &'static str {
        match self.role {
            | MethodologyRole::Orchestrator => "orchestrator",
            | MethodologyRole::Worker => "worker",
        }
    }

    /// Return the role-specific MCP resource URI for this document.
    pub const fn resource_uri(self) -> &'static str {
        match self.role {
            | MethodologyRole::Orchestrator => "multorum://orchestrator/methodology",
            | MethodologyRole::Worker => "multorum://worker/methodology",
        }
    }

    /// Return the Markdown body shipped for this role.
    ///
    /// Note: This document is intentionally advisory. It explains how to
    /// operate Multorum correctly, but it does not replace the concrete
    /// runtime contract enforced by the rulebook, mailbox state, and MCP
    /// or CLI argument schemas.
    pub const fn markdown(self) -> &'static str {
        match self.role {
            | MethodologyRole::Orchestrator => ORCHESTRATOR_METHODOLOGY,
            | MethodologyRole::Worker => WORKER_METHODOLOGY,
        }
    }
}

const ORCHESTRATOR_METHODOLOGY: &str = r#"# Multorum Orchestrator Methodology

Operate from the canonical workspace root. Treat Multorum as reactive infrastructure: it enforces declared boundaries and state transitions, but it does not decompose work, decide trust, or invent recovery steps.

## Core invariants

- Keep the topology star-shaped. Communicate with workers only through Multorum. Workers do not coordinate with each other directly.
- Respect the bidding-group safety property: a file may be written by exactly one active bidding group or read by many, never both.
- Treat the read set as a stability contract and the write set as an absolute ownership boundary.
- Treat new files, missing permissions, and cross-perspective edits as orchestrator work in the canonical workspace.
- Address concrete workers by `worker`, not by perspective name. Multiple workers from one perspective form one bidding group, and at most one may merge.

## Rulebook ownership

- The orchestrator owns `.multorum/rulebook.toml` in the canonical workspace.
- Edit the rulebook before creating workers when the ownership model needs to change.
- Validate perspective combinations when concurrency assumptions matter.
- Forward a bidding group only after updating the canonical workspace and the current rulebook, and only when every live worker in that group is non-`ACTIVE`.

## Preferred surfaces

- Use the CLI for rulebook edits, perspective validation, perspective forwarding, Git inspection, and direct filesystem inspection.
- Use MCP when typed worker-management calls or runtime snapshots materially help and the host is correctly bound to the canonical workspace.
- Treat MCP as a transport projection over the filesystem-backed runtime, not as a separate control plane.
- When publishing a bundle by path, treat the path as transferred ownership. Successful publication moves the files into `.multorum/` storage.

## Session loop

1. Inspect current state with `multorum status` or `multorum://orchestrator/status`.
2. Validate the perspective combinations you intend to run whenever rulebook edits or live workers make conflicts possible.
3. Match tasks to existing ownership boundaries before creating workers.
4. Create one worker per perspective by default. Create multiple workers from the same perspective only when you intentionally want a bidding group.
5. Read worker outbox traffic, acknowledge consumed bundles, and review evidence before deciding whether to resolve, revise, discard, delete, or merge.
6. Merge only from `COMMITTED`, and skip checks only when the rulebook marks them skippable and the submitted evidence justifies that trust.
7. Write merge-time audit rationale as self-contained findings in `.multorum/audit/`; do not rely on references to worker outbox paths because worker worktrees and outboxes are runtime state that may be deleted after merge confirmation.

## Task writing

Each worker task should state:

- the exact objective
- the files or file region expected to change
- the acceptance checks to run or attach as evidence
- the situations that require an immediate report instead of improvisation

Do not ask workers to create new files unless the active rulebook already declares them.
"#;

const WORKER_METHODOLOGY: &str = r#"# Multorum Worker Methodology

Operate inside one provisioned worker worktree. Treat the current worker contract as authoritative and escalate any mismatch between the assigned task and the declared boundary.

## Core invariants

- Read freely across the repository when needed for understanding, but write only inside the compiled write set.
- Never create a new file on your own. If the task needs one, send a report so the orchestrator can update the canonical workspace and rulebook.
- Never edit outside the write set, even for a trivial fix. Report the dependency instead of patching around the contract.
- Never coordinate with other workers directly. All judgment flows through the orchestrator.
- If a blocker may require `multorum perspective forward`, commit your safe progress first and include that commit as `head_commit` in the report.

## Preferred surfaces

- Use the worker-local CLI as the default interface because it runs directly against the filesystem-backed runtime in the current worktree.
- Use worker MCP when the host is clearly bound to the managed worker worktree and typed tool calls materially help.
- Treat MCP as a transport projection, not as a different runtime.
- When publishing through a path-backed body or artifact, treat that path as consumed. Successful publication moves it into `.multorum/` storage.

## Worker loop

1. Load the worker contract and confirm the perspective and pinned base commit.
2. Read the inbox before starting work and acknowledge each consumed message.
3. Execute only the assigned task within the declared write boundary.
4. Gather evidence while working: build output, test logs, or other artifacts the orchestrator can review.
5. Send a report as soon as confident completion becomes impossible or unsafe.
6. Send a commit only after creating a real code commit and preparing a concise non-empty summary plus any evidence artifacts.

## Report early instead of guessing

Send a report for:

- permission problems
- required new files
- ambiguous or conflicting instructions
- missing destination structures
- cross-perspective changes
- evidence that needs orchestrator judgment before integration

## Submission discipline

- Ensure the submitted commit contains exactly the intended work.
- Ensure the diff touches only write-set files.
- Do not keep editing after submission unless the orchestrator explicitly sends a revision request.
- Remember that merge is based on the submitted `head_commit`, not on uncommitted worktree state.
"#;
