<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Exact measured commands

Every row in `runs.jsonl` includes its exact argv, working directory, environment overrides, stdout, stderr, byte count, SHA-256 and exit. The disposable `/tmp` prefix differs on reproduction.

Each query below ran with each of `text` and `json` five times before the edit and once after it; JSON adds `--json`.

## parser-mixed

```sh
cd /tmp/dctx-stab-01-w5856k5q/parser-mixed
export DCTX_CACHE_DIR=/tmp/dctx-stab-01-w5856k5q/cache/parser-mixed
export XDG_CACHE_HOME=/tmp/dctx-stab-01-w5856k5q/xdg-cache
export XDG_CONFIG_HOME=/tmp/dctx-stab-01-w5856k5q/config
```

```sh
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find helper --limit 10
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:rust/src/support.rs#helper' --dir in --edge calls
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:rust/src/support.rs#helper' --dir in --edge calls --depth 2
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:rust/src/support.rs#helper' --dir out --edge calls
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map map rust/src/support.rs --depth 2 --limit 40
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map read 's:rust/src/support.rs#helper'
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map explain 's:rust/src/support.rs#helper'
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map skeleton rust/src/support.rs
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map grep helper --in rust/src/support.rs
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map hubs
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map path m:rust/src/support.rs 's:rust/src/support.rs#helper'
```

Setup and edit controls (one invocation per line, in execution order):

```sh
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find helper --limit 40 --json
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find helper --json --meta
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find helper --json --meta
```

## lexer-go

```sh
cd /tmp/dctx-stab-01-w5856k5q/lexer-go
export DCTX_CACHE_DIR=/tmp/dctx-stab-01-w5856k5q/cache/lexer-go
export XDG_CACHE_HOME=/tmp/dctx-stab-01-w5856k5q/xdg-cache
export XDG_CONFIG_HOME=/tmp/dctx-stab-01-w5856k5q/config
```

```sh
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find Add --limit 10
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:tests/fixtures/basic.go#Add' --dir in --edge calls
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:tests/fixtures/basic.go#Add' --dir in --edge calls --depth 2
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:tests/fixtures/basic.go#Add' --dir out --edge calls
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map map tests/fixtures/basic.go --depth 2 --limit 40
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map read 's:tests/fixtures/basic.go#Add'
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map explain 's:tests/fixtures/basic.go#Add'
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map skeleton tests/fixtures/basic.go
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map grep Add --in tests/fixtures/basic.go
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map hubs
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map path m:tests/fixtures/basic.go 's:tests/fixtures/basic.go#Add'
```

Setup and edit controls (one invocation per line, in execution order):

```sh
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find Add --limit 40 --json
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find Add --json --meta
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find Add --json --meta
```

## map-query

```sh
cd /tmp/dctx-stab-01-w5856k5q/map-query
export DCTX_CACHE_DIR=/tmp/dctx-stab-01-w5856k5q/cache/map-query
export XDG_CACHE_HOME=/tmp/dctx-stab-01-w5856k5q/xdg-cache
export XDG_CONFIG_HOME=/tmp/dctx-stab-01-w5856k5q/config
```

```sh
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find render_answer_at --limit 10
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:adapters/output.rs#render_answer_at' --dir in --edge calls
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:adapters/output.rs#render_answer_at' --dir in --edge calls --depth 2
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map neighbors 's:adapters/output.rs#render_answer_at' --dir out --edge calls
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map map adapters/output.rs --depth 2 --limit 40
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map read 's:adapters/output.rs#render_answer_at'
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map explain 's:adapters/output.rs#render_answer_at'
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map skeleton adapters/output.rs
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map grep render_answer_at --in adapters/output.rs
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map hubs
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map path m:adapters/output.rs 's:adapters/output.rs#render_answer_at'
```

Setup and edit controls (one invocation per line, in execution order):

```sh
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find render_answer_at --limit 40 --json
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find render_answer_at --json --meta
/path/to/workdir/codex/STAB-01R/.target-stab/debug/dctx-cli map find render_answer_at --json --meta
```
