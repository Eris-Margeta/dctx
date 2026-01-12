# DCTX Execution Plan (Rev. 3)

**Philosophy:** Zero Dependencies. High Performance. Enterprise Grade.
**Goal:** Reach feature parity with the legacy C utility and establish a proper system installation workflow.

## 🟢 Phase 0: System Integration (Immediate)
**Goal:** Ensure `dctx` can be installed and invoked globally as a system tool.
- [ ] Create `tools/install.sh`.
    - Builds `dctx-cli` in release mode.
    - Installs binary to `/usr/local/bin/dctx`.
    - Creates `~/.config/dctx/` structure.
    - Deploys default `config.toml`, `.dctxignore`, and `templates/`.
- [ ] Define default configuration values in `dctx-core` to match the installed files.

## 🟡 Phase 1: The Interface Layer (Pure Rust)
**Goal:** Implement argument parsing and Mode Switching.
- [ ] **Argument Parser:**
    - Remove dependencies. Use `std::env::args`.
    - Parse flags: `-c` (Clipboard), `-d` (Diff), `--reset`, `--help`.
- [ ] **Mode Switching:**
    - If `args.len() == 1` (just binary name): Launch **Interactive TUI**.
    - If `args.len() > 1`: Launch **Headless Walker**.
- [ ] **Interactive TUI (Stub):**
    - Simple `println!` menu with `std::io::stdin` selection.
    - Option 1: Run on current dir.
    - Option 2: Help.

## 🟠 Phase 2: The Core Logic Loop (The "Brain")
**Goal:** Connect the disconnected modules (`walker`, `diff`, `version`) in `main.rs`.
- [ ] **State Check:**
    - Look for existing `.dctx` binary in `~/.cache/dctx/` (or local `.dctx/`).
- [ ] **Versioning Logic:**
    - If binary exists -> Read Header -> Extract Version.
    - If binary missing -> Version = V1.
- [ ] **Walk & Diff:**
    - Scan current directory -> `new_tree`.
    - If old exists -> `diff::compare_trees(old, new)`.
    - Determine `next_version` (Incremental vs Major Bump).
- [ ] **Write Binary:** Update the `.dctx` state file.

## 🔴 Phase 3: Output & Clipboard Integration
**Goal:** Direct output piping without unnecessary file I/O.
- [ ] Refactor `llm_formatter` to accept `&mut dyn Write` (already done, just need to utilize it).
- [ ] **Clipboard Logic:**
    - Implement `platform::copy_to_clipboard` using `std::process::Command`.
    - Support `pbcopy` (macOS), `xclip`/`wl-copy` (Linux), `clip.exe` (Windows).
- [ ] **Diff Output:**
    - If `diff_mode` is active, generate the diff text instead of full context.

## 🟣 Phase 4: Polish & Templates
**Goal:** Ensure the output is aesthetically pleasing and matches the legacy C tool.
- [ ] Verify `default.dctx_template`.
- [ ] Ensure formatting (whitespace, markers) is identical to the C version.
- [ ] Final `cargo clippy` and `cargo fmt` pass.
