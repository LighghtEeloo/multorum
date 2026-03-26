---
name: "multorum-orchestrator"
description: "Coordinate a Multorum session from the canonical workspace. Use when Codex is acting as the orchestrator and must manage rulebooks, create or inspect workers, answer blockers, request revisions, discard work, delete finalized worktrees, or merge submissions through the Multorum CLI or the orchestrator MCP surface."
---

# Multorum Orchestrator

Coordinate the system from the canonical workspace root. Treat Multorum as reactive infrastructure: it enforces declared boundaries and state transitions, but it never decomposes work, decides trust, or invents recovery steps for you.

## Hold The Core Invariants

- Keep the topology star-shaped. Communicate with workers only through Multorum; never route work between workers directly. Workers do not communicate with each other. If a worker requests cross-worker coordination, decline and handle the dependency through orchestrator decisions and mailbox flows.
- Decompose tasks so active workers do not depend on each other's unpublished output.
- Respect the safety property at the bidding-group level: a file may be written by exactly one active bidding group or read by many, never both.
- Treat the read set as a stability contract and the write set as an absolute ownership boundary.
- Treat new files, missing permissions, and cross-perspective edits as orchestrator work. Update the canonical workspace and rulebook, install the rulebook, then decide whether the blocked bidding group should be forwarded or discarded.
- Remember that workers are addressed by `worker`, not by perspective name. Multiple workers from the same perspective form one bidding group, and at most one of them may merge.

## Edit The Rulebook Directly

The orchestrator owns `.multorum/rulebook.toml` and should edit it directly in the canonical workspace. This is the normal way to add files to perspectives, define new perspectives, adjust boundaries, or configure checks. The workflow is always:

1. Edit `.multorum/rulebook.toml` in the canonical workspace.
2. Commit the change (along with any new source files the rulebook references).
3. Run `multorum rulebook install` to activate the committed rulebook.

Editing the file on disk alone does nothing. The rulebook is activated from the committed `HEAD`, not from the working tree. Until the edit is committed and installed, the previous active rulebook remains in force and all workers continue under their pinned snapshots.

After install, active workers' read and write set boundaries are refreshed to match the new rulebook, but their pinned code snapshot does not move. To update a bidding group's code snapshot, use `multorum perspective forward <perspective>` explicitly.

## Treat The Filesystem Runtime As Canonical

The shipped runtime is filesystem-backed. `.multorum/` under the workspace root is the source of truth, and both the CLI and MCP surfaces project that same state.

Some MCP projections remain intentionally unimplemented. Do not assume a resource exists just because the design document describes the concept.

## Prefer The Exposed Surfaces

Use the CLI as the default surface for canonical-workspace work: editing the rulebook, committing and installing it, forwarding perspectives, inspecting Git state, and reading `.multorum/` directly. Those operations already live in the shell and filesystem, so the CLI keeps the workflow explicit.

Use orchestrator MCP when it materially helps with typed worker-management calls or read-only runtime snapshots. Fall back to the CLI whenever the MCP projection is missing or the shell shape is clearer.

When you publish a bundle with a body path or artifact path, treat those paths as transferred ownership. Successful publication moves the files into Multorum-managed `.multorum/` storage instead of copying them.

Start the orchestrator MCP server from the workspace root with:

```bash
multorum serve orchestrator
```

### Orchestrator MCP tools

- `rulebook_init`
- `rulebook_validate`
- `rulebook_install`
- `rulebook_uninstall`
- `list_perspectives`
- `forward_perspective`
- `list_workers`
- `get_worker`
- `read_worker_outbox`
- `ack_worker_outbox_message`
- `create_worker`
- `resolve_worker`
- `revise_worker`
- `discard_worker`
- `delete_worker`
- `merge_worker`
- `get_status`

### Orchestrator MCP resources

- `multorum://orchestrator/status`
- `multorum://orchestrator/rulebook/active`
- `multorum://orchestrator/perspectives`
- `multorum://orchestrator/workers`
- `multorum://orchestrator/workers/{worker}`
- `multorum://orchestrator/workers/{worker}/outbox`

The transport reserves worker sub-resources such as `/contract`, `/transcript`, and `/checks`, but they are not implemented yet and should not be treated as available.

### CLI commands

