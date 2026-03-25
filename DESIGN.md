# Project Multorum: Architecture Reference

## Table of Contents

1. [Introduction](#introduction)
2. [Core Model](#core-model)
3. [Rulebook](#rulebook)
4. [Workspace Model](#workspace-model)
5. [Worker Lifecycle](#worker-lifecycle)
6. [Mailbox Protocol](#mailbox-protocol)
7. [Merge Pipeline](#merge-pipeline)
8. [MCP Surface](#mcp-surface)
9. [Instruction Reference](#instruction-reference)

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

The write set is a closed list of existing files. Workers may not create files outside it. When a blocked worker discovers that the task really needs a new file, the orchestrator must update the canonical workspace and the rulebook, install that rulebook, and then explicitly forward the whole blocked bidding group to the new pinned snapshot before resolving the blocker. The read set is not a visibility filter — workers may read any file in the repository. The read set exists to tell Multorum which files must remain untouched by other concurrent work, and to tell the worker what the orchestrator considers stable context.

A worker is a runtime instantiation of a perspective. Perspectives are static policy. Workers are ephemeral executions with state.

### Bidding Groups

If the orchestrator creates multiple workers from the same perspective against the same pinned snapshot, those workers form a bidding group. All workers in a bidding group share the same perspective, pinned base commit, compiled read set, and compiled write set.

If the active rulebook later moves forward, the live bidding group does not follow automatically. The orchestrator must issue an explicit perspective-forward operation to move that group to the new pinned base. Until that happens, Multorum rejects creation of additional same-perspective workers from the newer active rulebook commit.

Only one worker from a bidding group may be merged. Once one member is merged, the remaining members are discarded.

### Conflict-Free Invariant

The central correctness invariant is:

> **A file may either be written by exactly one active bidding group, or read by any number of active bidding groups, but never both.**

For any two distinct active bidding groups `G` and `H`:

- `write(G) ∩ write(H) = ∅`
- `write(G) ∩ read(H) = ∅`
- `read(G) ∩ write(H) = ∅`

Inside one bidding group, every worker has the same boundary. Conflict detection belongs at the bidding-group level, not at the level of perspective names: perspectives describe policy, bidding groups are the concurrent runtime entities that must not interfere.

The invariant extends to the canonical branch. While any bidding group is active, the union of every active group's read and write sets forms the *orchestrator exclusion set* — files the orchestrator must not commit to until the owning workers are merged or discarded. The orchestrator may commit freely only to files outside the exclusion set.

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

Because the rulebook is version-controlled, every historical state is addressable by commit hash. When Multorum activates a rulebook, it pins that exact commit. Active workers are governed by an immutable snapshot — editing the file on disk does nothing until the orchestrator explicitly installs a new rulebook.

### Rulebook Install and Uninstall

The orchestrator evolves the rulebook through normal commits. Multorum never follows new commits automatically. To advance policy, the orchestrator issues `rulebook install`, which validates the rulebook at `HEAD` against currently active workers.

Install enforces two conditions:

**Continuity.** Every active bidding group must remain representable in the target rulebook. For each active group with perspective name P, the target rulebook must define P with a compiled boundary that is a superset of (or equal to) the group's materialized boundary — both read and write sets independently. Boundary expansion is permitted and takes effect immediately for the live group: Multorum rewrites each live worker's `read-set.txt` and `write-set.txt` to the expanded boundary while keeping the worker's base snapshot pinned. Boundary reduction is rejected, because it would break the contract that live workers were created under.

This design keeps the runtime exclusion set, conflict checks, and worker-local guidance aligned with the active rulebook. If Multorum accepted an expanded perspective but left live workers on stale boundary files, the installed policy and the runtime enforcement surface would diverge.

`rulebook install` never repins a live worker's code snapshot. Snapshot movement is a separate explicit step because it can replay worker-authored commits onto a newer base.

**Conflict-freedom.** Every candidate perspective in the target rulebook must satisfy the conflict-free invariant against every active bidding group whose name differs from the candidate. Same-name pairs are exempt — their compatibility is established by the continuity condition.

If both conditions hold, the install succeeds and Multorum pins the `HEAD` commit as the active rulebook. On failure, Multorum rejects the install and reports the blocking perspectives.

`rulebook uninstall` deactivates the active rulebook. It is rejected when any live bidding group still depends on the active rulebook.

### Perspective Forward

`multorum perspective forward <perspective>` moves one live bidding group from its old pinned base commit to the current active rulebook commit.

The operation is intentionally narrow:

- it addresses the whole live bidding group for one perspective, never one worker in isolation
- it is rejected unless every live worker in that bidding group is exactly `BLOCKED`
- it preserves progress only from the `head_commit` recorded in each worker's latest blocking `report`
- it rejects dirty or drifted worktrees rather than trying to invent recovery
- it leaves every forwarded worker in `BLOCKED`; the orchestrator must still issue `resolve` afterward

This keeps `rulebook install` as a policy update and `perspective forward` as the explicit snapshot move required for blocker-driven contract changes such as adding a new file.

---

## Workspace Model

### Bundles

A bundle is a directory containing a `body.md` primary content file and an `artifacts/` subdirectory for supplementary files. Bundles are the shared content container used wherever Multorum stores structured content atomically: mailbox messages carry one and audit entries carry one for the orchestrator's rationale.

```text
<bundle-directory>/
  body.md          # primary Markdown content
  artifacts/       # optional auxiliary files
```

`body.md` and `artifacts/` are opaque to Multorum. The runtime materializes them from user-supplied payloads but never parses their content.

When a payload supplies files by path, Multorum consumes them rather than copying. On successful publication, the runtime moves the files into bundle storage and becomes responsible for retaining them.

### Filesystem Layout

A Multorum project adds a `.multorum/` directory at the repository root:

```text
<project-root>/
  .multorum/
    .gitignore          # committed - ignores runtime directories
    rulebook.toml       # committed - file sets, perspectives, check pipeline
    orchestrator/       # gitignored - orchestrator-local control plane
    tr/                 # gitignored - managed worker worktrees
  src/
  tests/
  ...
```

The project commits only `.multorum/rulebook.toml` and `.multorum/.gitignore`. Everything else under `.multorum/` is runtime state that does not travel with the repository.

`.multorum/.gitignore` contains:

```text
orchestrator/
tr/
```

Multorum verifies these entries during `rulebook init` and warns if they are missing.

The runtime directory names are intentionally short. `tr/` keeps managed worktree paths compact, and worker state projections are stored as single files under `orchestrator/workers/` so the control plane stays shallow without changing the broader orchestrator layout.

### Orchestrator Runtime Surface

The orchestrator's control plane lives under `.multorum/orchestrator/`, created during `rulebook install`:

```text
.multorum/orchestrator/
  active-rulebook.toml   # pinned commit hash and compiled rulebook snapshot
  exclusion-set.txt      # materialized orchestrator exclusion set
  workers/               # per-worker state projections
    <worker-id>.toml     # lifecycle state, base commit, submitted head commit
  audit/                 # merge audit trail
    <worker-id>.toml     # per-worker TOML metadata record
    <worker-id>/         # optional rationale bundle
      body.md
      artifacts/
```

`active-rulebook.toml` records the commit at which the rulebook was installed and caches the compiled result. Worker state files track each worker's lifecycle independently of the worktree contents.

`audit/` records the decision trail for merged workers. Each entry is written atomically when `merge` succeeds and contains the worker id, perspective, base commit, integrated head commit, the list of changed files, which checks ran or were skipped, and the orchestrator-supplied rationale. The rationale is a bundle — a `body.md` and optional `artifacts/` — attached by the orchestrator at merge time to explain what the worker accomplished and why the merge was accepted. When the orchestrator supplies rationale, Multorum writes it as a bundle subdirectory alongside the TOML record. Audit entries are append-only; Multorum never modifies or deletes them.

`exclusion-set.txt` is the materialized orchestrator exclusion set: the union of all active bidding groups' read and write sets. Multorum rewrites this file on every lifecycle transition that changes the set of active groups (create, merge, discard). A pre-commit hook in the canonical workspace reads it and rejects commits that touch any listed file. When no workers are active the file is empty.

### Git Worktrees

Each worker workspace is a git worktree created from the pinned base commit:

```text
git worktree add .multorum/tr/<worker-id> <pinned-base-commit>
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
                 BLOCKED ──────►┐
                    ▲ │         │
             report │ │ resolve │
                    │ ▼         │
create ─────────► ACTIVE ──────►┼──────────► DISCARDED
                    │ ▲         │ discard
             commit │ │ revise  │
                    ▼ │         │
                 COMMITTED ────►┘
                     │
               merge │
                     ▼
                  MERGED
```

- `ACTIVE`: the workspace exists and execution may proceed
- `BLOCKED`: the worker has reported a blocker and awaits orchestrator resolution or discard
- `COMMITTED`: the worker has submitted a commit; the workspace is frozen pending orchestrator action
- `MERGED`: the commit passed the merge pipeline and was integrated
- `DISCARDED`: the worker was finalized without merge

Once one worker in a bidding group reaches `MERGED`, every sibling in that group becomes `DISCARDED`.

`delete` is not a lifecycle transition. It removes the worktree and worker state file of a finalized worker.

`perspective forward` is also not a lifecycle transition. It is a perspective-scoped runtime operation that repins a blocked bidding group while leaving worker states unchanged.

### Transitions

| From | To | Trigger |
|---|---|---|
| *(create)* | ACTIVE | worktree and runtime surface materialized |
| ACTIVE | BLOCKED | worker issues `report` |
| ACTIVE | COMMITTED | worker issues `commit` |
| ACTIVE | DISCARDED | orchestrator issues `discard` |
| BLOCKED | ACTIVE | orchestrator issues `resolve` |
| BLOCKED | DISCARDED | orchestrator issues `discard` |
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

Every message is a bundle (see [Bundles](#bundles)) extended with an `envelope.toml` that carries mailbox routing metadata. The envelope is the only file Multorum interprets inside a mailbox bundle.

```
<mailbox>/new/<sequence>-<kind>/
  envelope.toml    # machine-readable routing metadata
  body.md          # primary content (always present, may be empty)
  artifacts/       # optional auxiliary files
```

`envelope.toml` fields:

```toml
protocol    = "multorum/v1"
worker      = "my-worker-id"     # author runtime identity
perspective = "AuthImplementor"  # author perspective
kind        = "report"           # message classification
sequence    = 7                  # monotonic counter per author
created_at  = "2026-03-24T10:00:00Z"
in_reply_to = 5                  # optional, for correlation
head_commit = "a1b2c3d"          # optional, for submission kinds
```

The `kind` field classifies the message:

- `task` — orchestrator assigns or updates a task for the worker
- `report` — worker reports a blocker, transitions worker to `BLOCKED`
- `commit` — worker submits completed work, transitions worker to `COMMITTED`
- `resolve` — orchestrator resolves a blocker
- `revise` — orchestrator requests revisions to a submission

Mailbox bundles are published atomically: Multorum writes to a temporary name inside `new/` then renames into place. Readers see either the complete bundle or nothing. Sequence numbers are assigned by the author at publication time and never reused.

Published bundles are immutable. Receipt is recorded by writing an acknowledgement file with the same sequence number into the corresponding `ack/` directory. The unique runtime identity in all exchanges is the worker id, not the perspective name.

### Ownership and Acknowledgement

Each mailbox subtree has exactly one writer:

- orchestrator writes `inbox/new/`
- worker writes `inbox/ack/`
- worker writes `outbox/new/`
- orchestrator writes `outbox/ack/`

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

### Audit

After a successful merge, Multorum writes an audit entry to `.multorum/orchestrator/audit/<worker-id>.toml`. The entry records the worker id, perspective, base commit, integrated head commit, changed files, checks ran, checks skipped, and the orchestrator's rationale. The rationale is a bundle attached to the `merge` command via `--body`, `--body-path`, and `--artifact` flags. When supplied, Multorum writes the rationale as a bundle directory (see [Bundles](#bundles)) alongside the TOML record under the same audit directory.

---

## MCP Surface

Multorum exposes the runtime model over the Model Context Protocol as a transport projection, not as a separate source of truth. The filesystem-backed runtime remains canonical.

### Server Modes

The MCP surface is split into two stdio servers:

- orchestrator mode, started from the workspace root
- worker mode, started from inside one managed worker worktree

Each mode exposes only the tools and resources that make sense for that runtime role.

### Tools

MCP tools mirror the explicit runtime instructions. Their arguments are typed in the protocol schema so hosts can validate and render them correctly:

- strings for identifiers, paths, and commit references
- integers for mailbox sequence numbers
- booleans for explicit flags
- arrays of strings for repeated path or check arguments

Tool results are JSON payloads. Runtime failures remain tool-level failures rather than protocol transport failures.

### Resources

MCP resources expose read-only projections of runtime state and are returned as JSON.

Concrete resources should list only currently implemented projections. Parameterized URIs belong in resource templates rather than in the concrete resource list.

Current discovery rules:

- concrete resources cover fixed snapshots such as orchestrator status, the active rulebook commit, worker inbox contents, and worker status
- parameterized templates cover projections that require a runtime identity, such as orchestrator-side worker detail and worker outbox listings
- projections that are not implemented yet must not be advertised as concrete resources

### Error Contract

MCP-visible error codes are stable protocol values, independent of Rust enum variant names. Tool-level failures and resource-read failures should preserve the underlying domain category where possible, for example distinguishing invalid parameters from missing runtime objects.

---

## Instruction Reference

This section lists the instructions that the orchestrator and workers may issue, in the form of CLI commands. MCP tools mirror the same runtime operations with typed arguments.

### Rulebook

- `multorum rulebook init` — Initialize `.multorum/`, write the default committed artifacts if absent, prepare `.multorum/.gitignore`, and create orchestrator runtime directories.
- `multorum rulebook install` — Validate and activate the rulebook at `HEAD`. Rejected if any active bidding group's perspective is missing or reduced in the target, or if any candidate conflicts with a differently-named active group.
- `multorum rulebook uninstall` — Deactivate the active rulebook. Rejected if any live bidding group still depends on it.
- `multorum rulebook validate` — Perform the same validation as `install` without activating the rulebook.

### Perspective

- `multorum perspective list` — List the compiled perspectives from the active rulebook.
- `multorum perspective forward <perspective>` — Move the whole live bidding group for `perspective` to the active rulebook commit. Rejected unless every live worker in that bidding group is `BLOCKED`. Progress is preserved only from the `head_commit` recorded in each worker's latest blocking `report`. No lifecycle transition.

### Orchestrator Worker Commands

- `multorum worker create <perspective>` — Compile the selected perspective boundary, check it against active bidding groups, create the worker worktree, and materialize the runtime surface. Rejected if a live bidding group for the same perspective is still pinned to an older base commit and must be forwarded first. Transition: new worker enters `ACTIVE`.
- `multorum worker list` — List active workers.
- `multorum worker show <worker-id>` — Return one worker in detail.
- `multorum worker outbox <worker-id> [--after <sequence>]` — List worker-authored bundles from that worker's outbox. No lifecycle transition.
- `multorum worker ack <worker-id> <sequence>` — Record orchestrator receipt for one worker outbox bundle. No lifecycle transition.
- `multorum worker resolve <worker-id>` — Publish a `resolve` bundle to a blocked worker inbox. The worker returns to `ACTIVE` when it acknowledges that inbox message.
- `multorum worker revise <worker-id>` — Publish a `revise` bundle to a committed worker inbox. The worker returns to `ACTIVE` when it acknowledges that inbox message.
- `multorum worker merge <worker-id> [--skip-check <check>]... [--body <text>] [--body-path <file>] [--artifact <file>]...` — Verify the submitted head commit, enforce the write set, run the merge pipeline, and integrate the worker if checks pass. The optional payload arguments attach an audit rationale. Transition: `COMMITTED` to `MERGED`.
- `multorum worker discard <worker-id>` — Finalize a worker without integration. Allowed from `ACTIVE`, `BLOCKED`, or `COMMITTED`. Transition: worker enters `DISCARDED`. The workspace remains until deleted.
- `multorum worker delete <worker-id>` — Delete the worktree and worker state file of a finalized worker. Allowed only from `MERGED` or `DISCARDED`.

### Worker-Local Commands

- `multorum local contract` — Load the worker contract for the current worktree.
- `multorum local status` — Return the projected status for the current worktree.
- `multorum local inbox [--after <sequence>]` — List inbox messages for the current worker. No lifecycle transition.
- `multorum local ack <sequence>` — Acknowledge one inbox message. Acknowledging `task`, `resolve`, or `revise` transitions the worker into `ACTIVE`.
- `multorum local report [--head-commit <commit>]` — Publish a blocker report from the current worktree. Transition: `ACTIVE` to `BLOCKED`.
- `multorum local commit --head-commit <commit>` — Publish a completed worker submission from the current worktree. Transition: `ACTIVE` to `COMMITTED`.

### Query

- `multorum status` — Return the full orchestrator status snapshot, including active workers, bidding-group membership, and the active rulebook commit.

### Utility

- `multorum util completion <shell>` — Emit shell completions to stdout. Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

### MCP Server

- `multorum serve orchestrator` — Start the orchestrator MCP server on stdio from the workspace root.
- `multorum serve worker` — Start the worker MCP server on stdio from inside a worker worktree.
