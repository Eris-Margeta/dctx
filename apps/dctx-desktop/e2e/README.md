<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Desktop browser evidence

DX-E2E adapts the HQ e2e skill to the desktop's Vite frontend and mocked Tauri
transport. Requires Bun, Node and the already installed Chromium **1248** browser
and headless shell in `~/Library/Caches/ms-playwright`. Never download browsers.

From `apps/dctx-desktop`:

```sh
bun install
bun run e2e
```

This uses `node e2e/runner.mjs`. Playwright is pinned to 1.64.0.
The runner uses only local dependencies and never invokes Git (decision 078).
It starts Vite at `http://127.0.0.1:1420`, refuses an existing server and closes
Vite at the end. Set `E2E_PORT` to use another port for concurrent worktrees,
for example `E2E_PORT=1421 bun run e2e`. The server, readiness probe and browser
all use `http://127.0.0.1:<E2E_PORT>` (default port 1420); startup allows 90 seconds.
No Rust build or Tauri desktop process is needed.

Each ignored `e2e-runs/<ISO timestamp>/` contains:

- `manifest.json`: time, command/arguments, seed, viewports, and SHA-256 source
  digest plus individual hashes of all e2e files, `index.html`, `src/**/*.ts`,
  `package.json`, `bun.lock`, Vite configuration and TypeScript configuration.
- `results.json`: Playwright results, including startup failures where possible.
- `summary.md`: outcome and links to screenshots with their asserted steps.
- Per-test folders under `desktop/` and `narrow/`: numbered screenshots,
  corresponding `.dom.txt` files, `console.json`, `page-errors.json`, `steps.json`.
- `test-results/`: Playwright failure screenshots and traces.

The two projects run at 1440×900 and 390×844, one worker, no retries. Each checks
boot, fixture recent projects, and every existing Settings navigation entry.
Seven existing sections are required. Plugins is visited if present; its absence
does not fail this gate. Plugin interaction coverage belongs to PL-APPb2.
There is no console-error allowlist: every console error and page error fails.
Review screenshots and DOM text labelled with the source digest before accepting
a UI packet. Compare run directories manually; this is not a pixel-diff gate.

## Adding a spec

Add an `e2e/*.spec.mjs` using `test`/`expect` from `@playwright/test`.
Call `await installTauriMock(page)` before `page.goto('/')`, register console and
page-error listeners before navigation, and assert visible text before capturing.
Use unique per-test folders inside `process.env.DCTX_E2E_RUN_DIR`, numbered
screenshots (`01-boot.png`), matching DOM dumps, diagnostics and `steps.json`
entries `{screenshot, description}` (screenshot paths relative to the run folder).
The runner collects these recursively. The example spec saves diagnostics even
on failure; reuse that pattern and keep fixtures isolated per browser context.

```sh
bun run e2e -- --project=desktop
bun run e2e -- desktop.spec.mjs
```

The runner refuses zero spec files, and a run with zero passing tests fails.

## Mock contract

`tauri-mock.mjs` exports `installTauriMock(page)`, which uses `addInitScript` to
set `window.__TAURI__` (desktop-mode detection) and `__TAURI_INTERNALS__` with
async `invoke(cmd,args)`, callback registration/execution/removal, and the event
plugin listener cleanup hook. State resets at each navigation/context.
`window.__DCTX_E2E__` exposes command calls, mock clipboard and plugin rows for
additional assertions; it exists only in these browser tests.

| Command | Arguments and result |
| --- | --- |
| `get_desktop_settings` | Complete `DesktopConfig`, matching settings-manager defaults |
| `save_desktop_settings` | `{config}` replaces in-memory config; returns null |
| `reset_desktop_settings` | Restores defaults; returns null; subsequent get reads defaults |
| `get_recent_projects` | Seeded `dctx-demo` project with path, name, ISO last_accessed, access_count |
| `add_recent_project` | `{path}` moves project to front, increments access_count |
| `remove_recent_project` | `{path}` removes matching row |
| `generate_snapshot` | `{request:{path}}` returns `{success:true,output,error:null}` |
| `copy_to_clipboard` | `{text}` stores text in mock clipboard, returns null |
| `list_plugins` | `PluginRow[]` with bundled enabled `dctx/git` and installed `example/review` needing consent |
| `enable_plugin` | `{id}` sets enabled; preserves needs-consent until approval |
| `disable_plugin` | `{id}` sets disabled; returns updated row |
| `consent_plugin` | `{id,approval:{digest,capabilities,scope:{kind}}}` clears needs-consent; returns row |
| `check_plugin_update` | `{id}` returns `{id,version:null,containing_release}` |
| `plugin:event\|listen` / `plugin:event\|unlisten` | Register/remove synthetic listener IDs for startup file-drop subscriptions |
| `plugin:dialog\|open` | Returns `/fixtures/dctx-demo` without opening a native picker |

Plugin rows match the serialized desktop API (`id`, `name`, `version`, `source`,
`state`, `enabled`, `needs_consent`, `capabilities`, `last_failure`). Git grants
describe bounded project `.gitignore` reads/writes and the approved
`git.add-ignore` operation with `git-admin:stage-ignore`. No Git is executed.
Update discovery is deterministic and offline. Consent checks payload presence;
it does not reproduce native signature, digest, scope or capability validation.
Unknown commands and plugin IDs reject with explicit fixture errors; add a
deliberate mock when a new frontend command is introduced.

This gate proves browser rendering/navigation against fixtures. It does not prove
native filesystem, clipboard, plugin consent enforcement or plugin execution.
