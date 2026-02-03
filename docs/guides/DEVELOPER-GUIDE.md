# Developer Onboarding Guide

**`[AI Assistant Instruction: Purpose and Maintenance of This Document]`**

> `(1) **Purpose:** Your purpose is to maintain this DEVELOPER_GUIDE.md as the primary onboarding document for *human developers*. The language and content must be clear, direct, and focused on helping a human understand the project's architecture and workflow.`
>
> `(2) **Structure Mandate:** You must preserve the existing three-part structure of this document: 1. Onboarding Protocol, 2. System Architecture, and 3. Development Workflow. Do not alter these main sections.`
>
> `(3) **Content Generation Mandate:** Your primary task is to replace the `[AI-INSTRUCTIONS: ...]` placeholders with detailed, project-specific information relevant to a human developer. Your goal is to create a complete, standalone guide.`
>
> `(4) **Maintenance Rule:** Update the content within the placeholders only when the fundamental architecture, technology stack, or core development processes change. This document must always reflect the current state of the repository.`

---

## 1. Onboarding Protocol: Your First Hour

Welcome to the `dctx` engineering team. This guide provides the high-level overview necessary to understand the project's structure and begin contributing effectively.

#### **Step 1: Environment Setup**
The first step is to get the project running on your local machine.

**Prerequisites:**
*   **Rust:** Ensure you have the latest stable Rust toolchain installed (`rustup update`).
*   **Git:** Version control.
*   **Unix-like Env (Optional):** While Windows is supported, the `install.sh` script currently targets Bash environments (Linux/macOS/WSL).

**Installation:**
```bash
# 1. Clone the repository
git clone <repository_url>
cd dctx

# 2. Build and Install System-Wide (Recommended)
# This compiles the release binary, copies it to /usr/local/bin, 
# and sets up ~/.config/dctx/ with default templates.
./tools/install.sh

# 3. Verify Installation
dctx --version
```

#### **Step 2: Understand the Project's "Brain" - The `docs` Directory**
All project-specific documentation, rules, tasks, and operational protocols reside in the `docs/` directory. This is the **single source of truth** for the project's execution. Before writing any code, you must familiarize yourself with its contents.

#### **Step 3: Read the Core Documents (In This Order)**
To understand the rules of this repository, read the following documents from `docs/`:

1.  **`AI-INSTRUCTIONS.md`**: This is **"The Constitution"**. It defines the formal procedures that govern development, especially the interaction between you (the Operator) and the AI Assistant. It dictates the "how" of our work.
2.  **`DEVELOPER-GUIDE.md`**: This is the main development guide on how to approach the development of our project.
3.  **`DEVELOPMENT-PRACTICES.md`**: This is the project's **living knowledge base**. It starts empty and is populated with solutions to recurring technical challenges to prevent regressions and save time.

and from `docs/DOCUMENTS/` read:

4.  **`PROJECT-DEVELOPMENT-ELABORATION.md`**: This is the official **backlog and development plan**. It details what has been completed, what is in progress, and defines the next specific tasks. It dictates the "what" and "when" of our work.


---

## 2. System Architecture & Design Philosophy

This section describes the technical architecture and the core principles behind it.

#### **Design Philosophy: The "Enterprise Grade" Standard**
1.  **Zero Dependencies:** We minimize external crates. We do **not** use `tokio`, `serde`, or regex engines unless absolutely unavoidable. We want `dctx-core` to compile on embedded devices, WASM, and obscure architectures. We write our own parsers.
2.  **Performance First:** The tool must feel instant. We use `std::thread` for parallel file reading (Map-Reduce pattern) and `BufWriter` for all file I/O.
3.  **Extensibility via Templates:** The API logic is fixed, but the Output format is fluid. We use an **Ahead-of-Time (AOT) Compiled Template System**. The core "injects data" into pre-validated templates rather than printing hardcoded strings.

