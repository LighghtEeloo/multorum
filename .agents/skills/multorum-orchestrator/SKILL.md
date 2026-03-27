---
name: "multorum-orchestrator"
description: "Coordinate a Multorum session from the canonical workspace. Use when acting as the orchestrator and must edit the rulebook, create or inspect workers, answer blockers, request revisions, discard work, delete finalized worktrees, or merge submissions through the Multorum CLI or the orchestrator MCP surface."
---

# Multorum Orchestrator

Coordinate the system from the canonical workspace root. Treat Multorum as reactive infrastructure: it enforces declared boundaries and state transitions, but it never decomposes work, decides trust, or invents recovery steps for you.

## Hold The Core Invariants

- Keep the topology star-shaped. Communicate with workers only through Multorum; never route work between workers directly. Workers do not communicate with each other. If a worker requests cross-worker coordination, decline and handle the dependency through orchestrator decisions and mailbox flows.
- Decompose tasks so active workers do not depend on each other's unpublished output.
- Respect the safety property at the bidding-group level: a file may be written by exactly one active bidding group or read by many, never both.
- Treat the read set as a stability contract and the write set as an absolute ownership boundary.
- Treat new files, missing permissions, and cross-perspective edits as orchestrator work. Update the canonical workspace and rulebook, then decide whether the blocked bidding group should be forwarded or discarded.
- Remember that workers are addressed by `worker`, not by perspective name. Multiple workers from the same perspective form one bidding group, and at most one of them may merge.

## Compose And Update The Rulebook

The orchestrator owns `.multorum/rulebook.toml` and edits it directly in the canonical workspace. In the current implementation, `multorum rulebook init` bootstraps the committed files and runtime directories, and the live runtime operations consult the rulebook that is currently on disk.

Use this flow:

1. Edit `.multorum/rulebook.toml` in the canonical workspace.
2. Create any canonical files the new boundary refers to.
3. Run `multorum perspective validate ...` when concurrency assumptions matter.
4. Create workers or forward blocked bidding groups using the updated rulebook.
5. Commit the rulebook change when you want repository history to record the new ownership model.

Rulebook edits do not rewrite live worker contracts by themselves. New bidding groups compile from the current rulebook at creation time. Existing blocked bidding groups pick up updated boundaries only when the orchestrator runs `multorum perspective forward <perspective>`, which recompiles that perspective against the current workspace and moves the group's pinned base to `HEAD`.

### Build The File-Set Vocabulary

The `[fileset]` table defines the project's ownership vocabulary. Start with primitives that bind globs to names describing repository regions, then compose compounds from those names using set algebra.

Primitives use `.path` to bind a glob:

```toml
[fileset]
AuthFiles.path = "auth/**"
SpecFiles.path = "**/*.spec.md"
TestFiles.path = "**/test/**"
```

Compounds combine names with `|` (union), `&` (intersection), and `-` (difference):

```toml
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"
```

Name file sets for what lives in the region, not how they will be used. `AuthFiles` is better than `AuthWorkerScope` because the same region may appear in multiple perspectives with different roles.

Keep globs specific. `src/auth/**` is better than `**/*auth*` — the latter will silently match unrelated files like `docs/auth-migration-plan.md` as the repository grows.

Order definitions with primitives first, compounds after, grouped by subsystem. A reader should be able to scan the table top-to-bottom and understand the ownership map without jumping around. Use TOML comments to label groups:

```toml
[fileset]
# Cross-cutting project surface.
CargoToml.path = "Cargo.toml"
Readme.path = "README.md"
DesignDoc.path = "DESIGN.md"
ProjectSurface = "CargoToml | Readme | DesignDoc"

# Auth subsystem.
AuthFiles.path = "src/auth/**"
AuthTests.path = "tests/auth/**"
```

### Design Perspectives For Concurrent Work

A perspective is a reusable role, not a one-shot task label. Name it for the kind of work it authorizes. `AuthImplementor` can be reused across many tasks. `FixLoginBug` tells the next reader nothing about the boundary it controls.

Each perspective declares:

- `write` — the closed set of existing files this role may modify. Workers cannot create files outside it.
- `read` — the files that must remain stable while this role is active. Not a visibility filter; workers can still read the entire repository.

Design perspectives so the ones you intend to run concurrently have disjoint write sets and do not write into each other's read sets. If two perspectives cannot run at the same time because their write sets overlap, they are sequential work — do not pretend otherwise.

The most useful pattern is partition by set difference:

