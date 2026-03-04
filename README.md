# Multorum

Multorum is a tool for managing multiple simultaneous perspectives on a single codebase, designed for AI agent orchestration workflows. A coordinating agent — the *orchestrator* — decomposes a development goal into discrete tasks and assigns each to an independent agent — a *worker* — that operates in an isolated environment with precisely scoped file access.

Belua multorum es capitums!

---

## The Core Idea

Parallel development faces a fundamental tension: workers need *isolation* to make progress independently, but they need *integration context* to validate that their work is correct. Multorum threads this needle by separating authoring scope (what a worker may write) from execution scope (what a worker runs against). A worker may only write to its declared files, but it compiles, tests, and queries the LSP against the full codebase.

---

## How It Works

### Perspectives and the Rulebook

The project maintains a single `.multorum/rulebook.toml`, versioned in git. The rulebook declares *perspectives* — named roles, each with an explicit write set and read set of files. File permissions are expressed using a small algebra of *file sets*: explicit paths and globs as primitives, composed via union, intersection, and difference, and optionally given names for reuse across the rulebook.

```toml
[filesets]
AuthFiles  = "src/auth/**"
TestFiles  = "tests/**"
AuthTests  = "AuthFiles ∩ TestFiles"

[perspectives.AuthWorker]
write = "AuthFiles \\ AuthTests"
read  = "AuthTests"

[perspectives.TestWorker]
write = "AuthTests"
read  = "AuthFiles"
```

The rulebook is immutable once active — its versioning is handled entirely by git. The orchestrator evolves it by committing changes and explicitly instructing Multorum to switch to a new version.

### The Safety Property

Multorum enforces one core invariant at compile time: **a file may either be written exclusively by one perspective, or read by any number of perspectives — never both.** Write sets across all perspectives must be strictly disjoint. This means write conflicts between workers are impossible by construction, and integrating their work is always conflict-free.

### Sub-Codebase Provisioning

Each worker receives a git worktree — a full checkout of the codebase, pinned to the commit hash active when the session began. The worktree never updates during the worker's task, even if other workers finish and their changes are integrated. Stability is deliberate: each worker operates on a predictable, immutable world.

The write set is enforced server-side when Multorum integrates the worker's commit. The read set is not enforced at the filesystem level — it is guidance, communicating which files are relevant and guaranteeing they will not change. Workers may navigate the full codebase freely; what matters is controlling what they write.

Workers may not create new files. If a task requires a file not in the compiled write set, the worker reports back to the orchestrator rather than acting unilaterally.

### The Report-Back Protocol

Report-back is a first-class primitive, not an escape hatch. Workers report back whenever they cannot complete their task confidently: a missing file permission, an ambiguous specification, a vague function signature, nowhere appropriate to write tests — anything requiring orchestrator judgment. A worker that reports back rather than guessing is behaving correctly.

Multorum manages the lifecycle state of a report (blocking and resuming the worker); the content is an opaque payload between worker and orchestrator. Workers never communicate with each other. The communication topology is a strict star with the orchestrator at the center.

### Pre-Merge Pipeline

Before integration, every commit passes through a pipeline of gates. The first — a server-side file set check — is mandatory and non-negotiable. The remainder are project-defined checks configured in the rulebook: build, test, lint, format, or any arbitrary command.

Workers may submit evidence (e.g. test output from their worktree) to request that specific checks be skipped. The orchestrator reviews the evidence and decides whether to trust it. The file set check cannot be skipped under any circumstances.

---

## Multorum-Managed Project Layout

```
<project-root>/
  .multorum/
    rulebook.toml        # committed — versioned project configuration
    worktrees/           # gitignored — active worker worktrees
    state/               # gitignored — runtime state and audit logs
```

---

## Design Philosophy

Multorum is infrastructure, not an agent. It enforces invariants and executes instructions; all coordination intelligence belongs to the orchestrator. Every state change is the result of an explicit orchestrator instruction. Multorum never acts on its own initiative.
