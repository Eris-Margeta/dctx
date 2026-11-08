// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
// Build/check the dependency-free client and pack it locally. Never publishes.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
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
const engineManifest = readFileSync(resolve(root, 'crates/dctx-cli/Cargo.toml'), 'utf8');
if (engineManifest.match(/^version\s*=\s*"([^"]+)"/m)?.[1] !== manifest.version) throw new Error('npm and engine versions differ');
for (const [name, version] of Object.entries(manifest.optionalDependencies)) {
  const native = JSON.parse(readFileSync(resolve(root, 'packaging/npm', name.slice('@dctx/'.length), 'package.json')));
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
const args = [...(npmCli ? [npmCli] : []), 'pack', '--ignore-scripts', '--json', '--pack-destination', outDir];
process.stdout.write(run(command, args, packageDir));
