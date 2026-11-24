// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { accessSync, constants, mkdtempSync, mkdirSync, readFileSync, rmSync,
  statSync, truncateSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { connect } = require('../dctx');
const schema = require('../../../docs/protocol/dctx-rpc-v1.schema.json');
const { version } = require('../dctx/package.json');

const selectedBinary = process.env.DCTX_TEST_BIN;
const binaryPath = selectedBinary && path.resolve(selectedBinary);
const engineTest = {
  skip: !binaryPath && process.env.DCTX_REQUIRE_ENGINE_TESTS !== '1'
    ? 'Set DCTX_TEST_BIN to a built matching dctx executable' : false,
  timeout: 120000,
};
const repo = path.resolve(__dirname, '../../../crates/dctx/tests/fixtures/repository');
const casesFile = path.resolve(__dirname, '../../../tools/engine-contract/wrapper-cases/pages.json');

function quote(value) {
  return (JSON.stringify(value) ?? String(value)).replace(/[\u007f-\u009f\u2028\u2029\p{Cf}]/gu,
    (char) => `\\u{${char.codePointAt(0).toString(16)}}`);
}

function checkBytes(actual, expected, label) {
  if (!actual.equals(expected)) {
    assert.fail(`${label}: got ${quote(actual.toString())}, want ${quote(expected.toString())}`);
  }
}

function fixtureIO(action, operation) {
  try {
    return operation();
  } catch (error) {
    assert.fail(`${action}: ${quote(error.message)}`);
  }
}

function isolatedEngine(t) {
  assert.ok(binaryPath, 'DCTX_REQUIRE_ENGINE_TESTS=1 requires DCTX_TEST_BIN');
  fixtureIO('check engine executable', () => {
    accessSync(binaryPath, constants.X_OK);
    if (!statSync(binaryPath).isFile()) throw new Error(`engine path is not a file: ${binaryPath}`);
  });
  const root = mkdtempSync(path.join(tmpdir(), 'dctx-npm-parity-'));
  const clients = [];
  t.after(async () => {
    await Promise.all(clients.map((client) => client.abort()));
    fixtureIO('remove disposable project', () => rmSync(root, { recursive: true, force: true }));
  });
  const env = { ...process.env, DCTX_AUTO_CLEAN: '0', NO_COLOR: '1',
    DCTX_CACHE_DIR: path.join(root, 'cache'), DCTX_MAP_CACHE: path.join(root, 'map-cache'),
    HOME: path.join(root, 'home'), USERPROFILE: path.join(root, 'home'),
    XDG_CONFIG_HOME: path.join(root, 'config'), XDG_DATA_HOME: path.join(root, 'data'),
    XDG_CACHE_HOME: path.join(root, 'xdg-cache'), LOCALAPPDATA: path.join(root, 'local'),
    APPDATA: path.join(root, 'roaming'), };
  for (const dir of ['home', 'config', 'data', 'xdg-cache', 'local', 'roaming']) {
    fixtureIO('create isolated directory', () => mkdirSync(path.join(root, dir)));
  }
  const diagnostics = [];
  return {
    root, env, diagnostics,
    async open(cwd) {
      try {
        const client = await connect({ binaryPath, cwd, env, onStderr: (chunk) => diagnostics.push(chunk) });
        clients.push(client);
        return client;
      } catch (error) {
        assert.fail(`could not connect to engine: ${quote(error.message)}`);
      }
    },
  };
}

function runCLI(cwd, env, verb, args) {
  const prefix = verb === 'context' && !args.includes('--for') ? ['context']
    : verb === 'pack' || verb === 'context' ? ['pack']
      : verb === 'cache_status' ? ['cache', 'status']
        : verb === 'languages' ? ['languages'] : ['map', verb];
  const format = args.includes('--json') ? [] : ['--json'];
  const cli = spawnSync(binaryPath, [...prefix, ...args, ...format],
    { cwd, env, maxBuffer: 64 * 1024 * 1024, timeout: 30000 });
  if (cli.error || cli.status === null) {
    assert.fail(`CLI did not complete: ${quote(cli.error?.message ?? cli.signal)}`);
  }
  return cli;
}

async function request(client, verb, args) {
  try {
    const reply = await client.request(verb, args);
    assert.equal(reply.engine_version, version);
    return reply;
  } catch (error) {
    assert.fail(`engine request ${quote(verb)} failed: ${quote(error.message)}`);
  }
}

function checkSuccess(reply) {
  assert.ok(reply.result, `expected success, got ${quote(reply.error)}`);
  return Buffer.from(reply.result.output);
}

