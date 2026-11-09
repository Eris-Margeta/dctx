// Deterministic browser transport for DX-E2E; install before navigating to the app.
// Mirrors desktop settings/recent-project commands and PL-APP plugin row transport.
// Used by Playwright specs through addInitScript; no native commands are executed.
import { readFileSync } from 'node:fs';

const bundledVersion = JSON.parse(readFileSync(
  new URL('../../../plugins/dctx-plugin-git/plugin.json', import.meta.url), 'utf8',
)).version;

export async function installTauriMock(page, fixtures = {}) {
  await page.addInitScript(fixtures => {
    const clone = value => structuredClone(value);
    const defaults = {
      last_directory: null, window_width: 1200, window_height: 800,
      file_count_limit: 10000, theme: 'system', auto_copy_to_clipboard: false,
      filter_extensions: null, include_binary: false, only_hidden: false,
      min_size_mb: null, max_size_mb: null, only_empty: false,
      only_non_empty: false, tree_only: false,
    };
    // Opt-in session storage models native persistence across reloads, isolated per tab/context.
    const settingsKey = 'dctx-e2e-desktop-settings';
    const options = fixtures ?? {};
    let closedSaveSequence = 0;
    const fullSettings = value => {
      const merged = { ...clone(defaults), ...(value && typeof value === 'object' ? value : {}) };
      if (!Array.isArray(merged.filter_extensions)) merged.filter_extensions = null;
      return merged;
    };
    let settings = clone(defaults);
    if (options.persistSettings) {
      try {
        const storedSettings = sessionStorage.getItem(settingsKey);
        if (storedSettings) settings = fullSettings(JSON.parse(storedSettings));
      } catch {
        settings = clone(defaults);
      }
    }
    const persistSettings = () => {
      if (options.persistSettings) sessionStorage.setItem(settingsKey, JSON.stringify(settings));
    };
    let recent = clone(options.recentProjects ?? [{ path: '/fixtures/dctx-demo', name: 'dctx-demo',
      last_accessed: '2026-10-17T08:00:00.000Z', access_count: 3 }]);
    const plugins = [{
      id: 'dctx/git', name: 'Git', version: options.bundledVersion, source: 'bundled',
      state: 'enabled', enabled: true, needs_consent: false,
      capabilities: [
        'required: Read file (path: .gitignore; scope: project; max bytes: 1048576)',
        'required: Write file (path: .gitignore; scope: project; create: false; max growth bytes: 4096; max bytes: 1052672)',
        'optional: Run approved program operation (program: git; operation: git.add-ignore; resources: ["git-admin:stage-ignore"]; max output bytes: 65536)',
      ], last_failure: null,
    }, {
      id: 'example/review', name: 'Review Example', version: '1.0.0', source: 'installed',
      state: 'needs-consent', enabled: true, needs_consent: true,
      capabilities: ['required: Read file (path: README.md; scope: project; max bytes: 4096)'],
      last_failure: null,
      consent: {
        id: 'example/review', version: '1.0.0', digest: 'a'.repeat(64),
        capabilities: {
          required: [{ kind: 'file.read', path: 'README.md', scope: 'project', max_bytes: 4096 }],
          optional: [], outputs: [], network: [],
        },
        capability_lines: ['required: Read file (path: README.md; scope: project; max bytes: 4096)'],
        scope: { kind: 'user' },
      },
    }, {
      id: 'example/failed', name: 'Failed Example', version: '1.0.0', source: 'installed',
      state: 'failed', enabled: true, needs_consent: false,
      capabilities: [], consent: null,
      last_failure: 'Plugin process exited before initialization (exit code 1).',
    }, {
      id: 'example/inactive', name: 'Inactive Example', version: '1.0.0', source: 'installed',
      state: 'disabled', enabled: false, needs_consent: true,
      capabilities: ['required: Read file (path: README.md; scope: project; max bytes: 4096)'],
      last_failure: null,
      consent: {
        id: 'example/inactive', version: '1.0.0', digest: 'b'.repeat(64),
        capabilities: {
          required: [{ kind: 'file.read', path: 'README.md', scope: 'project', max_bytes: 4096 }],
          optional: [], outputs: [], network: [],
        },
        capability_lines: ['required: Read file (path: README.md; scope: project; max bytes: 4096)'],
        scope: { kind: 'user' },
      },
    }, {
      id: 'example/consented', name: 'Consented Example', version: '1.0.0', source: 'installed',
      state: 'disabled', enabled: false, needs_consent: false,
      capabilities: ['required: Read file (path: README.md; scope: project; max bytes: 4096)'],
      last_failure: null, consent: null,
    }];
    const callbacks = new Map();
    const listeners = new Map();
    let nextId = 1;
    const state = {
      calls: [], clipboard: '', plugins,
      deferSnapshots: false, snapshotResponse: null, completeSnapshot: null,
    };
    window.__DCTX_E2E__ = state;
    window.__TAURI__ = {};
    window.__TAURI_INTERNALS__ = {
      transformCallback(callback, once = false) {
        const id = nextId++;
        const wrapped = (...args) => {
          if (once) {
            callbacks.delete(id);
            delete window[`_${id}`];
          }
          return callback(...args);
        };
        callbacks.set(id, wrapped);
        window[`_${id}`] = wrapped;
        return id;
      },
      unregisterCallback(id) { callbacks.delete(id); delete window[`_${id}`]; },
      runCallback(id, data) {
        const callback = callbacks.get(id);
        if (!callback) throw new Error(`Tauri mock: unknown callback ${id}`);
        return callback(data);
      },
      async invoke(cmd, args = {}) {
        state.calls.push({ cmd, args: clone(args) });
        const plugin = () => {
          const row = plugins.find(row => row.id === args.id);
          if (!row) throw new Error(`Tauri mock: unknown plugin ${args.id}`);
          return row;
        };
        switch (cmd) {
          case 'get_desktop_settings': return clone(fullSettings(settings));
          case 'save_desktop_settings': {
            // Ignore a save issued before reset. The page publishes that sequence before invoke.
            const seq = window.__dctxPendingSeq ?? closedSaveSequence + 1;
            if (seq < closedSaveSequence) return null;
            closedSaveSequence = seq;
            settings = clone(fullSettings(args.config));
            persistSettings();
            return null;
          }
          case 'reset_desktop_settings': {
            closedSaveSequence = window.__dctxClosedSeq ?? closedSaveSequence;
            settings = clone(defaults);
            persistSettings();
            return null;
          }
          case 'get_recent_projects': return clone(recent);
          case 'add_recent_project': {
            const previous = recent.find(project => project.path === args.path);
            recent = recent.filter(project => project.path !== args.path);
            recent.unshift({ path: args.path, name: args.path.split('/').filter(Boolean).at(-1),
              last_accessed: '2026-10-17T09:00:00.000Z', access_count: (previous?.access_count ?? 0) + 1 });
            return null;
          }
          case 'remove_recent_project': recent = recent.filter(project => project.path !== args.path); return null;
          case 'generate_snapshot': {
            const response = clone(state.snapshotResponse ?? {
              success: true, output: `Fixture snapshot: ${args.request.path}\nREADME.md\nsrc/main.ts`, error: null,
            });
            if (!state.deferSnapshots) return response;
            return new Promise(resolve => {
              state.completeSnapshot = () => { state.completeSnapshot = null; resolve(response); };
            });
          }
          case 'copy_to_clipboard': state.clipboard = args.text; return null;
          case 'clear_all_cache': return null;
          case 'list_plugins': return clone(plugins);
          case 'enable_plugin': {
            const row = plugin();
            // PluginOps::enable returns needs-consent without writing enablement.
            if (row.needs_consent) return clone({ ...row, state: 'needs-consent' });
            row.enabled = true;
            row.state = 'enabled';
            return clone(row);
          }
          case 'disable_plugin': {
            const row = plugin(); row.enabled = false; row.state = 'disabled'; return clone(row);
          }
          case 'consent_plugin': {
            const row = plugin();
            // The command accepts only the consent object disclosed by list_plugins.
            if (JSON.stringify(args.approval ?? null) !== JSON.stringify(row.consent)) {
              throw 'Plugin needs-consent: explicit approval required';
            }
            row.needs_consent = false;
            row.state = row.enabled ? 'enabled' : 'disabled';
            return clone(row);
          }
          case 'check_plugin_update': {
            const row = plugin();
            // No verified index: independent checks are Network(Unreachable). Bundled checks stay local.
            if (row.source !== 'bundled') throw 'Plugin operation failed: network failure: Unreachable';
            return { id: row.id, version: null, containing_release: true };
          }
          case 'plugin:event|listen': {
            const id = nextId++; listeners.set(id, args); return id;
          }
          case 'plugin:event|unlisten': listeners.delete(args.eventId); return null;
          case 'plugin:dialog|open': return options.dialogPath ?? '/fixtures/dctx-demo';
          default: throw new Error(`Tauri mock: unimplemented command "${cmd}"; add an explicit fixture in e2e/tauri-mock.mjs`);
        }
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener(_event, id) { listeners.delete(id); },
    };
  }, { ...fixtures, bundledVersion });
}
