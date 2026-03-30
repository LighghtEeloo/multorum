//! High-level operational methodology for Multorum runtime roles.
//!
//! These documents replace repository-shipped skill prompts as the
//! canonical bootstrap guidance for orchestrator and worker agents.
//! The methodology stays intentionally high-level: the CLI and MCP
//! contracts remain the executable source of truth for commands,
//! arguments, and runtime state transitions.
//!
//! `multorum util methodology <role>` prints the same Markdown that the MCP
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
            | MethodologyRole::Orchestrator => METHODOLOGY_ORCHESTRATOR,
            | MethodologyRole::Worker => METHODOLOGY_WORKER,
        }
    }
}

const METHODOLOGY_ORCHESTRATOR: &str = include_str!("methodology_orchestrator.md");
const METHODOLOGY_WORKER: &str = include_str!("methodology_worker.md");
