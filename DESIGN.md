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

The rulebook is the project's declaration of ownership boundaries. It defines named file sets, perspectives, and the merge-time check pipeline.

A perspective is a named role in the rulebook. It declares:

- a write set: the files a worker from this role may modify
- a read set: the files that must remain stable while this role is active

The write set is a closed list of existing files. Workers may not create files outside it. When a blocked worker discovers that the task really needs a new file, the orchestrator must update the canonical workspace and the rulebook, then forward the blocked bidding group to HEAD before resolving the blocker. The read set is not a visibility filter — workers may read any file in the repository. The read set exists to tell Multorum which files must remain untouched by other concurrent work, and to tell the worker what the orchestrator considers stable context.

A worker is a runtime instantiation of a perspective. Perspectives are static policy. Workers are ephemeral executions with state.

### Bidding Groups

A bidding group forms when the orchestrator creates the first worker for a perspective. The group's base commit is HEAD at the moment of creation, and its compiled boundary is the perspective evaluated against that snapshot. Subsequent workers created for the same perspective join the existing group and share its base commit and boundary.

If the orchestrator wants a fresh base for a perspective that already has an active bidding group, the existing group must be fully merged or discarded first, or forwarded to HEAD via `perspective forward`.

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

Multorum enforces the conflict-free invariant at worker creation time. The invariant is a runtime property of active bidding groups, not a static property of the rulebook — the same set of perspectives may or may not conflict depending on which files their globs match in a given repository state.

### Perspective Validation

The orchestrator can check whether a set of perspectives satisfies the conflict-free invariant before creating workers. `perspective validate` compiles the named perspectives from the current rulebook, checks them against each other, and checks them against active bidding groups. With `--no-live`, the check covers only the named perspectives and ignores active groups.

### Perspective Forward

`perspective forward` moves a live bidding group from its current base commit to HEAD, recompiling the perspective boundary from the current rulebook.

The recompiled boundary must be a superset of the group's current materialized boundary, both read and write sets independently. Boundary expansion is permitted. Boundary reduction is rejected, because it would break the contract that live workers were created under.

The operation is intentionally narrow:

- it addresses the whole live bidding group for one perspective, never one worker in isolation
- it is rejected unless every live worker in that bidding group is exactly `BLOCKED`
- it preserves progress only from the `head_commit` recorded in each worker's latest blocking `report`
- it rejects dirty or drifted worktrees rather than trying to invent recovery
- it leaves every forwarded worker in `BLOCKED`; the orchestrator must still issue `resolve` afterward

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

File-set expressions are rulebook-level syntax only. When Multorum needs a concrete boundary — at worker creation, perspective validation, or perspective forward — it compiles expressions into concrete file lists by expanding globs against the working tree and evaluating the set operations.

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

The runtime directory names are intentionally short. `tr/` keeps managed worktree paths compact, and `state.toml` consolidates all bidding group and worker state into a single file so the control plane stays shallow.

### Orchestrator Runtime Surface

The orchestrator's control plane lives under `.multorum/orchestrator/`, created during `rulebook init`:

```text
.multorum/orchestrator/
  state.toml             # bidding groups, workers, and compiled boundaries
  exclusion-set.txt      # materialized orchestrator exclusion set
  audit/                 # merge audit trail
    <worker>.toml     # per-worker TOML metadata record
    <worker>/         # optional rationale bundle
      body.md
      artifacts/
```

`state.toml` is the orchestrator's single runtime state file. It records every bidding group and every worker within it. Each group entry carries the perspective name, base commit, and compiled boundary (read and write sets as concrete file lists). Each worker entry within a group carries the worker, lifecycle state, and submitted head commit where applicable.

`rulebook init` creates `state.toml` as an empty file. Subsequent operations update it:

