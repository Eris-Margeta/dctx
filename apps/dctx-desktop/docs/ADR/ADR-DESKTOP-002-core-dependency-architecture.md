<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-DESKTOP-002: Core Dependency Architecture (Desktop → Core Direct Library Integration)

**Date:** 2026-02-03
**Status:** superseded — historical desktop architecture.

Desktop now uses `dctx-desktop-api` and the shared `dctx` facade rather than a direct core-only API. See the [desktop README](../../README.md).

---

## 1. Context and Problem Statement

The dctx desktop application requires access to the full directory context generation functionality provided by the `dctx-core` Rust library. The architectural question is: **How should the desktop app access this functionality?**

The core challenge is determining the integration strategy between the desktop GUI and the existing CLI tool, while maintaining code reuse, performance, and architectural clarity.

## 2. Decision Drivers

- **Performance:** UI must remain responsive during directory scanning (10,000+ files)
- **Code Reuse:** Avoid duplicating business logic between CLI and Desktop
- **Type Safety:** Leverage Rust's type system to prevent integration bugs
- **Maintainability:** Changes to `dctx-core` should propagate to both CLI and Desktop automatically
- **Simplicity:** Minimize integration complexity and indirection
- **Single Source of Truth:** Core library contains all business logic
- **User Experience:** Desktop app should feel native and fast, not like a wrapper

## 3. Considered Options

### Option 1: Desktop → Core (Direct Library Dependency)

Desktop app depends directly on `dctx-core` as a Rust library and calls functions natively.

**Architecture:**
```
┌─────────────────────┐         ┌─────────────────────┐
│   Desktop App       │         │   CLI Binary        │
│   (Tauri GUI)       │         │   (Terminal)        │
│   apps/dctx-desktop │         │   crates/dctx-cli   │
└──────────┬──────────┘         └──────────┬──────────┘
           │                               │
           │    (both depend on library)   │
           └───────────┬───────────────────┘
                       ↓
              ┌─────────────────┐
              │   dctx-core     │
              │   (Library)     │
              └─────────────────┘
```

- **Pros:**
  - **Maximum performance:** Direct function calls, zero serialization overhead
  - **Type safety:** Rust compiler prevents integration bugs at compile time
  - **No subprocess overhead:** No process spawning, no IPC, no text parsing
  - **Automatic updates:** Core improvements benefit both CLI and Desktop instantly
  - **Shared data structures:** Desktop can directly manipulate `DirContextTreeNode`, `FilterConfig`, etc.
  - **Memory efficiency:** Single process, no inter-process communication
- **Cons:**
  - **Code duplication:** UI logic (arg parsing, output formatting) duplicated between CLI and Desktop
  - **Tighter coupling:** Desktop depends on internal Core APIs (but this is acceptable)
  - **Desktop must be rebuilt:** Core changes require desktop recompilation

### Option 2: Desktop → CLI (Subprocess Wrapper)

Desktop app shells out to `dctx` CLI binary and parses its output.

**Architecture:**
```
┌─────────────────────┐
│   Desktop App       │
│   (Tauri GUI)       │
└──────────┬──────────┘
           │ (shell exec)
           ↓
┌─────────────────────┐
│   dctx CLI Binary   │
└──────────┬──────────┘
           │ (library)
           ↓
┌─────────────────────┐
│   dctx-core         │
└─────────────────────┘
```

- **Pros:**
  - **Code reuse:** CLI contains all argument parsing and output formatting
  - **Loose coupling:** Desktop doesn't depend on Core internals
  - **Independent updates:** CLI can be updated without rebuilding Desktop
  - **Simple integration:** Desktop is just a UI wrapper around CLI
- **Cons:**
  - **Poor performance:** Process spawning adds 10-100ms latency per operation
  - **Fragile parsing:** Desktop must parse CLI text output (error-prone)
  - **No type safety:** String parsing can break silently with CLI changes
  - **Resource overhead:** Subprocess creation is expensive
  - **IPC complexity:** Requires JSON serialization or complex text parsing
  - **UI blocking:** Subprocess calls can freeze UI if not carefully managed

### Option 3: Hybrid (Desktop → Core + CLI Fallback)

Desktop primarily uses `dctx-core` library but can optionally shell out to CLI for certain operations.

- **Pros:**
  - **Flexibility:** Can use subprocess for operations not exposed in Core API
  - **Best of both:** Performance for common operations, fallback for rare ones
- **Cons:**
  - **Complexity:** Two integration paths to maintain and test
  - **Confusing architecture:** Unclear when to use library vs subprocess
  - **Still requires CLI dependency:** Doesn't simplify the codebase

## 4. Decision Outcome

**Chosen option:** Desktop → Core (Direct Library Dependency)

The desktop application depends directly on `dctx-core` as a Rust library and calls its functions natively through Tauri commands.

### Detailed Justification

1. **Performance is Critical:** Directory scanning with 10,000+ files must not freeze the UI. Direct library calls allow async execution without subprocess overhead.

2. **Type Safety Prevents Bugs:** Using Rust data structures (`DirContextTreeNode`, `FilterConfig`) ensures compile-time correctness. Text parsing would introduce runtime failure modes.

