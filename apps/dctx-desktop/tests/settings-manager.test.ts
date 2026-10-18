// F16 — reset must not stack listeners, and a late older save must not restore privacy settings.
// A read spy records every path the fake store returns or accepts and fails closed on credential paths.
// Positive control: the newest save (reset defaults plus the new theme) stays current after the older one completes.
import { expect, mock, test } from 'bun:test';

const FORBIDDEN_PATHS = ['/Users/private/.ssh', 'id_rsa', 'credentials.json'];
const readPaths: string[] = [];

function recordPath(path: string): void {
  for (const forbidden of FORBIDDEN_PATHS) {
    if (path.includes(forbidden)) throw new Error(`forbidden path read: ${forbidden}`);
  }
  readPaths.push(path);
}

function collectPaths(value: unknown): void {
  if (typeof value === 'string' && value.includes('/')) recordPath(value);
  else if (Array.isArray(value)) value.forEach(item => collectPaths(item));
  else if (value && typeof value === 'object') {
    Object.values(value as Record<string, unknown>).forEach(item => collectPaths(item));
  }
}

const defaults = {
  last_directory: null as string | null,
  window_width: 1200,
  window_height: 800,
  file_count_limit: 10000,
  theme: 'system',
  auto_copy_to_clipboard: false,
  filter_extensions: null as string[] | null,
  include_binary: false,
  only_hidden: false,
  min_size_mb: null as number | null,
  max_size_mb: null as number | null,
  only_empty: false,
  only_non_empty: false,
  tree_only: false,
};

const initial = {
  ...defaults,
  auto_copy_to_clipboard: true,
  last_directory: '/Users/private/secret-project',
};

let phase: 'initial' | 'defaults' = 'initial';
const commands: string[] = [];
const saves: Array<{ config: typeof defaults; resolve: () => void }> = [];

function copySettings(source: typeof defaults): typeof defaults {
  return { ...source, filter_extensions: source.filter_extensions ? [...source.filter_extensions] : null };
}

mock.module('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: { config?: typeof defaults }) => {
    commands.push(cmd);
    collectPaths(args);
    if (cmd === 'get_desktop_settings') {
      const settings = copySettings(phase === 'defaults' ? defaults : initial);
      collectPaths(settings);
      return Promise.resolve(settings);
    }
    if (cmd === 'reset_desktop_settings') {
      phase = 'defaults';
      return Promise.resolve(null);
    }
    if (cmd === 'save_desktop_settings') {
      const config = copySettings(args?.config ?? defaults);
      collectPaths(config);
      return new Promise<void>(resolve => { saves.push({ config, resolve }); });
    }
    if (cmd === 'clear_all_cache') return Promise.resolve(null);
    throw new Error(`unexpected command ${cmd}`);
  },
}));

type Listener = () => unknown;

function control(id: string, extra: Record<string, unknown> = {}) {
  const listeners: Record<string, Listener[]> = {};
  return {
    id,
    value: '',
    checked: false,
    dataset: {} as Record<string, string>,
    ...extra,
    addEventListener(type: string, fn: Listener) {
      (listeners[type] ??= []).push(fn);
    },
    listenerCount(type: string) {
      return listeners[type]?.length ?? 0;
    },
    dispatch(type: string) {
      return Promise.all((listeners[type] ?? []).map(fn => fn()));
    },
  };
}

const theme = control('theme-select');
const fileCount = control('file-count-input');
const autoCopy = control('auto-copy-toggle');
const filter = control('filter-extensions-input');
const includeBinary = control('include-binary-toggle');
const onlyHidden = control('only-hidden-toggle');
const minSize = control('min-size-input');
const maxSize = control('max-size-input');
const treeOnly = control('tree-only-toggle');
const resetButton = control('reset-settings-btn');
const clearButton = control('clear-all-cache-btn');
const radios = ['all', 'empty', 'non-empty'].map(value => control('', { name: 'content-filter', value }));
const byId = new Map([theme, fileCount, autoCopy, filter, includeBinary, onlyHidden, minSize, maxSize, treeOnly, resetButton, clearButton].map(node => [node.id, node]));

globalThis.document = {
  querySelector(selector: string) {
    return selector.startsWith('#') ? byId.get(selector.slice(1)) ?? null : null;
  },
  querySelectorAll(selector: string) {
    return selector === 'input[name="content-filter"]' ? radios : [];
  },
} as unknown as Document;

globalThis.confirm = () => true;
globalThis.alert = () => undefined;

const manager = await import('../src/settings-manager.ts');

test('read spy refuses credential paths before any settings IPC', () => {
  expect(() => recordPath('/Users/private/.ssh/id_rsa')).toThrow('forbidden path read: /Users/private/.ssh');
});

test('initialize, change a filter, reset, then change theme: one post-reset save, stale completion ignored', async () => {
  await manager.initSettingsUI();
  await manager.initSettingsUI();
  expect(theme.listenerCount('change')).toBe(1);
  expect(filter.listenerCount('blur')).toBe(1);
  expect(resetButton.listenerCount('click')).toBe(1);
  expect(clearButton.listenerCount('click')).toBe(1);

  filter.value = 'rs';
  const filterSaved = filter.dispatch('blur');
  expect(saves).toHaveLength(1);
  expect(saves[0].config.filter_extensions).toEqual(['rs']);
  expect(saves[0].config.auto_copy_to_clipboard).toBe(true);
  expect(saves[0].config.last_directory).toBe('/Users/private/secret-project');

  const resetDone = resetButton.dispatch('click');
  await resetDone;
  expect(commands.filter(cmd => cmd === 'reset_desktop_settings')).toEqual(['reset_desktop_settings']);
  expect(theme.listenerCount('change')).toBe(1);
  expect(resetButton.listenerCount('click')).toBe(1);
  expect(theme.value).toBe('system');
  expect(filter.value).toBe('');
  expect(autoCopy.checked).toBe(false);

  theme.value = 'dark';
  const themeSaved = theme.dispatch('change');
  expect(saves).toHaveLength(2);
  expect(saves[1].config).toEqual({ ...defaults, theme: 'dark' });

  // Reverse completion: the newer theme save finishes before the pre-reset filter save.
  saves[1].resolve();
  saves[0].resolve();
  await filterSaved;
  await themeSaved;

  expect(manager.getCurrentSettings()).toEqual({ ...defaults, theme: 'dark' });
  expect(manager.getCurrentSettings()?.auto_copy_to_clipboard).toBe(false);
  expect(manager.getCurrentSettings()?.last_directory).toBeNull();
  expect(readPaths).toContain('/Users/private/secret-project');
  for (const path of readPaths) {
    for (const forbidden of FORBIDDEN_PATHS) expect(path.includes(forbidden)).toBe(false);
  }
  const allowed = new Set(['get_desktop_settings', 'save_desktop_settings', 'reset_desktop_settings', 'clear_all_cache']);
  for (const cmd of commands) expect(allowed.has(cmd)).toBe(true);
});
