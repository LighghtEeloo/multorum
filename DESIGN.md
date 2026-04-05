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

Multorum is an infrastructure, NOT an agent. It enforces invariants, materializes worker environments, and records state transitions. All coordination intelligence stays in the orchestrator, and every state transition happens only because the orchestrator or a worker issues an explicit instruction.

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

Either set may be empty (omitted or set to `""`), meaning the perspective claims no files for that role. A perspective with an empty write set cannot modify any files. A perspective with an empty read set places no stability constraints on the rest of the repository.

The write set is a closed list of existing files. Workers may not write to or create files outside it. When a blocked worker discovers that the task really needs a new file, the orchestrator must update the canonical workspace and the rulebook, then forward the blocked candidate group to HEAD before resolving the blocker. The read set declares which files must remain untouched by other concurrent work and tells the worker what the orchestrator considers stable context. Workers may read any file in the repository regardless of the read set.

A worker is a runtime instantiation of a perspective. Perspectives are static policy. Workers are ephemeral executions with state.

### Candidate Groups

A candidate group forms when the orchestrator creates the first worker for a perspective. The group's base commit is HEAD at the moment of creation, and its compiled boundary is the perspective evaluated against that snapshot. Subsequent workers created for the same perspective join the existing group and share its base commit and boundary.

If the orchestrator wants a fresh base for a perspective that already has an active candidate group, the existing group must be fully merged or discarded first, or forwarded to HEAD via `perspective forward`.

Only one worker from a candidate group may be merged. Once one member is merged, the remaining members are discarded.

### Conflict-Free Invariant

The central correctness invariant is:

> **A file may either be written by exactly one active candidate group, or read by any number of active candidate groups, but never both.**

For any two distinct active candidate groups `G` and `H`:

- `write(G) ∩ write(H) = ∅`
- `write(G) ∩ read(H) = ∅`
- `read(G) ∩ write(H) = ∅`

Inside one candidate group, every worker has the same boundary. Conflict detection belongs at the candidate-group level, not at the level of perspective names: perspectives describe policy, candidate groups are the concurrent runtime entities that must not interfere.

The invariant extends to the canonical branch. While any candidate group is active, the union of every active group's read and write sets forms the *orchestrator exclusion set* — files the orchestrator must not commit to until the owning workers are merged or discarded. The orchestrator may commit freely only to files outside the exclusion set.

Multorum enforces the conflict-free invariant at worker creation time. The invariant is a runtime property of active candidate groups, not a static property of the rulebook — the same set of perspectives may or may not conflict depending on which files their globs match in a given repository state.

### Perspective Validation

The orchestrator can check whether a set of perspectives satisfies the conflict-free invariant before creating workers. `perspective validate` compiles the named perspectives from the current rulebook, checks them against each other, and checks them against active candidate groups. With `--no-live`, the check covers only the named perspectives and ignores active groups.

### Perspective Forward

`perspective forward` moves a live candidate group from its current base commit to HEAD, recompiling the perspective boundary from the current rulebook.

The recompiled boundary must be a superset of the group's current materialized boundary, both read and write sets independently. Boundary expansion is permitted. Boundary reduction is rejected, because it would break the contract that live workers were created under.

Before moving any worktree, Multorum validates the whole live candidate group: every live worker must be non-`ACTIVE`, must have a durable replay checkpoint, and must still be clean at that checkpoint. Worktrees are then forwarded one by one. If a later worker fails to forward, Multorum rolls back every worker it already moved and does not persist the new group base or boundary. The atomicity boundary is therefore the persisted runtime state, not each individual Git operation.

Auto-forward applies this same operation from orchestrator actions that already mean "continue this perspective under current HEAD". Multorum may auto-forward only after proving that the whole live candidate group can be forwarded successfully by the normal `perspective forward` rules.

Auto-forward is valid only when it is observationally equivalent to the orchestrator running `perspective forward <perspective>` first and then retrying the original command. When that proof is unavailable, Multorum leaves the group unchanged and tells the user to run `multorum perspective forward <perspective>` explicitly if they still want to move the group.

The rules are:

