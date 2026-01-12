# VISION: The dctx Ecosystem
**The Universal Standard for Context-Aware AI Development**

> "Context is the new currency of the AI era. `dctx` is the bank."

---

## Main Premise
Large Language Models are only as good as the context they are fed. Currently, developers rely on fragmented RAG (Retrieval Augmented Generation) or manual copy-pasting to bridge the gap between their codebase and the AI. Both are inefficient.

**`dctx` (Directory Context)** aims to be the **standard protocol** for packaging, versioning, and delivering holistic project state to LLMs. We are building a high-performance engine that understands file structures, respects ignore rules, and intelligently calculates semantic diffs to minimize token costs.

## Exploration of the Concept
We are not just building a CLI tool; we are building an ecosystem. Whether you are a Python data scientist, a Node.js backend engineer, or a non-technical Project Manager, `dctx` should be the invisible layer that connects your files to your AI intelligence.

---

### 1. The Core Philosophy

#### 1.1. The "Open Core" Model
The intelligence of `dctx`—the scanning, diffing, and templating logic—is **Open Source (MIT)** and free forever. We believe this logic belongs to the community.
*   **Sustainability:** We support the project by selling high-level convenience (The Desktop App) while keeping the developer tools free.

#### 1.2. Zero-Dependency & Native Performance
The core must be written in Rust. It must have zero runtime dependencies (no Python interpreter or Node runtime required). It must start instantly and run everywhere, from a Raspberry Pi to a CI/CD container.

#### 1.3. One Standard, Many Interfaces
The logic lives in one place (`dctx-core`). Every other tool—the CLI, the Python library, the Node package, the Desktop App—is simply a wrapper around this single source of truth. This guarantees consistent behavior across all platforms.

---

### 2. The Architecture Layers (The Monorepo)

The project is structured as a Monorepo to ensure tight integration and synchronized versioning.

#### **Layer 1: The Brain (`dctx-core`)**
*   **Role:** Pure Rust library.
*   **Responsibility:** Directory walking, `.gitignore` parsing, Binary State management, Diff calculation, Template rendering.
*   **Distribution:** Crates.io (`dctx-core`).

#### **Layer 2: The Developer Tool (`dctx-cli`)**
*   **Role:** The primary interface for engineers.
*   **Responsibility:** Argument parsing, System integration, TUI (Terminal User Interface), Clipboard piping.
*   **Distribution:** Homebrew, Winget, Apt, Cargo (`dctx`).

#### **Layer 3: The Universal Bindings (`dctx-ffi`)**
*   **Role:** Native bindings enabling `dctx` to run inside other languages.
*   **Responsibility:**
    *   **Python:** `pip install dctx` (via PyO3) -> Use in Jupyter Notebooks / LangChain.
    *   **Node.js:** `npm install dctx` (via NAPI-RS) -> Use in VSCode Extensions / Agentic Frameworks.
    *   **Go:** `go get github.com/dctx/go` (via CGo) -> Use in Backend Services.

#### **Layer 4: The Product (`apps/dctx-desktop`)**
*   **Role:** The monetized, user-friendly GUI.
*   **Target Audience:** Non-technical users, PMs, and devs who prefer drag-and-drop.
*   **Features:** Visual diff inspection, One-click "Copy for ChatGPT", Multi-project management.
*   **Monetization:** **$5 Lifetime License**. No subscriptions. Accessible pricing to drive mass adoption.

---

### 3. The Technology Stack

We choose technologies that offer performance, safety, and cross-platform compatibility.

*   **Core Logic:** **Rust** (2021 Edition). Unbeatable performance and memory safety.
*   **Desktop App:** **Tauri v2**.
    *   Why? It binds directly to the Rust core without the bloat of Electron. It uses the native OS webview, keeping the app size tiny (<10MB).
*   **Bindings:**
    *   Python: `PyO3` / `Maturin`.
    *   Node.js: `NAPI-RS`.
*   **CI/CD:** **GitHub Actions**. Automated matrix builds for Linux, Windows, and macOS (Intel & Apple Silicon).

---

### 4. The User Experience

#### **For the Developer (CLI)**
```bash
# Snapshot current folder to clipboard
dctx . -c

# Generate a diff from the last run (saves tokens)
dctx . -c -d
```
*Zero friction. It just works.*

#### **For the AI Engineer (Library)**
```python
import dctx

# Get the snapshot object
snapshot = dctx.snapshot("./my-project")

# Pass optimized context to LangChain/LLM
llm.predict(snapshot.to_string())
```

#### **For the Professional (Desktop App)**
1.  User drags a folder of PDF contracts and Excel sheets into the App.
2.  App visually displays the file tree.
3.  User clicks "Copy Context".
4.  User pastes into Claude/GPT-4 to ask questions about the business data.
5.  *Result:* Complex data analysis without writing code.

---

### 5. Solving the Specific Challenges

1.  **Token Economics:** Sending the whole codebase every time is expensive.
    *   *Solution:* Our Semantic Diff Engine (ADR-002) only sends what changed.
2.  **Privacy:** Users are wary of uploading code to 3rd party tools.
    *   *Solution:* `dctx` is **Local-First**. No code ever leaves the machine unless the user pastes it into an LLM themselves.
3.  **Environment Hell:** Python tools break on Node projects; Node tools break on Python projects.
    *   *Solution:* The Rust binary is self-contained. It has no external runtime dependencies.

---

### 6. Implementation Roadmap

This roadmap aligns with our `PROJECT-DEVELOPMENT-ELABORATION.md`.

#### **Phase 1: The Foundation (Completed)**
*   Rust Core implementation.
*   Basic CLI functionality.
*   System Installation scripts.

#### **Phase 2: Architectural Maturity (Current)**
*   Centralized State Management (ADR-001).
*   Smart Versioning & Diffing (ADR-002).
*   AOT Templating Engine (ADR-003).

#### **Phase 3: The Universal API (Next)**
*   Create `dctx-ffi` crate.
*   Build and publish `npm` package (Node.js binding).
*   Build and publish `pypi` package (Python binding).

#### **Phase 4: The Desktop App**
*   Initialize Tauri project.
*   Bind Tauri frontend to `dctx-core`.
*   Implement License Key validation server (simple, serverless).
*   Launch website for $5 purchases.

#### **Phase 5: Ecosystem Automation**
*   Set up "Release Please" or Semantic Release actions.
*   Automate publishing to `npm`, `pypi`, `crates.io`, and `brew` on every tag push.

---

### Summary

`dctx` is positioning itself to be the **"WinRAR of AI Context"**.
A robust, ubiquitous, essential utility that technical users love for its power, and non-technical users buy for its convenience. By building a unified Rust core, we ensure that every improvement benefits the entire ecosystem simultaneously.
