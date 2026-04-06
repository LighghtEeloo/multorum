# Perspective: {{change_name}}

## Overview
<!-- 
Name of the perspective and its purpose.
This perspective defines a role with specific file ownership boundaries.
-->

Perspective Name: `{{perspective_name}}` (e.g., "AuthRefactor", "ApiImplementation")

## File-Set Definitions
<!-- 
Define named file sets using Multorum's algebra.
These names become the vocabulary for write/read sets.

Syntax:
- Name.glob = "pattern"       (glob pattern)
- Name.opaque = "path/"       (exclusive directory ownership)
- Name = "Expr"               (compound: A | B, A & B, A - B)
-->

```toml
# Example file sets:
# SourceFiles.glob = "src/**/*.rs"
# TestFiles.glob = "**/tests/**"
# DocsFiles.glob = "docs/**/*.md"
#
# FeatureFiles.glob = "src/feature/**"
# FeatureTests = "FeatureFiles & TestFiles"
# FeatureImpl = "FeatureFiles - FeatureTests"
```

Your file sets:
```toml
# Define your file sets here:

```

## Write Set
<!-- 
Files this worker may MODIFY (closed list).

CRITICAL CONSTRAINTS:
- This is a CLOSED list of EXISTING files only
- Workers CANNOT create new files outside this set
- If new files are needed, orchestrator must create them first
- Workers CAN delete files within their write set

Use file-set expressions to define the boundary.
-->

**Expression:** 
```
# Example: FeatureImpl
# Example: AuthFiles - AuthTests - AuthSpecs
```

**Resolved files (at worker creation time):**
<!-- This will be populated when perspective is compiled -->

## Read Set  
<!-- 
Files that must remain STABLE while this worker is active.

Purpose:
- Tells Multorum what concurrent work must not disturb
- Defines the worker's stable context
- Workers can READ the entire repository; this set is for conflict detection

Keep this narrow - listing too many files blocks concurrent work.
Include only: specs, interfaces, shared types, configuration.
-->

**Expression:**
```
# Example: SpecFiles | InterfaceFiles
# Example: docs/auth*.md
```

**Resolved files (at worker creation time):**
<!-- This will be populated when perspective is compiled -->

## Conflict Analysis
<!-- 
Before creating this worker, validate:

1. Write set does not overlap with other active workers' write sets
2. This worker's write set does not intersect other workers' read sets
3. This worker's read set does not intersect other workers' write sets

Run validation:
```bash
multorum perspective validate {{perspective_name}}
```
-->

## Boundary Evolution
<!-- 
If this perspective's boundaries need to expand:

1. Worker must be in non-ACTIVE state (BLOCKED or COMMITTED)
2. Orcheator updates rulebook.toml
3. Run: multorum perspective forward {{perspective_name}}
4. New boundary must be a SUPERSET of current boundary

Boundary reduction is NOT allowed for live workers.
-->