```toml
[perspective.AuthImplementor]
read  = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"

[perspective.AuthTester]
read  = "AuthSpecs"
write = "AuthTests"
```

`AuthImplementor` writes production code, `AuthTester` writes tests, and their write sets are disjoint by construction. Both read the specs, so the specs stay stable while either role is active.

Keep read sets narrow. Listing every file as a read dependency blocks all concurrent writes, which defeats the purpose. Include only the files the worker genuinely depends on as stable context: specs, interfaces, shared types, manifests. The project's own rulebook demonstrates this — perspectives read `ProjectSurfaceFiles` (manifests, docs, entrypoints) rather than the entire tree.

When one perspective produces files that another consumes, the consumer reads them and the producer writes them. Never both writing.

Before creating workers, validate the perspectives you plan to run concurrently:

```bash
multorum perspective validate AuthImplementor AuthTester
```

### Configure The Check Pipeline

Declare checks in the order they should run. Put fast, cheap checks first so expensive ones only run on code that already passes basic hygiene:

```toml
[check]
pipeline = ["fmt", "clippy", "test"]

[check.command]
fmt = "cargo fmt --all --check"
clippy = "cargo clippy --workspace --all-targets -- -D warnings"
test = "cargo test --workspace"

[check.policy]
clippy = "skippable"
test = "skippable"
```

Mark a check `skippable` only when worker-submitted evidence can reasonably justify skipping it. Full test suites and whole-workspace lints are common candidates. Format checks are usually not worth skipping — they are fast and deterministic.

The mandatory write-set scope check is not declared in the pipeline. It always runs first and cannot be configured away.

Every declared check must appear exactly once in the pipeline, every pipeline entry must have a corresponding command, and no command may be empty. Use `multorum perspective validate ...` to check that the perspectives you intend to run together still satisfy the conflict-free invariant after a rulebook edit.

### Update The Rulebook For Live Sessions

When the repository's shape changes or a worker reports a boundary blocker, update the rulebook to match. Common update scenarios:

**Adding a file to an existing perspective.** If the file is new, create it in the canonical workspace. Then either widen the perspective's glob to include it or add a new file set and reference it in the perspective's write expression. Validate the updated perspective map, then forward the blocked bidding group if workers are live and need the new boundary.

**Adding a new perspective.** Define any new file sets it needs, then add the `[perspective.<Name>]` table. Validate against existing active perspectives before creating workers:

```bash
multorum perspective validate ExistingPerspective NewPerspective
```

**Narrowing a perspective.** Reduction is rejected while a bidding group is live because it would break the contract workers were created under. Finalize active workers first (discard or merge), then update the rulebook and validate the new concurrency shape before creating fresh workers.

**Removing a perspective.** Finalize all workers from that perspective, then remove its table from the rulebook and commit the cleanup when ready.

Always validate the perspectives you care about after a rulebook edit and before creating or forwarding workers. When live workers exist, treat `multorum perspective validate ...` as the preflight check for whether the intended concurrent shape is still legal.

## Treat The Filesystem Runtime As Canonical

The shipped runtime is filesystem-backed. `.multorum/` under the workspace root is the source of truth, and both the CLI and MCP surfaces project that same state.

Some MCP projections remain intentionally unimplemented. Do not assume a resource exists just because the design document describes the concept.

## Prefer The Exposed Surfaces

Use the CLI as the default surface for canonical-workspace work: editing the rulebook, validating perspective combinations, forwarding perspectives, inspecting Git state, and reading `.multorum/` directly. Those operations already live in the shell and filesystem, so the CLI keeps the workflow explicit and grounded in the canonical runtime.

Use orchestrator MCP when it materially helps with typed worker-management calls or read-only runtime snapshots. Treat MCP as a transport projection over the filesystem-backed runtime, not as a separate control plane.

If an MCP host reports an unmanaged project for `/` or another unexpected root, or otherwise appears to be bound outside the canonical workspace, stop trying to force MCP through that host and fall back to the CLI.

When you publish a bundle with a body path or artifact path, treat those paths as transferred ownership. Successful publication moves the files into Multorum-managed `.multorum/` storage instead of copying them.

Start the orchestrator MCP server from the workspace root with:

```bash
multorum serve orchestrator
```

### Orchestrator MCP tools

- `rulebook_init`
- `list_perspectives`
- `list_workers`
- `get_worker`
- `read_worker_outbox`
- `ack_worker_outbox_message`
- `create_worker`
- `forward_perspective`
- `hint_worker`
- `resolve_worker`
- `revise_worker`
- `discard_worker`
- `delete_worker`
- `merge_worker`
- `get_status`

