// Desktop browser evidence for DX-E2E and the e2e skill.
// Depends on Playwright and the mock transport; config runs both required viewports.
// Captures asserted UI steps plus diagnostics even when an assertion fails.
import { test, expect } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { installTauriMock } from './tauri-mock.mjs';

const monitors = new WeakMap();
const evidence = process.env.DCTX_E2E_RUN_DIR;
const sectionTitles = {
  general: 'General Settings', filters: 'Filter Settings', output: 'Output Settings',
  templates: 'Templates', ignore: 'Ignore Rules', advanced: 'Advanced Settings', about: 'About',
};

test.beforeEach(async ({ page }, info) => {
  const directory = join(evidence, info.project.name, `desktop-${info.testId.replace(/[^a-zA-Z0-9_-]/g, '_')}`);
  mkdirSync(directory, { recursive: true });
  const monitor = { directory, console: [], pageErrors: [], steps: [] };
  monitors.set(page, monitor);
  page.on('console', message => monitor.console.push({ type: message.type(), text: message.text(), location: message.location() }));
  page.on('pageerror', error => monitor.pageErrors.push(error.stack ?? error.message));
  await installTauriMock(page);
});

function saveDiagnostics(monitor) {
  writeFileSync(join(monitor.directory, 'console.json'), JSON.stringify(monitor.console, null, 2) + '\n');
  writeFileSync(join(monitor.directory, 'page-errors.json'), JSON.stringify(monitor.pageErrors, null, 2) + '\n');
  writeFileSync(join(monitor.directory, 'steps.json'), JSON.stringify(monitor.steps, null, 2) + '\n');
}

async function capture(page, name, description) {
  const monitor = monitors.get(page);
  await page.screenshot({ path: join(monitor.directory, `${name}.png`), fullPage: true, animations: 'disabled' });
  writeFileSync(join(monitor.directory, `${name}.dom.txt`), await page.locator('body').innerText() + '\n');
  monitor.steps.push({ screenshot: relative(evidence, join(monitor.directory, `${name}.png`)), description });
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
  expect(monitor.console.filter(entry => entry.type === 'error'), 'no unexpected console errors (allowlist is empty)').toEqual([]);
});

test('boot, recent projects, and every existing Settings section', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('h1')).toContainText('dctx Desktop');
  await expect(page.locator('#main-screen')).toBeVisible();
  await expect(page.locator('#main-screen')).toContainText('Generate Context Snapshot');
  await expect(page.locator('#status')).toHaveText('Ready - Drag & drop a folder to start');
  await capture(page, '01-boot', 'Boot screen with snapshot action and ready status');

  await page.locator('#projects-nav-btn').click();
  await expect(page.locator('#projects-screen')).toBeVisible();
  await expect(page.locator('#projects-list')).toContainText('dctx-demo');
  await expect(page.locator('#projects-list')).toContainText('/fixtures/dctx-demo');
  await expect(page.locator('#projects-list .btn-open')).toBeVisible();
  await capture(page, '02-recent-projects', 'Recent project fixture and Open action');

  await page.locator('#settings-btn').click();
  await expect(page.locator('#settings-screen')).toBeVisible();
  await expect(page.locator('.settings-header h2')).toHaveText('Settings');
  await expect(page.locator('#theme-select')).toHaveValue('system');
  await capture(page, '03-settings', 'Settings opened with loaded theme');

  const items = page.locator('.settings-nav [data-section]');
  const sections = await items.evaluateAll(nodes => nodes.map(node => ({
    id: node.getAttribute('data-section'), label: node.textContent.trim(),
  })));
  expect(sections.map(section => section.id)).toEqual(expect.arrayContaining(Object.keys(sectionTitles)));
  let step = 4;
  for (const { id, label } of sections) {
    await page.locator(`.settings-nav [data-section="${id}"]`).click();
    const section = page.locator(`.settings-section[data-section-id="${id}"]`);
    const heading = section.locator('.section-title');
    await expect(heading).toHaveText(sectionTitles[id] ?? label);
    await heading.scrollIntoViewIfNeeded();
    await expect(heading).toBeInViewport();
    await capture(page, `${String(step++).padStart(2, '0')}-settings-${id}`, `${label} section with visible heading`);
  }
});