- it addresses the whole live candidate group for one perspective, never one worker in isolation
- it is rejected unless every live worker in that candidate group is non-`ACTIVE`
- it preserves progress only from a durable checkpoint already recorded for each worker: the latest blocking `report` for `BLOCKED` workers, or the submitted head commit for `COMMITTED` workers
- it rejects dirty or drifted worktrees rather than trying to invent recovery
- it leaves every forwarded worker in its current non-`ACTIVE` state; blocked workers still need `resolve`, and committed workers still need `revise`, `merge`, or `discard`
- every successful auto-forward is announced to the caller

---

## Rulebook

The rulebook lives at `.multorum/rulebook.toml`, committed to version control alongside the codebase it governs. That said, the rulebook is an empharal declaration of perspectives that is not pinned to a specific version, so to some extent it acts more like a convenient shorthand for reasoning about the project's structure and layout, and how harnessed production can be tested and verified.

### File-Set Algebra

Multorum describes ownership boundaries through a small algebra of named file sets, giving the project a stable vocabulary for describing regions of the repository.

#### Syntax

```text
glob       ::= <glob pattern>              e.g. "src/auth/**", "**/*.spec.md"
directory  ::= <literal directory path>    e.g. "third_party/vendor/"
name       ::= <identifier>                e.g. AuthFiles, SpecFiles
expr       ::= name                        reference
             | expr "|" expr               union
             | expr "&" expr               intersection
             | expr "-" expr               difference
             | "(" expr ")"                grouping

definition ::= name ".glob"   "=" glob        primitive - binds a name to a glob
             | name ".opaque" "=" directory   opaque   - binds a name to a directory prefix
             | name "=" expr                  compound - binds a name to an expression
```

`A | B` produces every file in either set. `A & B` keeps only files present in both. `A - B` keeps files in `A` that are not in `B`. Precedence is flat; use parentheses when grouping matters.

File-set names and perspective names use CamelCase. Worker ids use kebab-case.

#### Named Definitions

Names are defined in the `[fileset]` table. A name may bind a primitive glob via `.glob`, an opaque directory via `.opaque`, or a compound expression referencing other names. Perspectives reference these names in their `read` and `write` fields.

