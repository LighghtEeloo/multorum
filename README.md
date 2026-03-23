# Multorum

Multorum is a tool for managing multiple simultaneous, conflict-free perspectives on a single codebase, designed for AI agent orchestration workflows. A coordinating agent, the *orchestrator*, decomposes a development goal into discrete tasks and assigns each to an independent group of *workers* that operate in isolated environments with precisely scoped file access.

> For a detailed design reference, see [DESIGN.md](DESIGN.md).

## Why It Exists

Parallel development has a persistent tension:

- workers need isolation so they can make progress independently
- workers need the full repository context so their code still builds, type-checks, and tests correctly

Multorum resolves that tension by separating *authoring scope* from *execution scope*:

- a worker may write only to its declared write set
- a worker still runs against a full checkout of the repository

As a result, Multorum lets an orchestrator run multiple workers against one repository in parallel, with conflict freedom enforced up front instead of repaired after the fact.

## Core Model

The rulebook at `.multorum/rulebook.toml` defines named *perspectives*. A perspective is a role with:

- a read set: stable context that concurrent work must not modify
- a write set: the exact files that role may change

When the orchestrator creates a worker from a perspective, Multorum creates a git worktree pinned to the active rulebook's base commit and materializes the compiled read and write sets into the worker-local runtime surface.

If the orchestrator wants multiple attempts at the same role, it can create multiple workers from the same perspective. Those workers form a *bidding group*: they share the same base snapshot and scope, and at most one of them may ultimately merge.

The active rulebook is immutable by commit hash. Changing `rulebook.toml` on disk does nothing until the orchestrator explicitly installs a new committed version.

## Conflict-Free Model

Multorum's core invariant is:

> A file may be written by exactly one active bidding group, or read by any number of active bidding groups, but never both.

This means:

- concurrent write sets are disjoint
- no active group's write set may overlap another active group's read set
- merge conflicts between active groups are prevented by construction rather than resolved later

Workers are allowed to read the full codebase. The read set is guidance plus a stability guarantee, not a filesystem restriction.

Workers may not create new files. The write set is a closed list of existing paths compiled at worker creation time. If new files are needed, the orchestrator must change the rulebook and create a fresh worker.

## Runtime Shape

The main workspace owns the orchestrator control plane under `.multorum/orchestrator/`. Each worker workspace has its own `.multorum/` runtime surface containing:

- `contract.toml`
- `read-set.txt`
- `write-set.txt`
- `inbox/` and `outbox/` mailboxes
- runtime-managed artifacts

All orchestrator-worker communication is file-based. Messages are directory bundles with an `envelope.toml`, an optional `body.md`, and optional artifacts. Publication is atomic, acknowledgements are separate, and workers never communicate directly with each other.

Workers move through a small lifecycle:

- `ACTIVE`: created and running
- `BLOCKED`: waiting for orchestrator input after a `report`
- `COMMITTED`: submission frozen pending orchestrator action
- `MERGED` or `DISCARDED`: finalized outcomes; the workspace is preserved until an explicit delete

Finalization and workspace deletion are separate actions. `merge` and `discard` change lifecycle state, while `delete` removes a finalized worker workspace. If the orchestrator reuses an explicit worker id after `MERGED` or `DISCARDED`, Multorum replaces the old finalized workspace with a fresh one for the new worker.

## Merge

Before any worker submission merges, Multorum runs a pre-merge pipeline:

1. a mandatory server-side write-set check
2. project-defined checks from the rulebook, such as build, lint, or test

Workers may attach evidence and ask the orchestrator to skip specific project-defined checks, but the write-set check is never skippable.

## Worker Commands

The orchestrator-side worker lifecycle commands are:

- `create`: create a worker workspace from a perspective
- `merge`: run the pre-merge pipeline and merge a committed worker
- `discard`: finalize a worker without merging while preserving its workspace
- `delete`: remove a finalized worker workspace

## Rulebook Example

```toml
[fileset]
SpecFiles.path = "**/*.spec.md"
TestFiles.path = "**/test/**"

AuthFiles.path = "auth/**"
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

[perspective.AuthImplementor]
read = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"

[perspective.AuthTester]
read = "AuthSpecs | AuthTests"
write = "AuthTests"

[check]
pipeline = ["fmt", "clippy", "test"]

[check.command]
fmt = "cargo fmt --all"
clippy = "cargo clippy --all"
test = "cargo test --all"

[check.policy]
test = "skippable"
```

This gives one role ownership of production auth code, another ownership of auth tests, and a shared stable context in the auth specs. Because the write sets are disjoint, both roles may run concurrently.

## Shell Completions

Generate tab completions for your shell with `util completion`:

```bash
# bash
source <(multorum util completion bash)

# zsh
autoload -U compinit
compinit
source <(multorum util completion zsh)

# fish
multorum util completion fish | source
```
