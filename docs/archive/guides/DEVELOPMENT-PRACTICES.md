Historical design record; superseded by [the current documentation](../../../CONTRIBUTING.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# DEVELOPMENT-PRACTICES Document

---

**FACTORY OF ELECTRONIC UNITS AND LOGIC – TEJL d.o.o.**
Board for Standardization and Development

  **Class:** 003-05/25-01
  **Reference Number:** 251-01-03-25-03 (Rev. 3)
  **Date:** 31.12.2025.

  SUBJECT: Technical Regulations on Concrete Development Practices
  Technical Annex to the MDAAI Protocol

---

## **Article 1: Purpose**

  (1) These regulations establish the binding technical practices for the development, execution, and maintenance of software projects.

  (2) The purpose of these regulations is to codify solutions for observed problems, prevent future regressions, and ensure the highest level of consistency, maintainability, and code quality.

  (3) The provisions of these regulations are complementary to and as binding as the "Protocol on the Methodology of Development with the Assistance of Artificial Intelligence (MDAAI)" itself.

## **Article 2: Relationship to Architecture Decision Records (ADRs)**

  (1) **Scope Distinction:** This document codifies the **"how"**—specific, concrete technical solutions and best practices for recurring problems. In contrast, Architecture Decision Records (ADRs), located in `docs/`, codify the **"why"**—the rationale behind significant, high-level architectural choices.

  (2) **Workflow:** An ADR is created to decide on a major architectural path (e.g., choosing a database technology). Once that path is implemented, if specific, recurring technical challenges or patterns emerge, their solutions are then formalized as new articles in *this* document.

---

[**Instruction for the AI Assistant:** This document is your responsibility. In accordance with the "MDAAI Protocol," you are obliged to proactively identify recurring problems or inefficient patterns during development. When such a pattern is identified and resolved, it is your duty to formalize the solution by adding a new article to this document. Before proposing a new practice, consider if the issue is a high-level architectural choice that requires an ADR instead. Maintaining this document is key to the long-term quality and efficiency of the project.]

---

## **Article 3: Gitignore Pattern Parsing Must Handle Inline Comments**

(1) **Problem:**
During Phase 10.7.3 (WO-064), a critical bug was discovered where gitignore-style patterns with inline comments (e.g., `.turbo/  # Turborepo`) were failing to match because the parser was not stripping the comment portion. This caused directories like `.turbo/`, `.pnpm-store/`, and `.localstack/` to be recursed into despite having correct ignore rules in the global ignore file.

(2) **Binding Solution:**
All functions that parse gitignore-style pattern files MUST strip inline comments (everything after the first unescaped `#` character) before processing the pattern. The pattern should be trimmed of trailing whitespace after comment removal.

(3) **Implementation Procedure:**
```rust
// CORRECT: Strip inline comments before processing
pub fn parse_ignore_pattern_line(line: &str) -> Option<IgnoreRule> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    // Strip inline comments (everything after unescaped #)
    if let Some(comment_pos) = trimmed.find('#') {
        trimmed = trimmed[..comment_pos].trim_end();
        if trimmed.is_empty() {
            return None;
        }
    }

    // Now process the pattern...
}
```

**PROHIBITION:** Do NOT parse gitignore patterns without stripping inline comments first. This will cause silent pattern match failures.

(4) **Reference:** WO-064, Phase 10.7.3 (2026-02-02)

---

## **Article 4: Use IgnoreEngine for File Handling Decisions**

(1) **Problem:**
Prior to Phase 10.7.3, ignore decision logic was scattered across multiple files (walker.rs, ignore.rs, llm_formatter.rs), using boolean flags and manual pattern matching. This created implicit state, made debugging difficult, and led to the recursion bug.

(2) **Binding Solution:**
All file handling decisions MUST be made through the centralized `IgnoreEngine` component. The engine provides a single decision point that returns an explicit `FileHandlingDecision` enum value.

(3) **Implementation Procedure:**
```rust
// CORRECT: Use IgnoreEngine for decisions
use dctx_core::ignore::IgnoreEngine;
use dctx_core::domain::{ItemContext, FileHandlingDecision};

// Build engine once at start
let engine = IgnoreEngine::build(&base_dir)?;

// Make decisions during traversal
let context = ItemContext {
    relative_path: "src/main.rs",
    name: "main.rs",
    is_directory: false,
    debug_mode: false,
};

let decision = engine.decide(&context);

match decision {
    FileHandlingDecision::Exclude => {
        // Skip entirely, don't recurse
    },
    FileHandlingDecision::DebugVisible => {
        // Show in tree with [IGNORED], don't read content
    },
    FileHandlingDecision::OmitContent => {
        // Show in tree, omit content with placeholder
    },
    FileHandlingDecision::Include => {
        // Normal processing
    },
}
```

**PROHIBITION:** Do NOT manually check ignore patterns using `should_ignore_item()` directly in walker or formatter code. Always delegate to `IgnoreEngine::decide()`.

**PROHIBITION:** Do NOT use the deprecated `load_ignore_rules()` function in new code. Use `IgnoreEngine::build()` instead.

(4) **Benefits:**
- Single source of truth for decisions
- Observable via statistics and logging
- Debuggable via `engine.explain()` method
- Type-safe with exhaustive match checking
- Testable in isolation

(5) **Reference:** WO-064, Phase 10.7.3 (2026-02-02), ADR-015

---

**(TEMPLATE FOR NEW ARTICLES - COPY AND FILL BELOW)**

## **Article X: [Title of the specific practice]**

(1) **Problem:**
`[A clear and concise description of the recurring technical problem, error, or inefficiency that was observed.]`

(2) **Binding Solution:**
`[A precise technical description of the mandated solution. This should be the standardized way to handle the problem going forward.]`

(3) **Implementation Procedure / Prohibition:**
`[Provide specific code examples, commands, or explicit prohibitions. For example: "PROHIBITION: Direct database calls from request handlers are forbidden." or "PROCEDURE: All API responses must be formatted using the `api.SuccessResponse()` helper function."]`
