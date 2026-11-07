<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-015: Centralized Ignore Engine with Explicit State Model

**Date:** 2026-02-02
**Status:** superseded — historical design and implementation record.

Use the [current product contracts](design/PRODUCT-CONTRACTS.md), [README](../README.md) and [testing guide](TESTING.md) for release behavior.

---

## 1. Context and Problem Statement

During Phase 10.6 and 10.7 development, a critical bug was discovered where ignored directories (`.turbo/`, `.pnpm-store/`, `.localstack/`) were being recursed into despite correct ignore patterns in the global ignore file. Investigation revealed multiple architectural issues:

1. **Scattered Decision Logic:** Ignore decisions were made in 3 different files (walker.rs, ignore.rs, llm_formatter.rs) with no single source of truth.

2. **Implicit State:** The system used boolean flags (`is_ignored`, `is_binary_file`) which created 2^N possible states, many invalid or with unclear semantics.

3. **Silent Failures:** The `load_ignore_rules()` function silently failed if global files were unreadable, providing no visibility into which rules were actually loaded.

4. **Parser Bug:** The gitignore pattern parser did not strip inline comments, causing patterns like `.turbo/  # Turborepo` to fail matching.

5. **No Observability:** When patterns failed to match, there was no way to debug which rules were checked or why a decision was made.

The bug symptom (directories being recursed) was not a logic error but an **architectural deficiency** - the code worked correctly in tests but failed in production due to unclear separation of concerns and lack of instrumentation.

## 2. Decision Drivers

*   **Driver 1: Single Responsibility** - Each component should have one clear purpose. The walker should traverse, not make ignore decisions.

*   **Driver 2: Observability** - Developers must be able to see which rules are loaded, which ones match, and why decisions are made.

*   **Driver 3: Type Safety** - Invalid states should be impossible to represent. `FileHandlingDecision` should be explicit, not implicit.

*   **Driver 4: Testability** - Decision logic must be testable in isolation without file I/O or directory traversal.

*   **Driver 5: Error Visibility** - When rule loading fails, the system must report exactly what failed and why, not silently use defaults.

*   **Driver 6: Backward Compatibility** - Existing code using `load_ignore_rules()` must continue to work during transition.

## 3. Considered Options

### Option 1: Keep Scattered Logic, Fix Parser Bug Only

Fix the inline comment parser bug in `parse_ignore_pattern_line()` but leave the architectural issues unchanged.

*   **Pros:**
    *   Minimal code changes
    *   Immediate bug fix
    *   No refactoring risk
*   **Cons:**
    *   Does not address root architectural causes
    *   Scattered decision logic remains
    *   No observability improvements
    *   Future bugs likely due to unclear responsibilities
    *   Technical debt accumulates

### Option 2: Centralized IgnoreEngine with Explicit State Model (CHOSEN)

Extract all ignore logic into a dedicated `IgnoreEngine` component that:
- Loads rules from 3 layers (hardcoded, global, local) with statistics
- Provides single decision point returning explicit `FileHandlingDecision` enum
- Includes comprehensive logging and diagnostic `explain()` method
- Returns `Result` instead of silent defaults
- Fixes parser bug as part of refactoring

*   **Pros:**
    *   Single source of truth for decisions
    *   Observable via statistics and logging
    *   Type-safe explicit state model
    *   Testable in isolation
    *   Error visibility with Result types
    *   Debuggable via explain() method
    *   Follows enterprise Rust patterns (Article 2-4 of RUST-ENTERPRISE-CODE-DOCTRINE.md)
*   **Cons:**
    *   Requires refactoring (~280 lines of new code)
    *   Temporary deprecation warnings during transition
    *   Learning curve for new architecture

### Option 3: Introduce Builder Pattern for Rules

Keep scattered logic but introduce a builder pattern for constructing ignore rules with better validation.

*   **Pros:**
    *   Better rule validation
    *   Clearer rule construction API
*   **Cons:**
    *   Does not address scattered decision logic
    *   No observability improvements
    *   Still requires manual pattern checking
    *   Does not solve single responsibility problem

## 4. Decision Outcome

**Chosen option:** Option 2 - Centralized IgnoreEngine with Explicit State Model

This option fundamentally addresses the architectural root causes of the bug rather than just patching symptoms. By establishing a single decision point with explicit state, we eliminate the boolean flag soup and create a testable, observable system.

