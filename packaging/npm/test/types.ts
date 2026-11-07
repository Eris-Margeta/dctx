// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
import { connect, cacheBinaryPath, resolveBinary } from '../dctx/src/index.js';
import type { Client, Request, Verb } from '../dctx/src/index.js';

async function use(): Promise<void> {
  const client: Client = await connect({ cwd: '.', binaryPath: resolveBinary(), handshakeTimeoutMs: 5000,
    onStderr: (bytes) => { void bytes.byteLength; } });
  const verb: Verb = 'read';
  const wire: Request = { id: 'test', verb, args: ['m:src/lib.rs'] };
  const response = await client.request(verb, wire.args);
  if (response.error) {
    const code: number = response.error.code;
    const output: string = response.error.output;
    void [code, output];
  } else {
    const output: string = response.result.output;
    void output;
  }
  // @ts-expect-error RPC verbs come from the engine schema.
  await client.request('invented');
  // @ts-expect-error CLI arguments retain string boundaries.
  await client.request('read', [1]);
  // @ts-expect-error Handshake metadata is read-only.
  client.hello.verbs.push('pack');
  await client.close();
  void cacheBinaryPath();
}
void use;
