<!-- SPDX-License-Identifier: Apache-2.0 -->
# CLI output size, 2026-11-12

## Scope and method

The supplied private benchmark aggregates were CLI 14.16 M tokens, 720 s,
130 turns; grep 14.90 M tokens, 701 s, 172 turns. No private task data or
individual traces were available. These aggregates do not attribute turns to
particular tool sequences. The [whole-session report](../BENCHMARK-20261111.md)
gives the full grep and CLI figures and limitations.

The local corpus was the complete tracked repository at `a6259da2`, exported
once into an ignored directory under `target/`. Both executables queried the
same frozen corpus, with its ignore rules, isolated configuration and a shared
worktree-local index cache. Measurements count UTF-8 stdout bytes including LF,
without a shell wrapper. They are not tokenizer measurements.

## CLI results

The before and after columns retain the historical command measurements.
Most commands used matching options. Task pack used the existing 3000-token
default before and an explicit `--budget 2000` after. Impact used its CLI
defaults before and explicit `--max-sites 50 --max-bytes 8192` after. The impact
reduction is a bounded, potentially truncated answer, not lossless compression.

| Operation / encoding | CLI text before / after |
| --- | ---: |
| find / text | 141 / 141 |
| find / JSON | 943 / 943 |
| read / text | 251 / 251 |
| read / JSON | 725 / 725 |
| neighbors / text | 297 / 297 |
| neighbors / JSON | 704 / 704 |
| named context / text | 1,038 / 1,038 |
| named context / JSON | 1,209 / 1,209 |
| task pack / text | 1,187 / 1,187 |
| task pack / JSON | 1,276 / 1,276 |
| impact / text | 42,994 / 6,339 |
| impact / JSON | 65,153 / 7,874 |

## Queries and interpretation

- Find: `dctx map find estimate_tokens`.
- Read: `dctx map read 's:crates/dctx-core/src/budget.rs#estimate_tokens'`.
- Neighbors: `dctx map neighbors ID --dir in --edge calls`, with the same ID.
- Named context: `dctx context ID`, with the same ID.
- Task pack: ``dctx pack --for '`estimate_tokens`' --budget 2000`` after;
  the before comparison used the existing 3000-token CLI default.
- Impact: `dctx impact ID`, with the same ID and the bounds described above.

JSON rows add `--json` to the CLI command. Count bytes from captured stdout,
for example `len(subprocess.run(..., capture_output=True).stdout)` in Python.
No filter or symbol changed between the impact measurements. Warm caches can
change latency but not the bytes of these answers. Exact byte counts also
depend on the frozen corpus and its root label in text headers; these are
historical observations, not output-size promises for the current checkout.

Task pack selects definitions, relations and source against one pinned snapshot.
It can combine work otherwise requiring find, neighbors and read calls, but
this study does not establish which sequences private benchmark agents used.
Use neighbors pagination or explicitly bounded impact for a complete
investigation; task pack is a ranked selection, not an exhaustive caller list.
These payload measurements do not establish whole-session token or wall-time
savings. No private session benchmark was rerun for this output-size study.