#### **Directory Structure**
The repository is organized to clearly separate concerns in a Monorepo format:

```text
dctx/
├── crates/
│   ├── dctx-core/       # THE BRAIN. Pure Rust. No CLI logic. 
│   │   ├── src/
│   │   │   ├── walker.rs    # Multi-threaded directory scanner
│   │   │   ├── ignore.rs    # Custom .gitignore parser (Glob logic)
│   │   │   ├── diff.rs      # Tree comparison engine
│   │   │   ├── state.rs     # Corruption recovery & Integrity checks
│   │   │   └── writer.rs    # Binary serializer
│   ├── dctx-cli/        # THE INTERFACE. Arg parsing & User interaction.
│   └── dctx-ffi/        # THE BRIDGE. C-bindings (Planned).
├── legacy-c/            # Reference implementation (Archived)
└── tools/               # Release and Install scripts
```

#### **Architectural Decision Records (ADRs)**
Significant architectural decisions, their context, and their consequences are formally documented in Architecture Decision Records (ADRs). This practice ensures that the reasoning behind our technical choices is preserved and accessible.

All ADRs are located in the `docs/ARCHITECTURE/` directory.
*   **ADR-001:** System Architecture (Centralized State).
*   **ADR-002:** Versioning & Recovery.
*   **ADR-003:** Templating Engine.
*   **ADR-004:** CLI Implementation Strategy.
*   **ADR-015:** Walker Architecture Refactoring (Phase 10.7).

#### **Walker Architecture (Phase 10.7 Refactoring)**

The walker subsystem underwent a major architectural refactoring in Phase 10.7 to address performance issues and architectural debt. Understanding this architecture is crucial for working with the codebase.

**Core Components:**

1. **Domain Model** (`src/domain.rs`)
   - **`FileHandlingDecision` enum:** Explicit state machine replacing boolean flags
     - `Include` - Include in tree and read content
     - `Exclude` - Exclude from tree entirely, never recurse
     - `DebugVisible` - Show in tree with [IGNORED] marker, don't read content
     - `OmitContent` - Include in tree, omit content (lockfiles, binaries)
   - **`ItemContext` struct:** Bundles decision-making parameters

2. **IgnoreEngine** (`src/ignore.rs`)
   - **Single decision point** for all file handling decisions
   - **3-layer rule loading:** Hardcoded → Global → Local
   - **Built-in diagnostics:** Logs statistics on build
   - **Error handling:** Returns `Result<Self, IgnoreError>`, no silent failures
   - **Methods:**
     - `build(base_dir: &Path) -> Result<Self, IgnoreError>` - Construct with rules
     - `decide(&self, context: &ItemContext) -> FileHandlingDecision` - Make decision
     - `explain(&self, path: &str) -> String` - Debug why file was ignored/included

3. **Walker** (`src/walker.rs`)
   - **Single responsibility:** Tree traversal only, no file I/O
   - **Linear control flow:** No nested conditionals
   - **Single decision point:** `if !decision.show_in_tree() { continue; }`
   - **501 lines** (down from 528, 5% reduction)

4. **ContentReader** (`src/content_reader.rs`)
   - **Separated concern:** File content reading isolated from traversal
   - **Privacy guarantee:** Only reads files where `handling_decision.should_read_content() == true`
   - **Parallel I/O:** Thread pool for file reading
   - **Methods:**
     - `populate(tree: &mut DirContextTreeNode)` - Main entry point
     - `collect_readable_files()` - Collect paths respecting decisions
     - `read_parallel()` - Read files in parallel
     - `inject_content()` - Inject content into tree with classification

**Data Flow:**

