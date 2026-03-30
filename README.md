# Multorum

Multorum is the infrastructure for orchestrated parallel development on a single repository with isolated workspaces, explicit file ownership, and conflict freedom enforced by construction. It doesn't care about the specific workflow, agentic tooling, or development process you use. It is a general-purpose tool for making parallel work safe and efficient, regardless of how you choose to organize it.

> Belua multorum es capitums!
> 
> <p align="center">
>   <img src="assets/multorum-20260327.png" alt="Multorum logo" width="40%">
> </p>
>
> [EN](README.md) | [CN](README-CN.md)

## Definition by Negation

Multorum is not an agent. It does not plan, negotiate, ideate, or otherwise hallucinate that it is management. It is infrastructure for orchestrated parallel work: isolated workspaces, explicit ownership, and hard guarantees that workers do not quietly sabotage each other.

## Y?

Parallel development breaks in two ways:

- either people work freely and pay for it later in merge hell, or ..
- .. they are boxed into such narrow sandboxes that they lose the context needed to do good work.

Multorum is built to avoid that tradeoff. Each worker keeps the full repository as readable context, but authorship is constrained to an explicit write scope, so the system preserves both global understanding and local isolation.

No vibes. No handshake deal. No "please try not to touch that."

Our approach is meant to be correct by construction. Coordination is not left to etiquette, guesswork, or cleanup after the fact; it is encoded directly into the model. The orchestrator remains the sole authority, scopes are declared up front, and conflicting access patterns are rejected before work begins rather than repaired later. So the project is not trying to create magical autonomous teamwork. It is trying to make parallel work mechanically safe, inspectable, and disciplined by design.

> For a detailed design reference, see [DESIGN.md](DESIGN.md).

## The Model

Everything revolves around three things: the orchestrator, the rulebook, and the workers.

The *orchestrator* is the sole coordinator. Human, model, or hybrid — Multorum does not care. It decides what work gets split up, which role gets which task, and which results deserve to survive.

The *rulebook* (`.multorum/rulebook.toml`) defines named perspectives. A perspective is a role with two boundaries: a *write set* (what it may modify) and a *read set* (what must stay stable while it works). The read set is not a visibility filter. Workers can inspect the whole repo. It exists so Multorum knows what concurrent work is forbidden to disturb.

A *worker* is a live instance of a perspective. Multorum gives it an isolated *git worktree*, a pinned base snapshot, and materialized boundary files. The contract becomes a real workspace with real limits. Before a worker's changes land, Multorum enforces write-scope compliance and runs project checks (build, lint, test) through *git hooks* declared in the rulebook.

Multiple workers created from the same perspective form a *bidding group*. They share the same boundaries, start from the same snapshot, and race independently. At most one merges. The others are discarded.

## The Guarantee

Multorum is built around one invariant:

> A file may be written by exactly one active bidding group, or read by any number of active bidding groups, but never both.

That is it. The soul of this product. The rest is just plumbing.

Concurrent write scopes should never overlap, and no active group may write into files another group depends on as stable context. Multorum rejects bad overlap before work starts. After all, it is better to fail people fast with realistic disappointment, rather than after hours of work ending in a catastrophic merge conflict.

## The Lifecycle

The orchestrator composes the rulebook, then creates workers from its perspectives. Each worker operates inside its own worktree, reports progress through the runtime surface, and eventually submits work. The orchestrator can merge the result, revise it, or discard it. The worker can work in the worktree no matter what, and when it's done, the worktree can be deleted along with the worker itself.


## Installation

