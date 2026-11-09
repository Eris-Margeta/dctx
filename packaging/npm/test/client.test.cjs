// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createRequire } = require('node:module');
const { connect, resolveBinary, cacheBinaryPath } = require('../dctx');
const { version } = require('../dctx/package.json');

function fixture(t, mode = '') {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'dctx-npm-client-'));
  t.after(() => fs.rmSync(cwd, { recursive: true, force: true }));
  fs.copyFileSync(path.join(__dirname, 'fake-engine.cjs'), path.join(cwd, 'rpc'));
  return { cwd, binaryPath: process.execPath, env: { ...process.env, FAKE_RPC_MODE: mode, FAKE_RPC_VERSION: version } };
}

test('exact opaque output, argument boundaries, future fields, FIFO and graceful drain', async (t) => {
  const client = await connect(fixture(t));
  t.after(() => client.abort());
  assert.equal(client.hello.protocol_version, '1.9.0');
  const args = ['with spaces', 'λ', '', '; $(no shell)', '--json'];
  const replies = [client.request('find', args), client.request('languages')];
  const closed = client.close();
  const [first, second] = await Promise.all(replies);
  assert.equal(first.result.output, JSON.stringify(args) + '\nλ\u0000opaque\n\n');
  assert.deepEqual(first.future, { retained: true });
  assert.equal(BigInt(second.id), BigInt(first.id) + 1n);
  await closed;
  await client.abort();
  await client.close();
  await assert.rejects(client.request('languages'), /closed/);
});

test('engine errors resolve without losing code, hint, output or unknown fields', async (t) => {
  const client = await connect(fixture(t, 'error'));
  t.after(() => client.abort());
  const reply = await client.request('read', ['missing']);
  assert.deepEqual(reply.error, { code: 1, message: 'engine message', hint: 'engine hint', output: 'gone\n\n', extra: true });
  await client.close();
});

for (const [mode, expected] of [
  ['version', /does not match/], ['protocol', /unsupported protocol/],
  ['hello-shape', /invalid result envelope/], ['hello-error', /handshake refused: refused/],
]) {
  test(`handshake rejects ${mode} before operations`, { timeout: 10000 }, async (t) => {
    await assert.rejects(connect(fixture(t, mode)), expected);
  });
}

test('handshake timeout terminates an unresponsive executable', { timeout: 10000 }, async (t) => {
  await assert.rejects(connect({ ...fixture(t, 'hang'), handshakeTimeoutMs: 100 }), /handshake timed out/);
});

for (const [mode, expected] of [
  ['id', /unexpected response id/], ['both', /exactly one/], ['shape', /invalid result/],
  ['malformed', /JSON/], ['bad-error', /invalid error/], ['exit', /exited/],
  ['version-after-hello', /does not match/],
]) {
  test(`transport failure ${mode} rejects pending calls and closes the process`, { timeout: 10000 }, async (t) => {
    const client = await connect(fixture(t, mode));
    t.after(() => client.abort());
    const results = await Promise.allSettled([client.request('find'), client.request('languages')]);
    for (const result of results) {
      assert.equal(result.status, 'rejected');
      assert.match(result.reason.message, expected);
    }
    await assert.rejects(client.close(), expected);
  });
}

test('abort rejects pending operations and reaps the engine', { timeout: 10000 }, async (t) => {
  const client = await connect(fixture(t, 'abort'));
  const pending = assert.rejects(client.request('languages'), /aborted/);
  await client.abort();
  await pending;
  await assert.rejects(client.request('languages'), /aborted/);
});

test('request limit counts UTF-8 bytes and refusal leaves the session usable', async (t) => {
  const client = await connect(fixture(t));
  t.after(() => client.abort());
  await assert.rejects(client.request('grep', ['λ'.repeat(524288)]), /1048576/);
  await assert.rejects(client.request('grep', [42]), /array of strings/);
  assert.ok((await client.request('languages')).result);
  await client.close();
});

test('response bytes are never truncated to the request limit', async (t) => {
  const client = await connect(fixture(t, 'large'));
  t.after(() => client.abort());
  assert.equal((await client.request('read')).result.output, 'λ'.repeat(600000) + '\n');
  await client.close();
});

test('stderr stays separate from engine output', async (t) => {
  const chunks = [];
  const client = await connect({ ...fixture(t, 'stderr'), onStderr: (chunk) => chunks.push(chunk) });
  t.after(() => client.abort());
  assert.ok((await client.request('languages')).result);
  await client.close();
  assert.equal(Buffer.concat(chunks).toString(), 'raw diagnostic λ\n');
});

test('explicit executable overrides environment; invalid explicit choice never falls back', (t) => {
  const { cwd } = fixture(t);
  assert.equal(resolveBinary({ binaryPath: process.execPath, env: { DCTX_BIN: 'missing' } }), process.execPath);
  assert.equal(resolveBinary({ env: { DCTX_BIN: process.execPath } }), process.execPath);
  assert.throws(() => resolveBinary({ binaryPath: 'missing', cwd }), /cannot execute/);
  assert.throws(() => resolveBinary({ env: { DCTX_BIN: '' } }), /nonempty/);
  assert.throws(() => resolveBinary({ binaryPath: cwd }), /not a regular file/);
  assert.equal(resolveBinary({ cwd: path.dirname(process.execPath), binaryPath: path.basename(process.execPath) }), process.execPath);
});