### Orchestrator MCP resources

- `multorum://orchestrator/status`
- `multorum://orchestrator/perspectives`
- `multorum://orchestrator/workers`
- `multorum://orchestrator/workers/{worker}`
- `multorum://orchestrator/workers/{worker}/outbox`

The transport reserves worker sub-resources such as `/contract`, `/transcript`, and `/checks`, but they are not implemented yet and should not be treated as available.

### CLI commands

```bash
multorum rulebook init
multorum perspective list
multorum perspective validate <perspective>...
multorum perspective forward <perspective>
multorum worker create <perspective> [--worker <worker>] [--overwriting-worktree] [--body-text <text> | --body-path <file>] [--artifact FILE ...]
multorum worker list
multorum worker show <worker>
multorum worker outbox <worker> [--after <sequence>]
multorum worker ack <worker> <sequence>
multorum worker hint <worker> [--reply-to <sequence>] [--body-text <text> | --body-path <file>] [--artifact FILE ...]
multorum worker resolve <worker> [--reply-to <sequence>] [--body-text <text> | --body-path <file>] [--artifact FILE ...]
multorum worker revise <worker> [--reply-to <sequence>] [--body-text <text> | --body-path <file>] [--artifact FILE ...]
multorum worker discard <worker>
multorum worker delete <worker>
multorum worker merge <worker> [--skip-check <check> ...]
multorum status
```

## Run The Session Deliberately

1. Inspect current state with `get_status` or `multorum status`.
2. Validate the perspective combinations you intend to run whenever a rulebook edit or live workers make conflicts possible.
3. Enumerate perspectives before assigning work so the task matches an existing ownership boundary.
4. Create one worker per perspective by default. Create multiple workers from the same perspective only when you intentionally want a bidding group evaluating the same boundary from the same pinned snapshot.
5. Attach an initial task bundle when the worker needs nontrivial instructions or evidence files.
6. Read worker outbox traffic, acknowledge each consumed bundle, and review the worker detail plus any attached artifacts before deciding whether to resolve, revise, discard, delete, or merge.
7. When a blocked perspective needs a new file or newer pinned base, update the canonical workspace and current rulebook, then use `multorum perspective forward <perspective>` only if every live worker in that bidding group is `BLOCKED`.
8. Merge only from `COMMITTED`, and skip checks only when the rulebook marks them `skippable` and the worker submitted evidence you trust.

## Write Better Worker Tasks

Each initial task or follow-up bundle should state:

- the exact objective
- the files or file region the worker is expected to change
- the acceptance checks the worker should run or attach as evidence
- the situations that require an immediate `report` instead of improvisation

Do not ask a worker to create new files unless the active rulebook already declares them. When a blocked worker reports that a new file is needed, update the canonical workspace and current rulebook first, then forward that perspective only if the whole live bidding group is blocked. Do not rely on "figure out the right place" when the change may cross perspective boundaries.
When attaching a task body, evidence log, or other artifact by path, do not plan to reuse the original path after publication unless you created a separate copy yourself.

## Resolve, Revise, Merge, And Delete Correctly

- Use `read_worker_outbox` or `multorum worker outbox` to inspect worker-authored `report` and `commit` bundles before taking follow-up action.
- Treat `body.md` as the primary human summary for `report` and `commit` bundles. If a worker submits an empty body without a clear reason, treat the submission as incomplete and revise or resolve accordingly.
- Acknowledge each consumed worker bundle with `ack_worker_outbox_message` or `multorum worker ack`. This records orchestrator receipt only; it does not change the worker lifecycle state.
- Use `forward_perspective` only for a live bidding group whose workers are all `BLOCKED`. Forwarding preserves progress only from the `head_commit` recorded in each worker's latest blocking report, rejects dirty or drifted worktrees, and leaves the workers blocked until you send `resolve`.
- Use `resolve` only for a blocked worker. Answer the blocker directly and include `--reply-to` or the MCP reply reference when responding to a specific report.
- Use `revise` only for a committed worker. State what changed in your evaluation, what must be corrected, and what evidence should accompany the next submission.
- Use `discard` when the task should be abandoned rather than repaired.
- Use `delete` only after a worker is already `MERGED` or `DISCARDED` and you no longer need its preserved worktree.
- Use `merge` only after reviewing the worker's summary, evidence, and affected files.
- Never skip the file-set check. It is mandatory by design.

## Example Command Shapes

