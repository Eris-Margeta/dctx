// F15 — Clear All Cache and Reset to Defaults are different commands.
// Cancelling either confirmation is the refusal; accepting it is the positive control.
// Clear cache must not emit reset_desktop_settings, including when the confirm is dismissed.
import { test, expect } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { installTauriMock } from './tauri-mock.mjs';

const evidence = process.env.DCTX_E2E_RUN_DIR;
const monitors = new WeakMap();
const clearConfirm = 'Are you sure you want to clear all cache? This cannot be undone.';
const resetConfirm = 'Are you sure you want to reset all settings to defaults? This cannot be undone.';

test.beforeEach(async ({ page }, info) => {
  const directory = join(evidence, info.project.name, `settings-${info.testId.replace(/[^a-zA-Z0-9_-]/g, '_')}`);
  mkdirSync(directory, { recursive: true });
  const monitor = { directory, console: [], pageErrors: [], steps: [] };
  monitors.set(page, monitor);
  page.on('console', message => monitor.console.push({ type: message.type(), text: message.text(), location: message.location() }));
  page.on('pageerror', error => monitor.pageErrors.push(error.stack ?? error.message));
  await installTauriMock(page, { persistSettings: true });
});

function saveDiagnostics(monitor) {
  for (const [name, data] of [['console', monitor.console], ['page-errors', monitor.pageErrors], ['steps', monitor.steps]]) {
    writeFileSync(join(monitor.directory, `${name}.json`), JSON.stringify(data, null, 2) + '\n');
  }
}

async function capture(page, name, description) {
  const monitor = monitors.get(page);
  const path = join(monitor.directory, `${name}.png`);
  await page.screenshot({ path, fullPage: true, animations: 'disabled' });
  writeFileSync(join(monitor.directory, `${name}.dom.txt`), await page.locator('body').innerText() + '\n');
  monitor.steps.push({ screenshot: relative(evidence, path), description });
  saveDiagnostics(monitor);
}

test.afterEach(async ({ page }, info) => {
  const monitor = monitors.get(page);
  try {
    if (info.status !== info.expectedStatus && !page.isClosed()) {
      await page.screenshot({ path: join(monitor.directory, 'failure.png'), fullPage: true });
      writeFileSync(join(monitor.directory, 'failure.dom.txt'), await page.locator('body').innerText());
    }
  } finally {
    saveDiagnostics(monitor);
  }
  expect(monitor.pageErrors, 'no browser page errors').toEqual([]);
  expect(monitor.console.filter(entry => entry.type === 'error'), 'no console errors (empty allowlist)').toEqual([]);
});

async function calls(page, cmd) {
  return page.evaluate(command => window.__DCTX_E2E__.calls.filter(call => call.cmd === command), cmd);
}

async function clickDialog(page, locator, accept, expectedCount) {
  const dialogs = [];
  const handler = async dialog => {
    dialogs.push({ type: dialog.type(), message: dialog.message() });
    if (dialog.type() === 'confirm' && !accept) await dialog.dismiss();
    else await dialog.accept();
  };
  page.on('dialog', handler);
  await locator.click();
  await expect.poll(() => dialogs.length).toBe(expectedCount);
  page.off('dialog', handler);
  return dialogs;
}

test('Clear All Cache and Reset to Defaults emit their own commands, including cancel', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#main-screen')).toBeVisible();
  await page.locator('#settings-btn').click();
  await expect(page.locator('#theme-select')).toHaveValue('system');
  await page.locator('#theme-select').selectOption('dark');
  await expect.poll(async () => (await calls(page, 'save_desktop_settings')).at(-1)?.args.config.theme).toBe('dark');

  await page.locator('.settings-nav [data-section="advanced"]').click();
  const clearButton = page.locator('#clear-all-cache-btn');
  const resetButton = page.locator('#reset-settings-btn');
  await expect(clearButton).toHaveText('Clear All Cache');
  await expect(resetButton).toHaveText('Reset to Defaults');
  await clearButton.scrollIntoViewIfNeeded();
  await expect(clearButton).toBeInViewport();
  await capture(page, '01-advanced-actions', 'Advanced settings shows distinct Clear All Cache and Reset to Defaults controls');

  expect(await clickDialog(page, clearButton, false, 1)).toEqual([
    { type: 'confirm', message: clearConfirm },
  ]);
  expect(await calls(page, 'clear_all_cache')).toEqual([]);
  expect(await calls(page, 'reset_desktop_settings')).toEqual([]);
  await expect(page.locator('#theme-select')).toHaveValue('dark');

  expect(await clickDialog(page, clearButton, true, 2)).toEqual([
    { type: 'confirm', message: clearConfirm },
    { type: 'alert', message: 'Cache cleared successfully' },
  ]);
  expect(await calls(page, 'clear_all_cache')).toEqual([{ cmd: 'clear_all_cache', args: {} }]);
  expect(await calls(page, 'reset_desktop_settings')).toEqual([]);
  await page.locator('.settings-nav [data-section="general"]').click();
  await expect(page.locator('#theme-select')).toHaveValue('dark');
  await capture(page, '02-cache-cleared', 'Confirming Clear All Cache leaves the saved dark theme in place');

  await page.locator('.settings-nav [data-section="advanced"]').click();
  await resetButton.scrollIntoViewIfNeeded();
  expect(await clickDialog(page, resetButton, false, 1)).toEqual([
    { type: 'confirm', message: resetConfirm },
  ]);
  expect(await calls(page, 'reset_desktop_settings')).toEqual([]);
  expect(await calls(page, 'clear_all_cache')).toHaveLength(1);
  await expect(page.locator('#theme-select')).toHaveValue('dark');

  expect(await clickDialog(page, resetButton, true, 2)).toEqual([
    { type: 'confirm', message: resetConfirm },
    { type: 'alert', message: 'Settings reset to defaults successfully' },
  ]);
  expect(await calls(page, 'reset_desktop_settings')).toEqual([{ cmd: 'reset_desktop_settings', args: {} }]);
  expect(await calls(page, 'clear_all_cache')).toHaveLength(1);
  await page.locator('.settings-nav [data-section="general"]').click();
  await expect(page.locator('#theme-select')).toHaveValue('system');
  await page.locator('#theme-select').scrollIntoViewIfNeeded();
  await capture(page, '03-settings-reset', 'Confirming Reset to Defaults invokes reset_desktop_settings and restores System theme');
});
