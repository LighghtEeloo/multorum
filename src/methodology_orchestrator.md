# Multorum Orchestrator Methodology

Operate from the canonical workspace root. Treat Multorum as reactive infrastructure: it enforces declared boundaries and state transitions, but it does not decompose work, decide trust, or invent recovery steps.

## Core invariants

- Keep the topology star-shaped. Communicate with workers only through Multorum. Workers do not coordinate with each other directly.
- Respect the bidding-group safety property: a file may be written by exactly one active bidding group or read by many, never both.
- Treat the read set as a stability contract and the write set as an absolute ownership boundary.
- Treat new files, missing permissions, and cross-perspective edits as orchestrator work in the canonical workspace.
- Address concrete workers by `worker`, not by perspective name. Multiple workers from one perspective form one bidding group, and at most one may merge.

## Rulebook ownership

- The orchestrator owns `.multorum/rulebook.toml` in the canonical workspace.
- Edit the rulebook before creating workers when the ownership model needs to change.
- Validate perspective combinations when concurrency assumptions matter.
- Forward a bidding group only after updating the canonical workspace and the current rulebook, and only when every live worker in that group is non-`ACTIVE`.

## Preferred surfaces

- Use the CLI for rulebook edits, perspective validation, perspective forwarding, Git inspection, and direct filesystem inspection.
- Use MCP when typed worker-management calls or runtime snapshots materially help and the host is correctly bound to the canonical workspace.
- Treat MCP as a transport projection over the filesystem-backed runtime, not as a separate control plane.
- When publishing a bundle by path, treat the path as transferred ownership. Successful publication moves the files into `.multorum/` storage.

## Session loop

1. Inspect current state with `multorum status` or `multorum://orchestrator/status`.
2. Validate the perspective combinations you intend to run whenever rulebook edits or live workers make conflicts possible.
3. Match tasks to existing ownership boundaries before creating workers.
4. Create one worker per perspective by default. Create multiple workers from the same perspective only when you intentionally want a bidding group.
5. Read worker outbox traffic, acknowledge consumed bundles, and review evidence before deciding whether to resolve, revise, discard, delete, or merge.
6. Merge only from `COMMITTED`, and skip checks only when the rulebook marks them skippable and the submitted evidence justifies that trust.
7. Write merge-time audit rationale as self-contained findings in `.multorum/audit/`; do not rely on references to worker outbox paths because worker worktrees and outboxes are runtime state that may be deleted after merge confirmation.

## Task writing

Each worker task should state:

- the exact objective
- the files or file region expected to change
- the acceptance checks to run or attach as evidence
- the situations that require an immediate report instead of improvisation

Do not ask workers to create new files unless the active rulebook already declares them.
