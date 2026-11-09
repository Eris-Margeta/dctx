# Binary ownership and cache scopes

The dctx engine produces every output byte. A package wrapper only transports requests and responses to its selected engine. Installing a library does not change the human's system installation.

| Caller | Binary | Owner and version policy |
| --- | --- | --- |
| Humans, interactive agents and MCP clients | System `dctx` | The human and their agents manage the installation on PATH. Configure MCP with its absolute executable path when PATH may be shadowed. |
| Application code and CI | Project `dctx` | The project's package manager installs and pins the engine within its virtualenv, `node_modules`, or project Cargo install root. |
| A caller configured with `DCTX_BIN` | Explicit executable | The caller chooses that exact path and is responsible for its version. |

A pip/npm/Cargo library invokes its own installed, version-pinned engine by absolute path. A library never silently falls back to PATH. A missing or unusable project engine is an installation error. An explicit engine override must be deliberate and validated by the calling library. `DCTX_BIN` is diagnostic input to the CLI; it does not make an already-running CLI forward commands elsewhere.

Activating a virtualenv or adding `node_modules/.bin` to PATH can shadow a system installation. Interactive agents and MCP should keep using the human's chosen system executable. Application code and CI should keep using their pinned project executable. These are ownership scopes of the same engine, not separate implementations.

## Inspect installations

```sh
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

The CLI cache root is `<cache-parent>/<engine-major.minor>/`. For example, engines 1.2 and 1.3 use separate indexes and ledgers. Compatible patch releases share their major/minor namespace. System and project installations of the same engine family may reuse this cache; binary ownership does not change the index format. A caller can set a separate `DCTX_CACHE_DIR` to isolate a project's cache further.

`DCTX_CACHE_DIR` is an absolute cache **parent**, not a final engine namespace. Without it, the existing platform parent is retained:

| Platform | Cache parent | Example engine root |
| --- | --- | --- |
| Linux / other Unix | `$XDG_CACHE_HOME/dctx/v2`, or `$HOME/.cache/dctx/v2` | `…/dctx/v2/1.2/` |
| macOS | `$HOME/Library/Caches/dctx/v2` | `…/dctx/v2/1.2/` |
| Windows | `%LOCALAPPDATA%\dctx\Cache\v2` | `…\dctx\Cache\v2\1.2\` |

The `v2` parent is the existing cache-container name; the next directory selects the engine family. All normal CLI cache readers, writers, policy, ledger, auto-clean and cache commands use the selected namespace. Explicit Rust host APIs that accept a cache root continue to use the exact root supplied by that host; custom hosts must supply an isolated engine namespace themselves.

Old unscoped cache files are ignored. Startup, `doctor`, auto-clean and ordinary commands never migrate or delete them, and other engine namespaces are untouched. A new engine builds its own index on first use. Existing `cache purge --all --dry-run` / `cache purge --all --yes` target the current engine namespace. A host needing to purge a legacy root can use the existing Rust `dctx_cli::run_in` cache-purge API with that exact root; the normal purge confirmation and managed-ledger rules still apply. There is no automatic legacy-directory cleanup or recursive deletion of unknown files.
