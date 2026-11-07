// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';

const { spawn } = require('node:child_process');
const { createInterface } = require('node:readline');
const { version } = require('../package.json');
const { resolveBinary, cacheBinaryPath } = require('./resolve.js');
const MAX_REQUEST_BYTES = 1048576;

class DctxClient {
  #child;
  #pending = [];
  #nextId = 0n;
  #failure;
  #closing = false;
  #exited = false;
  #done;
  #hello;

  constructor(binary, options) {
    this.#child = spawn(binary, ['rpc'], {
      cwd: options.cwd,
      env: options.env ?? process.env,
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
      shell: false,
    });
    const child = this.#child;
    if (options.onStderr) {
      child.stderr.on('data', (chunk) => {
        try { options.onStderr(chunk); } catch (error) { this.#fail(error); }
      });
    } else {
      child.stderr.pipe(process.stderr, { end: false });
    }
    const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
    lines.on('line', (line) => this.#receive(line));
    child.on('error', (error) => this.#fail(error));
    child.stdin.on('error', (error) => this.#fail(error));
    child.stdout.on('error', (error) => this.#fail(error));
    child.stderr.on('error', (error) => this.#fail(error));
    this.#done = new Promise((resolve) => {
      child.on('close', (code, signal) => {
        this.#exited = true;
        lines.close();
        if (this.#pending.length || !this.#closing || code !== 0) {
          this.#fail(new Error(`dctx rpc exited (code=${code}, signal=${signal})`));
        }
        resolve();
      });
    });
  }

  #fail(error) {
    this.#failure ??= error;
    for (const pending of this.#pending.splice(0)) pending.reject(this.#failure);
    this.#child.kill();
  }

  #receive(line) {
    try {
      const reply = JSON.parse(line);
      const pending = this.#pending[0];
      if (!pending || !reply || reply.id !== pending.id) throw new Error('dctx rpc: unexpected response id');
      if (reply.engine_version !== version) {
        throw new Error(`dctx rpc: engine version ${reply.engine_version} does not match npm package ${version}. Set DCTX_BIN to dctx ${version} or reinstall dctx@${version} with optional dependencies.`);
      }
      const result = Object.hasOwn(reply, 'result');
      const error = Object.hasOwn(reply, 'error');
      if (result === error) throw new Error('dctx rpc: expected exactly one result or error');
      if (error && (!reply.error || ![1, 2, 3].includes(reply.error.code) ||
        !['message', 'hint', 'output'].every((key) => typeof reply.error[key] === 'string'))) {
        throw new Error('dctx rpc: invalid error envelope');
      }
      if (result && (pending.verb === 'hello'
        ? !reply.result || typeof reply.result.protocol_version !== 'string' ||
          !Array.isArray(reply.result.verbs) || !reply.result.verbs.every((verb) => typeof verb === 'string')
        : !reply.result || typeof reply.result.output !== 'string')) {
        throw new Error('dctx rpc: invalid result envelope');
      }
      this.#pending.shift().resolve(reply);
    } catch (error) { this.#fail(error); }
  }

  #send(verb, args) {
    if (this.#failure) return Promise.reject(this.#failure);
    if (this.#closing) return Promise.reject(new Error('dctx rpc: client is closed'));
    let frame;
    const id = String(this.#nextId++);
    try {
      if (typeof verb !== 'string' || !Array.isArray(args) || !args.every((arg) => typeof arg === 'string')) {
        throw new TypeError('dctx rpc: verb must be a string and args must be an array of strings');
      }
      frame = JSON.stringify({ id, verb, args });
      if (Buffer.byteLength(frame) > MAX_REQUEST_BYTES) throw new RangeError('dctx rpc: request exceeds 1048576 bytes');
    } catch (error) { return Promise.reject(error); }
    return new Promise((resolve, reject) => {
      this.#pending.push({ id, verb, resolve, reject });
      this.#child.stdin.write(`${frame}\n`, (error) => { if (error) this.#fail(error); });
    });
  }

  async initialize() {
    const reply = await this.#send('hello', []);
    if (reply.error) throw new Error(`dctx rpc: handshake refused: ${reply.error.message}`);
    if (!/^1\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(reply.result.protocol_version)) {
      throw new Error(`dctx rpc: unsupported protocol version ${reply.result.protocol_version}; expected major 1`);
    }
    this.#hello = Object.freeze({ ...reply.result, verbs: Object.freeze([...reply.result.verbs]) });
  }

  get hello() { return this.#hello; }

  request(verb, args = []) { return this.#send(verb, args); }

  async close() {
    if (!this.#closing) {
      this.#closing = true;
      this.#child.stdin.end();
    }
    await this.#done;
    if (this.#failure) throw this.#failure;
  }

  async abort() {
    if (this.#exited) return;
    this.#closing = true;
    this.#fail(new Error('dctx rpc: session aborted'));
    await this.#done;
  }
}

async function connect(options = {}) {
  const timeout = options.handshakeTimeoutMs ?? 10000;
  if (!Number.isSafeInteger(timeout) || timeout <= 0 || timeout > 2147483647) {
    throw new RangeError('dctx rpc: handshakeTimeoutMs must be an integer from 1 to 2147483647');
  }
  const client = new DctxClient(resolveBinary(options), options);
  let timer;
  try {
    await Promise.race([
      client.initialize(),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error('dctx rpc: handshake timed out')), timeout);
      }),
    ]);
    return client;
  } catch (error) {
    await client.abort();
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

module.exports = { connect, resolveBinary, cacheBinaryPath };
