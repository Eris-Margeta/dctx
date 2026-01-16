# CHECKPOINT PROTOCOL

**"Protocol on the Methodology of Development Checkpoints"**

This document establishes the binding procedure for creating and maintaining development checkpoints as part of the TEJL development workflow.

---

**FACTORY OF ELECTRONIC UNITS AND LOGIC – TEJL d.o.o.**
Board for Standardization and Development

**Class:** 001-01/25-01
**Reference Number:** 251-01-01-25-CP (Rev. 1)
**Date:** 2026-01-16

SUBJECT: Protocol on Development Checkpoints and Knowledge Base Synchronization

---

### **Article 1: Purpose and Scope**

(1) A **Checkpoint** is a formal milestone marker that captures the complete state of a project at a significant point in development.

(2) The purpose of checkpoints is to:
*   (a) Preserve institutional knowledge and prevent data loss across sessions.
*   (b) Provide clear demarcation points between development phases.
*   (c) Ensure all documentation, guides, and architectural records are synchronized.
*   (d) Create a traceable history of project evolution.

(3) Checkpoints are **mandatory** at the end of each development phase and **optional** upon Operator request at any significant milestone.

---

### **Article 2: Checkpoint Triggers**

(1) **Mandatory Triggers:** A checkpoint MUST be created when:
*   (a) A development phase (as defined in PROJECT-DEVELOPMENT-ELABORATION.md) is completed.
*   (b) A major version increment occurs.
*   (c) An ADR implementation is finalized.

(2) **Optional Triggers:** A checkpoint MAY be created when:
*   (a) The Operator explicitly requests one.
*   (b) A significant feature or subsystem is completed.
*   (c) Before a major architectural refactoring.
*   (d) At the end of a long development session.

---

### **Article 3: Knowledge Base Synchronization (Pre-Checkpoint)**

(1) Before creating a checkpoint document, the AI Assistant is **obliged** to perform a full Knowledge Base Synchronization. This ensures all project documentation reflects the current state.

(2) **Documents Subject to Synchronization:**

| Category | Location | Documents |
|----------|----------|-----------|
| Core Documentation | `PROJECT-INTERNAL/DOCUMENTS/` | PROJECT-DEVELOPMENT-ELABORATION.md, VISION.md, WHITEPAPER.md |
| Guides | `PROJECT-INTERNAL/GUIDES/` | AI-INSTRUCTIONS.md, DEVELOPER-GUIDE.md, DEVELOPMENT-PRACTICES.md, RUST-STYLE-GUIDES.md |
| Architecture | `PROJECT-INTERNAL/ARCHITECTURE/` | All ADR-*.md files, adr-template.md |
| Work Orders | `PROJECT-INTERNAL/WORK-ORDERS/` | registry.json |
| Root Documentation | `/` | README.md, CHANGELOG.md, CONTRIBUTING.md |

(3) **Synchronization Procedure (Article 8 of MDAAI applies):**

For each document that requires updates, the AI Assistant must follow the **REVISION Phase** protocol:
*   (a) **Phase 1: Analysis and Proposal** - Identify what has changed and propose updates.
*   (b) **Phase 2: Agreement and Alignment** - Discuss with Operator if changes are significant.
*   (c) **Phase 3: Final Verification and Execution** - Apply updates with complete content.

(4) **Update Principles:**
*   (a) **Incremental Updates Only:** Never rewrite entire documents. Add to or modify specific sections.
*   (b) **Preserve History:** Do not delete historical information. Mark completed items, don't remove them.
*   (c) **Version Headers:** Update revision numbers and dates in document headers.
*   (d) **Cross-Reference Integrity:** Ensure all internal links and references remain valid.

---

### **Article 4: Checkpoint Document Structure**

(1) All checkpoint documents must be stored in `PROJECT-INTERNAL/CHECKPOINTS/`.

(2) **Naming Convention:** `CP-XXX-PHASE-Y-DESCRIPTION.md`
*   `XXX` = Sequential checkpoint number (001, 002, etc.)
*   `Y` = Phase number
*   `DESCRIPTION` = Brief descriptor (e.g., "COMPLETE", "MILESTONE")

(3) **Required Sections:** Every checkpoint document must contain:

| Section | Purpose |
|---------|---------|
| Header | Standard TEJL header with Class, Reference Number, Date |
| Checkpoint Summary | Table with ID, Version, Branch, Phases, Test Status |
| Deliverables | Detailed list of what was accomplished |
| Files Modified/Created | Complete list of changed files |
| Work Orders Generated | Table of related work orders |
| Issues Resolved | Problems encountered and solutions applied |
| Next Steps | Pending tasks for the next phase |
| Verification Commands | Commands to validate the checkpoint state |

(4) A template is provided at `PROJECT-INTERNAL/CHECKPOINTS/checkpoint-template.md`.

---

### **Article 5: Checkpoint Creation Procedure**

(1) The AI Assistant, upon receiving a checkpoint trigger, must execute the following procedure:

**Step 1: Announce Checkpoint Initiation**
```
"Initiating Checkpoint Protocol for [PHASE/MILESTONE]..."
```

**Step 2: Knowledge Base Synchronization**
*   Review all documents listed in Article 3.
*   Propose and apply necessary updates following REVISION protocol.

**Step 3: Generate Checkpoint Document**
*   Copy template from `checkpoint-template.md`.
*   Fill all sections according to Article 4.
*   Assign next sequential checkpoint number.

**Step 4: Verification**
*   Run all tests to confirm system stability.
*   Document test results in checkpoint.

**Step 5: Confirmation**
*   Report checkpoint completion to Operator.
*   Provide checkpoint ID and summary.

---

### **Article 6: Checkpoint Retrieval and Reference**

(1) Previous checkpoints serve as **binding historical records**.

(2) The AI Assistant must reference relevant checkpoints when:
*   (a) Resuming work after a session break.
*   (b) Investigating the origin of a bug or design decision.
*   (c) Reporting on project progress.

(3) Checkpoints are **never modified** after creation. If corrections are needed, a new checkpoint with amendments is created.

---

### **Article 7: Relationship to Other Protocols**

(1) This protocol is complementary to:
*   (a) **MDAAI Protocol (AI-INSTRUCTIONS.md)** - Article 8 (Revision of Strategic Documents) governs the synchronization phase.
*   (b) **ADR Protocol** - Checkpoints may reference ADRs but do not replace them.
*   (c) **Work Order Protocol** - Checkpoints summarize but do not replace work order tracking.

(2) In case of conflict, the MDAAI Protocol takes precedence.

---

### **Article 8: Final Provisions**

(1) This protocol enters into force on the day of its adoption.

(2) Failure to create mandatory checkpoints is considered a systemic deviation requiring corrective action.

---

**Compiled by:**
AI Assistant, System Integration Sector

**Approved by:**
Operator, Board for Standardization and Development