function checkFailure(reply, cli) {
  assert.ok(reply.error, `expected engine failure with CLI exit ${cli.status}`);
  const error = reply.error;
  assert.equal(error.code, cli.status);
  checkBytes(Buffer.from(error.output), cli.stdout, 'failure stdout');
  const stderr = cli.stderr.toString();
  const diagnostic = /^❌ Error: ([\s\S]*?)\n(?:hint: ([\s\S]*)\n)?$/.exec(stderr);
  assert.ok(diagnostic && diagnostic[0] === stderr, `unexpected CLI diagnostic ${quote(stderr)}`);
  assert.equal(quote(error.message), quote(diagnostic[1]), 'CLI message');
  assert.equal(quote(error.hint), quote(diagnostic[2] ?? ''), 'CLI hint');
  return error;
}

test('every schema verb preserves real CLI bytes, errors, ordering and shutdown', engineTest, async (t) => {
  const { env, diagnostics, open } = isolatedEngine(t);
  const client = await open(repo);
  assert.equal(client.hello.protocol_version, '1.0.0');
  const verbs = schema.definitions.Verb.oneOf.flatMap((item) => item.enum);
  assert.deepEqual(client.hello.verbs, verbs);
  const seen = new Set(['hello']);
  const hostile = loadCases().hostile_flag;
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
    ['languages', [hostile]],
  ];
  for (const [verb, args] of cases) {
    const cli = runCLI(repo, env, verb, args);
    const reply = await request(client, verb, args);
    if (args[0] === hostile) {
      const error = checkFailure(reply, cli);
      assert.equal(error.code, 2);
      assert.ok(error.message.includes(hostile) && error.hint, 'expected the hostile argument and a hint');
    } else if (cli.status === 0) {
      checkBytes(checkSuccess(reply), cli.stdout, `${verb} stdout`);
    } else {
      checkFailure(reply, cli);
    }
    seen.add(verb);
  }
  assert.deepEqual([...seen].sort(), verbs.sort(), 'every advertised verb must have CLI parity');
  const pending = Array.from({ length: 12 }, () => request(client, 'languages'));
  const closed = client.close();
  const replies = await Promise.all(pending);
  assert.equal(new Set(replies.map((reply) => reply.id)).size, replies.length);
  replies.forEach((reply) => checkBytes(checkSuccess(reply), checkSuccess(replies[0]), 'pending stdout'));
  await closed;
  assert.equal(Buffer.concat(diagnostics).length, 0);
  await assert.rejects(client.request('languages'), /closed/);
  await client.close();
});

function loadCases() {
  return fixtureIO('read shared cases', () => JSON.parse(readFileSync(casesFile, 'utf8')));
}

function wrapperFixture(root) {
  const cases = loadCases();
  const cwd = path.join(root, 'wrapper-pages');
  fixtureIO('write project fixture', () => {
    mkdirSync(cwd);
    for (const [name, content] of Object.entries(cases.files)) writeFileSync(path.join(cwd, name), content);
  });
  return { cases, cwd };
}

function decodePage(output) {
  try {
    return JSON.parse(output.toString());
  } catch {
    assert.fail(`invalid JSON page: ${quote(output.toString())}`);
  }
}

// Objects may contain additional fields. Arrays must have the expected length and order.
function checkFields(actual, expected, field = 'page') {
  if (Array.isArray(expected)) {
    assert.ok(Array.isArray(actual) && actual.length === expected.length,
      `${quote(field)} must contain ${expected.length} entries`);
    expected.forEach((item, index) => checkFields(actual[index], item, `${field}[${index}]`));
  } else if (expected !== null && typeof expected === 'object') {
    assert.ok(actual !== null && typeof actual === 'object' && !Array.isArray(actual),
      `${quote(field)} is not a JSON object`);
    for (const [key, value] of Object.entries(expected)) {
      assert.ok(Object.hasOwn(actual, key), `${quote(field)} is missing field ${quote(key)}`);
      checkFields(actual[key], value, `${field}.${key}`);
    }
  } else if (!Object.is(actual, expected)) {
    assert.fail(`${quote(field)}: got ${quote(actual)}, want ${quote(expected)}`);
  }
}

function cliPage(cwd, env, verb, args) {
  const cli = runCLI(cwd, env, verb, args);
  assert.equal(cli.status, 0, `CLI exit ${cli.status}: ${quote(cli.stderr.toString())}`);
  return cli.stdout;
}

