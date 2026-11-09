// Unit coverage for the pure plugin presentation model (PLUGINS.md §7.3 PL-APP).
// Depends on Bun's built-in runner and plugins-model; no DOM or Tauri runtime.
// Kernel-run separately from tsconfig's src-only compilation.

import { test, expect } from 'bun:test';
import { pluginsToViewModels } from '../src/plugins-model';
import type { PluginRow } from '../src/plugins-model';

function row(overrides: Partial<PluginRow> = {}): PluginRow {
  return {
    id: 'dctx/git',
    name: 'Git',
    version: '1.3.0',
    source: 'bundled',
    state: 'enabled',
    enabled: true,
    needs_consent: false,
    capabilities: ['Read file: .git/HEAD', 'Run approved program operation: git status'],
    last_failure: null,
    consent: null,
    ...overrides,
  };
}

test('bundled Git remains enabled and discloses every supplied grant', () => {
  const input = row();
  const [view] = pluginsToViewModels([input]);
  expect(view).toEqual({
    id: 'dctx/git',
    name: 'Git',
    versionLabel: 'Version 1.3.0',
    sourceLabel: 'Bundled',
    stateLabel: 'Enabled',
    badgeClass: 'plugin-badge plugin-state-enabled',
    capabilityLines: input.capabilities,
    lastFailure: null,
    toggleLabel: 'Disable',
    canEnable: false,
    canDisable: true,
    canGrantConsent: false,
    canCheckUpdate: true,
  });
});

test('installed inactive plugin has an honest version label and Enable action', () => {
  const [view] = pluginsToViewModels([row({
    source: 'installed', state: 'disabled', enabled: false, version: null, capabilities: [],
  })]);
  expect(view.sourceLabel).toBe('Installed');
  expect(view.versionLabel).toBe('No active version');
  expect(view.stateLabel).toBe('Disabled');
  expect(view.badgeClass).toBe('plugin-badge plugin-state-disabled');
  expect(view.toggleLabel).toBe('Enable');
  expect(view.canEnable).toBe(true);
  expect(view.canDisable).toBe(false);
  expect(view.canGrantConsent).toBe(false);
  expect(view.capabilityLines).toEqual([]);
});

test('needs-consent permits review and preserves the backend enablement flag', () => {
  const [view] = pluginsToViewModels([row({ state: 'needs-consent', needs_consent: true })]);
  expect(view.stateLabel).toBe('Needs consent');
  expect(view.badgeClass).toBe('plugin-badge plugin-state-needs-consent');
  expect(view.canGrantConsent).toBe(true);
  expect(view.canDisable).toBe(true);
  expect(view.canEnable).toBe(false);
});

test('failure text is preserved verbatim, with actions derived from enablement', () => {
  for (const enabled of [true, false]) {
    const [view] = pluginsToViewModels([row({
      state: 'failed', enabled, last_failure: '<untrusted> integrity failure', needs_consent: true,
    })]);
    expect(view.stateLabel).toBe('Failed');
    expect(view.badgeClass).toBe('plugin-badge plugin-state-failed');
    expect(view.lastFailure).toBe('<untrusted> integrity failure');
    expect(view.canDisable).toBe(enabled);
    expect(view.canEnable).toBe(!enabled);
    expect(view.canGrantConsent).toBe(false);
  }
});

test('consent review is allowed only in a consistent needs-consent state', () => {
  for (const state of ['enabled', 'disabled', 'needs-consent', 'failed'] as const) {
    for (const needsConsent of [true, false]) {
      const [view] = pluginsToViewModels([row({ state, needs_consent: needsConsent })]);
      expect(view.canGrantConsent).toBe(state === 'needs-consent' && needsConsent);
      expect(view.canCheckUpdate).toBe(true);
    }
  }
});

test('sorts bundled first, then case-insensitive name and stable identity', () => {
  const rows = [
    row({ id: 'installed/a', name: 'Alpha', source: 'installed' }),
    row({ id: 'bundled/z', name: 'Zebra' }),
    row({ id: 'bundled/b', name: 'alpha' }),
    row({ id: 'bundled/a', name: 'Alpha' }),
    row({ id: 'installed/z', name: 'Zebra', source: 'installed' }),
  ];
  const expected = ['bundled/a', 'bundled/b', 'bundled/z', 'installed/a', 'installed/z'];
  expect(pluginsToViewModels(rows).map(view => view.id)).toEqual(expected);
  expect(pluginsToViewModels([...rows].reverse()).map(view => view.id)).toEqual(expected);
});

test('mapping neither mutates input order nor aliases capability arrays', () => {
  const first = row({ id: 'z', name: 'Zebra' });
  const second = row({ id: 'a', name: 'Alpha' });
  const rows = [first, second];
  Object.freeze(first.capabilities);
  Object.freeze(first);
  Object.freeze(second.capabilities);
  Object.freeze(second);
  Object.freeze(rows);
  const views = pluginsToViewModels(rows);
  views[0].capabilityLines.push('view-only');
  expect(rows.map(value => value.id)).toEqual(['z', 'a']);
  expect(second.capabilities).not.toContain('view-only');
});

test('an empty registry produces no invented plugin or grants', () => {
  expect(pluginsToViewModels([])).toEqual([]);
});

test('unknown state or source refuses rather than suggesting an enabled plugin', () => {
  for (const input of [
    { ...row(), state: 'unknown' },
    { ...row(), source: 'downloaded' },
    { ...row(), state: 'toString' },
  ]) {
    expect(() => pluginsToViewModels([input as PluginRow])).toThrow('Unsupported plugin state or source');
  }
  expect(pluginsToViewModels([row()])[0].stateLabel).toBe('Enabled');
});

test('presentation preserves the exact consent payload and never treats approval as enablement', () => {
  const approval = {
    id: 'dctx/git', version: '1.3.0', digest: 'a'.repeat(64),
    capabilities: {
      required: [{ kind: 'file.read', path: '.git/HEAD', max_bytes: 4096 }],
      optional: [], outputs: [], network: [],
    },
    capability_lines: ['required: Read file (path: .git/HEAD; max bytes: 4096)'],
    scope: { kind: 'user' as const },
  };
  const input = row({ state: 'needs-consent', needs_consent: true, consent: approval });
  const before = JSON.stringify(input);
  const [view] = pluginsToViewModels([input]);
  expect(view.canGrantConsent).toBe(true);
  expect(view.stateLabel).toBe('Needs consent');
  expect(JSON.stringify(input)).toBe(before);
  expect(input.consent).toBe(approval);
});
