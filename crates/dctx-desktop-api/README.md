# dctx-desktop-api

Pure engine seam for the dctx desktop (ARCHITECTURE-PROPOSAL §7 row DX-E4, first half).

The Tauri app cannot be compiled on the Linux dev VM because it links webkit2gtk. This crate holds the command logic that app will call: snapshot generation, desktop settings, and the recent-project list. It depends only on the `dctx` facade, `serde`, and `serde_json`.

- Snapshots go through `dctx::Session`. The file-count limit and the error texts match `generate_snapshot`.
- Every directory is an argument. The crate does not read the home directory, `XDG_*`, or `APPDATA`.
- Recent-project timestamps are an argument. The crate does not link `chrono`.
- Git is not a runtime dependency (decision 078). The crate never spawns `git` and never reads a `.git` directory.

`DX-E4b` switches `apps/dctx-desktop/src-tauri` onto this crate. Clipboard and window glue stay in Tauri.

License: Apache-2.0.
