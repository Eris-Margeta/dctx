Historical design record; superseded by [the current documentation](../../../../../README.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Card 6: independent benchmark (bias removal)

Goal: a protocol we didn't design. Files: `bench/independent/{select.sh,gold_lsp.py,questions.md,run.sh}`. Method: repo selection by seed (sha256 of the v1.0.0 tag commit picks 3 repos from a public fixed list, excluding our six); callers gold from `rust-analyzer`/`pyright`/`gopls` find-references via a script, not from any of the three tools; 20 task questions from the repos' closed issue titles, facts checked against the repo; one tokenizer (`tiktoken cl100k`) for all tools. Gate: `run.sh` produces the table on the runner; results published regardless of outcome.
