# Independent benchmark (v1 card 6)

This benchmark uses a protocol the dctx team did not tune. The repos are picked by seed, the gold comes from language
servers, the questions follow fixed rules, and one tokenizer counts for every tool. The protocol, the question rules and
the per-tool commands are in [`questions.md`](questions.md).

## Run (Linux runner, after `git tag v1.0.0`)

```sh
docs/research/bench-independent/run.sh <release dctx-cli>
# INDEP_SEED=<full sha> overrides the v1.0.0 seed (recorded as "explicit --seed"); INDEP_TOOLS=dctx,trail limits the tools.
```

It prints the gold and question counts, then the table. Its last line is `INDEP-BENCH DONE <results dir>`. The results
dir holds `selection.json`, `gold/*.json`, `questions.json`, `questions.generated.md`, `tasks.jsonl`, `callers.jsonl`,
`independent.json`, `table.md` and the logs. A score never fails the run. Exit 3 means a harness step broke, and its log is
named on stderr.

| Step | File | Network |
|---|---|---|
| seeded selection | `select.sh` → `pick_repos.py`, `repos.json` | `git ls-remote` only |
| setup | `setup.sh`: repo clones, rust-analyzer / pyright / gopls, tiktoken, Trail-Code and graphify (via `../bench-3way-20261030/scripts/setup_tools.sh`) | yes, the only such step |
| gold | `gold_lsp.py` + `lsp.py` (stdlib JSON-RPC client) | no (`unshare -n`, `CARGO_NET_OFFLINE`, `GOPROXY=off`) |
| questions | `questions.py` | no |
| tools | `run_indep.py` + `adapters.py`, through `bench3lib.run` (`unshare -Urn`, `env -i`, `taskset`) | no |
| table | `report.py`, `tokens.py` | no |

Offline self-test, which needs no tools and no network: `python3 -m unittest discover -s docs/research/bench-independent/tests -v`.

## Pins and what they do not guarantee

- **Repos.** Each repo is pinned by release tag in `repos.json`. `select.sh` resolves the tag to a commit once and records
  it in `selection.json`, and `setup.sh` refuses any other HEAD. A tag that moved upstream before the first run would go
  unnoticed. One that moves later is caught.
- **Servers.**
  - rust-analyzer: release `2024-10-28`. Its sha256 is recorded on the first download and checked on every later run.
  - pyright: `1.1.386` from npm.
  - gopls: `v0.16.2`. It needs a Go toolchain on the runner.
- **Tokenizer.** tiktoken `0.8.0`, `cl100k_base`. The BPE file's sha256 is pinned in `repos.json` and checked by `tokens.py`,
  and there is no byte-estimate fallback. The 3-way and October tables used `ceil(bytes / 3.5)`, so their token numbers
  are not comparable with this table.
- **Comparison tools.** Trail-Code 0.28.0 and graphify 0.9.97 (code-only), the same installs as the 3-way benchmark.