```toml
[fileset]
SpecFiles.glob = "**/*.spec.md"
TestFiles.glob = "**/test/**"

AuthFiles.glob = "auth/**"
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

#### Opaque Directories

An opaque definition declares exclusive ownership of a directory subtree. Files under an opaque prefix are invisible to every `.glob` expansion in the same `[fileset]` table — the opaque owner is the sole gateway to those files in the algebra.

```toml
[fileset]
VendorLibs.opaque = "third_party/vendor/"
AllRust.glob = "**/*.rs"
SpecFiles.glob = "**/*.spec.md"
```

`AllRust` matches every `.rs` file outside `third_party/vendor/`. A file like `third_party/vendor/lib.rs` appears only in `VendorLibs`. To reference vendor Rust files in an expression, use `VendorLibs` directly.

The directory path is a literal relative path with no glob metacharacters. The compiler normalizes it to end with `/`. Compound expressions operate on already-resolved sets and are unaffected by opacity: an opaque set can appear in union, intersection, or difference like any other name.

#### Compilation and Validation

File-set expressions are rulebook-level syntax only. When Multorum needs a concrete boundary — at worker creation, perspective validation, or perspective forward — it compiles expressions into concrete file lists by expanding globs against the working tree and evaluating the set operations.

Compilation proceeds in three phases:

1. Resolve opaque definitions: for each `.opaque`, collect every file whose path starts with its directory prefix.
2. Build the reduced file list: the original file list minus all files claimed by opaque definitions.
3. Resolve `.glob` primitives against the reduced file list, then resolve compound expressions from already-resolved sets in topological order.

Compile-time validation checks:

- no cycles in file-set definitions
- no undefined references
- `.opaque` paths must not contain glob metacharacters (`*`, `?`, `[`, `{`)
- no two `.opaque` definitions may have overlapping prefixes (neither may be a prefix of the other)
- empty `.opaque` path (`""` or `"/"`) is rejected
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

`multorum init` creates the sparse committed rulebook template shown below and prepares the empty orchestrator runtime scaffold under `.multorum/orchestrator/` (`group/`, `worker/`, and `exclusion-set.txt`):

```toml
# Define shared file ownership vocabulary first.
# `Name.glob` binds a glob; `Name.opaque` claims a directory exclusively.
# `Name = "Expr"` combines names with |, &, and -.
[fileset]

# Add one table per perspective under `[perspective.<Name>]`.
# `write` names the files that perspective may modify (optional, default empty).
# `read` names stable context files that concurrent work must not write (optional, default empty).
[perspective]

# Add pre-merge gates in execution order.
# Add commands under `[check.command]` and optional skip policies under `[check.policy]`.
[check]
pipeline = []
```

### Writing a Good Rulebook

A rulebook succeeds when perspectives can run concurrently without the orchestrator constantly mediating boundary violations. The goal is a vocabulary of file sets and perspectives that maps naturally onto the work the project actually does, not a bureaucratic overlay that fights it.

#### Build the File-Set Vocabulary First

Start from primitives. Each primitive binds a glob or opaque directory to a name that describes a region of the repository in terms the team already uses: `AuthFiles`, `ApiHandlers`, `MigrationScripts`. Then use compound expressions to carve those regions into subsets that match how work is actually divided: specs versus implementation, tests versus production code.

Good file-set names read like a domain vocabulary. They describe what lives in the region, not how it will be used. `AuthFiles` is better than `AuthWorkerScope` because the same region may appear in multiple perspectives with different roles.

Use `.opaque` for directories that should be treated as indivisible units of ownership — vendored dependencies, generated code trees, or any subtree where a single perspective must have unchallenged authority over every file inside. Opaque directories prevent broad globs like `**/*.rs` from accidentally reaching into regions that belong to a dedicated owner. Use `.glob` for everything else: cross-cutting patterns, extension-based selections, and regions where set operations are expected to subdivide the matched files.

Keep primitive globs specific enough that they do not silently swallow unrelated files as the repository grows. `src/auth/**` is better than `**/*auth*` because the latter will match `docs/auth-migration-plan.md` and anything else that happens to contain the substring.

Order definitions so that opaques and primitives come first and compounds follow, grouped by subsystem. A reader should be able to scan the `[fileset]` table top-to-bottom and understand the repository's ownership map without jumping around.

#### Design Perspectives Around Parallel Work

A perspective is a role, not a task. Name it for the kind of work it authorizes, not for the specific ticket being worked. `AuthImplementor` is a role that can be reused across many tasks. `FixLoginBug` is a one-shot label that tells the next reader nothing about the boundary it controls.

Each perspective declares two things:

- **write**: the closed set of existing files this role may modify. Workers cannot create files outside it. If a task genuinely needs a new file, the orchestrator must create the file and update the rulebook before the worker can proceed. May be omitted or empty for read-only perspectives.
- **read**: the files that must remain stable while this role is active. The read set tells Multorum which files concurrent work must not disturb, and tells the worker what the orchestrator considers stable context. Workers can still read the entire repository regardless. May be omitted or empty when no stability guarantee is needed.

The conflict-free invariant operates at the candidate-group level: for any two distinct active groups, their write sets must be disjoint, and neither may write into the other's read set. Design perspectives so that the ones you intend to run concurrently satisfy this naturally. Two perspectives whose write sets overlap are not actually parallel work, so they must run sequentially.

Keep read sets narrow. Listing every file in the repository as a read dependency blocks all concurrent writes, which defeats the purpose. Include only the files that the worker genuinely depends on as stable context: specs, interfaces, shared types, configuration. The project's own rulebook demonstrates this — perspectives read `ProjectSurfaceFiles` (manifests, docs, entrypoints) rather than the entire tree.

#### Partition Rather Than Overlap

The most useful rulebook pattern is partition: split a subsystem into non-overlapping write sets using set difference. The design document's running example shows this:

```toml
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

[perspective.AuthImplementor]
write = "AuthFiles - AuthSpecs - AuthTests"

[perspective.AuthTester]
write = "AuthTests"
```

`AuthImplementor` writes production auth code. `AuthTester` writes auth tests. Their write sets are disjoint by construction because one subtracts what the other owns. Both read the specs, so the specs stay stable while either role is active.

When perspectives must share awareness of a region without writing to it, put the shared files in the read set of both. When one perspective produces files that another consumes, the consumer reads them and the producer writes them — never both writing.

#### Configure the Check Pipeline for the Project

The check pipeline is the last gate before a worker's commit reaches the canonical codebase. Declare checks in the order they should run. Fast, cheap checks go first — formatting, linting — so expensive ones like full test suites only run on code that already passes basic hygiene.

Mark a check `skippable` only when the orchestrator can reasonably judge from worker-submitted evidence that the check would pass. Full test suites and whole-workspace lints are common candidates: a worker whose changes are confined to one module can submit evidence that the relevant tests pass, and the orchestrator can decide whether to trust it. Format checks are usually not worth skipping because they are fast and deterministic.

The mandatory write-set scope check is not declared in the pipeline. It always runs first and cannot be configured away. The pipeline contains only the project-defined checks that follow it.

Every declared check must appear exactly once in the pipeline, every pipeline entry must have a corresponding command, and no command may be empty. These constraints are enforced at compilation time.

#### Evolve the Rulebook Incrementally

The rulebook is committed to version control and versioned alongside the code it governs. Treat it as living infrastructure, not as a one-time configuration.

When the repository's shape changes — new modules appear, subsystems are reorganized, ownership boundaries shift — update the rulebook to match. Add new file sets for new regions. Adjust perspective boundaries when responsibilities move. Remove file sets and perspectives that no longer correspond to real work.

Multorum has no separate rulebook activation step. Operations that compile policy (`perspective list`, `perspective validate`, `worker create`, and `perspective forward`) read `.multorum/rulebook.toml` from the current working tree when they run. Rulebook edits on disk therefore affect subsequent operations immediately, even before commit. For reproducible orchestration decisions, commit rulebook edits before creating workers. Active workers still run under their pinned snapshots, and their materialized boundaries change only when the orchestrator forwards the candidate group to HEAD.

When expanding a perspective's boundary for a live candidate group, the recompiled boundary must be a superset of the current one. Reduction is rejected because it would break the contract that live workers were created under. If a perspective needs to shrink, finalize its active workers first.

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
    audit/              # committed - append-only merge audit trail
    orchestrator/       # gitignored - orchestrator-local control plane
    tr/                 # gitignored - managed worker worktrees
  src/
  tests/
  ...
```

The project commits `.multorum/rulebook.toml`, `.multorum/.gitignore`, and the contents of `.multorum/audit/`. Everything else under `.multorum/` is runtime state that does not travel with the repository.

`.multorum/.gitignore` contains:

```text
orchestrator/
tr/
```

Multorum verifies these entries during `multorum init` and warns if they are missing.

The runtime directory names are intentionally short. `tr/` keeps managed worktree paths compact, while `group/` and `worker/` keep the orchestrator control plane shallow without forcing unrelated state updates into one monolithic file.

### Orchestrator Runtime Surface

The orchestrator's control plane lives under `.multorum/orchestrator/`, created during `multorum init`:

```text
.multorum/orchestrator/
  group/
    <Perspective>.toml   # one candidate-group record per perspective
  worker/
    <worker>.toml        # one worker record per worker id
  exclusion-set.txt      # materialized orchestrator exclusion set
```

`group/<Perspective>.toml` stores the group-scoped runtime state for one perspective: the perspective name, the pinned base commit, and the compiled boundary (read and write sets as concrete file lists).

`worker/<worker>.toml` stores the worker-scoped runtime state for one worker: the worker id, owning perspective, lifecycle state, managed worktree path, and submitted head commit where applicable.

`multorum init` creates empty `group/` and `worker/` directories. Subsequent operations update them as follows:

- `worker create` forming a new group writes `group/<Perspective>.toml` with perspective, base commit (HEAD), and compiled boundary, then writes the first `worker/<worker>.toml`.
- `worker create` joining an existing group writes only the new `worker/<worker>.toml`.
- `worker merge` marks the chosen worker `MERGED`, marks siblings `DISCARDED`, and clears the boundary in `group/<Perspective>.toml` so the group no longer contributes to the exclusion set.
- `worker discard` marks `worker/<worker>.toml` as `DISCARDED`. If the group has no remaining non-finalized members, the boundary in `group/<Perspective>.toml` is cleared.
- `worker delete` removes `worker/<worker>.toml`. If that was the last worker for the perspective, it also removes `group/<Perspective>.toml`.
- `perspective forward` rewrites `group/<Perspective>.toml` with the new base commit and recompiled boundary, then rewrites each forwarded worker's `read-set.txt` and `write-set.txt` to match.

Workers also update their own entries directly:

- Acknowledging a `task`, `resolve`, or `revise` inbox message updates `worker/<worker>.toml` to `ACTIVE`.
- `local report` updates `worker/<worker>.toml` to `BLOCKED`.
- `local commit` updates `worker/<worker>.toml` to `COMMITTED` and records the submitted head commit.

The orchestrator writes only the terminal states `MERGED` and `DISCARDED`. Once an entry reaches either terminal state, the worker must treat it as read-only.

`exclusion-set.txt` is a flat projection of the persisted group and worker state: the union of all read and write sets from groups that still have live workers. A pre-commit hook in the canonical workspace reads it and rejects commits that touch any listed file. Multorum regenerates it whenever group or worker state changes. When no groups carry a boundary the file is empty.

### Audit Trail

The merge audit trail lives under `.multorum/audit/`, a sibling of `orchestrator/` and `tr/`:

```text
.multorum/audit/
  <audit-entry-id>/
    entry.toml
    body.md
    artifacts/
```

Audit entries are committed project history. They sit outside the `orchestrator/` subtree and travel with the repository.

Each entry is written atomically when `merge` succeeds and contains the worker, perspective, base commit, integrated head commit, the list of changed files, which checks ran or were skipped, and the orchestrator-supplied rationale. The audit entry id format is `<worker>-<head-prefix6>`, where `<head-prefix6>` is the first six characters of the integrated worker head commit. The rationale is a bundle — a `body.md` and optional `artifacts/` — attached by the orchestrator at merge time to explain what the worker accomplished and why the merge was accepted. Multorum writes `entry.toml` and rationale files under the same audit-entry-id directory. Audit entries are append-only; Multorum never modifies or deletes them.

### Git Worktrees

Each worker workspace is a git worktree created from the candidate group's base commit:

```text
git worktree add .multorum/tr/<worker> <base-commit>
```

Workers in the same candidate group share the same base commit, set when the first worker in the group is created. Workers in different candidate groups may have different base commits.

After a worker reaches `MERGED` or `DISCARDED`, its identity may be reused for a new worker. Reuse is always "create a new worker here", not "reopen old state". When reusing an explicit worker id (`--worker <worker>`) and the finalized workspace still exists, `worker create` requires `--overwriting-worktree` to replace that preserved worktree. If the finalized workspace was already deleted, reuse does not require the overwrite flag.

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

All non-terminal state transitions belong to the worker: it writes `BLOCKED` when it issues `local report`, `COMMITTED` when it issues `local commit`, and `ACTIVE` when it `local ack` a `task`, `resolve`, or `revise` inbox message. The orchestrator's part in the resolve and revise arcs is to publish the inbox message; the transition fires only when the worker acknowledges it. The orchestrator writes a worker's state only to finalize: `MERGED` via `worker merge` and `DISCARDED` via `worker discard`. A worker must not update its entry once finalized.

The orchestrator may also issue `hint` while a worker is `ACTIVE`. A hint is advisory rather than transitional: it carries new information or asks the worker to take a follow-up action such as reporting a blocker, but publishing or acknowledging the hint does not change lifecycle state on its own.

`worker create` and `worker resolve` may auto-forward the candidate group before their own execution, but only when the full candidate-group proof described above succeeds. Auto-forward leaves worker lifecycle state unchanged. If the proof fails, Multorum leaves the group untouched and directs the user toward manual `perspective forward`.

For analysis-only tasks that intentionally produce no code diff, workers should still submit through the normal commit/merge path: create an empty commit (for example `git commit --allow-empty`), then publish it with `local commit` and attach evidence in `body.md` and optional artifacts. The orchestrator can merge that submission normally, preserving a reviewable audit trail and an explicit lifecycle completion.

Once one worker in a candidate group reaches `MERGED`, every sibling in that group becomes `DISCARDED`.

`delete` is not a lifecycle transition. It removes the worktree and the worker's state file. If that worker was the last member of its perspective, it also removes the group's state file.

`perspective forward` is also not a lifecycle transition. It repins a candidate group whose live workers are all non-`ACTIVE` to HEAD while leaving worker states unchanged.

### Transitions

| From       | To        | Trigger                                     |
| ---------- | --------- | ------------------------------------------- |
| *(create)* | ACTIVE    | worktree and runtime surface materialized   |
| ACTIVE     | BLOCKED   | worker issues `report`                      |
| ACTIVE     | COMMITTED | worker issues `commit`                      |
| ACTIVE     | DISCARDED | orchestrator issues `discard`               |
| ACTIVE     | ACTIVE    | orchestrator publishes `hint`               |
| BLOCKED    | ACTIVE    | worker acknowledges `resolve`               |
| BLOCKED    | DISCARDED | orchestrator issues `discard`               |
| COMMITTED  | ACTIVE    | worker acknowledges `revise`                |
| COMMITTED  | MERGED    | orchestrator issues `merge` and checks pass |
| COMMITTED  | DISCARDED | orchestrator issues `discard`               |

---

## Mailbox Protocol

All orchestrator-worker communication is file-based. Each worker exposes two mailbox trees in its `.multorum/` directory:

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

An intentionally empty worker commit is valid: it touches no files, so scope enforcement passes with an empty changed-file set. This supports analysis-only merges that carry evidence in bundle content rather than code diffs.

Client-side hooks in worker worktrees serve as early warnings only; scope enforcement at merge time is authoritative.

### Project Checks

After scope enforcement passes, Multorum runs the checks declared in `[check.pipeline]` in order. These may be builds, tests, linters, format checks, or any other command.

### Evidence

Workers may submit evidence with their reports or commits to support the case for merging or to ask the orchestrator to skip `skippable` checks. Evidence should include actual output or analysis, not just a claim — failed evidence is still valid when the worker wants the orchestrator to make a judgment call. Multorum carries evidence but does not judge it; the orchestrator decides whether to trust it or not.

### Audit

After a successful merge, Multorum writes an audit entry to `.multorum/audit/<worker>-<head-prefix6>/entry.toml`. The entry records the worker, perspective, base commit, integrated head commit, changed files, checks ran, checks skipped, and the orchestrator's rationale. The rationale is a bundle attached to the `merge` command via exactly one of `--body-text` or `--body-path`, plus any `--artifact` flags. Multorum writes rationale files under `.multorum/audit/<worker>-<head-prefix6>/` (see [Bundles](#bundles)).

Audit rationale should be self-contained. Record actual findings in the audit bundle body and artifacts rather than references to worker outbox paths, because worker worktrees and outboxes are runtime state and may be deleted after merge confirmation.

---

## MCP Surface

Multorum exposes the runtime model over the Model Context Protocol as a transport projection, not as a separate source of truth. The filesystem-backed runtime remains canonical.

High-level role guidance is shipped with the binary itself. The CLI prints that guidance through `multorum util methodology <role>`, and each MCP server exposes the same Markdown through a role-specific `methodology` resource. Repository-local skill files may exist as thin wrappers, but they are not a second documentation source.

### Server Modes

The MCP surface is split into two stdio servers:

- orchestrator mode
- worker mode

Each mode exposes only the tools and resources that make sense for that runtime role.

Both servers default to the process working directory at startup. If `cwd` is a valid workspace or worktree, the runtime is immediately available. Otherwise, the startup failure is deferred until the first tool or resource call. The `set_working_directory` tool allows the client to rebind the runtime to a different directory at any time.

### Tools

MCP tools mirror the explicit runtime instructions. Their arguments are typed in the protocol schema so hosts can validate and render them correctly:

- strings for identifiers, paths, and commit references
- integers for mailbox sequence numbers
- booleans for explicit flags
- arrays of strings for repeated path or check arguments

Tool results are JSON payloads. Runtime failures remain tool-level failures rather than protocol transport failures.

### Resources

MCP resources expose read-only projections of runtime state.

Most resources return JSON snapshots. The role methodology resources return Markdown because they are advisory operating guides meant for direct agent or human consumption.

#### Orchestrator-mode resources

Concrete:

| URI                                    | Description                                                          |
| -------------------------------------- | -------------------------------------------------------------------- |
| `multorum://orchestrator/methodology`  | High-level orchestrator operating methodology shipped with Multorum. |
| `multorum://orchestrator/status`       | Full orchestrator snapshot: active perspectives and workers.         |
| `multorum://orchestrator/perspectives` | Compiled perspective summaries from the current rulebook.            |
| `multorum://orchestrator/workers`      | Worker summary listing for the current runtime.                      |

Templates:

| URI template                                      | Description                                    |
| ------------------------------------------------- | ---------------------------------------------- |
| `multorum://orchestrator/workers/{worker}`        | Detailed orchestrator-side view of one worker. |
| `multorum://orchestrator/workers/{worker}/outbox` | Outbox mailbox listing for one worker.         |

#### Worker-mode resources

Concrete:

| URI                             | Description                                                    |
| ------------------------------- | -------------------------------------------------------------- |
| `multorum://worker/methodology` | High-level worker operating methodology shipped with Multorum. |
| `multorum://worker/contract`    | Immutable worker contract for the active perspective.          |
| `multorum://worker/inbox`       | Inbox mailbox listing for the active worker.                   |
| `multorum://worker/status`      | Projected worker lifecycle status.                             |

Worker-mode resources carry no worker identity parameter because the server is bound to a single worker worktree via `set_working_directory` — the identity is implicit.

### Error Contract

MCP-visible error codes are stable protocol values, independent of Rust enum variant names. Tool-level failures and resource-read failures should preserve the underlying domain category where possible, for example distinguishing invalid parameters from missing runtime objects.

---

## Instruction Reference

This section lists the instructions that the orchestrator and workers may issue, in the form of CLI commands. MCP tools mirror the same runtime operations with typed arguments.

### Initialization

- `multorum init` — Initialize `.multorum/`, write the default committed artifacts if absent, prepare `.multorum/.gitignore`, create orchestrator runtime directories, and install the shared pre-commit hook when the repository backend is already available.

### Perspective

- `multorum perspective list` — List perspectives from the current rulebook.
- `multorum perspective validate <perspectives>...` — Compile the named perspectives from the current rulebook, check conflict-freedom between them, and check them against active candidate groups. With `--no-live`, check only the named perspectives against each other.
- `multorum perspective forward <perspective>` — Move the whole live candidate group for `perspective` to HEAD. Recompile the perspective boundary from the current rulebook. Rejected unless every live worker in that candidate group is non-`ACTIVE` and the recompiled boundary is a superset of the current materialized boundary. Progress is preserved only from durable checkpoints already recorded for each worker: the latest blocking `report` for `BLOCKED` workers, or the submitted head commit for `COMMITTED` workers. No lifecycle transition.

### Orchestrator Worker Commands

Every bundle-publishing instruction requires exactly one body source: `--body-text` or `--body-path`. Artifacts remain optional.

- `multorum worker create <perspective> [--worker <worker>] [--overwriting-worktree] [--no-auto-forward] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — Compile the perspective boundary from the current rulebook against the working tree. If a candidate group for this perspective already exists, join it. Otherwise, form a new group with base commit set to HEAD and check conflict-freedom against all active candidate groups. Before creating the worker, Multorum may auto-forward the existing live candidate group for the same perspective when the full forward proof succeeds; `--no-auto-forward` disables that convenience and leaves the forward manual. Create the worker worktree and materialize the runtime surface, always creating the initial `task` inbox bundle; the required body populates that bundle's primary content and optional artifacts add supporting files. `--worker` sets an explicit worker identity; when omitted, Multorum derives one from the perspective name. Reusing an explicit worker id is allowed only after that worker is finalized; if its finalized worktree still exists, pass `--overwriting-worktree` to replace it. Transition: new worker enters `ACTIVE`.
- `multorum worker list` — List active workers.
- `multorum worker show <worker>` — Return one worker in detail.
- `multorum worker outbox <worker> [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — List messages sent by a worker to the orchestrator. `--from`/`--to` define an inclusive range; `--exact` selects one message by sequence number (mutually exclusive with range). No lifecycle transition.
- `multorum worker inbox <worker> [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — List messages sent by the orchestrator to a worker. Same filtering semantics as `outbox`. No lifecycle transition.
- `multorum worker ack <worker> <sequence>` — Record orchestrator receipt for one worker outbox bundle. No lifecycle transition.
- `multorum worker hint <worker> [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — Publish a `hint` bundle to an active worker inbox. `--reply-to` correlates the hint with an earlier outbox sequence number. The required body carries new project information or asks the worker to stop gracefully by issuing `report`. No lifecycle transition.
- `multorum worker resolve <worker> [--no-auto-forward] [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — Publish a `resolve` bundle to a blocked worker inbox. `--reply-to` correlates the resolve with an earlier outbox sequence number. Before publishing the bundle, Multorum may auto-forward the worker's live candidate group when the full forward proof succeeds; `--no-auto-forward` disables that convenience and leaves the forward manual. The required body carries resolution context for the worker. The worker returns to `ACTIVE` when it acknowledges that inbox message.
- `multorum worker revise <worker> [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — Publish a `revise` bundle to a committed worker inbox. `--reply-to` correlates the revision with an earlier outbox sequence number. The required body carries revision context for the worker. The worker returns to `ACTIVE` when it acknowledges that inbox message.
- `multorum worker merge <worker> [--skip-check <check>]... (--body-text <text> | --body-path <file>) [--artifact <file>]...` — Verify the submitted head commit, enforce the write set, run the merge pipeline, and integrate the worker if checks pass. The required body attaches an audit rationale; this rationale should contain self-contained findings instead of references to worker outbox paths. Transition: `COMMITTED` to `MERGED`.
- `multorum worker discard <worker>` — Finalize a worker without integration. Allowed from `ACTIVE`, `BLOCKED`, or `COMMITTED`. Transition: worker enters `DISCARDED`. The workspace remains until deleted.
- `multorum worker delete <worker>` — Delete the worktree and remove `worker/<worker>.toml`. If the worker is the last member of its candidate group, also remove `group/<Perspective>.toml`. Allowed only from `MERGED` or `DISCARDED`.

### Worker-Local Commands

- `multorum local contract` — Load the worker contract for the current worktree.
- `multorum local status` — Return the projected status for the current worktree.
- `multorum local inbox [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — List messages sent by the orchestrator to this worker. `--from`/`--to` define an inclusive range; `--exact` selects one message (mutually exclusive with range). No lifecycle transition.
- `multorum local outbox [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — List messages sent by this worker to the orchestrator. Same filtering semantics as `inbox`. No lifecycle transition.
- `multorum local ack <sequence>` — Acknowledge one inbox message. Acknowledging `task`, `resolve`, or `revise` transitions the worker into `ACTIVE`.
- `multorum local report [--head-commit <commit>] [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — Publish a blocker report from the current worktree. `--reply-to` correlates the report with an earlier inbox sequence number. The required body carries blocker details and optional artifacts carry evidence. Transition: `ACTIVE` to `BLOCKED`.
- `multorum local commit --head-commit <commit> (--body-text <text> | --body-path <file>) [--artifact <file>]...` — Publish a completed worker submission from the current worktree. The required body carries submission evidence or conclusions. For analysis-only outcomes with no code diff, submit an intentionally empty commit (`git commit --allow-empty`) and publish that `head_commit`. Transition: `ACTIVE` to `COMMITTED`.

### Query

- `multorum status` — Return the full orchestrator status snapshot, including active workers and candidate-group membership.

### Utility

- `multorum util methodology orchestrator` — Print the high-level orchestrator methodology as Markdown. This command is self-contained and does not require a managed repository.
- `multorum util methodology worker` — Print the high-level worker methodology as Markdown. This command is self-contained and does not require a managed repository.
- `multorum util completion <shell>` — Emit shell completions to stdout. Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

After running the command, source the output in your shell profile to enable tab completion.

```bash
# bash
command -v multorum &>/dev/null && source <(multorum util completion bash)

# zsh
autoload -U compinit
compinit
command -v multorum &>/dev/null && source <(multorum util completion zsh)

# fish
command -v multorum &>/dev/null && multorum util completion fish | source

# elvish
command -v multorum &>/dev/null && source <(multorum util completion elvish)

# powershell
multorum util completion powershell | Out-String | Invoke-Expression
```

### MCP Server

- `multorum serve orchestrator` — Start the orchestrator MCP server on stdio. Defaults to the process working directory; the client may call `set_working_directory` to rebind.
- `multorum serve worker` — Start the worker MCP server on stdio. Defaults to the process working directory; the client may call `set_working_directory` to rebind.
