# dctx Project Roadmap

This document outlines the development phases for `dctx`.

## 🟢 Phase 1: Core Architecture (Completed / In Progress)
*Goal: Port functionality to Rust and implement the new centralized architecture.*

- [x] **Project Structure:** Initialize Rust Workspace.
- [x] **Datatypes:** Port C structs to Rust.
- [x] **Ignore System:** Implement `.dctxignore` logic (L1/L2/L3).
- [x] **Walker:** Multi-threaded directory traversal.
- [x] **Writer:** Binary `.dctx` file generation (Little Endian).
- [ ] **Reader:** Implement robust reading of `.dctx` binary.
- [ ] **State Manager:** Implement logic for Corruption handling & Recreated folders.
- [ ] **Version Logic:** Implement Semantic Versioning & Major Bump triggers.
- [ ] **Diff Engine:** Implement Tree Comparison (`HashMap` based).

## 🟡 Phase 2: Templating Engine (The Next Focus)
*Goal: Implement the customizable output system.*

- [ ] **Template Parser (Core):**
    - [ ] Implement `dctx_template` file parser (Sections, Key-Value).
    - [ ] Implement Variable Resolver (Pass 1).
- [ ] **Template Compiler (CLI):**
    - [ ] Implement `dctx template build` command.
    - [ ] Implement Validation logic (Circular dependency check).
    - [ ] Implement Serialization to `~/.cache/dctx/templates/`.
- [ ] **Runtime Renderer:**
    - [ ] Implement Data Injection (Pass 2).
    - [ ] Integrate with `llm_formatter.rs`.

## 🟠 Phase 3: CLI & Interaction
*Goal: Build the user interface.*

- [ ] **CLI Framework:** Implement `clap`.
- [ ] **Headless Mode:**
    - [ ] `dctx <path>` (Standard run).
    - [ ] `-c` / `-d` (Clipboard & Diff).
    - [ ] `-t <template_name>` (Flag to select custom template).
- [ ] **Interactive Mode:** TUI Menu.
- [ ] **Config Loader:** TOML support for user preferences.

## 🔴 Phase 4: Distribution
*Goal: Release prep.*

- [ ] **Git Safety:** Auto-add `.dctx/` to `.gitignore`.
- [ ] **CI/CD:** GitHub Actions.
- [ ] **Documentation:** User Guide for Templating Syntax.
