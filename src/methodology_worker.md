# Multorum Worker Methodology

Operate inside one provisioned worker worktree. Treat the current worker contract as authoritative and escalate any mismatch between the assigned task and the declared boundary.

## Core invariants

- Read freely across the repository when needed for understanding, but write only inside the compiled write set.
- Never create a new file on your own. If the task needs one, send a report so the orchestrator can update the canonical workspace and rulebook.
- Never edit outside the write set, even for a trivial fix. Report the dependency instead of patching around the contract.
- Never coordinate with other workers directly. All judgment flows through the orchestrator.
- If a blocker may require `multorum perspective forward`, commit your safe progress first and include that commit as `head_commit` in the report.

## Preferred surfaces

- Use the worker-local CLI as the default interface because it runs directly against the filesystem-backed runtime in the current worktree.
- Use worker MCP when the host is clearly bound to the managed worker worktree and typed tool calls materially help.
- Treat MCP as a transport projection, not as a different runtime.
- When publishing through a path-backed body or artifact, treat that path as consumed. Successful publication moves it into `.multorum/` storage.

## Worker loop

1. Load the worker contract and confirm the perspective and pinned base commit.
2. Read the inbox before starting work and acknowledge each consumed message.
3. Execute only the assigned task within the declared write boundary.
4. Gather evidence while working: build output, test logs, or other artifacts the orchestrator can review.
5. Send a report as soon as confident completion becomes impossible or unsafe.
6. Send a commit only after creating a real code commit and preparing a concise non-empty summary plus any evidence artifacts.

## Report early instead of guessing

Send a report for:

- permission problems
- required new files
- ambiguous or conflicting instructions
- missing destination structures
- cross-perspective changes
- evidence that needs orchestrator judgment before integration

## Submission discipline

- Ensure the submitted commit contains exactly the intended work.
- Ensure the diff touches only write-set files.
- Do not keep editing after submission unless the orchestrator explicitly sends a revision request.
- Remember that merge is based on the submitted `head_commit`, not on uncommitted worktree state.