test('mixed named context preserves failure stdout and connection-local history', engineTest, async (t) => {
  const { root, env, open } = isolatedEngine(t);
  const { cases, cwd } = wrapperFixture(root);
  const { good, gone, session, target, content } = cases.context;
  const flags = ['--session', session, '--dedupe'];
  const mixed = [good, gone, ...flags];
  const client = await open(cwd);
  const error = checkFailure(await request(client, 'context', mixed), runCLI(cwd, env, 'context', mixed));
  assert.equal(error.code, 1);
  const pages = error.output.split('\n');
  assert.equal(pages.length, 3);
  assert.equal(quote(pages[2]), quote(''));
  const [full, missing] = pages.slice(0, 2).map(decodePage);
  checkFields(missing, { verb: 'context', target: gone, chunks: [] });
  checkFields(full, { verb: 'context', target, chunks: [{ target, role: 'source', ref: false, content }] });
  const id = full.chunks[0].id;
  assert.ok(typeof id === 'string' && id.length > 0, 'full chunk has no ID');

  const repeat = checkSuccess(await request(client, 'context', [good, ...flags]));
  const fullLine = Buffer.from(`${pages[0]}\n`);
  checkBytes(Buffer.concat([fullLine, repeat]), cliPage(cwd, env, 'context', [good, good, ...flags]),
    'batched context');
  const reference = decodePage(repeat);
  checkFields(reference, { target, chunks: [{ id, ref: true }] });
  assert.ok(!Object.hasOwn(reference.chunks[0], 'content'), 'deduplicated chunk still has content');

  const fresh = await open(cwd);
  const freshOutput = checkSuccess(await request(fresh, 'context', [good, ...flags]));
  checkBytes(freshOutput, cliPage(cwd, env, 'context', [good, ...flags]), 'fresh context stdout');
  checkBytes(freshOutput, fullLine, 'fresh context full body');
  await Promise.all([client.close(), fresh.close()]);
});

test('real pages preserve byte spans, bounded graphs, cursors and redirected output', engineTest, async (t) => {
  const { root, env, open } = isolatedEngine(t);
  const { cases, cwd } = wrapperFixture(root);
  const client = await open(cwd);
  const page = async (verb, args) => {
    const output = checkSuccess(await request(client, verb, args));
    checkBytes(output, cliPage(cwd, env, verb, args), 'CLI stdout');
    return decodePage(output);
  };
  await t.test('byte spans', async () => {
    for (const { target, file, lines } of cases.reads) {
      const read = await page('read', [target]);
      const disk = fixtureIO('read source bytes', () => readFileSync(path.join(cwd, file)));
      const starts = [0];
      for (let offset = 0; offset < disk.length; offset++) {
        if (disk[offset] === 0x0a) starts.push(offset + 1);
      }
      const start = starts[lines[0] - 1];
      let end = disk.length;
      if (lines[1] < starts.length) {
        end = starts[lines[1]] - 1;
        if (disk[end - 1] === 0x0d) end--;
      }
      checkFields(read, { records: [{ id: target, path: file, lines, span: [start, end] }] });
      checkBytes(Buffer.from(read.records[0].source_utf8), disk.subarray(start, end), 'read source');
    }
  });
  await t.test('truncated graphs', async () => {
    for (const row of cases.graph) checkFields(await page(row.verb, row.args), row.expected);
  });
  await t.test('cursor', async () => {
    const { target, ids } = cases.outline;
    const args = [target, '--limit', '1'];
    const first = await page('map', args);
    checkFields(first, { records: [{ id: ids[0] }], more: 1 });
    assert.ok(typeof first.next === 'string' && first.next, 'first page did not return a usable cursor');
    const second = await page('map', [...args, '--cursor', first.next]);
    checkFields(second, { records: [{ id: ids[1] }], more: 0, next: null });
  });
  await t.test('pack to file', async () => {
    const target = path.join(root, 'pack.json');
    const stdout = checkSuccess(await request(client, 'pack', ['--output', target]));
    assert.equal(stdout.length, 0);
    const saved = fixtureIO('read saved pack', () => readFileSync(target));
    checkBytes(saved, cliPage(cwd, env, 'pack', []), 'saved pack');
    checkFields(decodePage(saved), { files: Object.keys(cases.files).length });
  });
  await t.test('extraction markers', async () => {
    const { oversized_file, size, partial, opaque, refused_read } = cases.extraction;
    const oversized = path.join(cwd, oversized_file);
    // The index skips an oversized input before reading its contents.
    fixtureIO('create oversized fixture', () => {
      writeFileSync(oversized, '');
      truncateSync(oversized, size);
    });
    for (const row of [partial, opaque]) checkFields(await page(row.verb, row.args), row.expected);
    const { verb, args, expected } = refused_read;
    const error = checkFailure(await request(client, verb, args), runCLI(cwd, env, verb, args));
    assert.equal(error.code, 1);
    checkFields(decodePage(error.output), expected);
  });
  await client.close();
});
