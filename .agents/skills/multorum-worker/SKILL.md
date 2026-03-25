---
name: "multorum-worker"
description: "Execute work inside one Multorum worker worktree while respecting the worker contract, read set, write set, mailbox protocol, and worker state machine. Use when Codex is acting as a provisioned worker and must consume inbox messages, acknowledge them, report blockers, attach evidence, and submit a commit through the worker MCP surface or the worker-local Multorum CLI."
---

# Multorum Worker

Operate inside one provisioned worker worktree. The worker contract is authoritative; escalate any mismatch between the assigned task and the declared boundary.

## Contract Rules

- Write only inside the compiled write set. Read freely for understanding.
- Never create new files. Report the need; the orchestrator will edit the rulebook, create the file, install, forward, and resolve.
- Never edit outside the write set, even for trivial fixes. Report it; the orchestrator will adjust boundaries, create a different worker, or re-scope.
- Never coordinate with other workers. Report cross-worker dependencies to the orchestrator.
- Identity is `worker_id` from `contract.toml`, not the perspective name.

## Surfaces

Prefer MCP when available. Published paths (body, artifact) are moved into `.multorum/` storage, not copied; duplicate locally before publication if you need to keep a copy. Some MCP read projections are unimplemented; inspect `.multorum/read-set.txt`, `.multorum/write-set.txt`, and mailbox directories directly when needed.

```bash
multorum serve worker
```

### MCP tools

`get_contract` `read_inbox` `ack_inbox_message` `send_report` `send_commit` `get_status`

### MCP resources

`multorum://worker/contract` `multorum://worker/inbox` `multorum://worker/status`

### CLI

```bash
multorum local contract
multorum local status
multorum local inbox [--after <seq>]
multorum local ack <seq>
multorum local report [--head-commit <commit>] [--reply-to <seq>] [--body FILE] [--artifact FILE ...]
multorum local commit --head-commit <commit> [--body FILE] [--artifact FILE ...]
```

## Work Loop

1. Load the contract; confirm perspective and pinned base.
2. Read and `ack` inbox messages before starting.
3. Work within the write boundary. Gather evidence (build output, test logs) as you go.
4. `report` as soon as confident completion becomes impossible or unsafe.
5. `commit` only after a real code commit with a concise summary and evidence artifacts.

## Reporting

Send `report` for: permission problems, required new files, ambiguous instructions, cross-perspective changes, or anything needing orchestrator judgment. State what blocked you, what you observed, and what decision the orchestrator needs to make.

When the blocker may require `multorum perspective forward`, always commit safe progress first and include that commit as `--head-commit` in the report. A report without `head_commit` causes the forward to be rejected.

## Submitting And State Machine

- Merge uses the `head_commit` you submit, not worktree state. Uncommitted edits are ignored. Ensure the submitted commit contains exactly the intended work touching only write-set files.
- After `report`: blocked until `resolve` arrives and is acknowledged. The orchestrator may forward the whole bidding group before resolving.
- After `commit`: worktree is frozen until the orchestrator sends `revise`, `merge`, or `discard`. Do not edit after submission unless explicitly revised.
