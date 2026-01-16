# CHECKPOINT CP-XXX: [PHASE/MILESTONE DESCRIPTION]

<!--
================================================================================
AI ASSISTANT INSTRUCTIONS - CHECKPOINT TEMPLATE
================================================================================

This template defines the structure for all checkpoint documents. When creating
a checkpoint, the AI Assistant MUST follow these rules:

BEFORE CREATING THE CHECKPOINT:
1. Perform Knowledge Base Synchronization (see CHECKPOINT-PROTOCOL.md Article 3)
   - Review and update PROJECT-DEVELOPMENT-ELABORATION.md
   - Review and update relevant ADRs
   - Review and update DEVELOPER-GUIDE.md if APIs changed
   - Review and update DEVELOPMENT-PRACTICES.md if new patterns emerged
   - Update registry.json with any new work orders
   - Update README.md if user-facing features changed

2. Run all tests and verify system stability

3. Gather all information needed to fill this template

FILLING THIS TEMPLATE:
- Replace all [BRACKETED_PLACEHOLDERS] with actual values
- CP-XXX: Use next sequential number (check existing checkpoints)
- Delete this instruction block from the final document
- Preserve all section headings even if a section is "None"
- Use tables for structured data
- Include verification commands that actually work

NAMING THE FILE:
- Format: CP-XXX-PHASE-Y-DESCRIPTION.md
- Example: CP-002-PHASE-7-COMPLETE.md
- Example: CP-003-MILESTONE-AUTH-SYSTEM.md

================================================================================
-->

---

FACTORY OF ELECTRONIC UNITS AND LOGIC – TEJL d.o.o.
Sector for System Integration and Development

**Class:** [CLASS_CODE]
**Reference Number:** [REF_NUMBER]-CP[XX]
**Date:** [YYYY-MM-DD]

SUBJECT: Internal Development Checkpoint – [DESCRIPTION]

---

## CHECKPOINT SUMMARY

<!--
AI: Fill this table with current project state.
- Checkpoint ID: Next sequential CP number
- Version: Current version from VERSION file or Cargo.toml
- Branch: Current git branch
- Phases Completed: List all completed phase numbers
- Phases Pending: List remaining phases
- Unit Tests: Number passing (run `cargo test` or equivalent)
- Integration Suites: Number passing (run integration test script)
- Status: GREEN (all pass), YELLOW (minor issues), RED (blocking issues)
-->

| Field | Value |
|-------|-------|
| Checkpoint ID | CP-XXX |
| Version | X.Y.Z |
| Branch | `branch-name` |
| Phases Completed | 0, 1, 2, ... |
| Phases Pending | X (Name) |
| Unit Tests | XXX passing |
| Integration Suites | X passing |
| Status | **GREEN/YELLOW/RED** |

---

## DELIVERABLES IN THIS CHECKPOINT

<!--
AI: List all significant deliverables from this phase/milestone.
Group by category (features, subsystems, documentation, etc.)
Use bullet points with clear descriptions.
Include technical details where relevant.
-->

### 1. [Category Name]

- **[Deliverable Name]** (`file.rs::function()`)
  - Description of what was implemented
  - Key technical details

### 2. [Category Name]

- **[Deliverable Name]**
  - Description

---

## FILES MODIFIED/CREATED

<!--
AI: List ALL files that were modified or created during this phase.
Group by category for readability.
Use code blocks for paths.
-->

### [Category]
```
path/to/file1.ext      # Brief description
path/to/file2.ext      # Brief description
```

### [Category]
```
path/to/file3.ext      # Brief description
```

---

## WORK ORDERS GENERATED

<!--
AI: List all work orders created during this phase.
Pull data from PROJECT-INTERNAL/WORK-ORDERS/registry.json
-->

| ID | Title | Status |
|----|-------|--------|
| WO-XXX | [Title] | COMPLETED/IN_PROGRESS/PENDING |

---

## ISSUES RESOLVED

<!--
AI: Document any significant issues encountered and how they were resolved.
This section serves as institutional memory for future debugging.
If no issues, write "None - implementation proceeded without significant issues."
-->

1. **[Issue Title]**
   - **Problem:** Description of what went wrong
   - **Solution:** How it was fixed
   - **Prevention:** How to avoid in future (if applicable)

---

## NEXT STEPS

<!--
AI: List concrete next steps from PROJECT-DEVELOPMENT-ELABORATION.md
Use checkboxes for trackable items.
Reference specific phase numbers and task IDs.
-->

- [ ] X.Y.a. [Task description]
- [ ] X.Y.b. [Task description]

---

## VERIFICATION COMMANDS

<!--
AI: Provide working commands to verify this checkpoint state.
These should be copy-pasteable and actually work.
Include expected output indicators where helpful.
-->

```bash
# Run all unit tests
[command]

# Run integration tests
[command]

# Verify specific feature
[command]
```

---

**Recorded by:**
AI Assistant, System Integration Sector

**Checkpoint Status:** APPROVED FOR CONTINUATION

