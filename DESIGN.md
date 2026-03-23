# Project Multorum: Architecture Reference

## Table of Contents

1. [Introduction](#introduction)
2. [Core Model](#core-model)
3. [Rulebook](#rulebook)
4. [Workspace Model](#workspace-model)
5. [Worker Lifecycle](#worker-lifecycle)
6. [Mailbox Protocol](#mailbox-protocol)
7. [Merge Pipeline](#merge-pipeline)
8. [Instruction Reference](#instruction-reference)

---

## Introduction

Multorum manages multiple simultaneous perspectives on one codebase. It is designed for orchestrated development workflows in which a coordinating agent, called the orchestrator, decomposes a goal into tasks and assigns them to isolated workers. Each worker runs in its own workspace, sees the whole repository for execution and analysis, but may modify only the files declared by policy.

The system exists to solve one concrete tension in parallel development:

- workers need isolation so they do not interfere with each other
- workers need full repository context so their code, tests, and tooling still make sense

Multorum solves this by separating authoring scope from execution scope. A worker may only write within its declared write set, but it compiles, tests, and navigates against the full codebase.

Multorum is infrastructure, not an agent. It enforces invariants, materializes worker environments, and records state transitions. All coordination intelligence stays in the orchestrator, and every state transition happens only because the orchestrator or a worker issues an explicit instruction.

There is one canonical codebase under version control. Workers never modify it directly. All changes flow through Multorum's merge pipeline before the orchestrator integrates them.

---

## Core Model

### The Orchestrator

The orchestrator is the sole coordination authority. It may be a human, an LLM, or a hybrid. Its responsibilities are:

- decomposing development goals into tasks
- declaring the rulebook that defines ownership boundaries
- creating, revising, merging, discarding, and deleting workers
- receiving worker reports and resolving blockers
- evolving the rulebook over time

The communication topology is a strict star:

```
       Orchestrator
      /      |      \
     /       |       \
Worker A  Worker B  Worker C
```

Workers never communicate directly with each other.

### Rulebook, Perspectives, and Workers

The rulebook is the project policy artifact. It declares file-set definitions, perspectives, and merge-time validation checks.

A perspective is a named role in the rulebook. It declares:

- a write set: the files a worker from this role may modify
- a read set: the files that must remain stable while this role is active

The write set is a closed list of existing files. Workers may not create files outside it. The read set is not a visibility filter — workers may read any file in the repository. The read set exists to tell Multorum which files must remain untouched by other concurrent work, and to tell the worker what the orchestrator considers stable context.

A worker is a runtime instantiation of a perspective. Perspectives are static policy. Workers are ephemeral executions with state.

### Bidding Groups

If the orchestrator creates multiple workers from the same perspective against the same pinned snapshot, those workers form a bidding group. All workers in a bidding group share the same perspective, pinned base commit, compiled read set, and compiled write set.

Only one worker from a bidding group may be merged. Once one member is merged, the remaining members are discarded.

### Conflict-Free Invariant

The central correctness invariant is:

> **A file may either be written by exactly one active bidding group, or read by any number of active bidding groups, but never both.**

For any two distinct active bidding groups `G` and `H`:

- `write(G) ∩ write(H) = ∅`
- `write(G) ∩ read(H) = ∅`
- `read(G) ∩ write(H) = ∅`

Inside one bidding group, every worker has the same boundary. Conflict detection belongs at the bidding-group level, not at the level of perspective names: perspectives describe policy, bidding groups are the concurrent runtime entities that must not interfere.

---

## Rulebook

The rulebook lives at `.multorum/rulebook.toml`, committed to version control alongside the codebase it governs.

### File-Set Algebra

Multorum describes ownership boundaries through a small algebra of named file sets, giving the project a stable vocabulary for describing regions of the repository.

#### Syntax

```text
path  ::= <glob pattern>              e.g. "src/auth/**", "**/*.spec.md"
name  ::= <identifier>                e.g. AuthFiles, SpecFiles
expr  ::= name                        reference
        | expr "|" expr               union
        | expr "&" expr               intersection
        | expr "-" expr               difference
        | "(" expr ")"                grouping

definition ::= name ".path" "=" path  primitive - binds a name to a glob
             | name "=" expr          compound - binds a name to an expression
```

`A | B` produces every file in either set. `A & B` keeps only files present in both. `A - B` keeps files in `A` that are not in `B`. Precedence is flat; use parentheses when grouping matters.

File-set names and perspective names use CamelCase. Worker ids use kebab-case.

#### Named Definitions

Names are defined in the `[fileset]` table. A name may bind a primitive path via `.path` or a compound expression referencing other names. Perspectives reference these names in their `read` and `write` fields.

```toml
[fileset]
SpecFiles.path = "**/*.spec.md"
TestFiles.path = "**/test/**"

AuthFiles.path = "auth/**"
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

[perspective.AuthImplementor]
read  = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"

[perspective.AuthTester]
read  = "AuthSpecs | AuthTests"
write = "AuthTests"
```

This example uses intersection to carve out cross-cutting subsets and difference to partition ownership. `AuthImplementor` writes production code, `AuthTester` writes tests, and their write sets are disjoint, so they may run concurrently.

#### Compilation and Validation

File-set expressions are rulebook-level syntax only. When Multorum activates a rulebook, it compiles every expression into a concrete file list by expanding globs against the repository snapshot and evaluating the set operations.

Compile-time validation checks:

- no cycles in file-set definitions
- no undefined references
- empty sets are allowed but produce a warning

Compilation proves that the rulebook is structurally valid. It does not prove that a new worker can run concurrently with those already active — that check happens at worker creation time.

### Check Pipeline

The rulebook declares the project-specific merge pipeline:

```toml
[check]
pipeline = ["fmt", "clippy", "test"]

[check.command]
fmt = "cargo fmt --check"
clippy = "cargo clippy --workspace --all-targets -- -D warnings"
test = "cargo test --workspace"

[check.policy]
test = "skippable"
```

`[check.command]` maps check names to shell commands. `[check.policy]` overrides the default behavior for specific checks. Checks may declare one of two policies:

- `always` (the default): the check runs unconditionally
- `skippable`: the check may be skipped if the orchestrator accepts submitted evidence

The write-set scope check is always mandatory and cannot be configured away.

### Default Template

`rulebook init` creates a sparse template:

```toml
# Define shared ownership vocabulary first.
# `Name.path` binds a glob; `Name = "Expr"` combines names with |, &, and -.
[fileset]

# Add one table per perspective under `[perspective.<Name>]`.
# `write` names the files that perspective may modify.
# `read` names stable context files that concurrent work must not write.
[perspective]

# Add pre-merge gates in execution order.
# Add commands under `[check.command]` and optional skip policies under `[check.policy]`.
[check]
pipeline = []
```

### Activation and Immutability

Because the rulebook is version-controlled, every historical state is addressable by commit hash. When Multorum activates a rulebook, it pins that exact commit. Active workers are governed by an immutable snapshot — editing the file on disk does nothing until the orchestrator explicitly switches rulebooks.

### Rulebook Switching

The orchestrator evolves the rulebook through normal commits. Multorum never follows new commits automatically. To advance policy, the orchestrator issues `rulebook switch`, which validates the rulebook at `HEAD` against currently active workers.

Switching is file-based, not name-based. Multorum:

1. collects the materialized read and write sets of all active bidding groups
2. compiles the target rulebook at `HEAD`
3. treats each target perspective as a candidate future bidding group
4. checks each candidate against each active group using the conflict-free invariant

If every candidate is compatible, the switch succeeds. Perspectives may be renamed, split, merged, or replaced, as long as active file boundaries remain conflict-free. On failure, Multorum rejects the switch and reports the blocking groups.

---

## Workspace Model

### Filesystem Layout

A Multorum project adds a `.multorum/` directory at the repository root:

```text
<project-root>/
  .multorum/
    .gitignore          # committed - ignores runtime directories
    rulebook.toml       # committed - file sets, perspectives, check pipeline
    orchestrator/       # gitignored - orchestrator-local control plane
    worktrees/          # gitignored - managed worker worktrees
  src/
  tests/
  ...
```

The project commits only `.multorum/rulebook.toml` and `.multorum/.gitignore`. Everything else under `.multorum/` is runtime state that does not travel with the repository.

`.multorum/.gitignore` contains:

```text
orchestrator/
worktrees/
```

Multorum verifies these entries during `rulebook init` and warns if they are missing.

### Git Worktrees

Each worker workspace is a git worktree created from the pinned base commit:

```text
git worktree add .multorum/worktrees/<worker-id> <pinned-base-commit>
```

Every worker created under the same active rulebook starts from the same immutable snapshot, even if the orchestrator merges other work into `HEAD` later. This keeps workers comparable and prevents in-flight tasks from silently shifting underneath them.

If a worker id is reused after that worker reaches `MERGED` or `DISCARDED`, Multorum removes the finalized worktree first and creates a fresh workspace at the same path. Reuse means "create a new worker here", not "reopen old state".

### Worker Runtime Surface

Every worker worktree has its own `.multorum/` directory, separate from the orchestrator's. At creation time, Multorum materializes:

```text
.multorum/
  rulebook.toml      # checked out from the pinned commit
  contract.toml      # worker id, perspective, pinned base commit
  read-set.txt       # compiled read set
  write-set.txt      # compiled write set
  inbox/
    new/
    ack/
  outbox/
    new/
    ack/
  artifacts/
```

These files are runtime-only and must never be committed. Multorum installs local ignore rules in each worktree to keep them outside version control.

---

## Worker Lifecycle

### State Machine

```
                 BLOCKED
                    ▲ │
             report │ │ resolve
                    │ ▼
create ─────────► ACTIVE
                    │ ▲
             commit │ │ revise
                    ▼ │
                 COMMITTED
                     │
             ┌───────┴───────┐
             ▼               ▼
           MERGED        DISCARDED
```

- `ACTIVE`: the workspace exists and execution may proceed
- `BLOCKED`: the worker has reported a blocker and awaits resolution
- `COMMITTED`: the worker has submitted a commit; the workspace is frozen pending orchestrator action
- `MERGED`: the commit passed the merge pipeline and was integrated
- `DISCARDED`: the worker was finalized without merge

Once one worker in a bidding group reaches `MERGED`, every sibling in that group becomes `DISCARDED`.

`delete` is not a lifecycle transition. It removes the worktree of a finalized worker.

### Transitions

| From | To | Trigger |
|---|---|---|
| *(create)* | ACTIVE | worktree and runtime surface materialized |
| ACTIVE | BLOCKED | worker issues `report` |
| ACTIVE | COMMITTED | worker issues `commit` |
| ACTIVE | DISCARDED | orchestrator issues `discard` |
| BLOCKED | ACTIVE | orchestrator issues `resolve` |
| COMMITTED | ACTIVE | orchestrator issues `revise` |
| COMMITTED | MERGED | orchestrator issues `merge` and checks pass |
| COMMITTED | DISCARDED | orchestrator issues `discard` |

---

## Mailbox Protocol

All orchestrator-worker communication is file-based. There is no socket protocol, broker, or resident service.

Each worker exposes two mailbox trees in its `.multorum/` directory:

- `inbox/`: messages from the orchestrator to the worker
- `outbox/`: messages from the worker to the orchestrator

### Message Bundles

Every message is a directory bundle published atomically:

```text
.multorum/outbox/new/0007-report/
  envelope.toml
  body.md
  artifacts/
    test.log
```

`envelope.toml` carries the metadata Multorum interprets: `protocol`, `worker`, `perspective`, `kind`, `sequence`, `created_at`, and optionally `in_reply_to` and `head_commit`.

`body.md` and `artifacts/` are opaque payloads. Multorum validates the envelope but does not interpret the content.

Publication is atomic: bundles are written under a temporary name and renamed into `new/`, so readers see either the full message or nothing.

### Ownership and Acknowledgement

Each mailbox subtree has exactly one writer:

- orchestrator writes `inbox/new/`
- worker writes `inbox/ack/`
- worker writes `outbox/new/`
- orchestrator writes `outbox/ack/`

Published bundles are immutable. Receipt is recorded by writing an acknowledgement file with the same sequence number into the corresponding `ack/` directory.

The unique runtime identity is the worker id, not the perspective name. Perspective metadata travels in the envelope so the orchestrator can reason about role and bidding-group membership.

### Reports, Revisions, and Submission

Worker reports are first-class messages. A worker sends `report` for any issue that blocks confident completion: permission problems, task ambiguity, boundary mismatches, or evidence for orchestrator review. Multorum transitions the worker from `ACTIVE` to `BLOCKED`. The orchestrator answers with `resolve`.

The same transport handles post-review feedback: the worker submits `commit`, and the orchestrator responds with `revise` when more work is required.

`merge`, `discard`, and `delete` are orchestrator-local actions, not mailbox messages.

If a publication supplies payloads by path, Multorum consumes them rather than copying. On successful publish, the runtime moves the files into bundle storage and becomes responsible for retaining them.

---

## Merge Pipeline

Before a worker's commit reaches the canonical codebase, it must pass two gates.

### Scope Enforcement

Multorum verifies that every touched file is inside the worker's compiled write set. This check cannot be skipped, waived, or overridden. It is the authoritative enforcement point for write ownership.

Client-side hooks may serve as early warnings in worker worktrees, but they are not authoritative.

### Project Checks

After scope enforcement passes, Multorum runs the checks declared in `[check.pipeline]` in order. These may be builds, tests, linters, format checks, or any other command.

### Evidence

Workers may submit evidence with their reports or commits to support the case for merging or to ask the orchestrator to skip `skippable` checks. Evidence should include actual output or analysis, not just a claim — failed evidence is still valid when the worker wants the orchestrator to make a judgment call. Multorum carries evidence but does not judge it; the orchestrator decides whether to trust it or not.

---

## Instruction Reference

### Rulebook

**`rulebook init`** — Initialize `.multorum/`, write the default template if absent, prepare `.gitignore`, create orchestrator runtime directories.

**`rulebook switch`** — Validate and activate the rulebook at `HEAD`. Rejected if the target conflicts with any active bidding group.

**`rulebook validate`** — Same validation as `switch`, without activating.

### Worker Lifecycle

**`create <perspective>`** — Compile boundaries, check against active groups, create worktree, materialize runtime surface. Transition: `ACTIVE`.

**`resolve <worker-id>`** — Publish `resolve` to inbox. Transition: `BLOCKED` to `ACTIVE`.

**`revise <worker-id>`** — Publish `revise` to inbox. Transition: `COMMITTED` to `ACTIVE`.

**`merge <worker-id>`** — Run merge pipeline. Transition: `COMMITTED` to `MERGED` if checks pass.

**`discard <worker-id>`** — Finalize without merge. Workspace preserved until deleted.

**`delete <worker-id>`** — Remove worktree of a finalized worker.

### Worker-Originated

**`commit`** — Submitted via outbox. Transition: `ACTIVE` to `COMMITTED`.

**`report`** — Submitted via outbox. Transition: `ACTIVE` to `BLOCKED`.

### Query

**`status`** — Active workers, bidding-group membership, active rulebook commit, blocked workers.
