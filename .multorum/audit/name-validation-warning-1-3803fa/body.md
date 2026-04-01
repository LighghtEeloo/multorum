Reviewed worker submission `3803fa065df8ba19012c2faa843bfc6830e409c8` on top of `6645fd6c1c4aa7abcdd3e7068af6c74c8992e001`.

Audit findings:
- Rulebook deserialization now logs warnings and ignores invalid fileset and perspective names in owned schema modules.
- Orchestrator MCP tool parsing now logs warnings and returns tool-level `check_failed` results for invalid worker and perspective names, while malformed resource worker ids log warnings and map to `resource_not_found`.
- CLI parsing now accepts raw worker and perspective strings, defers validation into typed helpers in `src/cli.rs`, logs warnings, and returns runtime `check failed` instead of a Clap parse abort.
- Focused interface and transport tests passed under review, and direct CLI verification confirmed `cargo run -- worker create lowercase_bad --body-text test` now fails with runtime `check failed` rather than Clap argument parsing.

Conclusion:
- The user-facing surfaces covered by this task now route invalid naming checks through warning-plus-runtime/tool error handling instead of parser aborts, while internal constructor invariants remain strict.