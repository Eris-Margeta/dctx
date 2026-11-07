<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-004: CLI Implementation Strategy & Zero-Dependency Mandate

**Status:** superseded — historical design and implementation record.

Use the [current product contracts](design/PRODUCT-CONTRACTS.md), [README](../README.md) and [testing guide](TESTING.md) for release behavior.
**Date:** 2026-01-12
**Author:** dctx Team
**Context:** The `dctx` tool is designed to be a lightweight, "instant-on" utility that can run in diverse environments (CI/CD containers, old servers, developer laptops). We recently ported the Logic to Rust (dctx-core) and the Interface to Rust (dctx-cli). We need to codify the constraints for the CLI to prevent binary bloat and compilation overhead.

## 1. Decision: The Zero-Dependency Mandate
To ensure maximum portability and near-instant compilation times, `dctx-cli` must strictly avoid heavy external crates.

*   **Banned Crates:** `clap`, `structopt`, `tokio`, `async-std`, `anyhow`, `thiserror`, `serde` (unless absolutely necessary for core logic, but avoided in CLI).
*   **Allowed Standard Library:** `std::env`, `std::fs`, `std::io`, `std::process`, `std::path`.
*   **Rationale:** A simple directory walker and text formatter does not require an async runtime or a heavy argument parser. This keeps the binary size small (<5MB) and startup time negligible.

## 2. Decision: Custom Argument Parsing
Instead of using a library, we implement a custom, strict argument parser using `std::env::args`.

### Supported Arguments
*   **Positionals:**
    *   `[PATH]`: Optional. The target directory to snapshot. Defaults to current directory (`.`) if not provided in Headless Mode.
*   **Flags:**
    *   `-c` / `--clipboard`: Boolean. Redirects output to system clipboard. Disables file writing.
    *   `-d` / `--diff`: Boolean. Output *only* the diff (changes) since the last snapshot. Requires existing state.
    *   `-r` / `--reset`: Boolean. Forces a "Hard Reset" of the version history to V1, deleting previous state.
    *   `-h` / `--help`: Prints usage.
    *   `-v` / `--version`: Prints version.

### Behavior
*   Flags can appear anywhere after the binary name.
*   Unknown flags trigger a hard exit with error.
*   Multiple path arguments trigger a hard exit with error.

## 3. Decision: Hybrid Execution Mode
The CLI determines its mode based purely on the presence of arguments.

### Interactive Mode (TUI)
*   **Trigger:** `args.len() == 1` (Only the binary name is invoked).
*   **Implementation:** A simple `std::io::stdin` loop printing a numbered menu.
*   **Menu Options:**
    1.  Snapshot Current Directory (`.`)
    2.  Snapshot Current Directory to Clipboard (`. -c`)
    3.  Help
    4.  Exit

### Headless Mode
*   **Trigger:** Any argument is provided.
*   **Implementation:** Executes the logic flow immediately and exits. Logs are printed to `stderr` (or `stdout` if informational). Output content is handled based on flags.

## 4. Decision: Platform Abstraction Layer
OS-specific functionality (specifically Clipboard access) is isolated in a `platform.rs` module within the CLI crate.

*   **Mechanism:** usage of `std::process::Command` to pipe data to system utilities.
*   **macOS:** `pbcopy`
*   **Linux (Wayland):** `wl-copy`
*   **Linux (X11):** `xclip -selection clipboard`
*   **Windows:** `clip.exe`
*   **Fallback:** If the utility is missing, the program errors gracefully with a suggestion to install it. We do *not* link against X11/Wayland libraries directly.

## 5. Decision: Output Routing
*   **File Mode (Default):** Writes `.llmcontext.txt` (and/or diffs) to disk.
*   **Clipboard Mode (`-c`):** 
    *   Writes the generated content to an in-memory `Vec<u8>` buffer.
    *   Pipes buffer to the Platform Clipboard handler.
    *   **Crucial:** Does *not* write text files to disk.
    *   **State:** DOES update the binary `.dctx` state file (to ensure version continuity).
