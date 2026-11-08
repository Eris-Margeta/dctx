// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const root = path.resolve(__dirname, '../../..');

test('build/pack contains exactly the runtime, declarations, licenses and docs; packed client runs', { timeout: 60000 }, (t) => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'dctx-npm-pack-'));
  t.after(() => fs.rmSync(cwd, { recursive: true, force: true }));
  const packed = spawnSync(process.execPath, [path.join(root, 'packaging/npm/scripts/build-pack.mjs'), cwd], { cwd: root, encoding: 'utf8' });
  assert.ifError(packed.error);
  assert.equal(packed.status, 0, packed.stderr);
  const [metadata] = JSON.parse(packed.stdout);
  const expected = ['LICENSE', 'NOTICE', 'README.md', 'bin/dctx.js', 'package.json',
    'src/index.js', 'src/index.d.ts', 'src/resolve.js', 'src/protocol.d.ts'];
  assert.deepEqual(metadata.files.map((file) => file.path).sort(), expected.sort());
  assert.ok(metadata.files.find((file) => file.path === 'bin/dctx.js').mode & 0o111);
  const unpacked = spawnSync('tar', ['-xzf', path.join(cwd, metadata.filename), '-C', cwd], { encoding: 'utf8' });
  assert.ifError(unpacked.error);
  assert.equal(unpacked.status, 0, unpacked.stderr);
  const installed = path.join(cwd, 'node_modules/dctx');
  fs.mkdirSync(path.dirname(installed), { recursive: true });
  fs.renameSync(path.join(cwd, 'package'), installed);
  const manifest = JSON.parse(fs.readFileSync(path.join(installed, 'package.json')));
  assert.equal(manifest.scripts, undefined, 'no install hooks or implicit downloads');
  assert.deepEqual(Object.values(manifest.optionalDependencies), Array(5).fill(manifest.version));
  const script = `
    const assert = require('node:assert/strict');
    const cjs = require('dctx');
    async function main() {
      const esm = await import('dctx');
      assert.equal(esm.connect, cjs.connect);
      const client = await cjs.connect({ binaryPath: process.execPath, cwd: process.cwd() });
      assert.equal((await client.request('languages')).result.output, '[]\\nλ\\u0000opaque\\n\\n');
      await client.close();
    }
    main().catch((error) => { console.error(error); process.exitCode = 1; });
  `;
  fs.copyFileSync(path.join(__dirname, 'fake-engine.cjs'), path.join(cwd, 'rpc'));
  const run = spawnSync(process.execPath, ['-e', script], {
    cwd, env: { ...process.env, FAKE_RPC_VERSION: manifest.version }, encoding: 'utf8',
  });
  assert.ifError(run.error);
  assert.equal(run.status, 0, run.stderr);
});
