# Multorum

Multorum is a tool for managing multiple simultaneous perspectives on a single codebase, designed for AI agent orchestration workflows. A coordinating agent — the *orchestrator* — decomposes a development goal into discrete tasks and assigns each to an independent agent — a *worker* — that operates in an isolated environment with precisely scoped file access.

Belua multorum es capitums!

---

## The Core Idea

Parallel development faces a fundamental tension: workers need *isolation* to make progress independently, but they need *integration context* to validate that their work is correct. Multorum threads this needle by separating authoring scope (what a worker may write) from execution scope (what a worker runs against). A worker may only write to its declared files, but it compiles, tests, and queries the LSP against the full codebase.

---

## How It Works

### Rulebook, File Sets, and Perspectives

The project maintains a single `.multorum/rulebook.toml`, versioned in git. The rulebook declares *perspectives* — named roles, each with an explicit write set and read set of files. File permissions are expressed using a small algebra of *file sets*: explicit paths and globs as primitives, composed via union, intersection, and difference, and optionally given names for reuse across the rulebook.

```toml
# Named file set definitions
[filesets]
SpecFiles.path = "**/*.spec.md"
TestFiles.path = "**/test/**"

AuthFiles.path = "auth/**"
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

# Used in a perspective
[perspectives.AuthImplementor]
read  = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"

[perspectives.AuthTester]
read  = "AuthSpecs | AuthTests"
write = "AuthTests"
```

The rulebook is immutable once active — its versioning is handled entirely by git. The orchestrator evolves it by committing changes and explicitly instructing Multorum to switch to a new version.

### Multorum-Managed Project Layout

The main workspace and every active worker worktree each have a `.multorum/` directory, but they serve different roles. The main workspace holds the orchestrator control plane. Each worker worktree holds the worker-local runtime surface.

```text
<project-root>/
  .multorum/
    rulebook.toml        # committed — versioned project configuration
    orchestrator/        # gitignored — orchestrator control plane and audit data
    worktrees/           # gitignored — active worker worktrees
```

Inside each worker worktree, Multorum materializes runtime files such as the compiled read and write sets, a runtime contract, and the worker's inbox and outbox mailboxes. These files are local runtime state, not project configuration, and are ignored through the worktree's local exclude configuration. Path-backed message payloads are moved into this `.multorum/` runtime area on successful publication, so Multorum becomes responsible for storing them.

### The Safety Property

Multorum enforces one core invariant at compile time: **a file may either be written exclusively by one perspective, or read by any number of perspectives — never both.** Write sets across all perspectives must be strictly disjoint. This means write conflicts between workers are impossible by construction, and integrating their work is always conflict-free.

### Sub-Codebase Provisioning

Each worker receives a git worktree — a full checkout of the codebase, pinned to the commit hash active when the session began. The worktree never updates during the worker's task, even if other workers finish and their changes are integrated. Stability is deliberate: each worker operates on a predictable, immutable world.

The write set is enforced server-side when Multorum integrates the worker's commit. The read set is not enforced at the filesystem level — it is guidance, communicating which files are relevant and guaranteeing they will not change. Workers may navigate the full codebase freely; what matters is controlling what they write.

Workers may not create new files. If a task requires a file not in the compiled write set, the worker reports back to the orchestrator rather than acting unilaterally.

### The Mailbox Protocol

All orchestrator-to-worker and worker-to-orchestrator communication is file-based. Each active worker worktree exposes two mailbox trees in its local `.multorum/` directory:

- `inbox/` for messages authored by the orchestrator and consumed by the worker
- `outbox/` for messages authored by the worker and consumed by the orchestrator

Messages are published as directory bundles with an `envelope.toml` plus opaque payload files such as `body.md` and attached artifacts. Publication is atomic, and acknowledgement is recorded separately so each mailbox directory has exactly one writer. When a body file or artifact is supplied by path, Multorum consumes that path and moves the file into `.multorum/` bundle storage instead of copying it.

Report-back is one message kind within this protocol. Workers publish a `report` bundle whenever they cannot complete their task confidently: a missing file permission, an ambiguous specification, a vague function signature, nowhere appropriate to write tests — anything requiring orchestrator judgment. The orchestrator answers with `resolve` or `revise` bundles in the worker inbox. Initial task delivery, blocker resolution, revision requests, and final commit submission all use the same transport.

Workers never communicate with each other. The communication topology is a strict star with the orchestrator at the center.

### Pre-Merge Pipeline

Before integration, every commit passes through a pipeline of gates. The first — a server-side file set check — is mandatory and non-negotiable. The remainder are project-defined checks configured in the rulebook: build, test, lint, format, or any arbitrary command.

Workers may submit evidence (e.g. test output from their worktree) to request that specific checks be skipped. The orchestrator reviews the evidence and decides whether to trust it. The file set check cannot be skipped under any circumstances.

---

## Design Philosophy

Multorum is infrastructure, not an agent. It enforces invariants and executes instructions; all coordination intelligence belongs to the orchestrator. Every state change is the result of an explicit orchestrator instruction. Multorum never acts on its own initiative.
