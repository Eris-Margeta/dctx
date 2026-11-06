DID — remeasured the pinned current build with the card's local-corpus fallback; 33/33 text and 33/33 JSON answers survived unrelated edits and five runs, recorded M7 and exact raw commands, and updated only permitted status texts. Tier layering remains target.

Measured on **2026-11-06 UTC**, HEAD
`0495f5f87717df5385209d306b92328a8cd96cf6`. No commit or engine change.

Evidence: [report](../research/stability-20261105/REPORT.md),
[new CSV](../research/stability-20261105/stability.csv),
[M7 transitions](../research/stability-20261105/turn-prefixes.csv),
[exact query commands](../research/stability-20261105/commands.md),
[405 full invocation records](../research/stability-20261105/runs.jsonl),
[machine summary](../research/stability-20261105/summary.json).

The original producer was found in
`docs/research/trail-code-2026-10/scripts/phase_query.py` and `aggregate.py`.
The original benchmark host/corpora tree is absent. The fallback extracts 66
tracked files from three prefixes at HEAD: parser fixtures (17), Go lexer crate
(19), and map-query source (30). All inputs are pinned by SHA-256 in
`corpora.json`. Eleven query shapes per corpus, each text and JSON, make 66 cases.
Each case runs five times before and once after a comment appended to an
unrelated file in another directory. Three `--meta` before/after controls prove
the edits reached distinct snapshots. Disposable copies are restored and removed.

| Check | Count / result | Exit |
|---|---|---:|
| Initial offline build, configured `sccache` | Permission failure before compilation | 101 |
| Offline build after unsetting wrapper environment variable | Cargo config still selects `sccache` | 101 |
| Offline build with explicitly empty wrapper | `dctx-cli` built from this tree | 0 |
| Measurement script | 405 CLI invocations; all exit 0, all stderr empty | 0 |
| Text after unrelated edit | 33/33 identical | 0 for all 33 post-edit commands |
| JSON after unrelated edit | 33/33 identical | 0 for all 33 post-edit commands |
| Five-run repeatability | 33/33 text and 33/33 JSON; 330 invocations | 0 for all 330 |
| Setup / edit controls | Three resolutions + six metadata controls | 0 for all 9 |
| Same-request M7 | 165 text + 165 JSON transitions; minimum and mean 1.0 | audited |
| Changing-question M7 | 195 text + 195 JSON transitions; mean 0.21483025659446645 / 0.18244679358762136 | audited |
| Raw-data audit | 405 records, 66 pinned files, 720 M7 transitions, three changed-snapshot controls, old CSV preserved | 0 |
| Write-scope audit | Only permitted status spans in README/SPEC, new card/evidence files | 0 |
| Whitespace check | `git diff --check` | 0 |

Exact build and measurement commands, from `/path/to/workdir/codex/STAB-01R`:

```sh
export PATH=$HOME/.cargo/bin:$PATH
export CARGO_TARGET_DIR=$PWD/.target-stab
cargo --offline build --locked -p dctx-cli > /tmp/stab-01-build.log 2>&1
```

Exit 101. Retry:

```sh
export PATH=$HOME/.cargo/bin:$PATH
export CARGO_TARGET_DIR=$PWD/.target-stab
unset RUSTC_WRAPPER
cargo --offline build --locked -p dctx-cli > /tmp/stab-01-build-no-wrapper.log 2>&1
```

Exit 101. Successful execution:

```sh
export PATH=$HOME/.cargo/bin:$PATH
export CARGO_TARGET_DIR=$PWD/.target-stab
export RUSTC_WRAPPER=
cargo --offline build --locked -p dctx-cli > /tmp/stab-01-build-direct.log 2>&1
python3 docs/research/stability-20261105/measure.py > /tmp/stab-01-measure.log 2>&1
python3 docs/research/stability-20261105/verify.py > /tmp/stab-01-verify.log 2>&1
```

Each exited 0. Build logs are preserved as `build-initial.txt`,
`build-unset-wrapper.txt`, and `build-direct.txt` in the evidence directory;
measurement stdout is `measurement.txt`. Each of the 405 CLI records includes
exact argv, shell command, cwd, environment overrides, full stdout/stderr, byte
count, SHA-256 and exit. `commands.md` expands the query matrix and controls.

After recording binary identity and completing measurements, the generated target
directory was removed with:

```sh
python3 - <<'PY'
import shutil
from pathlib import Path
p=Path('.target-stab')
assert p.is_dir() and p.resolve() == Path('/path/to/workdir/codex/STAB-01R/.target-stab')
shutil.rmtree(p)
print('Removed generated .target-stab build artifacts; no Git ignore files changed.')
PY
```

Exit 0. Final checks:

```sh
python3 docs/research/stability-20261105/verify.py
python3 docs/research/stability-20261105/verify_scope.py
git diff --check
git status --short --untracked-files=all
```

Each exited 0. The historical CSV remains byte-identical, SHA-256
`46b54db2fb469dc4df70eaa0bf22b458a8089f437cdee3294efe071314280266`.
Its exact copy `stability-original.csv` sits beside the new CSV inside the own set;
the old evidence directory has no modifications. The 0/63 and 59/70 historical
values remain identified as history. The new 33/33 denominator and date are explicit.

No unrelated-edit instability was observed. Changing-question tool-answer M7 does
not establish the T0–T3 marker invariant or the tier-context M7 ≥ 0.9 gate; §12.1
is unchanged and remains target. No original-corpus, cross-platform, provider-cache
or task-acceptance result is claimed.
