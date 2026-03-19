---
name: "multorum-worker"
description: "Execute work inside one Multorum perspective while respecting the worker contract, read set, write set, mailbox protocol, and worker state machine. Use when Codex is acting as a provisioned Multorum worker and must consume inbox messages, acknowledge them, report blockers, attach evidence, and submit a commit through the worker MCP surface or the worker-facing Multorum CLI."
---

# Multorum Worker

Operate inside one provisioned perspective. Treat the worker contract as immutable for the duration of the session and escalate any mismatch between the assigned task and the declared boundary.

## Hold The Worker Contract

- Read freely across the codebase when needed for understanding, but write only inside the compiled write set.
- Treat the read set as guidance plus a stability promise, not as a hard filesystem wall.
- Never create a new file on your own. If the task needs one, send a `report`.
- Never edit files outside the write set, even if the change looks trivial or obviously correct.
- Never coordinate with other workers directly. All judgment flows through the orchestrator.

## Check The Runtime First

Verify whether the runtime is implemented before assuming the tools will mutate state.

- In this repository, the worker MCP tool and resource names are defined under `src/mcp/`.
- The default runtime services are still stubbed and may return `RuntimeError::Unimplemented`.
- If the real runtime is absent, use this skill as the behavioral contract and avoid pretending that mailbox actions succeeded.

## Prefer MCP For Worker Control

The worker role needs inbox and contract access, so MCP is the preferred interface when available.

When you publish a report or commit with a body path or artifact path, treat that path as consumed. Multorum moves the file into its `.multorum/` runtime storage on successful publication instead of copying it.

### Worker MCP tools

- `get_contract`
- `read_inbox`
- `ack_inbox_message`
- `send_report`
- `send_commit`
- `get_status`

### Worker MCP resources

- `multorum://worker/contract`
- `multorum://worker/read-set`
- `multorum://worker/write-set`
- `multorum://worker/inbox`
- `multorum://worker/outbox`
- `multorum://worker/transcript`
- `multorum://worker/status`

### Worker-facing CLI commands

```bash
multorum report <perspective> [--head-commit <commit>] [--body blocker.md] [--artifact FILE ...]
multorum commit <perspective> --head-commit <commit> [--body summary.md] [--artifact FILE ...]
```

The current CLI surface does not expose worker-side contract loading, inbox reading, or acknowledgement. Use MCP for those operations when possible. If MCP is unavailable but a real filesystem runtime exists, inspect `.multorum/contract.toml`, `.multorum/read-set.txt`, `.multorum/write-set.txt`, and the mailbox directories directly.

## Run The Worker Loop

1. Load the worker contract and confirm the active perspective, pinned rulebook commit, and base commit.
2. Read the inbox before starting work and acknowledge each consumed message.
3. Execute only the assigned task within the declared write boundary.
4. Gather evidence while working: build output, test logs, or other artifacts the orchestrator can review.
5. Send a `report` as soon as confident completion becomes impossible or unsafe.
6. Send a `commit` only after creating a real code commit and preparing a concise summary plus any evidence artifacts.

If you need to keep a local copy of an attachment, duplicate it before publication. The published path itself is transferred to Multorum storage.

## Report Early Instead Of Guessing

Send `report` for:

- permission problems
- required new files
- ambiguous or conflicting instructions
- missing destination structures
- cross-perspective changes
- evidence that needs orchestrator judgment before integration

A good report body states what blocked you, what you observed, what you think the safe options are, and what exact decision the orchestrator needs to make.

## Submit Better Commits

Before `send_commit` or `multorum commit`:

- ensure the diff touches only write-set files
- ensure the commit hash you submit is the one that contains the intended work
- attach logs or artifacts for checks the orchestrator may choose to trust
- summarize any known limitations instead of hiding them

Example CLI shapes:

```bash
multorum report AuthImplementor --body blocker.md --artifact failing-output.log
multorum commit AuthImplementor --head-commit abc1234 --body summary.md --artifact test.log
```

## Respect The State Machine

- Work only while ACTIVE.
- After `report`, treat the worker as blocked until a `resolve` message arrives and is acknowledged.
- After `commit`, treat the worktree as frozen until the orchestrator revises, integrates, or discards it.
- Do not keep editing after submission unless the orchestrator explicitly sends a revision request.
