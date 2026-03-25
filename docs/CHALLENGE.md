# Multorum Challenge Scenarios

This document collects practical challenge prompts for evaluating LLM skills that operate in a Multorum repository.
Each challenge is designed to be easy for a correct Multorum-aware skill and useful for revising orchestrator and worker instructions later.

Use these challenges as short scenario tests:

1. Give the scenario to the skill without the rubric.
2. Compare the answer against the "A correct answer should include" section.
3. Revise the skill whenever it misses a required step, invents unsupported behavior, or uses the wrong command.

## 1. New File Requested By A Blocked Worker

**Scenario**

The orchestrator created a worker from a perspective. While working, the worker reports a blocker: the task requires creating a new source file that is not in the worker's current write set.

What should the orchestrator do?

**A correct answer should include**

- Workers may not create new files on their own because the write set is a closed list of existing paths.
- The orchestrator must update the canonical workspace and rulebook so the new file exists and belongs to the perspective.
- Those canonical changes must be committed before activation.
- The orchestrator must run `multorum rulebook install`.
- The orchestrator must run `multorum perspective forward <perspective>` for the whole blocked bidding group, not just one worker.
- The orchestrator must then send `multorum worker resolve <worker-id>`.
- The answer should not suggest direct ad hoc file creation inside the blocked worker worktree.

## 2. Worker Needs To Edit A File Outside Its Write Set

**Scenario**

A worker reports: "I found the real bug, but the fix is in `src/runtime/orchestrator.rs`, which is outside my write set. Should I just patch it anyway?"

What should the orchestrator tell the worker, and what should happen next?

**A correct answer should include**

- The worker must not edit outside its write set.
- The orchestrator must decide whether to change the rulebook, create a different worker from a better perspective, or re-scope the task.
- If the boundary needs to expand for the same perspective, the answer should mention rulebook update, commit, install, forward, and resolve.
- The answer should not suggest bypassing Multorum's boundary model.

## 3. Active Bidding Group Is Still On An Older Base

**Scenario**

The orchestrator installed a newer rulebook commit and now wants to create another worker from the same perspective. Multorum rejects the command because the live bidding group for that perspective is still pinned to the older base commit.

What should the orchestrator do?

**A correct answer should include**

- Multorum does not automatically move live workers to the new rulebook snapshot.
- The orchestrator must forward the existing live bidding group for that perspective before creating more same-perspective workers from the newer active rulebook.
- `multorum perspective forward <perspective>` applies to the whole bidding group.
- Forwarding requires every live worker in that bidding group to be `BLOCKED`.
- The answer should not say that `rulebook install` alone updates the workers' pinned code snapshot.

## 4. One Worker In A Bidding Group Is Merged

**Scenario**

The orchestrator created two workers from the same perspective as alternative attempts. One worker submits a good result and is merged successfully.

What should happen to the other worker?

**A correct answer should include**

- Only one worker from a bidding group may be merged.
- The remaining worker or workers in that bidding group should be discarded.
- If cleanup is desired later, finalized workspaces may then be deleted explicitly.
- The answer should not suggest merging both alternatives.

## 5. Discard Versus Delete

**Scenario**

A worker is no longer useful. The orchestrator wants it gone immediately and also wants to preserve the option to inspect the workspace first.

What is the difference between `discard` and `delete`, and in what order should they happen?

**A correct answer should include**

- `multorum worker discard <worker-id>` finalizes the worker without merging and preserves the workspace.
- `multorum worker delete <worker-id>` removes the finalized workspace and worker state file.
- Delete is allowed only after the worker is finalized as `MERGED` or `DISCARDED`.
- The answer should distinguish lifecycle finalization from filesystem cleanup.

## 6. Orchestrator Wants To Commit A Hotfix In The Main Workspace

**Scenario**

While several workers are active, the orchestrator notices a typo and wants to commit a quick fix directly in the canonical workspace. The file is inside one active group's read set.

What should the orchestrator do?

**A correct answer should include**

- The orchestrator must respect the exclusion set formed by active workers' read and write sets.
- A file inside an active group's read set must remain stable while that group is active.
- The orchestrator should wait, discard conflicting workers, or deliberately evolve the rulebook and worker snapshots through the supported flow.
- The answer should not suggest committing directly to the canonical branch anyway.

## 7. Blocked Worker Must Be Forwarded, But The Report Lacks `head_commit`

