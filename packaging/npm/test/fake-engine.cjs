// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';
// Invoked as `node rpc` in an isolated fixture directory, on every OS.
const { createInterface } = require('node:readline');
const mode = process.env.FAKE_RPC_MODE;
const version = process.env.FAKE_RPC_VERSION;
const input = createInterface({ input: process.stdin });
let initialized = false;
let queued;
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
      reply.error = { code: 2, message: 'refused\n\u001b[31m"\u009b\u2028', hint: 'engine hint', output: '' };
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
    if (mode === 'malformed') return process.stdout.write('invalid-json "\u001b[31m\n');
    if (mode === 'version-after-hello') reply.engine_version = '0.0.0';
    if (mode === 'stderr' || mode === 'stderr-error') {
      process.stderr.write('raw diagnostic λ\n');
      if (mode === 'stderr-error') return;
    }
  }
  const bytes = Buffer.from(JSON.stringify(reply) + (mode === 'batch-crlf' ? '\r\n' : '\n'));
  if (request.verb !== 'hello') {
    if (mode === 'split') {
      if (request.verb === 'find') {
        queued = bytes;
        return;
      }
      if (request.verb === 'read') {
        const split = bytes.indexOf(Buffer.from('λ')) + 1;
        process.stdout.write(Buffer.concat([queued, bytes.subarray(0, split)]));
        // The test waits for the first reply before sending the request that flushes this tail.
        queued = bytes.subarray(split);
        return;
      }
      return process.stdout.write(Buffer.concat([queued, bytes]));
    }
    if (['unterminated', 'invalid-utf8', 'batch-crlf'].includes(mode)) {
      if (!queued) {
        if (mode === 'invalid-utf8') bytes[bytes.indexOf('opaque')] = 0xff;
        queued = bytes;
        return;
      }
      if (mode === 'unterminated') {
        return process.stdout.write(queued.subarray(0, -1), () => process.exit(0));
      }
      process.stdout.write(Buffer.concat([queued, bytes]));
      queued = undefined;
      return;
    }
  }
  process.stdout.write(bytes);
});
