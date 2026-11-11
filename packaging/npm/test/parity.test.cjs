// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { mkdtempSync, mkdirSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { connect } = require('../dctx');
const schema = require('../../../docs/protocol/dctx-rpc-v1.schema.json');
const { version } = require('../dctx/package.json');

const binaryPath = process.env.DCTX_TEST_BIN;
const repo = path.resolve(__dirname, '../../../crates/dctx/tests/fixtures/repository');

test('every schema verb preserves real CLI bytes, errors, ordering and shutdown', {
  skip: binaryPath ? false : 'Set DCTX_TEST_BIN to a built matching dctx executable', timeout: 120000,
}, async (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'dctx-npm-parity-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const env = { ...process.env, DCTX_AUTO_CLEAN: '0', NO_COLOR: '1',
    DCTX_CACHE_DIR: path.join(root, 'cache'), DCTX_MAP_CACHE: path.join(root, 'map-cache'),
    HOME: path.join(root, 'home'), USERPROFILE: path.join(root, 'home'),
    XDG_CONFIG_HOME: path.join(root, 'config'), XDG_DATA_HOME: path.join(root, 'data'),
    XDG_CACHE_HOME: path.join(root, 'xdg-cache'), LOCALAPPDATA: path.join(root, 'local'),
    APPDATA: path.join(root, 'roaming'), };
  for (const dir of ['home', 'config', 'data', 'xdg-cache', 'local', 'roaming']) mkdirSync(path.join(root, dir));
  const diagnostics = [];
  const client = await connect({ binaryPath, cwd: repo, env, onStderr: (chunk) => diagnostics.push(chunk) });
  t.after(() => client.abort());
  assert.equal(client.hello.protocol_version, '1.0.0');
  const verbs = schema.definitions.Verb.oneOf.flatMap((item) => item.enum);
  assert.deepEqual(client.hello.verbs, verbs);
  const seen = new Set(['hello']);
  const cases = [
    ['find', ['ping']], ['find', ['not_present_zz']], ['find', ['ping', '--limit', '1']],
    ['map', ['m:src/lib.rs']], ['read', ['s:src/lib.rs#ping']],
    ['read', ['m:src/lib.rs', '--lines', '1-2']], ['skeleton', ['m:src/lib.rs']],
    ['explain', ['s:src/lib.rs#ping']], ['neighbors', ['s:src/lib.rs#ping']],
    ['grep', ['pub']], ['hubs', []], ['path', ['m:src/lib.rs', 's:src/lib.rs#ping']],
    ['trace', ['s:src/lib.rs#ping', 's:src/lib.rs#ping']],
    // The isolated ping has no directed path to its module or incoming callers.
    ['trace', ['s:src/lib.rs#ping', 'm:src/lib.rs', '--max-depth', '1', '--max-paths', '1', '--max-bytes', '256']],
    ['impact', ['s:src/lib.rs#ping']],
    ['impact', ['s:src/lib.rs#ping', '--max-depth', '1', '--max-sites', '1', '--max-bytes', '256']],
    ['pack', []], ['context', ['--for', '`ping`']], ['languages', []], ['cache_status', []],
    // Failure codes, empty results, already-rendered output, explicit format and engine refusal.
    ['read', ['m:absent.rs']], ['find', ['']], ['find', ['ping', '--place', 'missing-directory']],
    ['context', ['--for', '`absent`']], ['pack', ['--ext', 'rs']],
    ['languages', ['--bad']], ['cache_status', ['--bad']],
    ['languages', ['--json']], ['cache_status', ['--json']],
    ['find', ['with spaces; $(no-shell) λ']],
  ];
  for (const [verb, args] of cases) {
    const prefix = verb === 'pack' || verb === 'context' ? ['pack']
      : verb === 'cache_status' ? ['cache', 'status']
        : verb === 'languages' ? ['languages'] : ['map', verb];
    const cli = spawnSync(binaryPath, [...prefix, ...args, ...(args.includes('--json') ? [] : ['--json'])], { cwd: repo, env, maxBuffer: 64 * 1024 * 1024 });
    assert.ifError(cli.error);
    const reply = await client.request(verb, args);
    assert.equal(reply.engine_version, version);
    if (cli.status === 0) {
      assert.ok(reply.result, `${verb}: ${reply.error?.message}`);
      assert.deepEqual(Buffer.from(reply.result.output), cli.stdout, `${verb} stdout`);
    } else {
      assert.ok(reply.error, `${verb}: expected exit ${cli.status}`);
      assert.equal(reply.error.code, cli.status, `${verb} status`);
      assert.deepEqual(Buffer.from(reply.error.output), cli.stdout, `${verb} failure stdout`);
      assert.equal(typeof reply.error.message, 'string');
      assert.equal(typeof reply.error.hint, 'string');
    }
    seen.add(verb);
  }
  assert.deepEqual([...seen].sort(), verbs.sort(), 'every advertised verb must have CLI parity');
  const pending = Array.from({ length: 12 }, () => client.request('languages'));
  const closed = client.close();
  const replies = await Promise.all(pending);
  assert.equal(new Set(replies.map((reply) => reply.id)).size, replies.length);
  replies.forEach((reply) => assert.equal(reply.result.output, replies[0].result.output));
  await closed;
  assert.equal(Buffer.concat(diagnostics).length, 0);
  await assert.rejects(client.request('languages'), /closed/);
  await client.close();
});
