// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
// Build/check the dependency-free client and pack it locally. Never publishes.
import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, resolve, dirname, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const packageDir = resolve(root, 'packaging/npm/dctx');
const outDir = resolve(process.argv[2] ?? resolve(root, 'packaging/npm/artifacts'));
function run(command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed (${result.status}): ${result.stderr || result.stdout}`);
  return result.stdout;
}
run(process.execPath, [resolve(dirname(fileURLToPath(import.meta.url)), 'generate-types.mjs'), '--check']);
const manifest = JSON.parse(readFileSync(resolve(packageDir, 'package.json')));
if (manifest.name !== '@tejl/dctx' || manifest.publishConfig?.access !== 'public') {
  throw new Error('The Node wrapper must be published as public @tejl/dctx');
}
const engineManifest = readFileSync(resolve(root, 'crates/dctx-cli/Cargo.toml'), 'utf8');
if (engineManifest.match(/^version\s*=\s*"([^"]+)"/m)?.[1] !== manifest.version) throw new Error('npm and engine versions differ');
const platformDirectories = {
  '@tejl/dctx-linux-x64': 'cli-linux-x64',
  '@tejl/dctx-linux-arm64': 'cli-linux-arm64',
  '@tejl/dctx-darwin-x64': 'cli-darwin-x64',
  '@tejl/dctx-darwin-arm64': 'cli-darwin-arm64',
  '@tejl/dctx-win32-x64': 'cli-win32-x64',
};
for (const [name, version] of Object.entries(manifest.optionalDependencies)) {
  const directory = platformDirectories[name];
  if (!directory) throw new Error(`Unknown platform package: ${name}`);
  const native = JSON.parse(readFileSync(resolve(root, 'packaging/npm', directory, 'package.json')));
  if (version !== manifest.version || native.version !== version || native.name !== name) throw new Error(`Version mismatch: ${name}`);
}
for (const source of ['bin/dctx.js', 'src/index.js', 'src/resolve.js']) {
  run(process.execPath, ['--check', resolve(packageDir, source)]);
}
mkdirSync(outDir, { recursive: true });
// On Windows npm is a .cmd shim. Use its JS entry point instead of a shell.
const npmCli = process.env.npm_execpath;
const command = npmCli ? process.execPath : process.platform === 'win32' ? 'npm.cmd' : 'npm';
if (process.platform === 'win32' && !npmCli) {
  throw new Error('On Windows run: npm exec -- node packaging/npm/scripts/build-pack.mjs [output-directory]');
}
// Local tarballs drop optionalDependencies so `npm install <tarball>` does not resolve
// platform packages from the configured registry. They use DCTX_BIN, binaryPath or the
// cache path. tools/release/prepare-npm.py restores the pinned @tejl/dctx-* dependencies
// for registry publication.
const staging = mkdtempSync(join(tmpdir(), 'dctx-npm-pack-'));
try {
  const stagedPackage = join(staging, 'dctx');
  cpSync(packageDir, stagedPackage, { recursive: true, filter: (source) => !relative(packageDir, source).split(sep).includes('node_modules') });
  const localManifest = { ...manifest };
  delete localManifest.optionalDependencies;
  writeFileSync(join(stagedPackage, 'package.json'), `${JSON.stringify(localManifest, null, 2)}\n`);
  const args = [...(npmCli ? [npmCli] : []), 'pack', '--ignore-scripts', '--json', '--pack-destination', outDir];
  process.stdout.write(run(command, args, stagedPackage));
} finally {
  rmSync(staging, { recursive: true, force: true });
}
