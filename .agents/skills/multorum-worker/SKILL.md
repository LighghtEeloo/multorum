---
name: "multorum-worker"
description: "Execute work inside one Multorum worker worktree while respecting the worker contract, read set, write set, mailbox protocol, and worker state machine. Use when Codex is acting as a provisioned worker and must consume inbox messages, acknowledge them, report blockers, attach evidence, and submit a commit through the worker MCP surface or the worker-local Multorum CLI."
---

# Multorum Worker

Operate inside one provisioned worker worktree. Treat the current worker contract as authoritative and escalate any mismatch between the assigned task and the declared boundary.

## Hold The Worker Contract

- Read freely across the codebase when needed for understanding, but write only inside the compiled write set.
- Treat the read set as guidance plus a stability promise, not as a hard filesystem wall.
- Never create a new file on your own. If the task needs one, send a `report`. The orchestrator will edit the rulebook, create the file in the canonical workspace, install, forward, and resolve.
- Never edit files outside the write set, even if the change looks trivial or obviously correct. If the real fix is outside your write set, report it. Do not patch it anyway. The orchestrator will either adjust your boundary, create a different worker, or re-scope the task.
- Never coordinate with other workers directly. All judgment flows through the orchestrator. If another worker owns files you need changed, report the dependency to the orchestrator. Do not ask for direct patches or cross-worker messaging.
- Remember that the runtime identity is the `worker` from `contract.toml`, even when multiple workers share the same perspective in one bidding group.
- If a blocker may require `multorum perspective forward`, commit your current safe progress first and include that commit as `--head-commit` in the `report`. Forwarding preserves progress only from that recorded commit. A report without `head_commit` will cause the forward to be rejected, so always commit and include it when you anticipate forwarding.

## Use The Worker Runtime Surface Directly

Each worker worktree contains a local `.multorum/` runtime surface with the immutable contract, materialized read and write set files, inbox and outbox mailboxes, and runtime-managed artifacts.

The worker CLI and MCP server are real filesystem-backed frontends over that runtime. Some read-only projections are still intentionally unimplemented over MCP, so inspect the on-disk files directly when needed.

## Prefer MCP For Worker Control

The worker role needs inbox and contract access, so MCP is the preferred interface when available.

When publishing through MCP, prefer `body_text` for the human-readable report or commit summary. Use `body` only when you already have a Markdown file on disk that should become `body.md`.

When you publish a report or commit with a body path or artifact path, treat that path as consumed. Multorum moves the file into its `.multorum/` runtime storage on successful publication instead of copying it.

Start the worker MCP server from inside the managed worker worktree with:

```bash
multorum serve worker
```

### Worker MCP tools

- `get_contract`
- `read_inbox`
- `ack_inbox_message`
- `send_report`
- `send_commit`
- `get_status`

### Worker MCP resources

- `multorum://worker/contract`
- `multorum://worker/inbox`
- `multorum://worker/status`

The runtime reserves `multorum://worker/read-set`, `multorum://worker/write-set`, `multorum://worker/outbox`, and `multorum://worker/transcript`, but those projections are not implemented yet. Read `.multorum/read-set.txt`, `.multorum/write-set.txt`, and the mailbox directories directly when you need them.

### Worker-facing CLI commands

```bash
multorum local contract
multorum local status
multorum local inbox [--after <sequence>]
multorum local ack <sequence>
multorum local report [--head-commit <commit>] [--reply-to <sequence>] [--body blocker.md] [--artifact FILE ...]
multorum local commit --head-commit <commit> [--body summary.md] [--artifact FILE ...]
```

## Run The Worker Loop

1. Load the worker contract and confirm the active perspective and pinned base commit.
2. Read the inbox before starting work and acknowledge each consumed message.
3. Execute only the assigned task within the declared write boundary.
4. Gather evidence while working: build output, test logs, or other artifacts the orchestrator can review.
5. Send a `report` as soon as confident completion becomes impossible or unsafe. If you need your current progress preserved across a possible forward, commit it first and report that commit with `--head-commit`.
6. Send a `commit` only after creating a real code commit and preparing a concise non-empty summary plus any evidence artifacts.

If you need to keep a local copy of an attachment, duplicate it before publication. The published path itself is transferred to Multorum storage.

## Report Early Instead Of Guessing

Send `report` for:

- permission problems
- required new files
- ambiguous or conflicting instructions
- missing destination structures
- cross-perspective changes
- evidence that needs orchestrator judgment before integration

A good report body states what blocked you, what you observed, what you think the safe options are, and what exact decision the orchestrator needs to make. When the blocker may require `multorum perspective forward`, include the committed `--head-commit` you want preserved.

An empty body is the exception, not the default. If you are using MCP, send that explanation as `body_text` unless you specifically need to hand off a file by path.

## Submit Better Commits

Before `send_commit` or `multorum local commit`:

- ensure the diff touches only write-set files
- ensure the commit hash you submit is the one that contains the intended work
- attach logs or artifacts for checks the orchestrator may choose to trust
- summarize any known limitations instead of hiding them in a non-empty body

Example CLI shapes:

```bash
multorum local inbox --after 7
multorum local ack 8
multorum local report --head-commit abc1234 --reply-to 8 --body blocker.md --artifact failing-output.log
multorum local commit --head-commit abc1234 --body summary.md --artifact test.log
```

## Respect The State Machine

- Work only while ACTIVE.
- After `report`, treat the worker as blocked until a `resolve` message arrives and is acknowledged. The orchestrator may forward the whole blocked bidding group to a newer pinned base before that resolve arrives.
- After `commit`, treat the worktree as frozen until the orchestrator revises, merges, or discards it. The orchestrator may send `revise` to return you to ACTIVE, or `merge`/`discard` to finalize.
- Do not keep editing after submission unless the orchestrator explicitly sends a revision request.

## Keep The Worktree Clean At Submission

Merge is based on the `head_commit` you submit, not on the worktree state. Uncommitted edits in the worktree are not part of the merge candidate and will be ignored. Before calling `multorum local commit --head-commit <commit>`:

- Ensure the commit hash you reference contains exactly the intended work.
- Ensure the diff touches only write-set files.
- Do not leave unrelated uncommitted changes in the worktree. If they exist, the orchestrator will not consider them part of your submission.