**Scenario**

A worker is `BLOCKED` and clearly needs a perspective forward, but its latest blocker report did not include `head_commit`.

What should the orchestrator do?

**A correct answer should include**

- Perspective forwarding preserves progress from the `head_commit` recorded in the latest blocking report.
- Without that `head_commit`, Multorum should reject the forward.
- The orchestrator should unblock the worker with a `resolve` message that asks for a new blocker report with the relevant `head_commit` if forwarding is still needed.
- The answer should not invent a manual replay or guess the commit.

## 8. Worker Submitted A Commit, But The Orchestrator Wants Changes

**Scenario**

A worker moved to `COMMITTED`, but the orchestrator wants the worker to revise the change instead of merging or discarding it.

What command should the orchestrator use, and what state transition matters next?

**A correct answer should include**

- The orchestrator should send `multorum worker revise <worker-id>`.
- The worker becomes `ACTIVE` again when it acknowledges that inbox message.
- The answer should not use `resolve`, which is for `BLOCKED` workers rather than `COMMITTED` ones.

## 9. Worker Wants To Coordinate Directly With Another Worker

**Scenario**

One worker says: "Another worker owns the test files I need. Ask them to add a helper and send me the patch directly."

What should the orchestrator do?

**A correct answer should include**

- Workers do not communicate directly with each other.
- The communication topology is orchestrator-centered.
- The orchestrator may create, revise, or retask workers, but cross-worker coordination must still go through orchestrator decisions and mailbox flows.
- The answer should not suggest worker-to-worker messaging outside the orchestrator.

## 10. Skipping A Merge Check Based On Worker Evidence

**Scenario**

A worker submits a change together with evidence that one project-defined merge check is unnecessary in this case. The check is marked skippable in the rulebook.

What should the orchestrator consider before skipping it?

**A correct answer should include**

- Only project-defined checks marked skippable may be skipped.
- The mandatory write-set scope check is never skippable.
- The orchestrator is responsible for deciding whether the submitted evidence is sufficient.
- The merge command may use `--skip-check <check>` only for allowed checks.
- The answer should not imply that workers can skip checks unilaterally.

## 11. Worker Solves The Task But Leaves The Runtime Dirty

**Scenario**

The worker says the code is complete, but the worktree contains unrelated uncommitted edits not represented by the submitted head commit.

What should the orchestrator rely on during merge?

**A correct answer should include**

- Merge is based on the submitted `head_commit`, not on an ambiguous dirty worktree state.
- The orchestrator should rely on the explicit submission recorded through `multorum local commit --head-commit <commit>`.
- If the submitted commit is not the desired result, the orchestrator should use `revise` rather than guessing from the worktree.
- The answer should not treat stray uncommitted edits as part of the merge candidate.

## 12. Rulebook Change Looks Valid On Disk But Is Not Active

**Scenario**

The orchestrator edited `.multorum/rulebook.toml` in the main workspace and assumes the new policy is already in force for future worker creation.

Is that correct?

**A correct answer should include**

- Editing the rulebook on disk does nothing by itself.
- The changed rulebook must be committed and installed explicitly.
- Active workers continue to follow their pinned snapshot until an explicit forward happens.
- The answer should distinguish committed policy, active rulebook activation, and worker snapshot movement.

## 13. Finalized Workspace Reuse With An Explicit Worker Id

**Scenario**

The orchestrator wants to create a new worker using an explicit worker id that previously belonged to a finalized worker.

What should the skill know about that reuse?

**A correct answer should include**

- Reuse is only valid for a finalized worker id.
- The orchestrator should use `multorum worker create <perspective> --worker-id <worker-id> --overwriting-worktree` when intentionally replacing the old finalized workspace.
- The answer should not treat old finalized state as if it automatically stays attached to the new worker.

## 14. Minimal Command-Sequence Challenge

**Scenario**

A blocked worker from perspective `AuthImplementor` needs one new file. The orchestrator agrees with the request and wants to preserve the worker's progress correctly.

Write the minimum safe sequence of orchestrator actions.

**A correct answer should include**

- Commit the canonical workspace and rulebook changes that add the file and assign it correctly.
- Run `multorum rulebook install`.
- Run `multorum perspective forward AuthImplementor`.
- Run `multorum worker resolve <worker-id>`.
- The sequence should not omit the commit, install, or forward steps.