```bash
multorum perspective validate AuthImplementor AuthTester
multorum worker create AuthImplementor --body-path task.md --artifact spec.md
multorum worker outbox auth-implementor-1 --after 6
multorum worker ack auth-implementor-1 7
multorum perspective forward AuthImplementor
multorum worker resolve auth-implementor-1 --reply-to 7 --body-path resolve.md
multorum worker revise auth-implementor-1 --reply-to 12 --body-path revise.md --artifact failing-test.log
multorum worker merge auth-implementor-1 --skip-check test
multorum worker delete auth-implementor-1
```

Use `--skip-check` only for checks that the rulebook marks as skippable and only after deciding the worker's evidence is trustworthy.

## Handle Boundary And Lifecycle Situations

### Blocked Worker Needs A New File Or Expanded Boundary

Workers cannot create new files or edit outside their write set. When a worker reports this kind of blocker, the orchestrator must act:

1. Edit `.multorum/rulebook.toml` to add the file to the perspective's write set (and create the file in the canonical workspace if it does not yet exist).
2. Validate the perspectives that matter for the session.
3. Commit the rulebook and file changes when you want the repository history updated.
4. Run `multorum perspective forward <perspective>` for the whole bidding group (not just one worker). Every live worker in that bidding group must be `BLOCKED` for forwarding to succeed.
5. Run `multorum worker resolve <worker>` to unblock the worker.

Never tell a worker to create files ad hoc in its worktree or to patch files outside its write set. If the real fix is in another perspective's territory, either adjust the rulebook, create a worker from the correct perspective, or re-scope the task.

### Forwarding A Bidding Group To A Newer Base

Updating `.multorum/rulebook.toml` changes what future creates and forwards consult, but it does not itself move a live bidding group's pinned code snapshot. To bring workers onto the newer base:

- Every live worker in the bidding group must be `BLOCKED`.
- Run `multorum perspective forward <perspective>`. This applies to the whole bidding group, not individual workers.
- Forwarding preserves each worker's progress from the `head_commit` recorded in their latest blocking report.
- If a worker's blocking report lacks `head_commit`, forwarding is rejected. Resolve the worker with a message asking for a new report that includes the committed `head_commit`, then retry the forward.

You must forward an existing live bidding group before treating it as though it now owns the newer boundary and base. Do not assume that editing the rulebook alone has updated the worker contract already in flight.

### Bidding Group Completion

Only one worker from a bidding group may be merged. After merging one, discard the remaining workers in that group. Do not merge multiple alternatives from the same bidding group.

### Discard Versus Delete

- `discard` finalizes a worker without merging and preserves the worktree for inspection.
- `delete` removes both the worker state file and the Git worktree.
- Delete is only valid after a worker reaches `MERGED` or `DISCARDED`.
- Use discard first if you want to inspect the worktree, then delete when cleanup is desired.

### Reusing A Finalized Worker Id

To create a new worker reusing an id that belonged to a finalized worker, pass `--overwriting-worktree`:

```bash
multorum worker create <perspective> --worker <worker> --overwriting-worktree
```

The old finalized state does not carry over to the new worker.

### Committing Directly In The Canonical Workspace

While workers are active, the orchestrator must respect the exclusion set formed by active workers' read and write sets. A file inside an active group's read set must remain stable while that group is active. Do not commit changes to those files directly. Either wait, discard conflicting workers, or evolve the rulebook and worker snapshots through the supported flow (edit rulebook, validate, forward).

### Revise Versus Resolve

- `resolve` is for `BLOCKED` workers. It publishes a `resolve` inbox message; the worker transitions to `ACTIVE` when it acknowledges that message.
- `revise` is for `COMMITTED` workers. It publishes a `revise` inbox message; the worker transitions to `ACTIVE` when it acknowledges that message.
- Do not use `resolve` on a committed worker or `revise` on a blocked worker.

### Merge Is Based On The Submitted Commit

Merge uses the `head_commit` the worker submitted through `multorum local commit --head-commit <commit>`, not the ambient worktree state. If the worktree is dirty but the submitted commit is correct, the merge candidate is still the submitted commit. If the submitted commit is not the desired result, use `revise` rather than guessing from the worktree.

### Skipping Merge Checks

- Only project-defined checks marked `skippable` in the rulebook may be skipped via `--skip-check`.
- The mandatory write-set scope check is never skippable.
- The orchestrator decides whether a worker's submitted evidence is sufficient to justify skipping.
- Workers cannot skip checks unilaterally.