The IgnoreEngine follows the RUST-ENTERPRISE-CODE-DOCTRINE.md principles:
- Article 2: Single Responsibility (engine only makes decisions)
- Article 3: Explicit State (FileHandlingDecision enum)
- Article 4: Single Decision Point (`decide()` method)
- Article 7: Error Handling (Result type, not silent failures)

### Positive Consequences

*   **Bug Prevention:** The architecture makes it impossible for ignore decisions to be scattered or inconsistent.

*   **Observability:** Statistics show exactly which rules were loaded:
    ```
    🔧 IgnoreEngine loaded:
       IGNORE: 18 hardcoded + 269 global + 0 local = 287 total
       OMIT:   25 hardcoded + 422 global + 0 local = 447 total
    ```

*   **Debuggability:** The `explain()` method shows exactly which rules matched and why:
    ```rust
    let explanation = engine.explain(&context);
    // Shows: Decision, matching rules, rule sources
    ```

*   **Type Safety:** Compiler enforces exhaustive match on `FileHandlingDecision`:
    ```rust
    match decision {
        FileHandlingDecision::Exclude => { /* ... */ },
        FileHandlingDecision::DebugVisible => { /* ... */ },
        FileHandlingDecision::OmitContent => { /* ... */ },
        FileHandlingDecision::Include => { /* ... */ },
    }
    // Missing a case? Compiler error!
    ```

*   **Testability:** Engine can be tested in isolation with 100% coverage (13 tests added).

*   **Documentation:** Code is self-documenting through explicit types and method names.

### Negative Consequences

*   **Deprecation Warnings:** Existing code using `load_ignore_rules()` shows deprecation warnings until migration is complete (Phase 3).

*   **Learning Curve:** Developers must learn the new `IgnoreEngine::build()` and `decide()` APIs.

*   **Temporary Duplication:** During transition, both old and new APIs exist (resolved in Phase 3).

## 5. Implementation Plan

Implementation follows Phase 10.7 of PROJECT-DEVELOPMENT-ELABORATION.md:

1.  **Phase 10.7.2 (COMPLETED):** Create domain model with `FileHandlingDecision` enum and `ItemContext` struct (WO-063)

2.  **Phase 10.7.3 (COMPLETED):** Extract IgnoreEngine with `build()`, `decide()`, `explain()` methods. Fix parser bug. (WO-064)

3.  **Phase 10.7.4 (PENDING):** Refactor walker to use `IgnoreEngine::decide()` as single decision point

4.  **Phase 10.7.5 (PENDING):** Extract content reader to separate concerns

5.  **Phase 10.7.6 (PENDING):** Integration testing and bug fix validation

6.  **Phase 10.7.7 (PENDING):** Remove deprecated APIs and cleanup

## 6. Validation

### Success Criteria

- [x] **Bug Fixed:** `.turbo/`, `.pnpm-store/`, `.localstack/` are correctly excluded (regression tests pass)
- [x] **No Regressions:** All 314 tests pass (301 existing + 13 new)
- [x] **Integration Tests:** All 6 integration test suites pass
- [x] **Observability:** Engine logs statistics showing rule counts from each layer
- [x] **Error Handling:** `Result` type catches file read errors with clear messages
- [ ] **Walker Migration:** Phase 3 completes with walker using engine (PENDING)
- [ ] **Performance:** No measurable performance degradation (validate in Phase 6)

### Metrics

- **Test Coverage:** 13 new tests added, 100% coverage of IgnoreEngine
- **Code Quality:** Follows all RUST-ENTERPRISE-CODE-DOCTRINE.md articles
- **Bug Resolution:** Regression tests prove .turbo/ and .pnpm-store/ are excluded
- **Observability:** Statistics logged on every engine build
- **Lines of Code:** +281 lines (IgnoreEngine + tests), ~280 lines to be removed in Phase 7 (deprecated code)

---

## Related Documents

- **Work Orders:** WO-063 (Phase 1: Domain Model), WO-064 (Phase 2: IgnoreEngine)
- **Bug Report:** BUG-REPORT-IGNORED-DIRECTORIES-RECURSION.md
- **Refactoring Proposal:** REFACTORING-PROPOSAL-WALKER-IGNORE.md
- **Code Doctrine:** GUIDES/RUST-ENTERPRISE-CODE-DOCTRINE.md
- **Development Practices:** GUIDES/DEVELOPMENT-PRACTICES.md (Articles 3-4)
