# DCTX Execution Plan (Rev. 4)

**Philosophy:** Zero Dependencies. High Performance. Enterprise Grade.
**Goal:** Reach feature parity with the legacy C utility and establish a proper system installation workflow.

## 🟢 Phase 0: System Integration (Completed)
- [x] Create `tools/install.sh`.
- [x] Install binary and default configs to system paths.

## 🟡 Phase 1: The Interface Layer (Pure Rust)
**Goal:** Implement argument parsing and Mode Switching without `clap`.
- [ ] **Argument Parser:**
    - Use `std::env::args`.
    - Parse flags: `-c` (Clipboard), `-d` (Diff), `--reset`, `--help`, `--version`.
    - Support `dctx .` and `dctx path/to/target`.
- [ ] **Mode Switching:**
    - If no path/args provided: Launch **Interactive TUI** (Simple `stdin` menu).
    - If path provided: Launch **Headless Walker**.
- [ ] **Clipboard Support:**
    - Implement `platform::copy_to_clipboard` logic (piping to `pbcopy`/`xclip`).

## 🟠 Phase 2: The Core Logic Loop (The "Brain")
**Goal:** Connect the disconnected modules (`walker`, `diff`, `version`) in `main.rs`.
- [ ] **Config Loader:**
    - Implement `config::load_user_config` to read `~/.config/dctx/config.toml`.
- [ ] **State Check:**
    - Look for existing `.dctx` binary in `~/.cache/dctx/HASH.dctx`.
- [ ] **Versioning Logic:**
    - If binary exists -> Read Header -> Extract Version (e.g., `V1.2`).
    - If binary missing -> Version = `V1`.
- [ ] **Walk & Diff:**
    - Scan current directory -> `new_tree`.
    - If old exists -> `diff::compare_trees(old, new)`.
    - Determine `next_version` (Incremental vs Major Bump).
- [ ] **Write Binary:** Update the `.dctx` state file.

## 🔴 Phase 3: Output Formatting Parity
**Goal:** Fix the regression in output quality to match Legacy C exactly.
- [ ] **Fix IDs:**
    - Update `datatypes.rs` or `llm_formatter.rs` to generate `ROOT`, `F001`, `D002` style IDs.
- [ ] **Fix Header:**
    - Hardcode `[DIRCONTXT_LLM_SNAPSHOT_V...]` instead of using the folder name.
- [ ] **Fix Indentation:**
    - Ensure directory tree rendering respects recursion depth.
- [ ] **Fix Instructions:**
    - Correct the newline (`\n`) parsing in the template engine so instructions are readable.
- [ ] **Clean Tags:**
    - Remove `SIZE` and `LINES` attributes from `<FILE_CONTENT_START>` to match Legacy cleanliness.

## 🟣 Phase 4: Final Polish
- [ ] Verify `dctx . -c` works (Clipboard mode).
- [ ] Verify `dctx` (No args) opens the menu.
- [ ] Final `cargo clippy` pass.