test('optional package precedes cache; missing binaries give registry and offline instructions', (t) => {
  const { cwd } = fixture(t);
  const packageDir = path.join(cwd, 'package');
  fs.cpSync(path.join(__dirname, '../dctx'), packageDir, { recursive: true, filter: (source) => !source.includes('node_modules') });
  const isolated = createRequire(path.join(packageDir, 'package.json'))('./src/index.js');
  const env = { HOME: cwd, USERPROFILE: cwd, LOCALAPPDATA: cwd, XDG_CACHE_HOME: cwd };
  const cached = isolated.cacheBinaryPath({ env });
  assert.ok(cached.includes(version));
  assert.throws(() => isolated.resolveBinary({ env }), (error) => {
    assert.ok(error.message.includes(cached));
    assert.ok(error.message.includes(`npm install @tejl/dctx-${process.platform}-${process.arch}@${version}`), error.message);
    assert.ok(error.message.includes('npm install dctx normally includes it as an optional dependency.'));
    assert.ok(error.message.includes(`https://github.com/Eris-Margeta/dctx/releases/tag/v${version}`));
    assert.ok(error.message.includes('DCTX_BIN'));
    assert.ok(error.message.includes('SHA256SUMS'));
    return true;
  });
  fs.mkdirSync(path.dirname(cached), { recursive: true });
  fs.writeFileSync(cached, 'fixture', { mode: 0o755 });
  assert.equal(isolated.resolveBinary({ env }), cached);
  const native = path.join(packageDir, 'node_modules/@tejl', `dctx-${process.platform}-${process.arch}`, 'bin', process.platform === 'win32' ? 'dctx.exe' : 'dctx');
  fs.mkdirSync(path.dirname(native), { recursive: true });
  fs.writeFileSync(native, 'fixture', { mode: 0o755 });
  // The optional package resolves through require.resolve, which returns the real path
  // (macOS temp dirs live under the /var -> /private/var symlink).
  assert.equal(fs.realpathSync(isolated.resolveBinary({ env })), fs.realpathSync(native));
});

test('a tarball without optional dependencies (CI pack) resolves explicit and cached binaries only', (t) => {
  const { cwd } = fixture(t);
  const packageDir = path.join(cwd, 'package');
  fs.cpSync(path.join(__dirname, '../dctx'), packageDir, { recursive: true, filter: (source) => !source.includes('node_modules') });
  const manifestPath = path.join(packageDir, 'package.json');
  const manifest = JSON.parse(fs.readFileSync(manifestPath));
  delete manifest.optionalDependencies;
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  const isolated = createRequire(manifestPath)('./src/index.js');
  const env = { HOME: cwd, USERPROFILE: cwd, LOCALAPPDATA: cwd, XDG_CACHE_HOME: cwd };
  assert.equal(isolated.resolveBinary({ env: { ...env, DCTX_BIN: process.execPath } }), process.execPath);
  // A planted platform package is ignored when the manifest does not declare it.
  const native = path.join(packageDir, 'node_modules/@tejl', `dctx-${process.platform}-${process.arch}`, 'bin', process.platform === 'win32' ? 'dctx.exe' : 'dctx');
  fs.mkdirSync(path.dirname(native), { recursive: true });
  fs.writeFileSync(native, 'fixture', { mode: 0o755 });
  assert.throws(() => isolated.resolveBinary({ env }), (error) => {
    assert.ok(!error.message.includes('npm install'), error.message);
    assert.ok(error.message.includes('Obtain dctx'));
    assert.ok(error.message.includes('DCTX_BIN'));
    assert.ok(error.message.includes('SHA256SUMS'));
    return true;
  });
  const cached = isolated.cacheBinaryPath({ env });
  fs.mkdirSync(path.dirname(cached), { recursive: true });
  fs.writeFileSync(cached, 'fixture', { mode: 0o755 });
  assert.equal(isolated.resolveBinary({ env }), cached);
});

test('cache path honors absolute XDG base and is version/platform specific', () => {
  const base = path.resolve('cache-fixture');
  const cached = cacheBinaryPath({ env: { HOME: base, USERPROFILE: base, LOCALAPPDATA: base, XDG_CACHE_HOME: base } });
  assert.ok(path.isAbsolute(cached));
  assert.ok(cached.startsWith(base + path.sep));
  assert.ok(cached.includes(`${process.platform}-${process.arch}`));
  assert.ok(cached.includes(version));
});

test('CLI shim checks the handshake before forwarding arguments and native exit status', { timeout: 10000 }, (t) => {
  const { spawnSync } = require('node:child_process');
  const options = fixture(t);
  const script = path.join(__dirname, '../dctx/bin/dctx.js');
  const cliArgs = ['-e', 'process.stdout.write(JSON.stringify(process.argv.slice(1))); process.exitCode = 23;', 'with spaces', '; $(no-shell)', 'λ'];
  const result = spawnSync(process.execPath, [script, ...cliArgs], {
    cwd: options.cwd, env: { ...options.env, DCTX_BIN: process.execPath }, encoding: 'utf8',
  });
  assert.ifError(result.error);
  assert.equal(result.status, 23, result.stderr);
  assert.equal(result.stdout, JSON.stringify(cliArgs.slice(2)));
  assert.equal(result.stderr, '');
  const mismatch = spawnSync(process.execPath, [script, ...cliArgs], {
    cwd: options.cwd, env: { ...options.env, DCTX_BIN: process.execPath, FAKE_RPC_MODE: 'version' }, encoding: 'utf8',
  });
  assert.ifError(mismatch.error);
  assert.equal(mismatch.status, 1);
  assert.equal(mismatch.stdout, '');
  assert.match(mismatch.stderr, /does not match/);
});


test('DCTX_BIN pointing at the npm command shim fails before recursive process startup', () => {
  const shim = path.join(__dirname, '../dctx/bin/dctx.js');
  assert.throws(() => resolveBinary({ env: { DCTX_BIN: shim } }), /npm command shim cannot be used as the native engine/);
});
