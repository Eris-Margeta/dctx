// Main desktop product flows for DX-E2Es: snapshots, refusal, settings and projects.
// Runs with the existing desktop/narrow Playwright projects and Tauri transport.
// The desktop runner collects numbered screenshots, DOM text and step summaries.
// Snapshot completion is explicit so intermediate status assertions cannot race.
import { test, expect } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { installTauriMock } from './tauri-mock.mjs';

const evidence = process.env.DCTX_E2E_RUN_DIR;
const monitors = new WeakMap();
const projectPath = '/fixtures/dctx-demo';
const snapshot = `Fixture snapshot: ${projectPath}\nREADME.md\nsrc/main.ts`;
const limitError = 'File count limit exceeded: found 10001 files, limit is 10000. Increase the file count limit in Settings or choose a smaller directory.';

test.beforeEach(async ({ page }, info) => {
  const directory = join(evidence, info.project.name, `flows-${info.testId.replace(/[^a-zA-Z0-9_-]/g, '_')}`);
  mkdirSync(directory, { recursive: true });
  const monitor = { directory, console: [], pageErrors: [], steps: [] };
  monitors.set(page, monitor);
  page.on('console', message => monitor.console.push({ type: message.type(), text: message.text(), location: message.location() }));
  page.on('pageerror', error => monitor.pageErrors.push(error.stack ?? error.message));
  await installTauriMock(page, { persistSettings: true, recentProjects: [] });
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

async function boot(page) {
  await page.goto('/');
  await expect(page.locator('#main-screen')).toBeVisible();
  await expect(page.locator('#status')).toHaveText('Ready - Drag & drop a folder to start');
}

async function chooseDirectory(page) {
  await page.locator('#browse-btn').click();
  await expect(page.locator('#path-input')).toHaveValue(projectPath);
  await expect(page.locator('#status')).toHaveText(`Selected: ${projectPath}`);
  expect(await calls(page, 'plugin:dialog|open')).toEqual(expect.arrayContaining([
    expect.objectContaining({ args: expect.objectContaining({ options: expect.objectContaining({ directory: true, multiple: false }) }) }),
  ]));
}

async function startGeneration(page, response = null) {
  await page.evaluate(response => {
    window.__DCTX_E2E__.deferSnapshots = true;
    window.__DCTX_E2E__.snapshotResponse = response;
  }, response);
  await page.locator('#generate-btn').click();
  await expect(page.locator('#status')).toHaveText('Generating snapshot...');
  await expect(page.locator('#output')).toContainText('Scanning directory structure...');
  await expect(page.locator('#generate-btn')).toBeDisabled();
  expect(await calls(page, 'generate_snapshot')).toEqual([
    { cmd: 'generate_snapshot', args: { request: { path: projectPath } } },
  ]);
}

test('generate a snapshot and copy its output', async ({ page }) => {
  await boot(page);
  await expect(page.locator('#copy-btn')).toBeDisabled();
  await capture(page, '01-ready', 'Ready status and disabled clipboard action before selecting a directory');

  await chooseDirectory(page);
  await capture(page, '02-directory-selected', 'Mock directory picker selects dctx-demo and updates the status');

  await startGeneration(page);
  await capture(page, '03-generating', 'Generating status and scanning message while the Generate button is disabled');
  await page.evaluate(() => window.__DCTX_E2E__.completeSnapshot());
  await expect(page.locator('#status')).toHaveText('Snapshot generated successfully');
  await expect(page.locator('#output')).toHaveText(`✅ Success!\n\n${snapshot}`);
  await expect(page.locator('#copy-btn')).toBeEnabled();
  await expect.poll(() => calls(page, 'add_recent_project')).toEqual([
    { cmd: 'add_recent_project', args: { path: projectPath } },
  ]);
  await capture(page, '04-snapshot-output', 'Successful snapshot displays the exact fixture text and enables Copy');

  await page.locator('#copy-btn').click();
  await expect(page.locator('#copy-btn')).toHaveText('✅Copied!');
  await expect(page.locator('#status')).toHaveText('Copied to clipboard');
  expect(await calls(page, 'copy_to_clipboard')).toEqual([
    { cmd: 'copy_to_clipboard', args: { text: snapshot } },
  ]);
  expect(await page.evaluate(() => window.__DCTX_E2E__.clipboard)).toBe(snapshot);
  await capture(page, '05-copied', 'Clipboard mock receives the exact snapshot and the UI confirms Copied');
});

test('file-count refusal displays a readable error', async ({ page }) => {
  await boot(page);
  await chooseDirectory(page);
  await capture(page, '01-limit-directory', 'Directory selected before exercising the file-count limit');
  await startGeneration(page, { success: false, output: null, error: limitError });
  await capture(page, '02-limit-generating', 'Generating status before the mocked file-count refusal');
  await page.evaluate(() => window.__DCTX_E2E__.completeSnapshot());
  const output = page.locator('#output');
  await expect(page.locator('#status')).toHaveText('Error generating snapshot');
  await expect(output).toHaveText(`❌ Error:\n\n${limitError}`);
  await expect(output).toBeVisible();
  await expect(page.locator('#copy-btn')).toBeDisabled();
  await expect(page.locator('#generate-btn')).toBeEnabled();
  await output.scrollIntoViewIfNeeded();
  await expect(output).toBeInViewport();
  await capture(page, '03-file-count-error', 'Full file-count error and recovery guidance are visible; Copy is disabled');
  const layout = await output.evaluate(node => {
    const rect = node.getBoundingClientRect();
    return { left: rect.left, right: rect.right, viewport: innerWidth,
      width: node.clientWidth, contentWidth: node.scrollWidth,
      height: node.clientHeight, contentHeight: node.scrollHeight };
  });
  expect(layout.left, 'error stays inside the viewport').toBeGreaterThanOrEqual(0);
  expect(layout.right, 'error stays inside the viewport').toBeLessThanOrEqual(layout.viewport);
  expect(layout.contentWidth, 'error wraps without horizontal clipping').toBeLessThanOrEqual(layout.width + 1);
  expect(layout.contentHeight, 'entire error fits without hidden lines').toBeLessThanOrEqual(layout.height + 1);
});

test('settings autosave survives reload and resets to defaults', async ({ page }) => {
  await boot(page);
  await page.locator('#settings-btn').click();
  await expect(page.locator('.settings-header h2')).toHaveText('Settings');
  await expect(page.locator('#theme-select')).toHaveValue('system');
  await page.locator('#theme-select').selectOption('dark');
  await expect(page.locator('#theme-select')).toHaveValue('dark');
  await expect.poll(async () => (await calls(page, 'save_desktop_settings')).at(-1)?.args.config.theme).toBe('dark');
  await capture(page, '01-theme-saved', 'General Settings theme changed to Dark and saved through the mock');

  await page.locator('.settings-nav [data-section="filters"]').click();
  const filter = page.locator('#filter-extensions-input');
  await filter.fill('ts, md');
  await filter.blur();
  await expect(filter).toHaveValue('ts, md');
  await expect(page.locator('[data-section-id="filters"] .section-title')).toHaveText('Filter Settings');
  await expect.poll(async () => (await calls(page, 'save_desktop_settings')).at(-1)?.args.config.filter_extensions).toEqual(['ts', 'md']);
  await capture(page, '02-filter-saved', 'Filter Settings extensions ts and md saved on blur');

  await page.reload();
  await expect(page.locator('#status')).toHaveText('Ready - Drag & drop a folder to start');
  await expect.poll(() => calls(page, 'get_desktop_settings')).not.toEqual([]);
  await page.locator('#settings-btn').click();
  await expect(page.locator('.settings-header h2')).toHaveText('Settings');
  await expect(page.locator('#theme-select')).toHaveValue('dark');
  await capture(page, '03-theme-reloaded', 'Dark theme value restored from mock storage after page reload');
  await page.locator('.settings-nav [data-section="filters"]').click();
  await expect(filter).toHaveValue('ts, md');
  await filter.scrollIntoViewIfNeeded();
  await expect(filter).toBeInViewport();
  await capture(page, '04-filter-reloaded', 'Saved ts and md extensions restored after page reload');

  const dialogs = [];
  page.on('dialog', async dialog => {
    dialogs.push({ type: dialog.type(), message: dialog.message() });
    await dialog.accept();
  });
  await page.locator('.settings-nav [data-section="advanced"]').click();
  const resetButton = page.getByRole('button', { name: 'Reset to Defaults', exact: true });
  await resetButton.scrollIntoViewIfNeeded();
  await expect(resetButton).toBeInViewport();
  await capture(page, '05-reset-control', 'Advanced Settings Reset to Defaults action before requesting the reset');
  await resetButton.click();
  // Keep this tied to the labelled product action: the mock must not compensate
  // for a missing UI handler or silently reset settings on another command.
  await expect.poll(() => calls(page, 'reset_desktop_settings'), {
    message: 'Reset to Defaults must invoke reset_desktop_settings',
  }).toHaveLength(1);
  await expect.poll(() => dialogs).toEqual([
    { type: 'confirm', message: 'Are you sure you want to reset all settings to defaults? This cannot be undone.' },
    { type: 'alert', message: 'Settings reset to defaults successfully' },
  ]);
  expect(await calls(page, 'reset_desktop_settings')).toHaveLength(1);
  await page.locator('.settings-nav [data-section="general"]').click();
  await expect(page.locator('#theme-select')).toHaveValue('system');
  await page.locator('#theme-select').scrollIntoViewIfNeeded();
  await capture(page, '06-theme-reset', 'Reset confirmation accepted and theme restored to System');
  await page.locator('.settings-nav [data-section="filters"]').click();
  await expect(filter).toHaveValue('');
  await filter.scrollIntoViewIfNeeded();
  await capture(page, '07-filter-reset', 'Reset restores the empty extension filter');
  await page.reload();
  await expect(page.locator('#status')).toHaveText('Ready - Drag & drop a folder to start');
  await page.locator('#settings-btn').click();
  await expect(page.locator('#theme-select')).toHaveValue('system');
  await expect(filter).toHaveValue('');
  await expect(page.locator('.settings-header h2')).toHaveText('Settings');
  await capture(page, '08-defaults-reloaded', 'Default settings remain restored after a second reload');
});

test('recent projects add, open, remove and return to empty', async ({ page }) => {
  await boot(page);
  await page.locator('#projects-nav-btn').click();
  const list = page.locator('#projects-list');
  await expect(list.locator('.empty-text')).toHaveText('No recent projects');
  await expect(list.locator('.empty-hint')).toHaveText('Generate a snapshot to add a project here');
  await capture(page, '01-projects-empty', 'Recent projects starts empty with guidance for adding a project');

  await page.locator('#add-project-btn').click();
  const card = list.locator('.project-card');
  await expect(card.locator('.project-name')).toHaveText('dctx-demo');
  await expect(card.locator('.project-path')).toHaveText(projectPath);
  expect(await calls(page, 'add_recent_project')).toEqual([
    { cmd: 'add_recent_project', args: { path: projectPath } },
  ]);
  await capture(page, '02-project-added', 'Add New Project uses the mock picker and displays dctx-demo');

  await card.locator('.btn-open').click();
  await expect(page.locator('#main-screen')).toBeVisible();
  await expect(page.locator('#path-input')).toHaveValue(projectPath);
  await expect(page.locator('#status')).toHaveText(`Opened: ${projectPath}`);
  await capture(page, '03-project-opened', 'Open returns to snapshot generation with the recent project path filled in');

  await page.locator('#projects-nav-btn').click();
  await expect(card.locator('.project-name')).toHaveText('dctx-demo');
  const confirmation = page.waitForEvent('dialog');
  const removal = card.locator('.btn-remove').click();
  const dialog = await confirmation;
  expect(dialog.type()).toBe('confirm');
  expect(dialog.message()).toBe(`Remove "${projectPath}" from recent projects?`);
  await dialog.accept();
  await removal;
  await expect(list.locator('.project-card')).toHaveCount(0);
  await expect(list.locator('.empty-text')).toHaveText('No recent projects');
  await expect(list.locator('.empty-hint')).toBeVisible();
  expect(await calls(page, 'remove_recent_project')).toEqual([
    { cmd: 'remove_recent_project', args: { path: projectPath } },
  ]);
  await capture(page, '04-project-removed', 'Confirmed removal calls the mock and restores the recent-projects empty state');
});
