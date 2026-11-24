# Binary ownership and cache scopes

The dctx engine produces every output byte. A package wrapper only transports requests and responses to its selected engine. Installing a library does not change the human's system installation.

| Caller | Binary | Owner and version policy |
| --- | --- | --- |
| Humans and interactive agents | System `dctx` | The human and their agents manage the installation on PATH. Invoke dctx with its absolute executable path when PATH may be shadowed. |
| Application code and CI | Project `dctx` | The project's package manager installs and pins the engine within its virtualenv, `node_modules`, or project Cargo install root. |
| A caller configured with `DCTX_BIN` | Explicit executable | The caller chooses that exact path and is responsible for its version. |

A pip/npm/Cargo library invokes its own installed, version-pinned engine by absolute path. A library never silently falls back to PATH. A missing or unusable project engine is an installation error. An explicit engine override must be deliberate and validated by the calling library. `DCTX_BIN` is diagnostic input to the CLI; it does not make an already-running CLI forward commands elsewhere.

Activating a virtualenv or adding `node_modules/.bin` to PATH can shadow a system installation. Interactive agents should keep using the human's chosen system executable. Application code and CI should keep using their pinned project executable. These are ownership scopes of the same engine, not separate implementations.

## Inspect installations

```sh runnable
dctx doctor
dctx doctor --json
dctx doctor --probe-project
dctx --version --verbose
```

`doctor` searches every PATH entry, `./node_modules/.bin`, the active `$VIRTUAL_ENV/bin` (`Scripts` on Windows), and `$DCTX_BIN`. Relative entries resolve from the current directory; duplicate visible paths appear once. The running executable is always included, even when it is absent from PATH. Symlink aliases remain visible as separate paths, and aliases of the running file are marked as running. Windows discovery includes `dctx.exe`, `.com`, `.cmd`, `.bat` and extensionless `dctx`. The Cargo package executable name `dctx-cli` is also discovered, with the same Windows extensions.

Each row reports the visible path, resolved path (JSON), engine version, build commit, scope (`system`, `project`, `explicit`), whether it is the running file, version-scoped cache root, logical file bytes in that root, and the executable file's SHA-256. The checksum follows an executable symlink to the file that runs. Cache accounting skips symlinks and special files. Missing cache directories have size zero and are not created. Sizes are a point-in-time observation; concurrent engine activity can change them.

Scope is inferred from the installation path: `DCTX_BIN` takes precedence for that visible path; paths in `node_modules`, the active virtualenv, an environment containing `pyvenv.cfg`, or a Cargo install root (`.crates.toml`) within the current project (located by `Cargo.toml`, `package.json` or `pyproject.toml`) are project scope; other paths are system scope. Cargo `--root` installations outside the current project cannot reliably be assigned to that project and appear as system unless selected explicitly with `DCTX_BIN`. Scope describes discovery, not a permission boundary.

The running engine's version and build commit are known directly. `doctor` executes only candidates the user chose: files in absolute PATH entries and `$DCTX_BIN`. Candidates found only through `./node_modules/.bin`, `$VIRTUAL_ENV`, or a relative PATH entry can be committed by an untrusted checkout, so by default they are never executed: their row reports the path and SHA-256, the version and commit are unavailable, and an error names `--probe-project`. Pass `--probe-project` only in a checkout you trust to also ask those candidates for `--version`. Executed candidates are asked for `--version` with auto-clean disabled, stdin closed, a two-second timeout and at most 4096 bytes of output; the commit is the hex word in parentheses of `dctx X.Y.Z (commit)`, otherwise unavailable. No update check, download, package-manager command, cache session, lock, or auto-clean runs. Candidates must honor the read-only `--version` contract. A broken or unrecognized candidate remains in the report with unavailable fields and errors. Text escapes terminal control characters. JSON is one document with `schema_version: 1` and a `binaries` array; unavailable values are `null`, and each row has an `errors` array. A completed inventory exits successfully even when a candidate has errors; an invalid or repeated doctor argument exits 2 and names that argument.

`--version --verbose` adds `executable:` and `scope:` to the existing version and capability diagnostics. Paths and installation versions are diagnostic metadata only. They are never added to pack output.

## Cache isolation

The 1.3 CLI uses `<cache-parent>/<engine-major.minor>/` as its cache root. Compatible patches such as 1.3.1 and 1.3.2 share the `1.3` directory and ledger when they use the same parent. System and project installations can share that cache or set separate parents for isolation.

Set `DCTX_CACHE_DIR` to an absolute cache parent path. For example, with `DCTX_CACHE_DIR=/path/to/dctx-cache`, the 1.3 CLI uses `/path/to/dctx-cache/1.3` and its ledger is `/path/to/dctx-cache/1.3/ledger.v1`. With `DCTX_CACHE_DIR` unset or empty, the paths are:

| Platform | Cache parent | Active cache directory (1.3) | Active ledger (1.3) |
| --- | --- | --- | --- |
| Linux / other Unix (`XDG_CACHE_HOME` set and nonempty) | `$XDG_CACHE_HOME/dctx/v2` | `$XDG_CACHE_HOME/dctx/v2/1.3` | `$XDG_CACHE_HOME/dctx/v2/1.3/ledger.v1` |
| Linux / other Unix (`XDG_CACHE_HOME` unset or empty) | `$HOME/.cache/dctx/v2` | `$HOME/.cache/dctx/v2/1.3` | `$HOME/.cache/dctx/v2/1.3/ledger.v1` |
| macOS | `$HOME/Library/Caches/dctx/v2` | `$HOME/Library/Caches/dctx/v2/1.3` | `$HOME/Library/Caches/dctx/v2/1.3/ledger.v1` |
| Windows | `%LOCALAPPDATA%\dctx\Cache\v2` | `%LOCALAPPDATA%\dctx\Cache\v2\1.3` | `%LOCALAPPDATA%\dctx\Cache\v2\1.3\ledger.v1` |

The `v2` parent is the existing cache-container name; the next directory selects the engine family. All normal CLI cache readers, writers, policy, ledger, auto-clean and cache commands use the selected namespace. Explicit Rust host APIs that accept a cache root continue to use the exact root supplied by that host; custom hosts must supply an isolated engine namespace themselves.

In the pre-1.3 layout used by the tagged 1.2.0 CLI, the unscoped ledger is `<cache-parent>/ledger.v1`. With the same `DCTX_CACHE_DIR=/path/to/dctx-cache` across an upgrade, that ledger is `/path/to/dctx-cache/ledger.v1`. The 1.3 CLI leaves the unscoped ledger in place and builds an index in its own child directory on first use. Auto-clean and `cache purge --all` are scoped to the active child directory.

A custom Rust host can target the legacy root explicitly with the `dctx_cli::run_in` cache-purge API. The normal purge confirmation and managed-ledger rules apply.

To back up the active 1.3 cache, copy the whole `1.3` directory, including its ledger, policy and managed objects. To retain the old 1.2.0 cache, back up the parent containing the unscoped ledger and its data. A backup of the parent also includes any versioned child directories present there. Leave the original files in place during the upgrade.

Run `dctx doctor --json` with the 1.3 binary and the same environment as your cache-using commands. The row with `running: true` gives the active directory in `cache_root`. For other detected binaries, doctor computes that field from the reported major and minor version. Use the parent to locate the actual unscoped ledger of a 1.2.0 installation.