- `worker create` forming a new group adds a group entry with perspective, base commit (HEAD), compiled boundary, and the first worker entry.
- `worker create` joining an existing group adds a worker entry to that group.
- `worker merge` marks the worker `MERGED`, marks siblings `DISCARDED`, and clears the group's boundary — the group no longer contributes to the exclusion set.
- `worker discard` marks the worker `DISCARDED`. If the group has no remaining non-finalized members, the group's boundary is cleared.
- `worker delete` removes the worker entry. If the group has no remaining members, the group entry is removed.
- `perspective forward` updates the group's base commit and recompiled boundary, and rewrites each forwarded worker's `read-set.txt` and `write-set.txt` to match.

`exclusion-set.txt` is a flat projection of `state.toml`: the union of all read and write sets from groups that still carry a boundary. A pre-commit hook in the canonical workspace reads it and rejects commits that touch any listed file. Multorum regenerates it whenever `state.toml` changes. When no groups carry a boundary the file is empty.

`audit/` records the decision trail for merged workers. Each entry is written atomically when `merge` succeeds and contains the worker, perspective, base commit, integrated head commit, the list of changed files, which checks ran or were skipped, and the orchestrator-supplied rationale. The rationale is a bundle — a `body.md` and optional `artifacts/` — attached by the orchestrator at merge time to explain what the worker accomplished and why the merge was accepted. When the orchestrator supplies rationale, Multorum writes it as a bundle subdirectory alongside the TOML record. Audit entries are append-only; Multorum never modifies or deletes them.

### Git Worktrees

Each worker workspace is a git worktree created from the bidding group's base commit:

```text
git worktree add .multorum/tr/<worker> <base-commit>
```

Workers in the same bidding group share the same base commit, set when the first worker in the group is created. Workers in different bidding groups may have different base commits.

If a worker is reused after that worker reaches `MERGED` or `DISCARDED`, Multorum removes the finalized worktree first and creates a fresh workspace at the same path. Reuse means "create a new worker here", not "reopen old state".

### Worker Runtime Surface

Every worker worktree has its own `.multorum/` directory, separate from the orchestrator's. At creation time, Multorum materializes:

```text
.multorum/
  rulebook.toml      # snapshot from the base commit
  contract.toml      # worker, perspective, base commit
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
- `BLOCKED`: the worker has reported a blocker; returns to `ACTIVE` once it acknowledges a `resolve` inbox message, or is discarded
- `COMMITTED`: the worker has submitted a commit; returns to `ACTIVE` once it acknowledges a `revise` inbox message, is merged, or is discarded
- `MERGED`: the commit passed the merge pipeline and was integrated
- `DISCARDED`: the worker was finalized without merge

The `resolve` and `revise` arcs in the diagram are labeled by the inbox message kind. The orchestrator publishes the message; the state transition fires when the worker acknowledges it via `local ack`.

The orchestrator may also issue `hint` while a worker is `ACTIVE`. A hint is advisory rather than transitional: it carries new information or asks the worker to take a follow-up action such as reporting a blocker, but publishing or acknowledging the hint does not change lifecycle state on its own.

Once one worker in a bidding group reaches `MERGED`, every sibling in that group becomes `DISCARDED`.

`delete` is not a lifecycle transition. It removes the worktree and the worker's entry from `state.toml`.

`perspective forward` is also not a lifecycle transition. It repins a blocked bidding group to HEAD while leaving worker states unchanged.

### Transitions

| From | To | Trigger |
|---|---|---|
| *(create)* | ACTIVE | worktree and runtime surface materialized |
| ACTIVE | BLOCKED | worker issues `report` |
| ACTIVE | COMMITTED | worker issues `commit` |
| ACTIVE | DISCARDED | orchestrator issues `discard` |
| ACTIVE | ACTIVE | worker issues `hint` |
| BLOCKED | ACTIVE | worker acknowledges `resolve` |
| BLOCKED | DISCARDED | orchestrator issues `discard` |
| COMMITTED | ACTIVE | worker acknowledges `revise` |
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
worker      = "my-worker"        # author runtime identity
perspective = "AuthImplementor"  # author perspective
kind        = "report"           # message classification
sequence    = 7                  # monotonic counter per author
created_at  = "2026-03-24T10:00:00Z"
in_reply_to = 5                  # optional, for correlation
head_commit = "a1b2c3d"          # optional, for submission kinds
```

