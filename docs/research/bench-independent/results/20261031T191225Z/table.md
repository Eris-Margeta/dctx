<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Independent benchmark (protocol v1)

Seed `f77f86ac255ad5c35645dab2242ed1a56804cc1f` (explicit --seed). Tokenizer: cl100k_base for every tool, raw stdout. Gold: language servers (callHierarchy), facts from the repos' own history. Published regardless of outcome.

| Repo | Language | Tag | Commit |
|---|---|---|---|
| zoxide | rust | v0.9.6 | `3d3267b4fd73` |
| requests | python | v2.32.3 | `0e322af87745` |
| gin | go | v1.10.0 | `75ccf94d605a` |

## Task questions

| Tool | Points | Max | Facts found | Facts | Tokens | Facts per 1k tokens |
|---|---|---|---|---|---|---|
| dctx | 22 | 28 | 31 | 36 | 4526 | 6.850 |
| trail | 19 | 28 | 17 | 36 | 5834 | 2.910 |
| graphify | 14 | 28 | 13 | 36 | 8926 | 1.460 |

## Callers against LSP gold (first page)

| Tool | Targets | Resolved | Precision | Recall | Recall, all pages | Tokens |
|---|---|---|---|---|---|---|
| dctx | 40 | 40 | 0.730 | 0.341 | 0.341 | 20126 |
| trail | 40 | 40 | 0.731 | 0.313 | n/a | 10469 |
| graphify | 40 | 38 | 0.920 | 0.264 | n/a | 1387 |

## Per repo

| Repo | Tool | Points / max | Callers P | Callers R |
|---|---|---|---|---|
| gin | dctx | 10 / 12 | 0.935 | 0.230 |
| gin | trail | 7 / 12 | 1.000 | 0.191 |
| gin | graphify | 9 / 12 | 1.000 | 0.257 |
| requests | dctx | 0 / 2 | n/a | n/a |
| requests | trail | 0 / 2 | n/a | n/a |
| requests | graphify | 0 / 2 | n/a | n/a |
| zoxide | dctx | 12 / 14 | 0.581 | 0.900 |
| zoxide | trail | 12 / 14 | 0.591 | 0.933 |
| zoxide | graphify | 5 / 14 | 0.692 | 0.300 |

Precision counts a caller the language server did not report as wrong; servers can miss dynamic calls (Python receivers, Go interfaces), so precision is a lower bound for every tool alike.
