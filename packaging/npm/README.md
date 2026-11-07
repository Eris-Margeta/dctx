# npm release packages

`dctx/` contains the JavaScript command shim. Each `cli-*` directory is an optional,
OS/CPU constrained binary package. Platform executables are generated and ignored
by git; nothing downloads at installation time.

From the repository root, fill all six packages from a GitHub Release's five
archives and `SHA256SUMS`:

```sh
python3 tools/release/prepare-npm.py v1.1.0 artifacts/
(cd packaging/npm/dctx && npm pack --dry-run)
```

The script verifies every archive before writing packages, sets all versions from
the tag and copies LICENSE/NOTICE. Archive names use full Cargo target triples.
Publish all five `cli-*` packages before `dctx`; the tag workflow does this with
provenance. See [the maintainer runbook](../../docs/RELEASING.md) for credentials and recovery.

Run `python3 tools/release/test-packaging.py` on Linux x64 to verify fixture archives,
all six npm tarballs, checksum rejection, argument forwarding, exit status and
signals without publishing. It requires Node/npm and uses the normal temp/cache
directories (override TMPDIR and npm_config_cache when needed).
