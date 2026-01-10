# dircontxt: Intelligent Context for LLMs

## [deprecated] -> new complete rewrite and a HUGE update on https://github.com/Eris-Margeta/dctx
> **Stop copy-pasting. Start engineering.**

`dircontxt` is a blazing fast, zero-dependency command-line tool that turns your codebase into a structured, version-aware context for Large Language Models (GPT-4, Claude 3, Llama 3).

Unlike vector databases that feed AI fragmented "chunks" of code, `dctx` provides a **holistic snapshot** of your project, allowing the AI to understand architecture, imports, and relationships instantly.

---

## 🚀 Key Features

*   **⚡ Blazing Fast:** Written in Rust. Scans thousands of files in milliseconds using multi-threaded walkers.
*   **📦 Zero Dependency:** A single static binary. No Python, Node, or heavy runtime required.
*   **🧠 Context-Aware Diffing:** Automatically detects changes. After the first run, it generates highly optimized **Diff Snapshots** containing only what changed, saving you 90% of tokens.
*   **🛡️ Smart Ignore System:** Respects `.gitignore` and its own `.dctxignore`. Never accidentally send `node_modules` or `.env` files to an AI again.
*   **🎨 Custom Templates:** Fully customizable output format via `.dctx_template` system. Tailor the output for XML, Markdown, or JSON.
*   **📋 Clipboard Integration:** Use `-c` to pipe the context directly to your clipboard.

---

## 📦 Installation

### From Source (Rust)
```bash
git clone https://github.com/your-username/dctx
cd dctx
cargo install --path crates/dctx-cli
```

*(Binary releases for macOS, Windows, and Linux coming soon via package managers)*

---

## 💻 Usage

### 1. The Basics
Go to your project folder and run:
```bash
dctx .
```
This creates a `.llmcontext.txt` file in your folder containing the full project structure and content. Drag and drop this into ChatGPT/Claude.

### 2. Clipboard Mode (No Files)
Don't want to clutter your disk?
```bash
dctx . -c
```
The context is now in your clipboard. Just hit `Ctrl+V` in your AI chat.

### 3. The Power of Diffs
Make some changes to your code. Run `dctx` again.
*   It detects the file history.
*   It increments the version (e.g., `V1` -> `V1.1`).
*   It generates a **Diff File** showing *only* the changes.

To copy *only* the diff to clipboard:
```bash
dctx . -c -d
```

---

## ⚙️ Configuration

`dctx` looks for configuration in:
1.  **Project Root:** `.dctxignore` (Same syntax as .gitignore)
2.  **Global Config:** `~/.config/dctx/config.toml`

### Example `.dctxignore`
```gitignore
# Ignore heavy folders
node_modules/
target/
dist/

# Un-ignore specific important files
!dist/config.json
```

---

## 🏗️ Architecture

`dctx` is a Monorepo designed for scale:
*   **dctx-core:** The brain. Pure Rust logic for walking, diffing, and templating. Platform agnostic.
*   **dctx-cli:** The interface.
*   *(Coming Soon)* **dctx-ffi:** Bindings for Python, Node, and Go.
*   *(Coming Soon)* **dctx-desktop:** GUI for non-technical users.

---

## 📄 License

MIT License. Open Source and Free Forever.

