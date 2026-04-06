# Tasks: {{change_name}}

## Phase 1: Preparation
<!-- 
Initial setup and exploration tasks.
These ensure the worker understands the context before modifying code.
-->

- [ ] 1.1 Read the task bundle from inbox
  - Review proposal.md for intent and scope
  - Review perspective.md for file boundaries
  - Review design.md for approach
  
- [ ] 1.2 Verify write-set boundaries
  - Run: `multorum local contract`
  - Confirm understanding of which files can be modified
  - Note: CANNOT create files outside write set

- [ ] 1.3 Review read-set files
  - Read all files in the read set for context
  - Understand stable interfaces and contracts
  - Note dependencies and integration points

- [ ] 1.4 Explore relevant codebase
  - Read existing implementation in write-set files
  - Understand patterns and conventions used
  - Identify test files and examples

## Phase 2: Implementation
<!-- 
Core implementation tasks.
Break these down based on the specific work being done.
Modify or expand these tasks as needed.
-->

- [ ] 2.1 [First implementation step]
  - Details...
  
- [ ] 2.2 [Second implementation step]
  - Details...
  
- [ ] 2.3 [Third implementation step]
  - Details...

## Phase 3: Verification
<!-- 
Tasks to verify the work is correct before submission.
-->

- [ ] 3.1 Run project checks
  - If check pipeline defined in rulebook.toml:
    - Run: `cargo fmt` (or project equivalent)
    - Run: `cargo clippy` (or project equivalent)
    - Run: `cargo test` (or project equivalent)

- [ ] 3.2 Verify write-set compliance
  - Review all modified files
  - Confirm all changes are within write set
  - Ensure no files outside write set were modified

- [ ] 3.3 Validate against success criteria
  - Review success criteria from proposal.md
  - Verify each criterion is met
  - Document any deviations or trade-offs

## Phase 4: Submission
<!-- 
Final tasks to submit the completed work.
-->

- [ ] 4.1 Prepare commit
  - Stage all changes: `git add ...`
  - Write clear commit message
  - Commit: `git commit -m "..."`
  - Note: Use `git commit --allow-empty` for analysis-only work

- [ ] 4.2 Create submission bundle
  - Summarize what was done
  - Reference completed tasks
  - Include any relevant evidence or notes
  - Submit: `multorum local commit --head-commit <commit>`

## Blocker Handling
<!-- 
If blocked during implementation:

1. Document the blocker clearly
2. Send report: `multorum local report --body-text "..."`
3. Include:
   - What you were trying to do
   - What blocked you
   - What you need from orchestrator
   - Any relevant context or evidence
4. Wait for `resolve` message in inbox
5. Acknowledge resolution: `multorum local ack <sequence>`
-->

## Notes
<!-- 
Space for worker to add notes during implementation:
-->