```bash
multorum rulebook init
multorum rulebook validate
multorum rulebook install
multorum rulebook uninstall
multorum perspective list
multorum perspective forward <perspective>
multorum worker create <perspective> [--worker <worker>] [--overwriting-worktree] [--body task.md] [--artifact FILE ...]
multorum worker list
multorum worker show <worker>
multorum worker outbox <worker> [--after <sequence>]
multorum worker ack <worker> <sequence>
multorum worker resolve <worker> [--reply-to <sequence>] [--body resolve.md] [--artifact FILE ...]
multorum worker revise <worker> [--reply-to <sequence>] [--body revise.md] [--artifact FILE ...]
multorum worker discard <worker>
multorum worker delete <worker>
multorum worker merge <worker> [--skip-check <check> ...]
multorum status
```

## Run The Session Deliberately

1. Inspect current state with `get_status` or `multorum status`.
2. Validate the `HEAD` rulebook before installing whenever live workers make conflicts possible.
3. Enumerate perspectives before assigning work so the task matches an existing ownership boundary.
4. Create one worker per perspective by default. Create multiple workers from the same perspective only when you intentionally want a bidding group evaluating the same boundary from the same pinned snapshot.
5. Attach an initial task bundle when the worker needs nontrivial instructions or evidence files.
6. Read worker outbox traffic, acknowledge each consumed bundle, and review the worker detail plus any attached artifacts before deciding whether to resolve, revise, discard, delete, or merge.
7. When a blocked perspective needs a new file or newer pinned base, install the updated rulebook and use `multorum perspective forward <perspective>` only if every live worker in that bidding group is `BLOCKED`.
8. Merge only from `COMMITTED`, and skip checks only when the rulebook marks them `skippable` and the worker submitted evidence you trust.

## Write Better Worker Tasks

Each initial task or follow-up bundle should state:

- the exact objective
- the files or file region the worker is expected to change
- the acceptance checks the worker should run or attach as evidence
- the situations that require an immediate `report` instead of improvisation

Do not ask a worker to create new files unless the active rulebook already declares them. When a blocked worker reports that a new file is needed, update the canonical workspace and rulebook first, install the rulebook, and forward that perspective only if the whole live bidding group is blocked. Do not rely on "figure out the right place" when the change may cross perspective boundaries.
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
multorum rulebook validate
multorum worker create AuthImplementor --body task.md --artifact spec.md
multorum worker outbox auth-implementor-1 --after 6
multorum worker ack auth-implementor-1 7
multorum perspective forward AuthImplementor
multorum worker resolve auth-implementor-1 --reply-to 7 --body resolve.md
multorum worker revise auth-implementor-1 --reply-to 12 --body revise.md --artifact failing-test.log
multorum worker merge auth-implementor-1 --skip-check test
multorum worker delete auth-implementor-1
```

Use `--skip-check` only for checks that the rulebook marks as skippable and only after deciding the worker's evidence is trustworthy.

## Handle Boundary And Lifecycle Situations

### Blocked Worker Needs A New File Or Expanded Boundary

Workers cannot create new files or edit outside their write set. When a worker reports this kind of blocker, the orchestrator must act:

1. Edit `.multorum/rulebook.toml` to add the file to the perspective's write set (and create the file in the canonical workspace if it does not yet exist).
2. Commit the rulebook and file changes.
3. Run `multorum rulebook install`.
4. Run `multorum perspective forward <perspective>` for the whole bidding group (not just one worker). Every live worker in that bidding group must be `BLOCKED` for forwarding to succeed.
5. Run `multorum worker resolve <worker>` to unblock the worker.

Never tell a worker to create files ad hoc in its worktree or to patch files outside its write set. If the real fix is in another perspective's territory, either adjust the rulebook, create a worker from the correct perspective, or re-scope the task.

### Forwarding A Bidding Group To A Newer Base

`multorum rulebook install` activates the committed rulebook and refreshes live workers' declared boundaries, but it does not move their pinned code snapshot. To bring workers onto the newer base:

- Every live worker in the bidding group must be `BLOCKED`.
- Run `multorum perspective forward <perspective>`. This applies to the whole bidding group, not individual workers.
- Forwarding preserves each worker's progress from the `head_commit` recorded in their latest blocking report.
- If a worker's blocking report lacks `head_commit`, forwarding is rejected. Resolve the worker with a message asking for a new report that includes the committed `head_commit`, then retry the forward.

You must forward an existing live bidding group before creating additional same-perspective workers from a newer active rulebook. Multorum rejects creation when the live group is pinned to an older base.

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

While workers are active, the orchestrator must respect the exclusion set formed by active workers' read and write sets. A file inside an active group's read set must remain stable while that group is active. Do not commit changes to those files directly. Either wait, discard conflicting workers, or evolve the rulebook and worker snapshots through the supported flow (edit rulebook, commit, install, forward).

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
