# Project Multorum: Architecture Reference

## Table of Contents

1. [Introduction](#introduction)
2. [Core Model](#core-model)
3. [Rulebook Language](#rulebook-language)
4. [Runtime Model](#runtime-model)
5. [Worker Lifecycle](#worker-lifecycle)
6. [Merge Model](#merge-model)
7. [Filesystem Layout](#filesystem-layout)
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

The orchestrator is the sole coordination authority in a Multorum workflow. It may be a human, an LLM, or a hybrid. Its responsibilities are:

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

The rulebook is the project policy artifact. It declares:

- file-set definitions
- perspectives
- merge-time validation checks

A perspective is a named role in the rulebook. It declares:

- a write set, which defines what files a worker from that role may modify
- a read set, which defines the files that must remain stable while that role is active

A worker is a runtime instantiation of a perspective. Perspectives are static policy. Workers are ephemeral executions with state.

### Bidding Groups

If the orchestrator creates multiple workers from the same perspective against the same pinned snapshot, those workers form a bidding group. A bidding group is the runtime unit of competition and merge selection.

All workers in a bidding group share:

- the same perspective
- the same pinned base commit
- the same compiled read set
- the same compiled write set

Only one worker from a bidding group may be merged into the canonical codebase. Once one member is merged, the remaining members of that group are discarded.

### Conflict-Free Invariant

The central correctness invariant is:

> **A file may either be written by exactly one active bidding group, or read by any number of active bidding groups, but never both.**

For any two distinct active bidding groups `G` and `H`:

- `write(G) ∩ write(H) = ∅`
- `write(G) ∩ read(H) = ∅`
- `read(G) ∩ write(H) = ∅`

Inside one bidding group `B`, every worker has the same boundary:

- `write(x) = write(y)` for all workers `x` and `y` in `B`
- `read(x) = read(y)` for all workers `x` and `y` in `B`

This is why conflict detection belongs at the active worker boundary, not at the level of perspective names. Perspectives describe policy. Bidding groups are the concurrent runtime entities that must not interfere.

---

## Rulebook Language

The rulebook lives at `.multorum/rulebook.toml`. It is committed to version control alongside the codebase it governs.

### File Set Algebra

Multorum describes ownership boundaries through a small algebra of named file sets. The point is to avoid scattering raw glob patterns across perspective declarations and to give the project a stable vocabulary for describing regions of the repository.

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

`A | B` produces every file in either set. `A & B` keeps only files present in both. `A - B` keeps files in `A` that are not in `B`. Precedence is intentionally flat, so parentheses should be used whenever grouping matters.

#### Naming Conventions

File set names and perspective names should use CamelCase. Worker ids should use kebab-case.

#### Named Definitions

Names are defined in the `[fileset]` table. A name may bind either:

- a primitive path via `.path`
- a compound expression that references other names

Perspectives then reference those names in their `read` and `write` fields.

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

This example uses intersection to carve out cross-cutting subsets and uses difference to partition ownership. `AuthImplementor` may write production code. `AuthTester` may write only tests. Their write sets are disjoint, so they may run concurrently.

#### Compilation and Validation

File-set expressions are rulebook-level syntax only. They do not exist at runtime. When Multorum activates a rulebook, it compiles every expression into a concrete file list by expanding globs against the repository snapshot and then evaluating the set operations.

Compile-time validation checks:

- no cycles in file-set definitions
- no undefined references
- empty sets are allowed, but produce a warning

Compilation proves only that the rulebook is structurally valid and reducible to concrete file lists. It does not prove that a new worker may run concurrently with the workers already active. That check happens when runtime state changes.

### Perspective Declarations

A perspective is declared under `[perspective.<Name>]` and contains two fields:

- `write`: the file-set expression that compiles to the exact files a worker from this perspective may modify
- `read`: the file-set expression that compiles to the files that must remain stable while this perspective is active

```toml
[perspective.AuthImplementor]
read  = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"
```

The read set is part of the concurrency contract. The write set is part of the modification contract. Both compile against the active rulebook snapshot.

### Check Pipeline Declarations

The rulebook also declares the project-specific merge pipeline:

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

`[check.command]` maps check names to commands. `[check.policy]` is optional and only needs to mention checks whose default behavior should be overridden.

### Complete Example Rulebook

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

[check]
pipeline = ["fmt", "clippy", "test"]

[check.command]
fmt = "cargo fmt --check"
clippy = "cargo clippy --workspace --all-targets -- -D warnings"
test = "cargo test --workspace"
```

This is enough to define shared ownership vocabulary, two concurrent roles, and a project merge pipeline.

### Default Template

`rulebook init` creates an intentionally sparse template:

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

The template provides the minimum valid structure without inventing project-specific boundaries or checks.

---

## Runtime Model

### Rulebook Activation and Immutability

Because the rulebook is a normal version-controlled file, every historical state of it is addressable by commit hash. When Multorum activates a rulebook, it pins that exact commit. Active workers are therefore governed by an immutable rulebook snapshot. Editing the file on disk does nothing until the orchestrator explicitly switches rulebooks.

This deliberately delegates immutability to version control rather than inventing a second mechanism.

### Rulebook Switching

The orchestrator evolves the rulebook through normal commits. Multorum never follows new commits automatically. To advance policy, the orchestrator issues `rulebook switch`, which validates the rulebook at `HEAD` against the currently active workers.

Rulebook switching is file-based, not name-based. Multorum:

1. collects the materialized read and write sets of all active bidding groups
2. compiles the target rulebook at `HEAD`
3. treats each target perspective as a candidate future bidding group
4. checks each candidate against each active group using the conflict-free invariant

If every candidate is compatible with every active group, the switch succeeds. Perspectives may be renamed, split, merged, or replaced, as long as the active file boundaries remain conflict-free. If the check fails, Multorum rejects the switch and reports the blocking active groups.

### Worker Creation

When the orchestrator issues `create <perspective-name>`, Multorum:

1. compiles that perspective's read and write sets from the active rulebook snapshot
2. checks those sets against active bidding groups
3. creates a worker workspace pinned to the active base commit
4. materializes runtime metadata and mailbox directories for that worker

Repeated creation from the same perspective produces more workers in the same bidding group.

### Layered Workspace Model

A worker workspace must satisfy two requirements that normally pull in opposite directions:

- authoring must stay inside a strict boundary
- execution tooling must see the whole repository

Multorum resolves this by enforcing write boundaries rather than hiding the rest of the codebase. The worker workspace is a full repository checkout. The boundary is enforced later, at submission and merge time.

### Git Worktrees

Each worker workspace is a git worktree created from the pinned base commit:

```text
git worktree add .multorum/worktrees/<worker-id> <pinned-base-commit>
```

Every worker created under the same active rulebook starts from the same immutable repository snapshot, even if the orchestrator merges other work into `HEAD` later. This keeps workers comparable and prevents in-flight tasks from silently changing underneath them.

If the orchestrator reuses a worker id after that worker reaches `MERGED` or `DISCARDED`, Multorum removes the finalized worktree registration first and then creates a fresh workspace at the same managed path. Worker-id reuse means "create a new worker here", not "reopen old runtime state".

### Worker-Local Runtime Surface

Every worker worktree has its own `.multorum/` directory, separate from the orchestrator's `.multorum/` directory in the main workspace.

At creation time, Multorum materializes:

```text
.multorum/
  rulebook.toml      # checked out from the pinned commit
  contract.toml      # runtime - worker id, perspective, pinned base commit
  read-set.txt       # runtime - compiled read set for guidance
  write-set.txt      # runtime - compiled write set for audit and enforcement
  inbox/
    new/
    ack/
  outbox/
    new/
    ack/
  artifacts/
```

`contract.toml`, the mailbox directories, and `artifacts/` are runtime-only files. They must never be committed. Multorum installs local ignore rules in the worktree so these paths stay outside normal version-control operations.

If a mailbox publication supplies payloads by path, Multorum consumes them rather than copying them. On successful publish, the runtime moves the files into `.multorum/` bundle storage and becomes responsible for retaining them.

### Read and Write Semantics

The compiled write set is absolute. A worker may submit changes only to files in that set.

The compiled read set is not a read permission filter. Workers may read any file in the repository. The read set exists for two other reasons:

- it tells the worker what the orchestrator considers the stable context of the task
- it tells Multorum what files must remain untouched by other active bidding groups

This is intentionally permissive for reading. Restricting repository reads would make language tooling and code understanding brittle without improving ownership guarantees.

### New Files

Workers may not create files that did not exist when their write set was compiled. The compiled write set is a closed list of existing files. If a task requires a new file, the worker must report the issue to the orchestrator. The orchestrator may then update the rulebook, switch to the new rulebook, and create a fresh worker under the new policy.

This keeps file ownership explicit and keeps the compiled file lists authoritative.

---

## Worker Lifecycle

### State Machine

Each worker moves through a fixed lifecycle:

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

States:

- `ACTIVE`: the worker workspace exists and execution may proceed
- `BLOCKED`: the worker has reported a blocker and is waiting for resolution
- `COMMITTED`: the worker has submitted a commit and the workspace is frozen pending orchestrator action
- `MERGED`: the worker's commit passed the merge pipeline and was integrated
- `DISCARDED`: the worker was finalized without merge

Once one worker in a bidding group reaches `MERGED`, every sibling worker in that group becomes `DISCARDED`.

`delete` is not a lifecycle transition. It removes the git worktree of a worker that is already finalized.

### Valid Transitions

| From | To | Trigger |
|---|---|---|
| create | ACTIVE | Multorum creates the worktree and runtime surface |
| ACTIVE | BLOCKED | worker issues `report` |
| ACTIVE | COMMITTED | worker issues `commit` |
| BLOCKED | ACTIVE | orchestrator issues `resolve` |
| COMMITTED | ACTIVE | orchestrator issues `revise` |
| COMMITTED | MERGED | orchestrator issues `merge` and checks pass |
| COMMITTED | DISCARDED | orchestrator issues `discard` |
| ACTIVE | DISCARDED | orchestrator issues `discard` |

### Mailbox Protocol

All orchestrator-to-worker and worker-to-orchestrator communication is file-based. There is no socket protocol, broker, or required resident service. Each active worker exposes two mailbox trees in its own `.multorum/` directory:

- `inbox/`: messages authored by the orchestrator and consumed by the worker
- `outbox/`: messages authored by the worker and consumed by the orchestrator

This keeps the communication model aligned with the star topology and with the fact that each worker has an isolated runtime surface.

### Message Bundles

Every message is a directory bundle published atomically into a mailbox:

```text
.multorum/outbox/new/0007-report/
  envelope.toml
  body.md
  artifacts/
    test.log
```

`envelope.toml` contains the metadata Multorum interprets:

- `protocol`
- `worker`
- `perspective`
- `kind`
- `sequence`
- `created_at`
- `in_reply_to` (optional)
- `head_commit` (optional)

`body.md` and `artifacts/` are opaque payloads. Multorum validates the envelope and stores the bundle, but it does not interpret the content body.

Publication is atomic: the bundle is written under a temporary name and then renamed into `new/`, so readers either see the full message or nothing.

### Mailbox Ownership and Acknowledgement

Each mailbox subtree has exactly one writer:

- orchestrator writes `inbox/new/`
- worker writes `inbox/ack/`
- worker writes `outbox/new/`
- orchestrator writes `outbox/ack/`

Published bundles are immutable. Receipt is recorded by writing an acknowledgement file with the same sequence number into the corresponding `ack/` directory. This avoids rename races, preserves audit history, and keeps the concurrency model simple.

The unique runtime identity is the worker id, not the perspective name. Perspective metadata still travels in the envelope so the orchestrator can reason about role ownership and bidding-group membership.

### Reports, Revisions, and Submission

Worker reports are first-class messages, not a side channel. A worker may send `report` for any issue that blocks confident completion, including:

- permission problems, such as needing a new file
- task ambiguity
- structural mismatches between the requested change and the declared boundaries
- evidence the worker wants the orchestrator to review before merge

When Multorum accepts a `report`, the worker moves from `ACTIVE` to `BLOCKED`. The orchestrator answers with `resolve`, and the worker returns to `ACTIVE` once that message is acknowledged.

The same mailbox transport handles post-review feedback and final submission:

- the worker submits `commit`
- the orchestrator responds with `revise` when more work is required

`merge`, `discard`, and `delete` are orchestrator-local actions. They are not content-carrying mailbox messages.

---

## Merge Model

Before a worker's commit reaches the canonical codebase, it must pass Multorum's merge pipeline.

### Mandatory Scope Enforcement

Multorum always verifies that every touched file is inside that worker's compiled write set. This check cannot be skipped, waived, or overridden. It is the authoritative enforcement point for write ownership and therefore for the conflict-free invariant.

Client-side hooks may be installed in worker worktrees as early warnings, but they are not authoritative and do not replace server-side enforcement.

### Project Validation Checks

After scope enforcement passes, Multorum runs the project-defined checks from the rulebook in declared order. These may be builds, tests, linters, format checks, or any other command.

```toml
[check]
pipeline = ["fmt", "clippy", "test"]

[check.command]
fmt = "cargo fmt --all"
clippy = "cargo clippy --all"
test = "cargo test --all"
```

### Evidence and Trust

Workers may submit evidence with their reports or commits to ask the orchestrator to skip specific project-defined checks for a particular merge. The model is:

1. the worker attaches evidence for a named check
2. the orchestrator decides whether to trust it
3. if trusted, Multorum may skip that check for this merge
4. if not trusted, Multorum runs the check normally

Evidence should include actual output, not just a claim. Failed evidence is still valid to submit if the worker wants the orchestrator to make a judgment call rather than letting the pipeline decide automatically.

### Check Policies

Checks may declare one of two policies under `[check.policy]`:

- `always`: the check always runs
- `skippable`: the check may be skipped if the orchestrator accepts submitted evidence

Any check without an explicit policy entry defaults to `always`. The write-set scope check is always mandatory and cannot be configured away.

---

## Filesystem Layout

A Multorum project adds a `.multorum/` directory at the repository root. The main workspace and each worker worktree both contain `.multorum/`, but they serve different purposes.

```text
<project-root>/
  .multorum/
    .gitignore          # committed - ignores Multorum runtime directories
    rulebook.toml       # committed - file sets, perspectives, check pipeline
    orchestrator/       # gitignored - orchestrator-local control plane
    worktrees/          # gitignored - managed worker worktrees
  src/
  tests/
  ...
```

### Committed Region

The project commits only:

- `.multorum/rulebook.toml`
- `.multorum/.gitignore`

`rulebook.toml` is the canonical project policy file. `.multorum/.gitignore` keeps Multorum runtime directories out of version control while scoping that policy to the `.multorum/` subtree.

### Runtime Region

In the main workspace:

- `.multorum/orchestrator/` stores the orchestrator's local control-plane data
- `.multorum/worktrees/` stores the managed worker worktrees

Inside each worker worktree:

- `.multorum/` stores the worker contract, compiled boundaries, mailboxes, and runtime artifacts

These runtime files are authoritative for local operation but are not project configuration and do not travel with the repository.

### Gitignore

`.multorum/.gitignore` should contain:

```text
orchestrator/
worktrees/
```

Multorum verifies these entries during `rulebook init` and warns if they are missing. Worker-local runtime files are ignored through local exclude rules inside each worktree rather than through the committed `.multorum/.gitignore`.

---

## Instruction Reference

This section is a compact reference. The conceptual meaning of these instructions is defined in the earlier sections.

### Rulebook Instructions

**`rulebook init`**  
Initializes `.multorum/`, writes the default rulebook template if one does not already exist, prepares `.multorum/.gitignore`, and creates the local orchestrator runtime directories. It must not overwrite an existing rulebook.

**`rulebook switch`**  
Validates and activates the rulebook at `HEAD`. If the target rulebook conflicts with any active bidding group, the switch is rejected.

**`rulebook validate`**  
Performs the same validation as `rulebook switch` against `HEAD` but does not activate the rulebook.

### Worker Lifecycle Instructions

**`create <perspective-name>`**  
Compiles the selected perspective's boundaries, checks them against active bidding groups, creates a pinned worker worktree, materializes the worker runtime surface, and transitions the worker to `ACTIVE`.

**`resolve <worker-id>`**  
Publishes a `resolve` bundle to the worker inbox and transitions the worker from `BLOCKED` to `ACTIVE` once acknowledged.

**`revise <worker-id>`**  
Publishes a `revise` bundle to the worker inbox and returns a committed worker to `ACTIVE` once acknowledged.

**`merge <worker-id>`**  
Runs the merge pipeline for the worker's submitted commit. If all checks pass, the commit is merged and the worker transitions to `MERGED`.

**`discard <worker-id>`**  
Finalizes a worker without merging its work. The preserved workspace remains available until explicitly deleted.

**`delete <worker-id>`**  
Removes the worktree of a worker that is already in `MERGED` or `DISCARDED`.

### Worker-Originated Instructions

**`commit <worker-id>`**  
Submitted by the worker through its outbox. Multorum records the commit, freezes the workspace, and transitions the worker from `ACTIVE` to `COMMITTED`.

**`report <worker-id>`**  
Submitted by the worker through its outbox. Multorum records the report and transitions the worker from `ACTIVE` to `BLOCKED`.

### Query Instruction

**`status`**  
Returns the current active workers, their bidding-group membership, the active rulebook commit, and a summary of blocked workers awaiting resolution.
