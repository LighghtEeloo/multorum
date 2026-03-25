---
name: "multorum-orchestrator"
description: "Coordinate a Multorum session from the canonical workspace. Use when Codex is acting as the orchestrator and must manage rulebooks, create or inspect workers, answer blockers, request revisions, discard work, delete finalized worktrees, or merge submissions through the Multorum CLI or the orchestrator MCP surface."
---

# Multorum Orchestrator

Coordinate the system from the canonical workspace root. Multorum enforces declared boundaries and state transitions but never decomposes work, decides trust, or invents recovery steps for you.

## Core Invariants

- Star topology only. Workers never communicate with each other; decline cross-worker coordination requests and handle dependencies through orchestrator decisions and mailbox flows.
- A file may be written by exactly one active bidding group or read by many, never both. The read set is a stability contract; the write set is an absolute ownership boundary.
- While workers are active, the orchestrator must not commit changes to files inside any active group's read or write set. Wait, discard conflicting workers, or evolve snapshots through the supported flow.
- Workers are addressed by `worker_id`. Multiple workers from the same perspective form one bidding group; at most one may merge. Discard the rest after merging.

## Edit The Rulebook Directly

The orchestrator owns `.multorum/rulebook.toml` and should edit it directly to add files to perspectives, define new perspectives, adjust boundaries, or configure checks. Editing the file on disk alone does nothing. The workflow is always: edit, commit, `multorum rulebook install`. Until committed and installed, the previous active rulebook remains in force.

Install refreshes live workers' declared boundaries but does not move their pinned code snapshot. Use `multorum perspective forward <perspective>` explicitly to repin. You must forward an existing live bidding group before creating additional same-perspective workers from a newer active rulebook.

## Surfaces

Prefer orchestrator MCP when available; fall back to CLI when MCP is unavailable or shell automation is simpler. Published bundle paths (body, artifact) are moved into `.multorum/` storage, not copied.

```bash
multorum serve orchestrator
```

### MCP tools

`rulebook_init` `rulebook_validate` `rulebook_install` `rulebook_uninstall` `list_perspectives` `forward_perspective` `list_workers` `get_worker` `read_worker_outbox` `ack_worker_outbox_message` `create_worker` `resolve_worker` `revise_worker` `discard_worker` `delete_worker` `merge_worker` `get_status`

### MCP resources

`multorum://orchestrator/status` `multorum://orchestrator/rulebook/active` `multorum://orchestrator/perspectives` `multorum://orchestrator/workers` `multorum://orchestrator/workers/{worker}` `multorum://orchestrator/workers/{worker}/outbox`

Sub-resources `/contract`, `/transcript`, `/checks` are reserved but not implemented.

### CLI

```bash
multorum rulebook init
multorum rulebook validate
multorum rulebook install
multorum rulebook uninstall
multorum perspective list
multorum perspective forward <perspective>
multorum worker create <perspective> [--worker-id <id>] [--overwriting-worktree] [--body FILE] [--artifact FILE ...]
multorum worker list
multorum worker show <id>
multorum worker outbox <id> [--after <seq>]
multorum worker ack <id> <seq>
multorum worker resolve <id> [--reply-to <seq>] [--body FILE] [--artifact FILE ...]
multorum worker revise <id> [--reply-to <seq>] [--body FILE] [--artifact FILE ...]
multorum worker discard <id>
multorum worker delete <id>
multorum worker merge <id> [--skip-check <check> ...]
multorum status
```

## Running A Session

1. Inspect state with `multorum status`.
2. Validate the `HEAD` rulebook before installing when live workers make conflicts possible.
3. Match tasks to existing perspective boundaries. Create one worker per perspective by default; create multiple only for intentional bidding groups.
4. Each task bundle should state: the objective, expected files, acceptance checks, and when to `report` instead of improvise. Do not ask workers to create new files unless the active rulebook already declares them.
5. Read outbox traffic and `ack` each consumed bundle before deciding the next action.

## Worker Lifecycle Commands

- **`resolve`**: unblocks a `BLOCKED` worker. Use `--reply-to` to answer a specific report.
- **`revise`**: returns a `COMMITTED` worker to `ACTIVE`. State what must be corrected. Do not use `resolve` for committed workers or `revise` for blocked ones.
- **`discard`**: finalizes without merging; preserves the worktree for inspection.
- **`delete`**: removes the worker state file and worktree. Only valid after `MERGED` or `DISCARDED`.
- **`merge`**: uses the worker's submitted `head_commit`, not ambient worktree state. If the submission is wrong, `revise` instead of guessing from the worktree. Skip checks only when the rulebook marks them `skippable` and you trust the evidence. The write-set scope check is never skippable.

To reuse a finalized worker id: `multorum worker create <perspective> --worker-id <id> --overwriting-worktree`. Old finalized state does not carry over.

## Handling Blocked Workers

When a worker reports needing a new file, expanded boundary, or cross-perspective edit:

1. Edit `.multorum/rulebook.toml` (and create any new source files).
2. Commit.
3. `multorum rulebook install`.
4. `multorum perspective forward <perspective>` — applies to the whole bidding group; every live worker must be `BLOCKED`.
5. `multorum worker resolve <id>`.

Never tell workers to create files ad hoc or patch outside their write set. If the fix belongs to another perspective, adjust the rulebook, create a different worker, or re-scope.

### Forward Requires `head_commit`

Forwarding preserves progress from the `head_commit` in each worker's latest blocking report. If a report lacks `head_commit`, the forward is rejected. Resolve the worker asking for a new report with the committed `head_commit`, then retry.
