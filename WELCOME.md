# AI Development Protocol
## Entry Point for AI Assistants

**AI ROLE:** Development Assistant & System Architect
**VERSION:** 1.0

---

## Welcome, AI Assistant

You have been granted access to this repository. Before proceeding with any development work, you must read and internalize the protocols defined in this project's internal documentation.

---

### 1. Core Operating Principle: External Memory

This project operates on the principle of **Externalized Memory**. You are expected to:

1. **Minimize Internal Memorization:** Do not rely on your internal context to track project state, progress, or decisions. Your context window is finite and will be lost between sessions.

2. **Read Before Acting:** Always consult the project documentation before making changes. The files in `docs/` contain the current state of the project, active tasks, architectural decisions, and operational protocols.

3. **Write After Acting:** After completing any significant work, you must update the relevant documentation files. This ensures continuity for yourself (in future sessions) and for any human or AI collaborator who continues the work.

4. **Trust the Documents:** The project files are the **Single Source of Truth**. If there is a conflict between your memory and the documentation, the documentation is correct.

---

### 2. The Single Source of Truth: `docs/`

All binding instructions, protocols, architectural decisions, and development tasks reside within the `docs/` directory. You are to operate strictly based on the information contained therein.

**Directory Structure:**

| Folder | Purpose |
|--------|---------|
| `ARCHITECTURE/` | Architecture Decision Records (ADRs). Consult before proposing structural changes. |
| `DOCUMENTS/` | Project planning documents, roadmaps, and elaborations. |
| `GUIDES/` | Developer guides, coding standards, and AI operational instructions. |
| `WORK-ORDERS/` | Registry of all formal Work Orders issued during development. |

---

### 3. Document Hierarchy (Read in This Order)

Before writing any code, you must read and understand the following documents:

1. **`docs/GUIDES/AI-INSTRUCTIONS.md`** — **The Constitution**
   Contains the unbreakable rules of engagement, communication methodology, and formal protocols for collaboration. This is your most important document.

2. **`docs/DOCUMENTS/PROJECT-DEVELOPMENT-ELABORATION.md`** — **The Backlog**
   The official project blueprint and task list. It outlines completed phases, defines the current active phase, and lists specific tasks to be executed. Your work is driven by this document.

3. **`docs/GUIDES/DEVELOPER-GUIDE.md`** — **The Onboarding Guide**
   Instructions for developers (human and AI) on how to approach development in this project.

4. **`docs/GUIDES/DEVELOPMENT-PRACTICES.md`** — **The Knowledge Base**
   Codified solutions to recurring problems. You are responsible for updating this document when you identify and solve recurring issues.

5. **`docs/ARCHITECTURE/`** — **Architectural Records**
   Formal records of significant technical decisions. Consult before proposing changes that affect system architecture.

---

### 4. Work Order Protocol

All development work must be formalized through the **Work Order** system:

1. **Before Coding:** Generate and issue a formal Work Order describing the task, its context, and expected outcomes.

2. **During Execution:** Follow the Work Order precisely. If scope changes, issue a new Work Order.

3. **After Completion:** Log the Work Order to `docs/registry.json` with all metadata (time expenditure, tasks completed, etc.).

4. **Function Definitions:** The schema for Work Order logging is defined in `function-calls.json` in the project root.

---

### 5. What NOT to Do

- **Do NOT execute tasks from `docs/NOTES.md`** — This file is a scratchpad for human brainstorming. Ideas there are not actionable until formally added to the backlog.

- **Do NOT propose architectural changes without consulting existing ADRs** — Ensure your proposals are consistent with past decisions.

- **Do NOT use placeholders in code** — When providing code, always provide complete, copy-pasteable content. No `// ... existing code ...` shortcuts.

- **Do NOT strip comments** — Comments are code. If you change logic, update the corresponding comments.

- **Do NOT manually edit lockfiles** — Use official package manager commands for dependency changes.

---

### 6. Session Initialization

At the start of each work session:

1. **Acknowledge this protocol** by confirming you have read this document.

2. **Read the Constitution** (`AI-INSTRUCTIONS.md`) to load the rules of engagement.

3. **Identify Current Task** by reviewing `PROJECT-DEVELOPMENT-ELABORATION.md` to determine the active phase and next specific task.

4. **Read the Developer Guide** to understand the project's technical architecture and standards.

5. **Commence Work** by issuing the first formal Work Order for the identified task.

---

### 7. Communication Standards

- Maintain a **formal, precise, and technically accurate** tone.
- Use **structured responses** with clear headers and sections.
- When in doubt, **ask for clarification** rather than assuming.
- **Admit errors formally** and immediately propose corrective action.
- Document everything — your work should be traceable and reproducible.

---

### 8. Continuity Guarantee

By following these protocols, you ensure that:

- Any AI assistant can pick up where you left off.
- Any human developer can understand what was done and why.
- The project maintains consistency across sessions and collaborators.
- Knowledge is preserved in the repository, not lost in context windows.

---

**COMMAND:** Acknowledge this protocol and proceed to read `docs/GUIDES/AI-INSTRUCTIONS.md`.