```
CLI main.rs
    │
    ├─> IgnoreEngine::build(base_dir)
    │       └─> Loads rules: Hardcoded → Global (~/.config/dctx/ignore) → Local (.dctxignore)
    │       └─> Logs statistics
    │
    ├─> Walker::walk_directory_structure(path, &engine, debug)
    │       └─> Recursively traverses directories
    │       └─> For each item: engine.decide(&context)
    │       └─> If !decision.show_in_tree(): skip
    │       └─> Sets node.handling_decision on each node
    │
    └─> ContentReader::populate(&mut tree)
            └─> Collects files where should_read_content() == true
            └─> Reads files in parallel (thread pool)
            └─> Injects content with binary classification
```

**Key Benefits:**

- **Performance:** 50-100x faster on large monorepos (skips ignored directories entirely)
- **Maintainability:** Clear separation of concerns, each module has ONE purpose
- **Testability:** 317 unit tests (up from 284)
- **Debuggability:** Built-in statistics and diagnostics
- **Correctness:** Bug fixed as natural consequence of proper architecture

**Privacy Guarantee:**

The walker architecture enforces privacy through multiple layers:

1. **Exclusion:** Ignored files never enter the tree (unless `--debug-ignored`)
2. **Collection:** `ContentReader` never collects ignored files for reading
3. **Guard:** `!node.is_ignored` guards prevent content rendering in formatter

This makes it **structurally impossible** to accidentally read ignored files, ensuring `.env`, credentials, and sensitive data are never loaded into memory.

**For More Details:** See ADR-015 in `docs/ARCHITECTURE/`.

#### **Data Flow & Cross-Platform Standards**
We strictly adhere to OS standards for storing configuration and state. We do NOT clutter the user's home folder.

1.  **Configuration (Text files):**
    *   *Linux/macOS:* `~/.config/dctx/` (XDG_CONFIG_HOME)
    *   *Windows:* `%APPDATA%\dctx\`
    *   *Key Files:* `config.toml`, `ignore`, `templates/`.

2.  **Cache & State (Binary .dctx files):**
    *   *Linux:* `~/.cache/dctx/` (XDG_CACHE_HOME)
    *   *macOS:* `~/Library/Caches/dctx/`
    *   *Windows:* `%LOCALAPPDATA%\dctx\`
    *   *Role:* This is the **Single Source of Truth** for the project history.

3.  **The Registry (`projects.db`):**
    *   Maps project paths to their Hash IDs in the cache. This ensures state recovery even if folders are renamed.

4.  **Binary Format (.dctx):**
    *   **Magic Bytes:** `DIRCTXTV`
    *   **Header:** Version + Serialized Directory Tree.
    *   **Data:** Raw bytes of file contents.
    *   **Endianness:** Little Endian (LE).

#### **Key Technologies & Libraries**
*   **Rust (Edition 2021):** The core language.
*   **Standard Library Only:** For `dctx-core` and `dctx-cli`.
*   **No Async Runtime:** Threading is handled via `std::thread`.

#### **Template Variable Safety**

When creating custom templates, developers must understand the safety classification of template variables to maintain privacy guarantees for ignored files.

**Metadata-Only Variables** (safe for all files, including ignored):
- `{{{node.type_char}}}`, `{{{node.path}}}`, `{{{node.id}}}`
- `{{{node.mod}}}`, `{{{node.size_part}}}`
- `{{{file.id}}}`, `{{{file.path}}}`, `{{{file.size}}}`

These variables only access filesystem metadata (via `stat`/`lstat`) and **never open or read file contents**.

**Content-Requiring Variables** (only safe in content blocks):
- `{{{file.lines}}}` - Line count (requires reading file into memory)
- `{{{file.content}}}` - Full content (requires reading file into memory)

The system ensures content-requiring variables are never resolved for ignored files through a multi-layer defense strategy:

1. **Walker Layer:** Ignored files are excluded from the tree (unless `--debug-ignored` is used)
2. **Collection Layer:** Ignored files are never added to the content reading queue
3. **Formatter Layer:** Content blocks are never rendered for ignored files (enforced by `!node.is_ignored` guard)

**Best Practice for Custom Templates:**

Only use content-requiring variables (`{{{file.lines}}}`, `{{{file.content}}}`) within the `[FILE_BLOCK]` section of your template. If you use them in the `[TREE_ENTRY]` section, they will remain as literal strings (not replaced), which is safe but not ideal for template aesthetics.

**Privacy Guarantee:**

The system **guarantees** that ignored files (`.env`, credentials, private keys, `.git/` contents) are **never read into memory**, regardless of template configuration. This is enforced structurally through guard placement in `llm_formatter.rs`, not through runtime checks, making it impossible to bypass accidentally.

**Example:**

```rust
// SAFE: Using metadata-only variables in tree entry
"{{{node.path}}} (ID={{{node.id}}}, SIZE={{{node.size_part}}})"

