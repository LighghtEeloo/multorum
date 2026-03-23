---
name: "multorum-orchestrator"
description: "Coordinate a Multorum session from the canonical workspace. Use when Codex is acting as the orchestrator and must manage rulebooks, create or inspect workers, answer blockers, request revisions, discard work, delete finalized worktrees, or merge submissions through the Multorum CLI or the orchestrator MCP surface."
---

# Multorum Orchestrator

Coordinate the system from the canonical workspace root. Treat Multorum as reactive infrastructure: it enforces declared boundaries and state transitions, but it never decomposes work, decides trust, or invents recovery steps for you.

## Hold The Core Invariants

- Keep the topology star-shaped. Communicate with workers only through Multorum; never route work between workers directly.
- Decompose tasks so active workers do not depend on each other's unpublished output.
- Respect the safety property at the bidding-group level: a file may be written by exactly one active bidding group or read by many, never both.
- Treat the read set as a stability contract and the write set as an absolute ownership boundary.
- Treat new files, missing permissions, and cross-perspective edits as orchestrator work. Update the rulebook, install it, and create a fresh worker instead of telling a worker to proceed anyway.
- Remember that workers are addressed by `worker_id`, not by perspective name. Multiple workers from the same perspective form one bidding group, and at most one of them may merge.

## Treat The Filesystem Runtime As Canonical

The shipped runtime is filesystem-backed. `.multorum/` under the workspace root is the source of truth, and both the CLI and MCP surfaces project that same state.

Some MCP projections remain intentionally unimplemented. Do not assume a resource exists just because the design document describes the concept.

## Prefer The Exposed Surfaces

Prefer orchestrator MCP when it exists because it gives typed worker-management tools and read-only runtime projections. Fall back to the CLI when MCP is unavailable or shell automation is simpler.

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
- `list_workers`
- `get_worker`
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

The transport reserves worker sub-resources such as `/contract`, `/transcript`, and `/checks`, but they are not implemented yet and should not be treated as available.

### CLI commands

```bash
multorum rulebook init
multorum rulebook validate
multorum rulebook install
multorum rulebook uninstall
multorum perspective list
multorum worker create <perspective> [--worker-id <worker>] [--overwriting-worktree] [--body task.md] [--artifact FILE ...]
multorum worker list
multorum worker show <worker-id>
multorum worker resolve <worker-id> [--reply-to <sequence>] [--body resolve.md] [--artifact FILE ...]
multorum worker revise <worker-id> [--reply-to <sequence>] [--body revise.md] [--artifact FILE ...]
multorum worker discard <worker-id>
multorum worker delete <worker-id>
multorum worker merge <worker-id> [--skip-check <check> ...]
multorum status
```

## Run The Session Deliberately

1. Inspect current state with `get_status` or `multorum status`.
2. Validate the `HEAD` rulebook before installing whenever live workers make conflicts possible.
3. Enumerate perspectives before assigning work so the task matches an existing ownership boundary.
4. Create one worker per perspective by default. Create multiple workers from the same perspective only when you intentionally want a bidding group evaluating the same boundary from the same pinned snapshot.
5. Attach an initial task bundle when the worker needs nontrivial instructions or evidence files.
6. Review worker detail, mailbox evidence, and any attached artifacts before deciding whether to resolve, revise, discard, delete, or merge.
7. Merge only from `COMMITTED`, and skip checks only when the rulebook marks them `skippable` and the worker submitted evidence you trust.

## Write Better Worker Tasks

Each initial task or follow-up bundle should state:

- the exact objective
- the files or file region the worker is expected to change
- the acceptance checks the worker should run or attach as evidence
- the situations that require an immediate `report` instead of improvisation

Do not ask a worker to create new files unless the active rulebook already declares them. Do not rely on "figure out the right place" when the change may cross perspective boundaries.
When attaching a task body, evidence log, or other artifact by path, do not plan to reuse the original path after publication unless you created a separate copy yourself.

## Resolve, Revise, Merge, And Delete Correctly

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
multorum worker resolve auth-implementor-1 --reply-to 7 --body resolve.md
multorum worker revise auth-implementor-1 --reply-to 12 --body revise.md --artifact failing-test.log
multorum worker merge auth-implementor-1 --skip-check test
multorum worker delete auth-implementor-1
```

Use `--skip-check` only for checks that the rulebook marks as skippable and only after deciding the worker's evidence is trustworthy.
