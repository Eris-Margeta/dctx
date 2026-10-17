// Settings > Plugins browser evidence for DX-E2Ep and PLUGINS.md §7.3.
// Uses Playwright's two configured viewports and the deterministic Tauri mock.
// The e2e runner collects numbered PNGs, DOM text, diagnostics and step descriptions.
import { test, expect } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { installTauriMock } from './tauri-mock.mjs';

const evidence = process.env.DCTX_E2E_RUN_DIR;
const monitors = new WeakMap();
const gitCapabilities = [
  'required: Read file (path: .gitignore; scope: project; max bytes: 1048576)',
  'required: Write file (path: .gitignore; scope: project; create: false; max growth bytes: 4096; max bytes: 1052672)',
  'optional: Run approved program operation (program: git; operation: git.add-ignore; resources: ["git-admin:stage-ignore"]; max output bytes: 65536)',
];
const reviewCapabilities = ['required: Read file (path: README.md; scope: project; max bytes: 4096)'];
const approval = {
  id: 'example/review', version: '1.0.0', digest: 'a'.repeat(64),
  capabilities: {
    required: [{ kind: 'file.read', path: 'README.md', scope: 'project', max_bytes: 4096 }],
    optional: [], outputs: [], network: [],
  },
  capability_lines: reviewCapabilities,
  scope: { kind: 'user' },
};

test.beforeEach(async ({ page }, info) => {
  const directory = join(evidence, info.project.name, `plugins-${info.testId.replace(/[^a-zA-Z0-9_-]/g, '_')}`);
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
      const layout = await page.locator('[data-section-id="plugins"] .plugin-capabilities li').evaluateAll(items =>
        items.map(item => {
          const ancestors = [];
          for (let node = item; node; node = node.parentElement) {
            const style = getComputedStyle(node);
            ancestors.push({
              tag: node.tagName, id: node.id, className: node.className,
              rect: node.getBoundingClientRect().toJSON(),
              clientWidth: node.clientWidth, clientHeight: node.clientHeight,
              scrollWidth: node.scrollWidth, scrollHeight: node.scrollHeight,
              display: style.display, visibility: style.visibility,
              overflowX: style.overflowX, overflowY: style.overflowY,
              width: style.width, minWidth: style.minWidth, flex: style.flex,
            });
          }
          return { text: item.textContent, ancestors };
        }));
      const layoutJson = JSON.stringify(layout, null, 2) + '\n';
      writeFileSync(join(monitor.directory, 'failure-layout.json'), layoutJson);
      await info.attach('plugin-capability-layout', { body: layoutJson, contentType: 'application/json' });
      await page.screenshot({ path: join(monitor.directory, 'failure.png'), fullPage: true });
      writeFileSync(join(monitor.directory, 'failure.dom.txt'), await page.locator('body').innerText());
    }
  } finally {
    saveDiagnostics(monitor);
  }
  expect(monitor.pageErrors, 'no browser page errors').toEqual([]);
  expect(monitor.console.filter(entry => entry.type === 'error'), 'no console errors (empty allowlist)').toEqual([]);
});

