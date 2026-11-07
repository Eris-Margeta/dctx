<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
<!-- PP6 dependency gate for PL-V; uses Python stdlib and Cargo.
     See docs/design/plugin-evidence/PP-MATRIX.md, PP6 and V-ISO.
     The kernel runs the real gate; unit tests use synthetic Cargo output. -->
# PP6 dependency isolation

`isolation.py` checks three separate normal dependency resolutions:

1. `cargo tree -p dctx --no-default-features --features pack,map,templates,fs,segments,cache -e normal`
2. `cargo tree -p dctx-cli --no-default-features --features strict -e normal`
3. `cargo tree -p dctx-cli -e normal`

Each command also uses `--prefix none --color never` and tries `--offline`
first. A missing registry package/version or uncached crate download triggers one
retry without offline mode; other failures are reported immediately. Neither
`--frozen` nor `--locked` is used. The original `Cargo.lock` bytes are backed up
before the three resolutions and restored in `finally`, including on failure.
The registry cache is copied to a temporary Cargo home, without user
configuration or credentials. Cargo target and temporary paths also point into
that directory, removed on exit. Registry downloads on retry stay there.
This can require disk space comparable to the existing Cargo registry cache.

The first two closures must exclude `dctx-plugin-host`, `dctx-plugin-platform`,
and every crate whose name starts with `dctx-plugin-git`, including the worker
crate named in `plugins/dctx-plugin-git/Cargo.toml` (`dctx-plugin-git`).
PP6 forbids plugin workers and Git execution in the strict closure.
`dctx-plugin-distribution` is the installer/trust/discovery client (no code runs
during install), and `dctx-plugin-protocol` supplies wire types. Both are allowed
so the strict CLI can retain `dctx plugin list/install` management. See
`docs/design/PLUGINS.md` PP6 and `docs/design/plugin-evidence/PP-MATRIX.md` PP6.
The default closure must contain `dctx-plugin-git` as a positive control.
Empty, malformed or wrong-root output fails. Host/platform and the positive
control use exact names; Git exclusion uses the prefix above. Versions, source
paths, feature suffixes and duplicate markers are excluded from crate names.

Run the fixture suite once, then collect the real gate verdicts:

```sh
python3 -I -B -m unittest discover -s tools/plugin-conformance -p test_isolation.py
python3 -I -B tools/plugin-conformance/isolation.py --require-strict
```

`-B` prevents Python bytecode writes. Tests mock Cargo and require no installed
toolchain. The gate prints one PASS/FAIL/SKIP line per profile and returns nonzero
for any violation or failed invocation. Without `--require-strict`, only missing
Cargo is an explicit SKIP; required mode makes it FAIL. Diagnostics never turn an
unobserved closure into PASS. Each Cargo invocation has a 300-second timeout.

This packet establishes the normal dependency gate only. Binary/source audits,
successful pack/map execution and OS child-process observation described by the
broader V-ISO matrix require separate evidence.
