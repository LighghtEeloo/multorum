# Multorum

Multorum lets one orchestrator run multiple workers on the same repository at the same time without turning the codebase into a landfill of branch collisions, accidental overlap, and heroic merge folklore.

Each worker gets a full checkout of the repo. It can read everything, build everything, test everything, and inspect the system like a grown-up. But when it writes, it writes only inside a declared scope. It gets the whole map, and exactly one knife.

That split is the entire trick.

Multorum is not an agent. It does not plan, negotiate, ideate, or otherwise hallucinate that it is management. It is infrastructure for orchestrated parallel work: isolated workspaces, explicit ownership, and hard guarantees that workers do not quietly sabotage each other.

> Belua multorum es capitums!

## Y?

Parallel development usually goes wrong in one of two boring ways.

Either everyone edits freely and you discover the damage later, when the branches come back from the dead and start eating each other. Or everyone is locked into such a tiny sandbox that they lose the context needed to do competent work, which is a fantastic way to mass-produce local correctness and global nonsense.

Multorum takes the less idiotic route. Workers execute against the full repository, but they author only within a declared write set. So they keep the context they need, while the orchestrator keeps the control it needs.

No vibes. No handshake deal. No “please try not to touch that.”

> For a detailed design reference, see [DESIGN.md](DESIGN.md).

## The Model

Everything revolves around three things: the orchestrator, the rulebook, and the workers.

The orchestrator is the sole coordinator. Human, model, or some suspicious hybrid creature — Multorum does not care. It decides what work gets split up, which role gets which task, and which results deserve to survive.

The rulebook lives at `.multorum/rulebook.toml`. It defines named perspectives. A perspective is a role with two boundaries: a write set and a read set.

The write set is the territory that role may modify. The read set is the territory that must stay stable while that role is active. It is not a visibility filter. Workers can still inspect the whole repo. The read set exists so Multorum knows what other concurrent work is forbidden to disturb.

A worker is a live instance of a perspective. When one is created, Multorum gives it an isolated git worktree, a pinned base snapshot, and materialized boundary files. The contract stops being theory and becomes a real workspace with real limits.

## The Guarantee That Matters

Multorum is built around one invariant:

> A file may be written by exactly one active bidding group, or read by any number of active bidding groups, but never both.

That is the product. The rest is plumbing.

In practice, this means concurrent write scopes cannot overlap, and no active group may write into files another active group depends on as stable context. So instead of discovering conflicts after parallel work has already happened, Multorum rejects bad overlap before the work starts.

That is a much nicer time to disappoint people.

## Bidding Groups

Sometimes the orchestrator does not want one attempt. Sometimes it wants a small knife fight.

Multorum allows multiple workers to be created from the same perspective on the same pinned base. Those workers form a bidding group. They share the same boundaries, start from the same snapshot, and race independently.

At most one may merge.

The others are discarded, which is not cruelty. It is standards.

## What Workers May Actually Do

A worker may read the whole repository, because code without context tends to produce garbage with excellent self-esteem.

A worker may write only inside its materialized write set. That write set is closed over existing files. Workers do not get to expand their own authority by casually touching random paths or inventing new files outside the contract.

If new files are genuinely needed, the orchestrator updates the canonical workspace and the rulebook explicitly. Multorum is strict here on purpose. A boundary that grows itself is not a boundary. It is a bedtime story.

## How Work Flows

The orchestrator defines perspectives in the rulebook and installs it. Then it creates workers from those perspectives. Each worker runs inside its own workspace, reports blockers or results through the runtime surface, and eventually submits work for consideration.

Before anything lands, Multorum performs a mandatory write-scope check and then runs whatever project checks the rulebook declares. The orchestrator can merge the result, revise it, or discard it.

Discarding is healthy. Not every artifact deserves citizenship.

## The Rulebook

The rulebook gives the repository a vocabulary for ownership. It lets you describe regions of the tree and assign them to roles in a way Multorum can actually enforce.

A tiny example:

```toml
[fileset]
SpecFiles.path = "**/*.spec.md"
TestFiles.path = "**/test/**"

AuthFiles.path = "auth/**"
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

[perspective.AuthImplementor]
read = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"

[perspective.AuthTester]
read = "AuthSpecs | AuthTests"
write = "AuthTests"
```

This says one role owns auth implementation, another owns auth tests, and both depend on the auth specs staying stable. Since their write scopes do not overlap, they can run concurrently without stepping on each other like amateurs.

## Runtime Shape

The main workspace keeps orchestrator state under `.multorum/orchestrator/`. Each worker workspace gets its own `.multorum/` directory containing its contract, its materialized read and write sets, and file-based inbox and outbox mailboxes.

That choice is deliberately boring. The runtime is visible on disk, easy to inspect, easy to script, and not held together by some daemon mumbling in the attic. Workers do not talk to each other directly. Everything routes through the orchestrator, where coordination belongs.

## Merging

Before a worker result is allowed to land, Multorum verifies that the worker touched only files inside its write set. Then it runs the project checks declared by the rulebook: build, lint, test, whatever the repository demands.

Some project checks may be skippable if the orchestrator accepts evidence. The write-scope check is not skippable, because once that becomes optional the entire model collapses into decorative fraud.

## What Multorum Is Not

Multorum is not a merge tool with a better attitude. It is not a chat protocol pretending to be a runtime. It is not a replacement for orchestration logic. It is not a system that assumes parallel work will behave nicely if everyone expresses themselves clearly.

It assumes the opposite, because it has met software projects.

## In Conclusion (❁´◡`❁)

Multorum is for orchestrated parallel development on a single repository with isolated workspaces, explicit file ownership, and conflict freedom enforced by construction.

If you are running multiple workers against one codebase and are tired of treating merge pain as an immutable law of the universe, Multorum is the tool.
