// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
import type { Error as RpcError, Hello, Verb, Response } from './protocol.js';
export type * from './protocol.js';

export interface BinaryOptions {
  /** Explicit executable path; takes precedence over DCTX_BIN. No PATH lookup. */
  binaryPath?: string;
  /** Project directory, also the base for relative explicit binary paths. */
  cwd?: string;
  /** Complete child environment; defaults to process.env. */
  env?: Record<string, string | undefined>;
}
export interface ConnectOptions extends BinaryOptions {
  /** Handshake deadline in milliseconds (default 10000); operations have no transport timeout. */
  handshakeTimeoutMs?: number;
  /** Raw engine diagnostics; defaults to forwarding to process.stderr. */
  onStderr?: (chunk: Uint8Array) => void;
}
export type OperationResponse = Pick<Response, 'id' | 'engine_version'> & (
  | { result: { output: string }; error?: never }
  | { error: RpcError; result?: never }
);
export interface Client {
  readonly hello: Readonly<Omit<Hello, 'verbs'>> & { readonly verbs: readonly Verb[] };
  /** Engine failures resolve with their original error envelope; transport failures reject. */
  request(verb: Exclude<Verb, 'hello'>, args?: readonly string[]): Promise<OperationResponse>;
  /** Close stdin, drain submitted requests, and wait for engine exit. */
  close(): Promise<void>;
  /** Terminate the engine and reject pending requests. No reply is promised. */
  abort(): Promise<void>;
}
export function connect(options?: ConnectOptions): Promise<Client>;
export function resolveBinary(options?: BinaryOptions): string;
export function cacheBinaryPath(options?: Pick<BinaryOptions, 'env'>): string;
