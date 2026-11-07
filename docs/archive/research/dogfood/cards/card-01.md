Historical design record; superseded by [the current documentation](../../../../../README.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Card 1: privacy gate for all commands

Goal: canary never reaches stdout from any subcommand. Files: `crates/cli/tests/privacy.rs`, `map-scan/src/ignore.rs`, `map-query/src/read.rs`. Gate: test iterates every subcommand (including `map read m:.env`) on `bench/corpora/canary`, asserts canary strings absent; `--allow-sensitive` path tested separately.
