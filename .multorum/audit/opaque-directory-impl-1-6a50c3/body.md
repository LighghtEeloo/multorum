Adds the opaque directory primitive (`Definition::Opaque(DirectoryPath)`) to the fileset algebra, implementing the design from DESIGN.md.

The `DirectoryPath` type validates against metacharacters and normalizes trailing slashes. The three-phase compilation pipeline resolves opaques first, builds a reduced file list, then expands globs against only non-opaque files. Validation rejects overlapping opaque prefixes.

8 new integration tests and unit tests across all changed modules confirm correctness. All existing tests continue to pass.