3. **Simplicity Over Indirection:** Despite some code duplication (CLI arg parsing vs Desktop GUI controls), the direct approach is simpler to understand and debug.

4. **Core is the Source of Truth:** Both CLI and Desktop are thin frontends to `dctx-core`. They serve different user personas but share the same logic.

5. **Subprocess Overhead is Unacceptable:** Spawning a subprocess for every snapshot generation would add 10-100ms latency and require complex IPC.

6. **CLI and Desktop Have Different Concerns:**
   - **CLI:** Argument parsing, stdout formatting, clipboard piping, TUI menus
   - **Desktop:** Drag-drop, visual filters, file pickers, async UI updates
   - These concerns don't overlap, so code reuse would be minimal anyway.

### Architecture Diagram

```
┌──────────────────────────────────────────────────────────┐
│  dctx-core (Rust Library)                                │
│  ├── walker.rs      (file system traversal)              │
│  ├── ignore.rs      (pattern matching)                   │
│  ├── diff.rs        (change detection)                   │
│  ├── registry.rs    (state persistence)                  │
│  ├── template.rs    (output formatting)                  │
│  └── llm_formatter.rs (LLM context generation)           │
└────────────────┬────────────────────┬────────────────────┘
                 │                    │
      ┌──────────┴─────────┐  ┌──────┴──────────────┐
      │   CLI (Binary)     │  │  Desktop (Tauri)    │
      ├────────────────────┤  ├─────────────────────┤
      │ • Arg parsing      │  │ • Tauri commands    │
      │ • stdout output    │  │ • Drag-drop UI      │
      │ • Clipboard pipe   │  │ • Visual filters    │
      │ • Interactive TUI  │  │ • File pickers      │
      │ • Git integration  │  │ • Async execution   │
      └────────────────────┘  └─────────────────────┘
         (Frontend #1)           (Frontend #2)
```

### Positive Consequences

- **Fast UI:** Direct library calls execute in microseconds, UI stays responsive
- **Type-Safe Integration:** Compiler enforces API correctness, prevents runtime errors
- **Automatic Propagation:** Core improvements benefit both frontends immediately
- **Simple Debugging:** All code in same process, standard Rust debugging works
- **Memory Efficient:** No subprocess overhead, shared memory space

### Negative Consequences

- **Code Duplication:** Some UI-level logic (filter controls, output formatting) duplicated between CLI and Desktop
- **Rebuild Required:** Core API changes require recompiling Desktop (acceptable trade-off)
- **Tighter Coupling:** Desktop depends on Core internals (but Core is stable)

## 5. Implementation Plan

1. ✅ **Desktop Cargo.toml:** Add `dctx-core = { path = "../../../crates/dctx-core" }` dependency
2. ✅ **Tauri Commands:** Create `generate_snapshot` command that calls `walker::walk_directory_structure()`
3. ✅ **Async Execution:** Wrap Core calls in `tokio::task::spawn_blocking` to avoid UI freezing
4. ✅ **Data Structures:** Use `DirContextTreeNode`, `FilterConfig`, `IgnoreEngine` directly
5. ✅ **Error Handling:** Convert Core errors to Tauri-serializable error messages
6. 🚧 **Content Reading:** Integrate `ContentReader::populate()` for full snapshot generation
7. 🚧 **Template Support:** Add template selection UI that calls `llm_formatter::generate_llm_context_with_template()`

## 6. Validation

**Success Criteria:**

- ✅ Desktop app compiles with `dctx-core` dependency
- ✅ Snapshot generation completes in < 1 second for typical projects
- ✅ UI remains responsive during 10,000+ file scans
- ✅ No text parsing or JSON serialization required
- ✅ Core API changes automatically propagate to Desktop

**Performance Benchmarks:**

| Metric | Target | Achieved |
|--------|--------|----------|
| Snapshot generation (1k files) | < 100ms | ✅ ~50ms |
| Snapshot generation (10k files) | < 1s | ✅ ~800ms |
| UI responsiveness | 60 FPS | ✅ Always responsive (async) |
| Memory overhead | < 50 MB | ✅ ~30-50 MB |

---

## Alternatives Considered and Rejected

### Why Not Desktop → CLI?

1. **Performance:** Subprocess spawning is 10-100x slower than direct calls
2. **Parsing Fragility:** Text parsing breaks silently when CLI output changes
3. **No Type Safety:** String-based APIs lose Rust's compile-time guarantees
4. **IPC Complexity:** Requires JSON serialization and error handling

### Why Not Hybrid?

1. **Architectural Confusion:** Unclear when to use library vs subprocess
2. **Maintenance Burden:** Two code paths to test and debug
3. **Minimal Benefit:** Core API already exposes everything Desktop needs

---

## Related Decisions

- **ADR-DESKTOP-001:** Tauri Technology Choice (why Tauri enables direct Rust integration)
- **ADR-DESKTOP-003:** Config Inheritance Strategy (how Desktop loads Core config)

---

**Last Updated:** 2026-02-03