The `kind` field classifies the message:

- `task` — orchestrator assigns or updates a task for the worker
- `hint` — orchestrator sends advisory follow-up context to an active worker
- `report` — worker reports a blocker, transitions worker to `BLOCKED`
- `commit` — worker submits completed work, transitions worker to `COMMITTED`
- `resolve` — orchestrator resolves a blocker
- `revise` — orchestrator requests revisions to a submission

Mailbox bundles are published atomically: Multorum writes to a temporary name inside `new/` then renames into place. Readers see either the complete bundle or nothing. Sequence numbers are assigned by the author at publication time and never reused.

Published bundles are immutable. Receipt is recorded by writing an acknowledgement file with the same sequence number into the corresponding `ack/` directory. The unique runtime identity in all exchanges is the worker, not the perspective name.

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

After a successful merge, Multorum writes an audit entry to `.multorum/orchestrator/audit/<worker>.toml`. The entry records the worker, perspective, base commit, integrated head commit, changed files, checks ran, checks skipped, and the orchestrator's rationale. The rationale is a bundle attached to the `merge` command via `--body`, `--body-path`, and `--artifact` flags. When supplied, Multorum writes the rationale as a bundle directory (see [Bundles](#bundles)) alongside the TOML record under the same audit directory.

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

#### Orchestrator-mode resources

Concrete:

| URI | Description |
|---|---|
| `multorum://orchestrator/status` | Full orchestrator snapshot: active perspectives and workers. |
| `multorum://orchestrator/perspectives` | Compiled perspective summaries from the current rulebook. |
| `multorum://orchestrator/workers` | Worker summary listing for the current runtime. |

Templates:

| URI template | Description |
|---|---|
| `multorum://orchestrator/workers/{worker}` | Detailed orchestrator-side view of one worker. |
| `multorum://orchestrator/workers/{worker}/outbox` | Outbox mailbox listing for one worker. |

#### Worker-mode resources

Concrete:

| URI | Description |
|---|---|
| `multorum://worker/contract` | Immutable worker contract for the active perspective. |
| `multorum://worker/inbox` | Inbox mailbox listing for the active worker. |
| `multorum://worker/status` | Projected worker lifecycle status. |

Worker-mode resources carry no worker identity parameter because the server is started from inside a single worker worktree — the identity is implicit.

### Error Contract

MCP-visible error codes are stable protocol values, independent of Rust enum variant names. Tool-level failures and resource-read failures should preserve the underlying domain category where possible, for example distinguishing invalid parameters from missing runtime objects.

---

## Instruction Reference

This section lists the instructions that the orchestrator and workers may issue, in the form of CLI commands. MCP tools mirror the same runtime operations with typed arguments.

### Rulebook

- `multorum rulebook init` — Initialize `.multorum/`, write the default committed artifacts if absent, prepare `.multorum/.gitignore`, and create orchestrator runtime directories.

### Perspective

- `multorum perspective list` — List perspectives from the current rulebook.
- `multorum perspective validate <perspectives>...` — Compile the named perspectives from the current rulebook, check conflict-freedom between them, and check them against active bidding groups. With `--no-live`, check only the named perspectives against each other.
- `multorum perspective forward <perspective>` — Move the whole live bidding group for `perspective` to HEAD. Recompile the perspective boundary from the current rulebook. Rejected unless every live worker in that bidding group is `BLOCKED` and the recompiled boundary is a superset of the current materialized boundary. Progress is preserved only from the `head_commit` recorded in each worker's latest blocking `report`. No lifecycle transition.

### Orchestrator Worker Commands