// UNSAFE: Using content-requiring variables in tree entry (will not be replaced)
"{{{node.path}}} has {{{file.lines}}} lines"  // BAD - file.lines only works in FILE_BLOCK

// CORRECT: Content-requiring variables in content block
[FILE_BLOCK]
FILE {{{file.id}}}: {{{file.path}}}
Lines: {{{file.lines}}}
{{{file.content}}}
```

---

## 3. Development Workflow & Contribution

Your day-to-day contributions must follow these standardized procedures to ensure code quality and project consistency.

#### **Branching and Commit Strategy**
We use a structured branching model to keep the main branch stable.
*   **Branch Naming:** `feature/[short-description]` or `fix/[short-description]`.
*   **Commit Messages:** Use Conventional Commits (e.g., `feat: implement config loader`, `fix: resolve version loop bug`).

#### **Testing Protocol**
All new code must be accompanied by tests.
*   **Unit Tests:** Located alongside source code in `#[cfg(test)]` modules.
*   **Integration Tests:** Run the CLI against sample directories.
*   **Commands:**
    ```bash
    # Test Core Logic
    cargo test -p dctx-core
    
    # Run CLI manually
    cargo run -p dctx-cli -- .
    ```

#### **Working with the AI Assistant**
While you can code independently, the standard workflow for this repository involves collaborating with the AI Assistant to ensure adherence to protocols. The cycle is simple:

1.  **Receive a Work Order:** The AI will issue a formal Work Order based on a task from the `PROJECT-DEVELOPMENT-ELABORATION.md`.
2.  **Execute the Work:** You apply the code, run the commands, and perform the task as specified.
3.  **Verify and Report:** You run the necessary tests and report the outcome (success or a verbatim error log) back to the AI. This loop ensures every step is documented and validated.

#### **Error Handling & Recovery Guidelines**
When contributing to `dctx-core`, assume the filesystem is hostile.
*   **Corruption:** If the binary reader fails, perform a **Hard Reset** (delete binary, revert to V1).
*   **Recreated Folder:** If metadata mismatches drastically, trigger a **Major Version Bump** (V1 -> V2).
*   **Public API:** Changes to `DirContextTreeNode` or `DiffReport` are breaking changes.

#### **How to Propose a New Feature or Change**
Architectural changes and new features must be formally integrated into the plan **before** implementation.

1.  **Do Not Start Coding:** Directly implementing an unplanned feature is a violation of the protocol.
2.  **Initiate a Revision:** Following the procedure in `docs/GUIDES/AI-INSTRUCTIONS.md`, instruct the AI to begin a "Revision Phase" for the `docs/DOCUMENTS/PROJECT-DEVELOPMENT-ELABORATION.md` document.
3.  **Update The Plan:** Work with the AI to formally add the new feature, breaking it down into phases and tasks.
4.  **Formalize the Decision:** If the change involves a significant architectural decision (e.g., adding a new database, changing a core library), it **must be documented** in an **Architecture Decision Record (ADR)**. Use the template located at `docs/ARCHITECTURE/adr-template.md`.
5.  **Begin Development:** Once the plan is updated and the decision is recorded, development can commence by following the standard Work Order cycle.
