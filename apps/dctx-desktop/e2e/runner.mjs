// Local browser evidence runner implementing DX-E2E and the e2e skill.
// Depends on Node builtins and the pinned local Playwright CLI; never invokes Git.
// Used by bun run e2e; all runtime evidence stays under ignored e2e-runs/.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const config = join(root, 'e2e/playwright.config.mjs');
const cli = join(root, 'node_modules/@playwright/test/cli.js');
const specs = readdirSync(join(root, 'e2e')).filter(name => name.endsWith('.spec.mjs')).sort();
if (!specs.length) {
  console.error('e2e: refusing to run with zero specs in e2e/*.spec.mjs');
  process.exit(2);
}
if (!existsSync(config) || !existsSync(cli)) {
  console.error('e2e: local Playwright and config required; run bun install (never download browsers)');
  process.exit(2);
}

function filesUnder(directory) {
  return readdirSync(join(root, directory), { withFileTypes: true }).flatMap(entry => {
    const name = `${directory}/${entry.name}`;
    return entry.isDirectory() ? filesUnder(name) : entry.isFile() ? [name] : [];
  });
}
const sources = [
  ...filesUnder('e2e'), 'index.html', ...filesUnder('src').filter(name => name.endsWith('.ts')),
  'package.json', 'bun.lock', 'vite.config.ts', 'tsconfig.json',
].sort();
const hash = createHash('sha256');
const sourceFiles = sources.map(path => {
  const bytes = readFileSync(join(root, path));
  hash.update(path).update('\0').update(bytes).update('\0');
  return { path, sha256: createHash('sha256').update(bytes).digest('hex') };
});
const timestamp = new Date().toISOString();
const runDir = join(root, 'e2e-runs', timestamp.replaceAll(':', '-'));
mkdirSync(runDir, { recursive: true });
const args = [cli, 'test', '--config', config, ...process.argv.slice(2)];
const manifest = {
  command: 'bun run e2e', args: process.argv.slice(2), timestamp,
  commit: 'not collected (decision 078: no runtime Git)', sourceDigest: hash.digest('hex'), sourceFiles,
  seed: 'dctx-desktop-fixture-v1', browser: 'chromium', workers: 1, retries: 0,
  viewports: [{ width: 1440, height: 900 }, { width: 390, height: 844 }], specs,
};
writeFileSync(join(runDir, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
const run = spawnSync(process.execPath, args, {
  cwd: root, stdio: 'inherit',
  env: { ...process.env, DCTX_E2E_RUN_DIR: runDir, PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD: '1' },
});
let code = run.status ?? 1;
let results;
try { results = JSON.parse(readFileSync(join(runDir, 'results.json'), 'utf8')); }
catch {
  code = code || 1;
  results = { stats: {}, errors: [{ message: run.error?.message ?? `Playwright ended without results (${run.signal ?? code})` }] };
  writeFileSync(join(runDir, 'results.json'), JSON.stringify(results, null, 2) + '\n');
}
const counts = results.stats ?? {};
if (!counts.expected) code = code || 1;
const screenshots = [];
const steps = [];
function collect(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) collect(path);
    else if (entry.name.endsWith('.png')) screenshots.push(relative(runDir, path));
    else if (entry.name === 'steps.json') steps.push(...JSON.parse(readFileSync(path, 'utf8')));
  }
}
collect(runDir);
writeFileSync(join(runDir, 'summary.md'), [
  `# Browser evidence ${timestamp}`, '', 'Command: `bun run e2e`',
  `Source digest: \`${manifest.sourceDigest}\``,
  `Result: ${code === 0 ? 'pass' : 'fail'} (exit ${code})`,
  `Playwright: ${counts.expected ?? 0} passed, ${counts.unexpected ?? 0} failed, ${counts.skipped ?? 0} skipped`, '',
  '## Screenshots', '', ...screenshots.sort().map(path => {
    const step = steps.find(item => item.screenshot === path);
    return `- [${path}](${path}): ${step ? `${step.description}; visible-text assertions passed` : 'failure capture; inspect results.json'}`;
  }), '', 'DOM text and console/page errors are saved alongside screenshots. Native Rust, filesystem, clipboard and plugin execution are mocked.', '',
].join('\n'));
console.log(`e2e evidence: ${runDir}`);
process.exitCode = code;
