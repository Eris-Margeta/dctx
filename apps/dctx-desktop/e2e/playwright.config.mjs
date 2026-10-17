// Browser evidence configuration for DX-E2E and the e2e skill.
// Depends on local Playwright and Vite; runner.mjs supplies the evidence directory.
// Each project gets an isolated browser context and the same deterministic mock.
import { defineConfig } from '@playwright/test';
import { join } from 'node:path';

const runDir = process.env.DCTX_E2E_RUN_DIR;
if (!runDir) throw new Error('Run browser evidence with bun run e2e');
const port = process.env.E2E_PORT || '1420';
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.mjs',
  fullyParallel: false,
  forbidOnly: true,
  workers: 1,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 8_000 },
  outputDir: join(runDir, 'test-results'),
  reporter: [['list'], ['json', { outputFile: join(runDir, 'results.json') }]],
  use: {
    browserName: 'chromium', baseURL,
    colorScheme: 'light', reducedMotion: 'reduce', locale: 'en-US', timezoneId: 'UTC',
    screenshot: 'only-on-failure', trace: 'retain-on-failure', video: 'off',
  },
  projects: [
    { name: 'desktop', use: { viewport: { width: 1440, height: 900 } } },
    { name: 'narrow', use: { viewport: { width: 390, height: 844 } } },
  ],
  webServer: {
    cwd: new URL('..', import.meta.url).pathname,
    command: `bunx --bun vite --host 127.0.0.1 --port ${port} --strictPort`,
    url: baseURL, reuseExistingServer: false, timeout: 90_000,
    gracefulShutdown: { signal: 'SIGTERM', timeout: 5_000 },
  },
});