If anything above intrigued you (or at least didn't scare you away), here's how to get started.

After installing [Rust](https://rust-lang.org/tools/install), you can install stable Multorum with Cargo:

```bash
cargo install multorum
```

Distribution packages and prebuilt binaries are on their way, once the project is production ready.

If you want the latest yet potentially unstable features, you can install directly from the GitHub repository:

```bash
cargo install --git https://github.com/LighghtEeloo/multorum.git
```

Use `cargo uninstall multorum` to completely remove it. We won't litter. Promise.

<details>
<summary><strong>Shell completion</strong></summary>
Multorum can generate shell completion scripts for Bash, Zsh, Fish, Elvish, and PowerShell. You can source those scripts directly from the binary for an always-up-to-date experience.

```bash
# bash
command -v multorum &>/dev/null && source <(multorum util completion bash)

# zsh
autoload -U compinit
compinit
command -v multorum &>/dev/null && source <(multorum util completion zsh)

# fish
command -v multorum &>/dev/null && multorum util completion fish | source

# elvish
command -v multorum &>/dev/null && source <(multorum util completion elvish)

# powershell
multorum util completion powershell | Out-String | Invoke-Expression
```
</details>

## What's next?

Multorum is designed to be usable by both humans and LLMs, but LLMs have the unfair advantage of generating way more detailed instructions and contexts than mortals (like me). To that end, you can jump directly to [setting up an MCP server](#using-multorum-with-mcp).

### Using Multorum with CLI (mainly as a human)

Multorum has pretty good CLI ergonomics for human users.
- Press tab to see available options.
- Check out `--help` at all levels of commands and you should be knowledgeable enough to run the orchestrator and workers manually.
- Search for any concept in doubt in [DESIGN.md](DESIGN.md) and you should find a detailed explanation.
- To understand the core idea, run `multorum util methodology <role>` to see short markdown documents on these subjects.

I'd say most human users can understand most concepts within 10 minutes of poking around.

### Using Multorum with MCP

If your orchestrator is an MCP-capable agent, you can run the whole Multorum loop through tool calls instead of ad-hoc shell choreography.

Multorum ships the high-level orchestrator and worker guidance inside the binary and MCP surfaces. That guidance is the canonical bootstrap text. Repository-local skills, when present, are only thin wrappers that point agents at the shipped methodology.

<details>
<summary><strong>MCP installation guide</strong></summary>

#### 1) Add the orchestrator MCP server

Add this to your MCP host config. Optionally, pass the canonical workspace root explicitly so the server cannot bind itself from an accidental host `cwd`.

```json
{
  "mcpServers": {
    "multorum-orchestrator": {
      "command": "multorum",
      "args": ["serve", "orchestrator"],
    }
  }
}
```

#### 2) Add a worker MCP server

Repeat for each worker worktree. Optionally, pass that specific worktree explicitly so the worker server cannot accidentally bind to the canonical root or another repo.

```json
{
  "mcpServers": {
    "multorum-worker": {
      "command": "multorum",
      "args": ["serve", "worker"],
    }
  }
}
```

#### 3) Reload and verify

Reload your MCP host and see if the servers are up.
</details>

<details>
<summary><strong>Optional skill bootstrap guide</strong></summary>

This repository also ships minimal skills for hosts that auto-discover prompt files. They should stay thin:

- Orchestrator skill: "You are the orchestrator. Read `multorum://orchestrator/methodology` before acting."
- Worker skill: "You are the worker. Read `multorum://worker/methodology` before acting."

You can install them with

```bash
npx skills add LighghtEeloo/multorum
```

</details>

## Again, Multorum Is NOT a Vibe-Drown Agentic Orchestration System

Multorum is not a merge tool with a better attitude. It is not a chat protocol pretending to be a runtime. It is not a replacement for orchestration logic. It is not a system that assumes parallel work will behave nicely if everyone expresses themselves clearly.

It assumes the opposite, because it believes in the power of hard boundaries and mechanical guarantees.

## Versioning

Multorum follows semantic versioning, but it's a bit boring. So addtionally, Multorum follows *shift versioning™*. 
- The first version will be `0.0.1`, releasing when the core model is well understood without obvious issues.
- The second version will be `0.1.0`, releasing when the implementation details are solidified and battle-tested.
- The third version will be `1.0.0`, releasing when all interfaces are stable and ready for production use.

After that will come the infinite boring maintenance versions, and the development will shift towards ecosystem integration, quality of life improvements, and daily maintenance. I sincerely hope these versions are never surprising to anyone forever.

## In Conclusion (❁´◡`❁)

Multorum is for orchestrated parallel development on a single repository with isolated workspaces, explicit file ownership, and conflict freedom enforced by construction.

If you are running multiple workers against one codebase and are tired of treating merge pain as an immutable law of the universe, Multorum is the tool.

## Credits

I'd like to take a moment here to thank everyone who has contributed to the design and development of Multorum, especially [@17-qxm](https://github.com/17-qxm), [@Yanmeeei](https://github.com/Yanmeeei), and [@TomCC7](https://github.com/TomCC7), as early supporters and adopters.
