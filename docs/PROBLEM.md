# Multorum Challenge Scenarios

This document collects practical challenge prompts for evaluating LLM skills that operate in a Multorum repository.
Each challenge is designed to be easy for a correct Multorum-aware skill and useful for revising orchestrator and worker instructions later.

## 1. New File Requested By A Blocked Worker

**Scenario**

The orchestrator created a worker from a perspective. While working, the worker reports a blocker: the task requires creating a new source file that is not in the worker's current write set.

What should the orchestrator do?

## 2. Worker Needs To Edit A File Outside Its Write Set

**Scenario**

A worker reports: "I found the real bug, but the fix is in `src/runtime/orchestrator.rs`, which is outside my write set. Should I just patch it anyway?"

What should the orchestrator tell the worker, and what should happen next?

## 3. Active Bidding Group Is Still On An Older Base

**Scenario**

The orchestrator installed a newer rulebook commit and now wants to create another worker from the same perspective. Multorum rejects the command because the live bidding group for that perspective is still pinned to the older base commit.

What should the orchestrator do?

## 4. One Worker In A Bidding Group Is Merged

**Scenario**

The orchestrator created two workers from the same perspective as alternative attempts. One worker submits a good result and is merged successfully.

What should happen to the other worker?

## 5. Discard Versus Delete

**Scenario**

A worker is no longer useful. The orchestrator wants it gone immediately and also wants to preserve the option to inspect the workspace first.

What is the difference between `discard` and `delete`, and in what order should they happen?

## 6. Orchestrator Wants To Commit A Hotfix In The Main Workspace

**Scenario**

While several workers are active, the orchestrator notices a typo and wants to commit a quick fix directly in the canonical workspace. The file is inside one active group's read set.

What should the orchestrator do?

## 7. Blocked Worker Must Be Forwarded, But The Report Lacks `head_commit`

**Scenario**

A worker is `BLOCKED` and clearly needs a perspective forward, but its latest blocker report did not include `head_commit`.

What should the orchestrator do?

## 8. Worker Submitted A Commit, But The Orchestrator Wants Changes

**Scenario**

A worker moved to `COMMITTED`, but the orchestrator wants the worker to revise the change instead of merging or discarding it.

What command should the orchestrator use, and what state transition matters next?

## 9. Worker Wants To Coordinate Directly With Another Worker

**Scenario**

One worker says: "Another worker owns the test files I need. Ask them to add a helper and send me the patch directly."

What should the orchestrator do?

## 10. Skipping A Merge Check Based On Worker Evidence

**Scenario**

A worker submits a change together with evidence that one project-defined merge check is unnecessary in this case. The check is marked skippable in the rulebook.

What should the orchestrator consider before skipping it?

## 11. Worker Solves The Task But Leaves The Runtime Dirty

**Scenario**

The worker says the code is complete, but the worktree contains unrelated uncommitted edits not represented by the submitted head commit.

What should the orchestrator rely on during merge?

## 12. Rulebook Change On Disk Versus Worker Snapshots

**Scenario**

The orchestrator edited `.multorum/rulebook.toml` in the main workspace and assumes the new policy is already in force for future worker creation.

Is that correct? What about existing workers?

## 13. Finalized Workspace Reuse With An Explicit Worker Id

**Scenario**

The orchestrator wants to create a new worker using an explicit worker that previously belonged to a finalized worker.

What should the skill know about that reuse?

## 14. Minimal Command-Sequence Challenge

**Scenario**

A blocked worker from perspective `AuthImplementor` needs one new file. The orchestrator agrees with the request and wants to preserve the worker's progress correctly.

Write the minimum safe sequence of orchestrator actions.