- `multorum worker create <perspective> [--worker <worker>] [--overwriting-worktree] [--body <file>] [--artifact <file>]...` — Compile the perspective boundary from the current rulebook against the working tree. If a bidding group for this perspective already exists, join it. Otherwise, form a new group with base commit set to HEAD and check conflict-freedom against all active bidding groups. Create the worker worktree and materialize the runtime surface, seeding the initial `task` inbox bundle with the optional payload. `--worker` sets an explicit worker identity; when omitted, Multorum derives one from the perspective name. `--overwriting-worktree` replaces an existing finalized workspace for the same explicit worker. Transition: new worker enters `ACTIVE`.
- `multorum worker list` — List active workers.
- `multorum worker show <worker>` — Return one worker in detail.
- `multorum worker outbox <worker> [--after <sequence>]` — List worker-authored bundles from that worker's outbox. No lifecycle transition.
- `multorum worker ack <worker> <sequence>` — Record orchestrator receipt for one worker outbox bundle. No lifecycle transition.
- `multorum worker hint <worker>` — Publish a `hint` bundle to an active worker inbox. Use this to pass new project information or ask the worker to stop gracefully by issuing `report`. No lifecycle transition.
- `multorum worker resolve <worker> [--reply-to <sequence>] [--body <file>] [--artifact <file>]...` — Publish a `resolve` bundle to a blocked worker inbox. `--reply-to` correlates the resolve with an earlier outbox sequence number. The optional payload carries resolution context for the worker. The worker returns to `ACTIVE` when it acknowledges that inbox message.
- `multorum worker revise <worker> [--reply-to <sequence>] [--body <file>] [--artifact <file>]...` — Publish a `revise` bundle to a committed worker inbox. `--reply-to` correlates the revision with an earlier outbox sequence number. The optional payload carries revision context for the worker. The worker returns to `ACTIVE` when it acknowledges that inbox message.
- `multorum worker merge <worker> [--skip-check <check>]... [--body <text>] [--body-path <file>] [--artifact <file>]...` — Verify the submitted head commit, enforce the write set, run the merge pipeline, and integrate the worker if checks pass. The optional payload arguments attach an audit rationale. Transition: `COMMITTED` to `MERGED`.
- `multorum worker discard <worker>` — Finalize a worker without integration. Allowed from `ACTIVE`, `BLOCKED`, or `COMMITTED`. Transition: worker enters `DISCARDED`. The workspace remains until deleted.
- `multorum worker delete <worker>` — Delete the worktree and remove the worker's entry from `state.toml`. If the worker is the last member of its bidding group, the group entry is also removed. Allowed only from `MERGED` or `DISCARDED`.

### Worker-Local Commands

- `multorum local contract` — Load the worker contract for the current worktree.
- `multorum local status` — Return the projected status for the current worktree.
- `multorum local inbox [--after <sequence>]` — List inbox messages for the current worker. No lifecycle transition.
- `multorum local ack <sequence>` — Acknowledge one inbox message. Acknowledging `task`, `resolve`, or `revise` transitions the worker into `ACTIVE`.
- `multorum local report [--head-commit <commit>] [--reply-to <sequence>] [--body <file>] [--artifact <file>]...` — Publish a blocker report from the current worktree. `--reply-to` correlates the report with an earlier inbox sequence number. The optional payload carries blocker details and evidence. Transition: `ACTIVE` to `BLOCKED`.
- `multorum local commit --head-commit <commit> [--body <file>] [--artifact <file>]...` — Publish a completed worker submission from the current worktree. The optional payload carries submission evidence. Transition: `ACTIVE` to `COMMITTED`.

### Query

- `multorum status` — Return the full orchestrator status snapshot, including active workers and bidding-group membership.

### Utility

- `multorum util completion <shell>` — Emit shell completions to stdout. Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

After running the command, source the output in your shell profile to enable tab completion.

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

### MCP Server

- `multorum serve orchestrator` — Start the orchestrator MCP server on stdio from the workspace root.
- `multorum serve worker` — Start the worker MCP server on stdio from inside a worker worktree.
