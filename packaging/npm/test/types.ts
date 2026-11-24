// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
import { connect, cacheBinaryPath, resolveBinary } from '../dctx/src/index.js';
import type { Client, Id, OperationResponse, Request, Response, Verb } from '../dctx/src/index.js';

async function use(): Promise<void> {
  const client: Client = await connect({ cwd: '.', binaryPath: resolveBinary(), handshakeTimeoutMs: 5000,
    onStderr: (bytes) => { void bytes.byteLength; } });
  const verb: Verb = 'read';
  const wire: Request = { id: 'test', verb, args: ['m:src/lib.rs'] };
  const refusal = {
    id: null,
    engine_version: '1.3.2',
    error: { code: 2, message: 'invalid request', hint: 'send hello with an id first', output: '' },
  } satisfies Response;
  // @ts-expect-error An uncorrelated refusal cannot be returned by Client.request.
  const uncorrelated: OperationResponse = refusal;
  // @ts-expect-error Requests require a non-null id.
  const nullRequest: Request = { id: null, verb };
  // @ts-expect-error A wire refusal always has an id field.
  const missingId: Response = { engine_version: refusal.engine_version, error: refusal.error };
  const response = await client.request(verb, wire.args);
  const id: Id = response.id;
  void [uncorrelated, nullRequest, missingId, id];
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
