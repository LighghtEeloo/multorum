---
name: "multorum-orchestrator"
description: "Coordinate a Multorum session from the canonical workspace. Use when Codex is acting as the central orchestrator and must decompose work, select or switch rulebooks, provision perspectives, review worker transcripts and check results, resolve blockers, request revisions, discard work, or integrate submissions through the Multorum CLI or the orchestrator MCP surface."
---

# Multorum Orchestrator

Coordinate the system from the main workspace. Treat Multorum as reactive infrastructure: it enforces declared boundaries and state transitions, but it never decomposes work, decides trust, or invents recovery steps for you.

## Hold The Core Invariants

- Keep the topology star-shaped. Communicate with workers only through Multorum; never route work between workers directly.
- Decompose tasks so active workers do not depend on each other's unpublished output.
- Respect the safety property: a file may be written by exactly one perspective or read by many, never both.
- Treat the read set as a stability contract and the write set as an absolute ownership boundary.
- Treat new files, missing permissions, and cross-perspective edits as orchestrator work. Update the rulebook, switch rulebooks, and reprovision instead of telling a worker to proceed anyway.

## Check The Runtime First

Verify whether the runtime is real or still scaffolded before relying on automation.

- In this repository, `src/cli.rs` defines the CLI surface and `src/mcp/*` defines the intended MCP surface.
- The default runtime services are currently `Noop*` implementations that return `RuntimeError::Unimplemented`.
- If the installed `multorum` binary or MCP server reports unimplemented operations, use this skill as the operating contract and limit actions to inspection, planning, and repo changes until a real runtime replaces the stubs.

## Prefer The Exposed Surfaces

Prefer orchestrator MCP when it exists because it gives role-scoped tools and resources. Fall back to the CLI when MCP is unavailable or shell automation is simpler.

When you publish a bundle with a body path or artifact path, treat those paths as transferred ownership. Successful publication moves the files into Multorum-managed `.multorum/` storage instead of copying them.

### Orchestrator MCP tools

- `rulebook_validate`
- `rulebook_switch`
- `list_perspectives`
- `provision_worker`
- `resolve_worker`
- `revise_worker`
- `discard_worker`
- `integrate_worker`
- `get_status`

### Orchestrator MCP resources

- `multorum://orchestrator/status`
- `multorum://orchestrator/rulebook/active`
- `multorum://orchestrator/perspectives`
- `multorum://orchestrator/workers`
- `multorum://orchestrator/workers/{perspective}/contract`
- `multorum://orchestrator/workers/{perspective}/transcript`
- `multorum://orchestrator/workers/{perspective}/checks`

### CLI commands

```bash
multorum rulebook validate <commit>
multorum rulebook switch <commit>
multorum provision <perspective> [--body task.md] [--artifact FILE ...]
multorum resolve <perspective> [--reply-to <sequence>] [--body resolve.md] [--artifact FILE ...]
multorum revise <perspective> [--reply-to <sequence>] [--body revise.md] [--artifact FILE ...]
multorum discard <perspective>
multorum integrate <perspective> [--skip-check <check> ...]
multorum status
```

## Run The Session Deliberately

1. Inspect current state with `get_status` or `multorum status`.
2. Validate a rulebook commit before switching whenever worker activity makes conflicts possible.
3. Enumerate perspectives before assigning work so the task matches an existing ownership boundary.
4. Provision exactly one worker per perspective and attach an initial task body when the worker needs nontrivial instructions.
5. Review worker contract, transcript, and check results before deciding whether to resolve, revise, discard, or integrate.
6. Integrate only from the committed state and skip checks only when the rulebook allows it and the worker submitted trustworthy evidence.

## Write Better Worker Tasks

Each initial task or follow-up bundle should state:

- the exact objective
- the files or file region the perspective is expected to change
- the acceptance checks the worker should run or attach as evidence
- the situations that require an immediate `report` instead of improvisation

Do not ask a worker to create new files unless the active rulebook already declares them. Do not rely on "figure out the right place" when the change may cross perspective boundaries.
When attaching a task body, evidence log, or other artifact by path, do not plan to reuse the original path after publication unless you created a separate copy yourself.

## Resolve, Revise, And Integrate Correctly

- Use `resolve` only for a blocked worker. Answer the blocker directly and include `--reply-to` or the MCP reply reference when responding to a specific report.
- Use `revise` only for a committed worker. State what changed in your evaluation, what must be corrected, and what evidence should accompany the next submission.
- Use `discard` when the task should be abandoned rather than repaired.
- Use `integrate` only after reviewing the worker's summary, evidence, and affected files.
- Never skip the file-set check. It is mandatory by design.

## Example Command Shapes

```bash
multorum rulebook validate 3a6ee314
multorum provision AuthImplementor --body task.md --artifact spec.md
multorum resolve AuthImplementor --reply-to 7 --body resolve.md
multorum revise AuthImplementor --reply-to 12 --body revise.md --artifact failing-test.log
multorum integrate AuthImplementor --skip-check test
```

Use `--skip-check` only for checks that the rulebook marks as skippable and only after deciding the worker's evidence is trustworthy.
