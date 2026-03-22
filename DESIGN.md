# Project Multorum: Architecture Reference

## Table of Contents

1. [Introduction](#introduction)
2. [Core Concepts](#core-concepts)
3. [File Set Algebra](#file-set-algebra)
4. [Perspectives](#perspectives)
5. [Workers](#workers)
6. [The Rulebook](#the-rulebook)
7. [Project Layout](#project-layout)
8. [Worker Creation](#worker-creation)
9. [The Worker State Machine](#the-worker-state-machine)
10. [The Mailbox Protocol](#the-mailbox-protocol)
11. [The Pre-Merge Pipeline](#the-pre-merge-pipeline)
12. [The Orchestrator Instruction Set](#the-orchestrator-instruction-set)

---

## Introduction

Multorum is a programming tool that manages multiple simultaneous perspectives on a single codebase. It is designed primarily for AI agent orchestration workflows, where a coordinating agent (the *orchestrator*) decomposes a development goal into discrete tasks and assigns each task to an independent agent (a *worker*). Each worker operates in an isolated environment with precisely scoped access to the codebase, then submits its work back to the orchestrator for merge.

### The Problem Multorum Solves

Parallel development — whether by humans or AI agents — faces a fundamental tension: workers need *isolation* to make progress independently, but they need *integration context* to validate that their work is correct. Without isolation, workers interfere with each other. Without integration context, workers produce code that may be syntactically valid but semantically broken in the context of the whole system.

Existing tools address one side of this tension or the other. Multorum addresses both simultaneously by separating *authoring scope* (what a worker may write) from *execution scope* (what a worker runs against). A worker may only write to its declared files, but it compiles, tests, and uses language services against the full codebase.

### Design Philosophy

Multorum is infrastructure, not an agent. It enforces invariants and executes instructions; all coordination intelligence lives in the orchestrator. Multorum never acts on its own initiative. Every state transition in the system is the result of an explicit orchestrator instruction.

---

## Core Concepts

### The Orchestrator

The orchestrator is the sole coordination authority in a Multorum workflow. It may be a human, an LLM, or a hybrid. The orchestrator is responsible for:

- Decomposing development goals into discrete tasks
- Declaring the rulebook that governs which workers exist and what they may access
- Issuing instructions to Multorum to create, resume, merge, discard, and delete workers
- Receiving and resolving worker reports
- Evolving the rulebook as the project matures

The orchestrator communicates downward to Multorum and to individual workers. Workers never communicate with each other; the communication topology is a strict star with the orchestrator at the center.

```
        Orchestrator
       /      |      \
      /       |       \
  Worker A  Worker B  Worker C
```

### Workers and Perspectives

A *perspective* is a declaration in the rulebook that defines a named role, its write scope, and its read scope. A *worker* is a runtime instantiation of a perspective, executing a task inside the environment that Multorum creates for that role.

The distinction matters: perspectives are static declarations that live in the rulebook; workers are runtime entities with lifecycle state. A perspective can exist in the rulebook without any live workers. When the orchestrator creates multiple workers from the same perspective against the same pinned snapshot, those workers form a *bidding group*: several competing executions of one declared role, of which at most one may ultimately be merged.

### The Canonical Codebase

There is one canonical codebase, managed under version control. It represents the authoritative state of the project. Workers never write to it directly. All changes flow through Multorum's pre-merge pipeline before being merged into the canonical codebase by the orchestrator.

---

## File Set Algebra

Multorum manages file permissions through a small algebra of *file sets*. This algebra allows permissions to be expressed precisely and maintainably, without resorting to scattered glob patterns that are difficult to audit or reason about.

### Motivation

A naive approach to file permissions might assign raw glob patterns directly to each perspective. This breaks down quickly in practice: the same pattern appears in multiple places, changes require updating every occurrence, and the relationship between permission sets is implicit rather than explicit. The file set algebra solves this by giving the project a shared vocabulary for describing regions of the codebase.

### Syntax

```
path  ::= <glob pattern>              e.g. "src/auth/**", "**/*.spec.md"
name  ::= <identifier>                e.g. AuthFiles, SpecFiles
expr  ::= name                        reference
        | expr "|" expr               union
        | expr "&" expr               intersection
        | expr "-" expr               difference
        | "(" expr ")"                grouping

definition ::= name ".path" "=" path  primitive — binds a name to a glob
             | name "=" expr          compound — binds a name to an expression
```

`A | B` produces every file in either set. `A & B` keeps only files present in both. `A - B` keeps files in A that are not in B. Expressions nest arbitrarily; precedence is flat, so use parentheses to disambiguate.

### Named Definitions

File set expressions are given names, making them referenceable by other file sets and by perspective declarations. Naming a file set creates a shared vocabulary for the project — a single place to update when boundaries change, and a readable shorthand in perspective declarations.

Names are defined in the `[filesets]` table. A name may bind either a primitive (a glob or explicit path) or a compound expression that references other names. Perspectives then reference these names in their `read` and `write` fields.

Consider a project with specification files, test files, and an authentication module:

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

Primitive names bind globs via the `.path` key (`SpecFiles.path`, `AuthFiles.path`). Compound names (`AuthSpecs`, `AuthTests`) reference other names through set expressions, narrowing a module to a cross-cutting concern via intersection. Perspectives then use union and difference to partition the module: `AuthImplementor` writes only production code by subtracting specs and tests from the full auth set, while `AuthTester` writes only tests. The two write sets are disjoint, so workers created from those perspectives may run concurrently in separate bidding groups.

### Compilation

File set expressions are a *rulebook-level concept only*. They do not exist at runtime. When Multorum activates a rulebook, it immediately compiles all file set expressions into concrete file lists by expanding globs against the current state of the filesystem and evaluating all set operations. From that point on, Multorum works exclusively with concrete lists.

Rulebook compilation produces candidate ownership sets. The conflict-free invariant is checked later, when those compiled sets would become concurrent with active work.

The compilation and activation flow is:

```
Rulebook file set expressions
        │
        ▼
Expand all globs against filesystem
        │
        ▼
Evaluate all set operations
        │
        ▼
Concrete file lists per perspective
        │
        ▼
Rulebook structural validation
        │
        ▼
Candidate bidding group from selected perspective
        │
        ▼
Compare candidate read/write sets against every active bidding group's materialized read/write sets
        │
        ▼
Create worker and materialize identical read/write sets in its runtime
```

### Constraints

The file set algebra imposes a few constraints that Multorum validates at compile time:

- **No cycles** — a named file set may not reference itself, directly or transitively
- **No undefined references** — every name used in an expression must be defined in the rulebook
- **Empty sets** — a file set that compiles to an empty list is valid; Multorum warns but does not error

Compile-time validation only proves that the rulebook is well-formed and that its file-set expressions reduce to concrete lists. It does not by itself prove that any particular worker may run concurrently with the workers that are already active.

---

## Perspectives

A *perspective* is a named declaration in the rulebook that defines a role's relationship to the codebase. It specifies three things: a name, a write set, and a read set.

### Anatomy of a Perspective

```toml
[perspectives.AuthImplementor]
read  = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"
```

- *Name*: an identifier (`AuthImplementor`) that the orchestrator uses to reference the perspective in instructions.
- *Write Set*: a file set expression that compiles to the exact list of files any worker instantiated from this perspective may modify. Write enforcement is absolute: submitted changes to files outside the compiled write set are rejected.
- *Read Set*: a file set expression identifying files relevant to the role and required to remain stable against other active bidding groups while that role is active. The read set is guidance, not a hard restriction — workers can read any file in the codebase, but the read set communicates what the orchestrator considers relevant and what concurrent work must leave untouched.

### Declaration Semantics

The write set is a closed, compiled list of files. A perspective authorizes modifications only to files that existed in the codebase at rulebook activation time and that appear in its compiled write set. Creating new files requires orchestrator intervention and a new rulebook version.

The read set names the stable context for the role. It tells the orchestrator which files must remain untouched by other concurrent work when the perspective is instantiated, and it tells workers where the intended context for the task lives.

## Workers

A *worker* is a runtime execution of a perspective. Worker creation pins a worker to the base commit and the compiled read and write sets derived from that perspective at that moment. The worker's governing rulebook is the one committed at its base commit. Perspectives are static policy; workers are ephemeral attempts.

### Bidding Groups

When the orchestrator wants multiple attempts at the same role, it may create multiple workers from one perspective. Those workers form a *bidding group*. A bidding group is the runtime unit of competition and merge selection.

All workers in the same bidding group share:

- the same perspective
- the same pinned base commit (and therefore the same governing rulebook)
- the same compiled read set
- the same compiled write set

Because workers inside a bidding group are alternative realizations of the same declared role, they are not isolated from each other by file ownership. Instead, they are kept comparable: they start from the same snapshot and operate under the same scope. Only one worker from a bidding group may be merged into the canonical codebase. Once one member is selected for merge, the remaining members of that group are discarded.

### The Conflict-Free Invariant

The conflict-free invariant is the core correctness invariant governing concurrent bidding groups:

> **A file may either be written by exactly one active bidding group, or read by any number of active bidding groups — never both.**

For any two distinct active bidding groups `G` and `H` in a compiled rulebook:

- `write(G) ∩ write(H) = ∅` — write sets are pairwise disjoint across groups
- `write(G) ∩ read(H) = ∅` — no file written by one group appears in another group's stable context
- `read(G) ∩ write(H) = ∅` — the same condition viewed from the other group's stable context

Inside a bidding group `B`, all workers are instantiations of the same perspective, so for any workers `x` and `y` in `B`:

- `write(x) = write(y)`
- `read(x) = read(y)`

This is why the conflict-free invariant belongs at the worker boundary rather than the perspective boundary. Perspectives declare ownership once; bidding groups are the concurrent runtime entities that must not interfere with each other. Once a valid set of bidding groups is active, workers execute in full parallel with no runtime conflict detection, arbitration, or rollback between groups. Merge stays conflict-free because each written file has at most one active writing group, and within a group the orchestrator selects at most one submission to merge.

### When It Is Checked

Multorum checks the conflict-free invariant when concurrent runtime state would change:

- On `create`, it takes the selected perspective's compiled read and write sets as the candidate bidding-group boundary.
- If the worker being created joins an existing bidding group, Multorum checks equality with that group's materialized read and write sets.
- If the worker being created creates a new bidding group, Multorum checks the candidate group's read and write sets against every other active bidding group's materialized read and write sets.
- On `rulebook switch`, Multorum compiles the target rulebook and checks every target perspective's candidate read and write sets against every currently active bidding group's materialized read and write sets.

In other words, conflict freedom is checked against the runtime state that already exists, not against perspectives in isolation.

---

## The Rulebook

The rulebook is the central configuration artifact of a Multorum project. It declares all perspectives, their file set permissions, and project-level settings. It lives at `.multorum/rulebook.toml` in the project root and is versioned in git alongside the codebase it governs.

### Structure

A rulebook contains:

- **File set definitions** — named expressions in the file set algebra
- **Perspective declarations** — named roles, each with a write set and a read set
- **Project-level settings** — the pre-merge check pipeline and its policies

### Example Rulebook

The following example shows a small but complete rulebook:

```toml
[filesets]
SpecFiles.path = "**/*.spec.md"
TestFiles.path = "**/test/**"

AuthFiles.path = "auth/**"
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

[perspectives.AuthImplementor]
read  = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"

[perspectives.AuthTester]
read  = "AuthSpecs | AuthTests"
write = "AuthTests"

[check]
pipeline = ["fmt", "clippy", "test"]

[check.command]
fmt = "cargo fmt --check"
clippy = "cargo clippy --workspace --all-targets -- -D warnings"
test = "cargo test --workspace"
```

This rulebook reuses the same file set vocabulary introduced earlier, then adds the project-level `check` table to make the example complete. Workers created from `AuthImplementor` and `AuthTester` may run in parallel because their write sets are disjoint, while `AuthSpecs` stays read-only across those groups. The `check` table defines the ordered pre-merge pipeline that every submitted change must pass before merge.

### Default Rulebook Template

`rulebook init` creates `.multorum/rulebook.toml` with a valid but intentionally empty template. The file should be immediately editable by the orchestrator and should explain only the decisions the orchestrator must make next:

```toml
# Define shared file ownership vocabulary first.
# `Name.path` binds a glob; `Name = "Expr"` combines names with |, &, and -.
[filesets]

# Add one table per perspective under `[perspectives.<Name>]`.
# `write` names the files that perspective may modify.
# `read` names stable context files that concurrent work must not write.
[perspectives]

# Add pre-merge gates in execution order.
# Add commands under `[check.command]` and optional skip policies under `[check.policy]`.
[check]
pipeline = []
```

This template is deliberately sparse. It gives the orchestrator the minimum structure needed to begin declaring ownership boundaries and checks without smuggling in project-specific assumptions.

### Immutability via Version Control

Because the rulebook is a version-controlled file, every historical state of it is addressable by a git commit hash. When Multorum activates a rulebook, it pins to a specific commit. This means the rulebook governing an active set of workers and bidding groups is immutable by construction — changing the file on disk does not affect active workers until the orchestrator explicitly instructs Multorum to switch rulebooks.

This approach deliberately delegates immutability enforcement to git rather than inventing a separate mechanism.

### Rulebook Lifecycle

The orchestrator evolves the rulebook by committing changes to `rulebook.toml` in the normal git workflow. Multorum never automatically acts on a new commit. When the orchestrator is ready to advance to a new rulebook version, it issues an explicit `rulebook switch` instruction. Multorum resolves `HEAD`, validates the switch, and, if valid, activates the rulebook at that commit. There is no separate commit argument — the repository-wide rulebook is always consistent with `HEAD`.

The history of rulebook evolution is inspectable with standard git tooling:

```
git log .multorum/rulebook.toml
git diff <hash-a> <hash-b> -- .multorum/rulebook.toml
```

Development phases and their rationale can be communicated through git commit messages on the rulebook file, making the intent behind each evolution explicit and auditable.

### Rulebook Switching

A rulebook switch is valid if and only if it does not conflict with any currently active bidding group. The unit of concern is **files**, not perspectives. Multorum validates a switch by:

1. Collecting the compiled read and write sets of all currently active bidding groups (as materialized when their workers were created)
2. Compiling the target rulebook's read and write sets per perspective
3. Treating each target perspective as a candidate future bidding group
4. Checking every candidate target perspective against every active bidding group:
   - `write(target) ∩ write(active) = ∅`
   - `write(target) ∩ read(active) = ∅`
   - `read(target) ∩ write(active) = ∅`

If this check passes, the switch is valid regardless of how extensively the rest of the rulebook has changed. Perspectives may be renamed, restructured, or entirely replaced — as long as the files actively being worked on are undisturbed, the switch proceeds.

If the check fails, Multorum rejects the switch and reports which active bidding groups are blocking it. The orchestrator must wait for those groups to complete and merge before retrying.

---

## Project Layout

A Multorum project adds a `.multorum/` directory to the project root. Every worker worktree also has its own `.multorum/` directory because each worktree is a full checkout of the repository plus local runtime files. The orchestrator and each worker therefore have separate `.multorum/` directories with different responsibilities.

```
<project-root>/
  .multorum/
    .gitignore          # committed — ignores Multorum runtime directories
    rulebook.toml        # committed — perspectives, file sets, check pipeline
    orchestrator/        # gitignored — orchestrator control plane and audit data
    worktrees/           # gitignored — git worktrees for managed worker workspaces
  src/
  tests/
  ...
```

### The Committed Region

**`.multorum/rulebook.toml`** and **`.multorum/.gitignore`** are the Multorum files that the project team owns and commits. `rulebook.toml` contains file set definitions, perspective declarations, and project-level check pipeline settings. `.multorum/.gitignore` keeps the runtime-only `orchestrator/` and `worktrees/` directories out of version control while keeping that policy scoped to the `.multorum/` subtree. Their history is available via standard git tooling.

### The Runtime Region

In the main workspace, **`.multorum/orchestrator/`** contains the orchestrator's local control-plane data: the active rulebook commit, worker state projections, integration records, check results, and audit logs. This data is local to the machine and does not travel with the repository.

**`.multorum/worktrees/`** contains one subdirectory per worker workspace, each being a git worktree. Multorum creates these directories on `create`. Finalized workers keep their workspaces until the orchestrator explicitly issues `delete`.

Inside each worker worktree, the worker-local **`.multorum/`** directory contains the runtime contract, the compiled read and write sets, the inbox and outbox mailboxes, and any runtime artifacts attached to messages. These files are authoritative for orchestrator-worker communication, but they are local runtime state rather than project configuration. When the orchestrator or worker submits payloads by filesystem path, Multorum moves them into this runtime area and becomes responsible for retaining them.

### Gitignore

The following entries should be present in `.multorum/.gitignore`:

```
orchestrator/
worktrees/
```

Multorum verifies that these entries are present during `rulebook init` and warns if they are missing. Worker-local runtime files inside each worktree are ignored through that worktree's local exclude configuration rather than through the committed `.multorum/.gitignore`.

---

## Worker Creation

When the orchestrator issues a `create` instruction for a perspective, Multorum creates an isolated working environment for a new worker. This environment is called a *sub-codebase*. Repeated creation from the same perspective creates additional workers in the same bidding group so the orchestrator can compare alternative implementations under one declared scope.

### The Layered View Problem

A worker's environment must simultaneously satisfy two requirements that are in tension:

- **Authoring view** — the worker should operate within a clearly bounded scope, writing only what it has been assigned
- **Execution context** — the LSP, compiler, and test runner need the full codebase to produce meaningful results; type resolution, import graphs, and test suites do not work on partial trees

Multorum addresses this by making the authoring constraint a matter of enforcement rather than visibility. The worker's sub-codebase is a full copy of the codebase, but writes outside the declared write set are rejected.

### Git Worktrees

Each sub-codebase is a git worktree, created from the canonical codebase at the base commit pinned when the rulebook was activated:

```
git worktree add .multorum/worktrees/<worker-id> <pinned-base-commit>
```

All worktrees are created from the same pinned commit. This means every worker starts from an identical snapshot of the codebase, and that snapshot does not change for the lifetime of the worker's task — even if the orchestrator integrates other workers' commits into HEAD in the meantime. Workers in the same bidding group therefore begin from the same world and differ only in how they execute the assignment.

This stability is intentional. Each worker operates on a predictable, immutable world. The orchestrator is responsible for decomposing work such that workers do not depend on each other's in-progress output. If such a dependency exists, the orchestrator should sequence the tasks across separate worker creation steps rather than running them concurrently.

If the orchestrator explicitly reuses a worker id after that worker reaches `MERGED` or `DISCARDED`, Multorum first removes the finalized worker's old git worktree registration and then creates a fresh workspace at the same managed path. Worker-id reuse is therefore a request for a new worker, not a request to reopen old runtime state.

### Worker-Local Runtime

Every worker worktree has its own `.multorum/` directory, distinct from the orchestrator's `.multorum/` directory in the main workspace. Multorum uses this worker-local directory as the worker's runtime control surface.

At worker creation time, Multorum creates the following runtime files inside the worker worktree:

```text
.multorum/
  rulebook.toml      # checked out from the pinned commit
  contract.toml      # runtime — worker id, perspective, pinned base commit
  read-set.txt       # runtime — compiled read set for worker guidance
  write-set.txt      # runtime — compiled write set for enforcement and audit
  inbox/
    new/
    ack/
  outbox/
    new/
    ack/
  artifacts/
```

`contract.toml`, the mailbox directories, and `artifacts/` are runtime-only files. They are not part of the canonical codebase and must never be committed by the worker. Multorum installs local ignore rules in the worktree so these paths remain invisible to normal version-control operations.

Any payload passed by path during mailbox publication is **consumed** rather than copied. On successful publish, Multorum atomically moves the supplied body file into `body.md` and moves each supplied artifact into runtime-managed storage under `.multorum/`. This transfer makes Multorum, not the caller, responsible for retaining the published payload.

### Write Enforcement

Write set enforcement is implemented as a server-side pre-merge check in Multorum's merge pipeline. When a worker submits its commit, Multorum verifies that every changed file is within that worker's compiled write set before allowing merge. This is a hard check that cannot be waived.

A client-side git hook may additionally be installed in the worktree as an early-warning mechanism for the worker, but client-side hooks are not considered authoritative — they can be bypassed. The server-side check is the enforcement point.

### The Read Set as Guidance

A worker's read set is not enforced at the filesystem level. The worker has access to the full codebase in its worktree and may read any file. The read set serves a different purpose: it communicates to the worker which files are the expected sources of information for the task, and guarantees that no other active bidding group will change them during the session. It is a contract of stability and relevance, not a restriction.

This design acknowledges that LLM-based agents often need to navigate the codebase freely to understand context — chasing imports, reading interfaces, understanding patterns. Strictly enforcing the read set would make agents brittle. What matters is controlling what they *write*, not what they *read*.

### New Files

Workers may not create files that were not present in the codebase at worker creation time. The write set, compiled from the rulebook, is a closed list of existing files. If a worker determines that its task cannot be completed without creating a new file, it must report back to the orchestrator rather than creating the file unilaterally. The orchestrator may then update the rulebook to declare the new file, switch to the updated rulebook, and create a fresh worker.

This constraint keeps the compiled file lists authoritative and ensures that every file in the system has an explicit, declared owner.

---

## The Worker State Machine

A worker progresses through a defined set of states during its lifecycle. Multorum enforces valid state transitions and rejects instructions that would produce invalid ones. This state machine is defined per worker; a bidding group is simply a set of workers moving through the same machine independently until one is selected for merge or the group is discarded.

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

### States

- **ACTIVE** — the worktree has been created, the worker's environment is ready, and execution may proceed immediately.
- **BLOCKED** — the worker has reported a blocker and is awaiting orchestrator resolution. The worker is suspended; no execution occurs in this state.
- **COMMITTED** — the worker has completed its task and submitted a commit to Multorum. The worktree is frozen pending orchestrator action: merge, revision, or discard.
- **MERGED** — the worker's commit has passed the pre-merge pipeline and been merged into the canonical codebase. The worktree remains available for inspection until the orchestrator explicitly deletes it.
- **DISCARDED** — the worker has been finalized without merge. The worktree remains available for inspection until the orchestrator explicitly deletes it.

Merging one worker is also a group-level decision: once a worker in a bidding group reaches `MERGED`, every sibling worker in that bidding group becomes `DISCARDED`.

### Valid Transitions

| From | To | Trigger |
|---|---|---|
| create | ACTIVE | Multorum creates the worktree and runtime surface |
| ACTIVE | BLOCKED | Worker issues `report` |
| ACTIVE | COMMITTED | Worker submits commit |
| BLOCKED | ACTIVE | Orchestrator issues `resolve` |
| COMMITTED | ACTIVE | Orchestrator issues `revise`; worker resumes to address problems |
| COMMITTED | MERGED | Orchestrator issues `merge`; pre-merge checks pass |
| COMMITTED | DISCARDED | Orchestrator issues `discard` |
| ACTIVE | DISCARDED | Orchestrator issues `discard` |

`delete` is not a lifecycle transition. It removes the git worktree for a worker that is already in `MERGED` or `DISCARDED`.

---

## The Mailbox Protocol

All orchestrator-to-worker and worker-to-orchestrator communication in Multorum is file-based. Multorum does not require sockets, RPC, or a resident broker process. Each active worker worktree exposes two mailbox trees in its own `.multorum/` directory:

- **`inbox/`** — messages authored by the orchestrator and consumed by the worker
- **`outbox/`** — messages authored by the worker and consumed by the orchestrator

This preserves the star topology of the system while taking full advantage of the fact that the orchestrator workspace and each worker worktree have separate `.multorum/` directories. The orchestrator owns the control plane in the main workspace; each worker owns the runtime surface inside its own worktree.

### Message Bundles

Every message is represented as a directory bundle published atomically into a mailbox.

```text
.multorum/outbox/new/0007-report/
  envelope.toml
  body.md
  artifacts/
    test.log
```

`envelope.toml` carries the routing and state-transition metadata that Multorum interprets:

- `protocol` — protocol version
- `worker` — the unique worker identity
- `perspective` — the perspective name
- `kind` — message type, such as `task`, `report`, `resolve`, `revise`, or `commit`
- `sequence` — a monotonic number unique within the mailbox
- `created_at` — creation timestamp
- `in_reply_to` — optional reference to the message being answered
- `head_commit` — optional git commit hash relevant to the message

`body.md` and `artifacts/` are opaque payloads. They may contain natural-language instructions, structured evidence, test output, or any other worker-orchestrator content. Multorum validates the envelope and records the bundle, but it does not interpret the body.

When a payload is supplied by path, publication transfers ownership of that file to Multorum. The runtime moves the file into `.multorum/` bundle storage instead of copying it, so callers must not assume the original path remains populated after a successful publish.

Messages are published by writing the bundle under a temporary name and atomically renaming it into `new/`. This guarantees that readers either see the complete message or do not see it at all.

### Ownership and Acknowledgement

Each mailbox subtree has exactly one writer:

- the orchestrator writes `inbox/new/`
- the worker writes `inbox/ack/`
- the worker writes `outbox/new/`
- the orchestrator writes `outbox/ack/`

The original message bundle is immutable once published. Receipt is recorded by writing a small acknowledgement file with the same sequence number into the corresponding `ack/` directory. This avoids rename races, preserves an audit trail, and keeps concurrent access simple: no directory has more than one writer.

The unique runtime identity is the worker id, not the perspective name. Perspective metadata still travels in the envelope so the orchestrator can reason about role ownership and merge selection, and bidding-group membership is derived from active workers that share one perspective and boundary. Mailbox routing is therefore unambiguous even when several workers share one perspective. The orchestrator may choose that worker id explicitly when creating a worker; if it does not, Multorum assigns a default worker id automatically. Multorum avoids temporal ambiguity by requiring worker creation to start with empty mailboxes and by preserving finalized worker state until the orchestrator either deletes the workspace or reuses that worker id for a fresh worker.

Worker creation may seed the worker inbox with an initial `task` bundle carrying the orchestrator's assignment and any supporting material. This keeps the initial task description in the same transport as later resolutions and revisions.

### Worker Reports

Worker reporting is a first-class primitive and one message kind within the mailbox protocol. It is not a special side channel.

Workers may send `report` bundles for any reason that prevents confident, correct completion of the task. Common categories include:

- **Permission issues** — the task requires creating a new file or writing outside the write set
- **Ambiguity** — the task description is underspecified or a design choice needs explicit judgment
- **Structural issues** — the required change cuts across perspective boundaries or the necessary destination does not exist yet
- **Evidence submission** — the worker wants the orchestrator to review test output or other execution evidence before merge

When Multorum accepts a `report` bundle from the worker outbox, it transitions the worker from ACTIVE to BLOCKED. The orchestrator answers by writing a `resolve` bundle into the worker inbox. When that bundle is acknowledged, the worker returns to ACTIVE and resumes from the preserved worktree state.

### Revision and Submission

The same mailbox protocol is used for post-review feedback and final submission:

- a worker writes a `commit` bundle to the outbox to submit its git commit and any evidence
- the orchestrator writes a `revise` bundle to the inbox to request changes after review or failed checks

`discard`, `merge`, and `delete` are orchestrator-local actions, not content-carrying mailbox messages. This unifies initial task delivery, blocker resolution, revision requests, and final submission into one transport. There is no separate out-of-band channel for task content.

---

## The Pre-Merge Pipeline

Before a worker's commit is merged into the canonical codebase, it must pass the pre-merge pipeline. This pipeline starts with mandatory scope enforcement, then continues through the project's configured validation checks.

### Scope Enforcement: Compiled Write Set (Mandatory)

Multorum always verifies that every file touched by the worker's commit is within that worker's compiled write set. This check cannot be skipped, waived, or overridden by any party. It is the server-side enforcement of the worker's declared scope and, at system level, of the conflict-free invariant between bidding groups.

### Project Validation Checks: Rulebook Pipeline

The project may define a pipeline of additional checks in the rulebook: building, testing, linting, formatting, or any other command. These run in the declared order after the file set check passes. The `[check.command]` table defines the command for each named check in the pipeline. The `[check.policy]` table is optional and only needs to mention checks whose default behavior should be overridden.

```toml
[check]
pipeline = ["fmt", "clippy", "test"]

[check.command]
fmt = "cargo fmt --all"
clippy = "cargo clippy --all"
test = "cargo test --all"

[check.policy]
test = "skippable"
```

### Evidence and Trust Negotiation

Workers may submit evidence alongside their commits to request that specific checks be skipped. This reflects a pragmatic reality: a worker that has already run tests in its worktree has produced evidence that the orchestrator can evaluate, and re-running the same tests in the pipeline may be redundant.

The evidence submission model works as follows:

1. The worker includes structured evidence in its report or commit submission — the check name, the claimed result (pass or fail), and the raw output
2. The orchestrator reviews the evidence and decides whether to trust it
3. If trusted, the orchestrator instructs Multorum to skip that project-defined check for this merge
4. If not trusted, Multorum runs the check regardless

Evidence should carry the actual output of the check, not merely an assertion. This gives the orchestrator — whether human or LLM — enough information to make an informed trust decision rather than accepting the worker's claim blindly.

Failed evidence is valid to submit. A worker may report that tests failed on specific cases and ask the orchestrator to make a judgment call rather than letting the failure block merge.

### Check Policies

Individual checks can be assigned a policy in the rulebook under `[check.policy]`:

- `always` — the check always runs, regardless of any evidence submitted
- `skippable` — the check may be skipped if the orchestrator accepts the worker's evidence

The `[check.policy]` table is optional. Any pipeline check without an explicit policy entry defaults to `always`.

The file set check is always `always` and this cannot be configured.

---

## The Orchestrator Instruction Set

Multorum exposes a set of instructions that the orchestrator may issue. Every state change in Multorum is the result of one of these instructions. Multorum is purely reactive.

Orchestrator-local instructions operate on the main workspace control plane under `.multorum/`. Worker-facing instructions are delivered by writing message bundles into the worker's inbox. Worker-originated instructions are observed by reading message bundles from the worker's outbox.

### Rulebook Instructions

**`rulebook init`**
Initializes the project's `.multorum/` directory. Multorum creates `.multorum/` if it does not already exist, writes the default commented `rulebook.toml` template shown above, prepares `.multorum/.gitignore` so runtime directories stay ignored within that subtree, prepares the local orchestrator runtime directories, and verifies that the recommended ignore entries are present. The instruction must not overwrite an existing `.multorum/rulebook.toml`; if a rulebook already exists, initialization is rejected so project policy is never replaced implicitly.

**`rulebook switch`**
Validates and activates the rulebook at `HEAD`. Multorum resolves the current `HEAD` commit, compiles the rulebook there, treats each target perspective as a candidate future bidding group, and runs the conflict check against all currently active bidding groups. If the check passes, the new rulebook is activated and `HEAD` becomes the pinned base commit for future workers. If it fails, the instruction is rejected and Multorum reports which active bidding groups are blocking the switch.

**`rulebook validate`**
Performs a dry run of the switch validation against `HEAD` without making any changes. Useful for the orchestrator to check whether a switch is currently possible before committing to it.

### Worker Lifecycle Instructions

**`create <perspective-name>`**
Compiles the file sets for the named perspective, derives the candidate bidding group's read and write sets, and checks them against the materialized read and write sets of all active bidding groups. If the worker joins an existing bidding group, Multorum instead checks that the compiled sets are identical to that group's existing boundary. Once the check passes, Multorum creates a git worktree at the pinned base commit, records the worker's bidding-group membership under the orchestrator-supplied worker id or a default auto-assigned one, installs the client-side write hook, materializes the worker-local runtime files in `.multorum/`, and injects the read set as worker guidance metadata. Multorum also prepares empty inbox and outbox mailboxes for the worker and may publish an initial `task` bundle into the worker inbox. Worker creation transitions the worker directly to ACTIVE.

**`resolve <worker-id>`**
Publishes a `resolve` bundle into the worker's inbox. The bundle carries both the state transition and the resolution content. Once the worker acknowledges it, Multorum transitions the worker from BLOCKED to ACTIVE.

**`revise <worker-id>`**
Publishes a `revise` bundle into the worker's inbox. The bundle carries the required changes. Once the worker acknowledges it, Multorum returns the committed worker to ACTIVE state so it can address the feedback.

**`merge <worker-id>`**
Runs the pre-merge pipeline against the worker's commit. If all checks pass, merges the commit into the canonical codebase and transitions the worker to MERGED. Sibling workers in the same bidding group are then discarded, since only one worker from the group may merge. If any check fails, the instruction is rejected and the worker remains in COMMITTED state pending orchestrator action.

**`discard <worker-id>`**
Finalizes a worker without merging its work. It may be issued while the worker is ACTIVE or COMMITTED. The worker's workspace is preserved for later inspection or explicit deletion.

**`delete <worker-id>`**
Removes the git worktree for a worker that is already in `MERGED` or `DISCARDED`. `delete` does not change lifecycle state; it only tears down the preserved workspace.

### Worker-Facing Instructions

**`commit <worker-id>`**
Issued by the worker by publishing a `commit` bundle into its outbox. The bundle includes the submitted git commit hash and may include evidence artifacts. When Multorum accepts the bundle, it freezes the worktree and transitions the worker from ACTIVE to COMMITTED. The orchestrator then decides whether to `merge`, `revise`, or `discard` the submission.

**`report <worker-id>`**
Issued by the worker by publishing a `report` bundle into its outbox. When Multorum accepts the bundle, it transitions the worker to BLOCKED and records the payload for orchestrator review.

### Query Instructions

**`status`**
Returns the current state of all active workers, their bidding-group membership, the active rulebook commit hash, and a summary of any blocked workers awaiting resolution. This view is derived from the orchestrator control-plane metadata together with the mailbox history, not from a single mutable global state file.
