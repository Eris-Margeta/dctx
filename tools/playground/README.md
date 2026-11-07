# dctx-play: the website playground

`dctx-play` runs the real `dctx` binary for the playground at <https://www.dctx.internet.technology/playground/>.
It is a separate Cargo package (excluded from the workspace), standard library only.

- **API** (`/api/v1`, same origin, no CORS): `GET health`, `GET samples`, `POST run`, `POST mcp`,
  `POST run-form` (the no-JavaScript form, answered with HTML), `GET metrics.json`, `GET openapi.json`.
  The full contract is [`openapi.json`](openapi.json).
- **Grammar** (`src/grammar.rs`): the first word is `pack`, `map` or `languages`; map verbs and flags are
  those `dctx map --help` lists, with their bounds. `--place`, `--output`, `--clipboard`, `--rev`,
  `--profile`, `cache`, `template`, `plugin`, `git` and the legacy snapshot are refused with a reason.
  Paths are relative and never contain `..`. Uploads: at most 48 text files, 96 KiB each, 384 KiB in all.
- **Each run**: a fresh directory under `--work`, the place copied in under its sample name (so answers
  read `# kiosk find`, exactly as on the website) or `project` for pasted files; an empty environment;
  `RLIMIT_CPU` 2 s, `RLIMIT_AS` 256 MiB, `RLIMIT_NOFILE` 64, `RLIMIT_FSIZE` 8 MiB, `RLIMIT_NPROC` 16;
  a 3 s wall clock, then `SIGKILL`; stdout capped at 256 KiB; the directory is deleted afterwards.
- **Abuse controls**: 20 runs per minute per visitor (burst 5; the first `X-Forwarded-For` entry, set by
  the proxy), 2 concurrent runs, 4 workers and a queue of 8 connections (then `503 Retry-After: 2`),
  a 60 s cache of 64 identical requests, counters only (no addresses, nothing on disk).

## Build, test, run

```sh
cd tools/playground
cargo test                       # unit tests and HTTP tests against a stand-in dctx
./build-dist.sh                  # dist/: static musl binaries, samples, runtime Dockerfile
dist/bin/dctx-play --listen 127.0.0.1:8080 --samples dist/samples --dctx "$PWD/dist/bin/dctx" \
  --work /tmp/dctx-play --nproc 0  # outside a container RLIMIT_NPROC counts all your processes
./smoke.sh http://127.0.0.1:8080 # real dctx: recorded bytes, files, MCP, form, 400/413/429
```

`RLIMIT_NPROC` counts every process and thread of the user. In the container (uid 61802, a uid no other container on the host uses: the count is host-wide) that is the
server (5 threads) and at most two runs; dctx parses with `min(available_parallelism, 8)` threads,
which a 0.5-CPU container limits to one. Pass `--nproc 0` when running the server as a normal user.

Deployment: [`DEPLOY.md`](DEPLOY.md).
