# Repository Guidelines

> `CLAUDE.md` is a symlink to this file. Do not edit `CLAUDE.md` directly -- edit `AGENTS.md` instead.


Above all: consolidate key understandings and new findings as inline Rust documentation
(`//!` module docs and `///` item docs) close to the relevant code.
Inline Rust documentation is the canonical documentation source for this repository.
Write `/// Note: ` to explain why unusual design choices / compromises are made.

Prioritize a clean codebase and elegant design over compatibility or migration work.
Do not keep transitional layers, compatibility shims, or legacy interfaces unless they are
explicitly required by the task. If compatibility or migration concerns conflict with a
clearer design, prefer the clearer design.

Before and after any edit to DESIGN.md, evaluate the document as a reader.
- Is the structure clear and logically ordered?
- Does the prose read like it was written by a knowledgeable practitioner, not like generated text?
- Are there redundant or overlapping sections that should be merged or reordered?
Apply these standards to every edit.

## Documentation and Language

Actively write documentation for the program. Make sure *all* public APIs are documented.
All written documentation must be concise, clear, and accurate.
No emojis unless strictly necessary.
Add bold text only if it emphasizes truly valuable information.
All documentation should be written in English unless explicitly stated.
Prefer direct definitions over defensive framing.
- Define what the system does before explaining limits or exclusions.
- Keep definition-by-negation to a minimum; use it only when a nearby confusion is likely and the contrast is genuinely clarifying.
- Avoid prose that reads like a rebuttal, disclaimer, or argument with an imaginary reviewer.
- When documenting a constraint, state the positive rule first, then the consequence if needed.

When writing DESIGN.md, follow these style guidelines:
- Declarative, dry, precise.
- Each paragraph does one thing: introduces a concept, states its properties, and stops.
- No motivational framing, no rhetoric, no "this is important because."
- Terms are introduced once and then trusted to carry themselves.
- The voice is impersonal but not bureaucratic so that it reads closer to
  a concise mathematical text than to a software README.
- Sentences are structurally simple, favoring short main clauses over nested subordination.
- Analogies appear sparingly and only to established PL concepts (well-typedness, nominal binding),
  never to everyday metaphors.

### Canonical Documentation Location

- Prefer inline Rust documentation in `src/**/*.rs`.
- Keep design rationale near the owning modules and types.
- Do not rely on a standalone `docs/` tree as the canonical source.

## Rust Code Style Guideline

Panic fast. If some edge case is not specified by design, log and panic.
Never attempt to recover from an error path if it's not absolutely correct.

Prefer declaration instead of manual implementation. For example,
- Utilize `thiserror` crate for error messages instead of manual implementations.
- Encode invariants into Rust's type system, so that they are enforced by the compiler.
  - Write documentation about invariants per struct, field, function, and method.
  - Use "constructor" or "builder" pattern for creating instances that satisfy the invariants.
- Prefer `serde` for serialization and deserialization instead of manual parsing and pretty printing.
- Prefer derive-style `clap` for command-line argument parsing.

Always prefer typed data structures over strings + parsers, and
Prefer introducing a named type over reusing a generic one (e.g., `String`, `HashMap`)
when the type carries domain meaning.
For examples,
- Include specific types of errors when creating an error type, not just strings.
- User input should be parsed to be structured data as soon as possible.
- Never use strings to represent states in the software's state machine.
- Never pass strings between internal components when the message could be typed.
- Whenever a hashmap of strings is created, consider whether the keys represent
  a closed set of fields -- if so, replace it with a struct or a trait object.

Prefer to use structs to pack a group of useful functions; prefer methods over functions.
Rust structs have better namespace-ish features than Rust modules.
Prefer methods on a struct over free functions. Free functions are acceptable only for
trait implementations, entry points (`main`), or cases where no meaningful receiver exists.
When defining methods, abide by the following rules:
- Mention `self` in the signature if the methods are built around the struct type.
  - Take ownership (`self`) if being the elimination form of the struct type,
    namely consuming the struct.
  - Take reference (`&self` or `&mut self`) if the struct only needs to be borrowed.
- Use associated functions (similar to static methods) when the struct is purely a namespace;
  specifically, write `fn new` for "constructors" with no perspective,
  and `fn with_*` for "constructors" that hints how the struct is created.

For builder patterns, pick receivers based on whether the finalizer must move owned fields out.
If build/finish consumes,
- Use `fn build(self) -> T` for the builder.
- Make all setter methods take and return self `fn with_*(mut self, ...) -> Self` for easy chaining.
If build can borrow,
- Prefer setters `fn set_*(&mut self, ...) -> &mut Self`, and
- Prefer a finalizer `fn build(&self) -> T` so the builder can be reused.
Expose an associated entry point `fn new(required, ...)`,
and use `with_*/set_*` names consistently for optional configuration.

Add concise yet critical documentation for structs, fields, and methods.
Ensure that documentation is clear, concise, and accurate. No emojis unless strictly necessary.

When adding new features, record and observe details with the `tracing` crate.
Use `trace` level at the beginning and end of major interface calls to record arguments.
Use `debug` for task-specific debug output, and remove them when the problem is solved.

Run `cargo clippy` and `cargo fmt` before committing.

Avoid using nested `super::` imports. Use absolute paths in that case.

## Version Control

This project uses git. Use git to operate.

### Commit Message Convention

Format: `prefix: lowercase description`

No capitalization after the colon. No trailing period. One line.
The description should say *what changed*, not *why* (the diff shows what; the description names it).

#### Prefix Vocabulary

| Prefix | When to use |
|--------|-------------|
| `feat`  | A user-visible capability that did not exist before. |
| `incr`  | Incremental progress on an existing feature: bug fixes, polish, tuning, small additions. |
| `sisy`  | Mechanical changes: formatting, linting, renaming passes, internal restructuring with no behavior change. |
| `vibe`  | Exploratory, prototype-quality work. Expect rough edges; may be revised or replaced. |
| `repo`  | Repository housekeeping: migrations, dependency changes, formatter config, file reorganization, one-off maintenance. |
| `docs`  | Documentation-only changes (AGENTS.md, README, inline Rust docs/comments). |
| `test`  | Adding or updating tests without changing production code. |

#### Guidelines

- One logical change per commit. If two things can be reverted independently, they are two commits.
- Pair implementation files with their tests in the same commit.
- Order commits by dependency level: types and utilities first, then logic, then UI, then config.
- Prefer many small commits over one large commit. Rule of thumb: a reviewer should understand a commit in under 30 seconds.
