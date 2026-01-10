# Developer Guide

Welcome to the `dctx` engineering team. This document outlines the philosophy, architecture, and standards required to contribute to this project.

## 1. Core Philosophy: The "Enterprise Grade" Standard

### 1.1. Zero Dependencies
We minimize external crates. We do **not** use `tokio`, `serde`, or regex engines unless absolutely unavoidable.
*   **Why?** Portability. We want `dctx-core` to compile on embedded devices, WASM, and obscure architectures without fighting dependency trees.
*   **Consequence:** We write our own parsers (like the `.dctxignore` glob matcher and the Template engine).

### 1.2. Performance First
The tool must feel instant.
*   We use `std::thread` and `mpsc` channels for parallel file reading (Map-Reduce pattern).
*   We use `BufWriter` for all file I/O to minimize syscalls.
*   We strictly control memory allocations during directory walking.

### 1.3. Extensibility via Templates
The API logic is fixed, but the Output format is fluid. We use an **Ahead-of-Time (AOT) Compiled Template System**. The core does not "print strings"; it "injects data" into pre-validated templates.

---

## 2. Monorepo Structure

```text
dctx/
├── crates/
│   ├── dctx-core/       # THE BRAIN. Pure Rust. No CLI logic. 
│   │   ├── src/
│   │   │   ├── walker.rs    # Multi-threaded directory scanner
│   │   │   ├── ignore.rs    # Custom .gitignore parser (Glob logic)
│   │   │   ├── diff.rs      # Tree comparison engine (HashMap based)
│   │   │   ├── template.rs  # Two-pass template rendering engine
│   │   │   ├── state.rs     # Corruption recovery & Integrity checks
│   │   │   └── writer.rs    # Binary serializer
│   ├── dctx-cli/        # THE INTERFACE. Arg parsing & User interaction.
│   └── dctx-ffi/        # THE BRIDGE. C-bindings for other languages.
├── legacy-c/            # Reference implementation (Archived)
└── tools/               # Release scripts
```

## 3. Cross-Platform Data Standards

We strictly adhere to OS standards for storing configuration and state. We do NOT clutter the user's home folder.

### Configuration (Text files, Templates)
*   **Linux/macOS:** `~/.config/dctx/` (XDG_CONFIG_HOME)
*   **Windows:** `%APPDATA%\dctx\` (Roaming)

### Cache & State (Binary .dctx files)
*   **Linux:** `~/.cache/dctx/` (XDG_CACHE_HOME)
*   **macOS:** `~/Library/Caches/dctx/`
*   **Windows:** `%LOCALAPPDATA%\dctx\` (Local)

**The Registry:**
A `projects.db` file in the Config folder maps project paths to their Binary Hash in the Cache folder. This ensures that even if you rename a project folder, we can attempt to recover state (or reset gracefully).

---

## 4. The Binary State Format (.dctx)

The `.dctx` file is the **Single Source of Truth**.
*   **Magic Bytes:** `DIRCTXTV` (8 bytes).
*   **Header:** Serialized Directory Tree (Paths, Timestamps, Sizes).
*   **Data:** Raw bytes of file contents (appended sequentially).
*   **Endianness:** Little Endian (LE) strictly enforced for cross-architecture compatibility.

## 5. Error Handling & Recovery

We assume the filesystem is hostile.
*   **Corruption:** If the binary reader encounters a checksum fail or invalid magic bytes, `dctx` performs a **Hard Reset** (deletes binary, reverts to V1) and logs a warning.
*   **Recreated Folder:** If the target folder's metadata drastically mismatches the binary history (e.g., all files changed), `dctx` triggers a **Major Version Bump** (V1.x -> V2.0) and clears previous diffs.

## 6. Future-Proofing API

When contributing to `dctx-core`, remember:
*   **Public Structs:** `DirContextTreeNode` and `DiffReport` are the public face of the library. Changes here break the Desktop App and Python bindings.
*   **Backward Compatibility:** The binary reader must handle older format versions (once we reach V1.0 stable).

## 7. How to Build & Test

```bash
# Build all crates
cargo build --workspace

# Run CLI from root
cargo run -p dctx-cli -- . 

# Run Core Tests
cargo test -p dctx-core
