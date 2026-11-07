Historical design record; superseded by [the current documentation](../../../../../README.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Card 5: CLI errors and help

Goal: no panics, actionable messages. Files: `crates/cli/src/main.rs`, `crates/cli/src/error.rs`, `tests/cli_errors.rs`. Pre-written tests: 12 bad inputs each expect exit code and a message containing a hint ("run `dctx index` first"), no `RUST_BACKTRACE` text; `--help` for every subcommand contains "Example:".
