# Multorum

Multorum is the infrastructure for orchestrated parallel development on a single repository with isolated workspaces, explicit file ownership, and conflict freedom enforced by construction. It doesn't care about the specific workflow, agentic tooling, or development process you use. It is a general-purpose tool for making parallel work safe and efficient, regardless of how you choose to organize it.

> Belua multorum es capitums!
> 
> <p align="center">
>   <img src="assets/multorum-20260325.png" alt="Multorum logo" width="40%">
> </p>

## Definition by Negation

Multorum is not an agent. It does not plan, negotiate, ideate, or otherwise hallucinate that it is management. It is infrastructure for orchestrated parallel work: isolated workspaces, explicit ownership, and hard guarantees that workers do not quietly sabotage each other.

## Y?

Problem: parallel development breaks in two ways.

- Either people work freely and pay for it later in merge hell, or..
- ..They are boxed into such narrow sandboxes that they lose the context needed to do good work.

Multorum is built to avoid that tradeoff. Each worker keeps the full repository as readable context, but authorship is constrained to an explicit write scope, so the system preserves both global understanding and local isolation.

No vibes. No handshake deal. No "please try not to touch that."

Our approach is meant to be correct by construction. Coordination is not left to etiquette, guesswork, or cleanup after the fact; it is encoded directly into the model. The orchestrator remains the sole authority, scopes are declared up front, and conflicting access patterns are rejected before work begins rather than repaired later. So the project is not trying to create magical autonomous teamwork. It is trying to make parallel work mechanically safe, inspectable, and disciplined by design.

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


## Installation

```bash
cargo install multorum
```

## Using Multorum with MCP and Skills

If your orchestrator is an MCP-capable agent, you can run the whole Multorum loop through tool calls instead of ad-hoc shell choreography.

<details>
<summary><strong>MCP installation guide</strong></summary>

### 1) Add the orchestrator MCP server

Add this to your MCP host config. `cwd` must be the canonical workspace root.

```json
{
  "mcpServers": {
    "multorum-orchestrator": {
      "command": "/absolute/path/to/multorum",
      "args": ["serve", "orchestrator"],
      "cwd": "/absolute/path/to/your/repo"
    }
  }
}
```

### 2) Add a worker MCP server

Repeat for each worker worktree. `cwd` must point at that specific worktree, not the canonical root.

```json
{
  "mcpServers": {
    "multorum-worker": {
      "command": "/absolute/path/to/multorum",
      "args": ["serve", "worker"],
      "cwd": "/absolute/path/to/worker-worktree"
    }
  }
}
```

### 3) Reload and verify

Reload your MCP host. If it reports an unmanaged repository or root mismatch, `cwd` is pointed at the wrong directory for that server role.
</details>

<details>
<summary><strong>Skills installation guide</strong></summary>

Multorum ships two skills that encode the orchestrator and worker roles as reusable agent prompts. Each skill is a complete operational guide: the agent reads the skill once and then drives the entire session through CLI commands or MCP tool calls without needing further instruction.

### Skill files

```
.agents/skills/
  multorum-orchestrator/
    SKILL.md          # orchestrator operational guide (loaded as system prompt)
    agents/
      openai.yaml     # OpenAI-compatible agent manifest
  multorum-worker/
    SKILL.md          # worker operational guide (loaded as system prompt)
    agents/
      openai.yaml     # OpenAI-compatible agent manifest
```

### Claude Code

Claude Code discovers skills automatically from the `.agents/skills/` directory when opened in the repository. No manual installation step is needed.

**Invoke the orchestrator skill** from the canonical workspace:

```
/multorum-orchestrator
```

This loads `multorum-orchestrator/SKILL.md` as the session context. The agent then uses `multorum init`, `multorum perspective ...`, and `multorum worker ...` CLI commands, or the orchestrator MCP surface if configured.

**Invoke the worker skill** from inside a provisioned worker worktree:

```
/multorum-worker
```

This loads `multorum-worker/SKILL.md` as the session context. The agent then uses `multorum local ...` commands or the worker MCP surface if configured.

The session must be opened from the correct directory. Invoking `/multorum-orchestrator` from a worker worktree, or `/multorum-worker` from the canonical workspace, produces the wrong runtime binding. Open two separate Claude Code sessions: one rooted at the canonical workspace for orchestration, one rooted at each worker worktree for worker execution.

### OpenAI-compatible agents

Each skill includes an `agents/openai.yaml` manifest for OpenAI-compatible agent runtimes. Register each manifest with your runtime and supply `SKILL.md` as the system prompt:

| Role | Manifest | System prompt |
|------|----------|---------------|
| Orchestrator | `.agents/skills/multorum-orchestrator/agents/openai.yaml` | `.agents/skills/multorum-orchestrator/SKILL.md` |
| Worker | `.agents/skills/multorum-worker/agents/openai.yaml` | `.agents/skills/multorum-worker/SKILL.md` |

Configure each agent with the MCP server that matches its role:
- The orchestrator agent gets the `multorum-orchestrator` MCP server (see MCP installation above) bound to the canonical workspace.
- Each worker agent gets the `multorum-worker` MCP server bound to its specific worktree.

### Role discipline after installation

Keep the role separation strict:
- The orchestrator agent creates workers, manages the rulebook, reads outboxes, resolves blockers, and merges or discards submissions. It never edits inside a worker worktree directly.
- Each worker agent reads its inbox, performs the assigned task within its declared write set, and submits reports or commits. It never coordinates with other workers directly.

All judgment and routing flow through the orchestrator. That separation is the entire point.

</details>

## Again, Multorum Is NOT a Vibe-Drown Agentic Orchestration System

Multorum is not a merge tool with a better attitude. It is not a chat protocol pretending to be a runtime. It is not a replacement for orchestration logic. It is not a system that assumes parallel work will behave nicely if everyone expresses themselves clearly.

It assumes the opposite, because it believes in the power of hard boundaries and mechanical guarantees.

## In Conclusion (❁´◡`❁)

Multorum is for orchestrated parallel development on a single repository with isolated workspaces, explicit file ownership, and conflict freedom enforced by construction.

If you are running multiple workers against one codebase and are tired of treating merge pain as an immutable law of the universe, Multorum is the tool.
