# Merge rationale: DESIGN.md rulebook semantics correction

This merge applied findings 1 and 2 from the design analysis:

1. Replaced the non-existent `rulebook install` activation model.
2. Corrected effect timing so the document matches current behavior: operations that compile policy read `.multorum/rulebook.toml` from the current working tree, so on-disk edits affect subsequent operations immediately.

Remaining findings retained as supplementary notes:

3. Worker-id reuse behavior is still underspecified in docs.
   Explicit worker-id reuse with an existing finalized worktree requires `--overwriting-worktree`.
4. Analysis-only completion path is not explicit in lifecycle docs.
   The canonical path for no-code deliverables should be documented.
5. Repeated normative statements increase drift risk.
   Consolidating duplicated guidance would reduce future contradictions.
