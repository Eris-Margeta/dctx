// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';
// Invoked as `node rpc` in an isolated fixture directory, on every OS.
const { createInterface } = require('node:readline');
const mode = process.env.FAKE_RPC_MODE;
const version = process.env.FAKE_RPC_VERSION;
const input = createInterface({ input: process.stdin });
let initialized = false;
input.on('line', (line) => {
  const request = JSON.parse(line);
  const reply = { id: request.id, engine_version: mode === 'version' ? '0.0.0' : version };
  if (request.verb === 'hello') {
    if (mode === 'hang') return;
    initialized = true;
    reply.result = { protocol_version: mode === 'protocol' ? '2.0.0' : '1.9.0',
      verbs: ['hello', 'languages'], future: 'ignored' };
    if (mode === 'hello-shape') reply.result.verbs = null;
    if (mode === 'hello-error') {
      delete reply.result;
      reply.error = { code: 2, message: 'refused', hint: 'engine hint', output: '' };
    }
  } else {
    if (!initialized) process.exit(90);
    if (mode === 'exit') process.exit(3);
    if (mode === 'abort') return;
    if (mode === 'large') reply.result = { output: 'λ'.repeat(600000) + '\n' };
    else reply.result = { output: JSON.stringify(request.args) + '\nλ\u0000opaque\n\n' };
    reply.future = { retained: true };
    if (mode === 'error') {
      delete reply.result;
      reply.error = { code: 1, message: 'engine message', hint: 'engine hint', output: 'gone\n\n', extra: true };
    }
    if (mode === 'bad-error') {
      delete reply.result;
      reply.error = { code: 5, output: '' };
    }
    if (mode === 'id') reply.id = 'wrong';
    if (mode === 'both') reply.error = { code: 2, message: '', hint: '', output: '' };
    if (mode === 'shape') reply.result = { output: 3 };
    if (mode === 'malformed') return process.stdout.write('invalid-json\n');
    if (mode === 'version-after-hello') reply.engine_version = '0.0.0';
    if (mode === 'stderr') process.stderr.write('raw diagnostic λ\n');
  }
  const bytes = Buffer.from(JSON.stringify(reply) + '\n');
  // Exercise fragmented NDJSON and UTF-8 without changing opaque output bytes.
  const split = bytes.indexOf(Buffer.from('λ')) + 1;
  process.stdout.write(bytes.subarray(0, split));
  process.stdout.write(bytes.subarray(split));
});