test('Plugins capabilities, explicit consent, disable, failure, and offline updates', async ({ page }) => {
  await page.goto('/');
  await page.locator('#settings-btn').click();
  await expect(page.locator('#settings-screen')).toBeVisible();
  await page.locator('.settings-nav [data-section="plugins"]').click();
  const section = page.locator('[data-section-id="plugins"]');
  await expect(section.locator('.section-title')).toHaveText('Plugins');
  await expect(section).toHaveAttribute('aria-busy', 'false');
  const card = name => section.locator('article.plugin-card').filter({ has: page.getByRole('heading', { name, exact: true }) });
  const git = card('Git');
  const review = card('Review Example');
  const failed = card('Failed Example');
  const status = section.locator(':scope > [role="status"]');
  const calls = cmd => page.evaluate(command => window.__DCTX_E2E__.calls.filter(call => call.cmd === command), cmd);

  await expect(git.locator('.plugin-heading')).toContainText('Git');
  await expect(git.locator('.plugin-heading > .plugin-badge').first()).toHaveText('Bundled');
  await expect(git.locator('.plugin-state-enabled')).toHaveText('Enabled');
  await expect(git.locator('.plugin-capabilities li')).toHaveText(gitCapabilities);
  for (const line of gitCapabilities) await expect(git.getByText(line, { exact: true })).toBeVisible();
  await expect(git.getByRole('button', { name: 'Disable', exact: true })).toBeEnabled();
  await expect(git.getByRole('button', { name: 'Grant consent', exact: true })).toHaveCount(0);
  await git.locator('.plugin-capabilities').scrollIntoViewIfNeeded();
  await capture(page, '01-plugins-bundled-git', 'Enabled bundled Git plugin with its exact required and optional capability lines');

  await expect(review.locator('.plugin-heading > .plugin-badge').first()).toHaveText('Installed');
  await expect(review.locator('.plugin-state-needs-consent')).toHaveText('Needs consent');
  await expect(review.locator('.plugin-capabilities li')).toHaveText(reviewCapabilities);
  const grant = review.getByRole('button', { name: 'Grant consent', exact: true });
  await expect(grant).toBeVisible();
  await expect(grant).toBeEnabled();
  await expect(review.getByRole('button', { name: 'Disable', exact: true })).toBeEnabled();
  await review.scrollIntoViewIfNeeded();
  await capture(page, '02-plugins-needs-consent', 'Installed Review Example needs consent and offers an enabled Grant consent button');

  expect(await calls('consent_plugin')).toEqual([]);
  await grant.click();
  const dialog = page.getByRole('dialog', { name: 'Grant consent to Review Example?', exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText('example/review · Version 1.0.0', { exact: true })).toBeVisible();
  await expect(dialog.getByText(`Artifact SHA-256: ${approval.digest}`, { exact: true })).toBeVisible();
  await expect(dialog.getByText('This grants the following capabilities for all projects of this user.', { exact: true })).toBeVisible();
  await expect(dialog.locator('.plugin-capabilities li')).toHaveText(reviewCapabilities);
  expect(JSON.parse(await dialog.locator('pre').innerText())).toEqual(approval.capabilities);
  await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeEnabled();
  const confirm = dialog.getByRole('button', { name: 'Confirm grant', exact: true });
  await expect(confirm).toBeEnabled();
  expect(await calls('consent_plugin')).toEqual([]);
  await dialog.locator('.plugin-capabilities').scrollIntoViewIfNeeded();
  await capture(page, '03-plugins-consent-confirmation', 'Consent confirmation shows exactly the reviewed capability lines, artifact digest, user scope and bounded grant');
  await confirm.click();
  await expect(dialog).toHaveCount(0);
  expect(await calls('consent_plugin')).toEqual([{ cmd: 'consent_plugin', args: { id: approval.id, approval } }]);
  await expect(review.locator('.plugin-state-enabled')).toHaveText('Enabled');
  await expect(review.getByRole('button', { name: 'Grant consent', exact: true })).toHaveCount(0);
  await expect(status).toHaveText('Review Example: consent granted and plugin enabled.');

  await review.getByRole('button', { name: 'Disable', exact: true }).click();
  await expect(review.locator('.plugin-state-disabled')).toHaveText('Disabled');
  await expect(review.getByRole('button', { name: 'Enable', exact: true })).toBeVisible();
  await expect(review.getByRole('button', { name: 'Enable', exact: true })).toBeEnabled();
  await expect(review.getByRole('button', { name: 'Disable', exact: true })).toHaveCount(0);
  await expect(review.locator('.plugin-capabilities li')).toHaveText(reviewCapabilities);
  await expect(status).toHaveText('Review Example: Disabled.');
  expect(await calls('disable_plugin')).toEqual([{ cmd: 'disable_plugin', args: { id: 'example/review' } }]);
  await review.scrollIntoViewIfNeeded();
  await capture(page, '04-plugins-disabled', 'Review Example is Disabled after Disable, retains its capabilities and now offers Enable');

  await expect(failed.locator('.plugin-state-failed')).toHaveText('Failed');
  await expect(failed.locator('.plugin-failure')).toHaveText('Last failure: Plugin process exited before initialization (exit code 1).');
  await expect(failed.locator('.plugin-failure')).toBeVisible();
  await expect(failed.locator('.plugin-capabilities li')).toHaveText(['No capabilities requested.']);
  await expect(failed.getByRole('button', { name: 'Disable', exact: true })).toBeEnabled();
  await expect(failed.getByRole('button', { name: 'Check for update', exact: true })).toBeEnabled();
  await expect(failed.getByRole('button', { name: 'Grant consent', exact: true })).toHaveCount(0);
  await failed.scrollIntoViewIfNeeded();
  await capture(page, '05-plugins-failed', 'Failed Example shows Failed and the explicit initialization failure with exit code 1');

  await review.getByRole('button', { name: 'Check for update', exact: true }).click();
  await expect(status).toHaveText('Review Example: no cached update. Update discovery is offline; no network adapter is available.');
  await expect(section).toHaveAttribute('aria-busy', 'false');
  await expect(review.getByRole('button', { name: 'Check for update', exact: true })).toBeEnabled();
  await expect(section.getByRole('button', { name: 'Refresh plugins', exact: true })).toBeEnabled();
  expect(await calls('check_plugin_update')).toEqual([{ cmd: 'check_plugin_update', args: { id: 'example/review' } }]);
  await status.scrollIntoViewIfNeeded();
  await expect(status).toBeInViewport();
  await capture(page, '06-plugins-update-offline', 'Check for update honestly reports no cached update and offline discovery without a network adapter');
});
