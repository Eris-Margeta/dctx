#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';

const { spawnSync } = require('node:child_process');
const { connect, resolveBinary } = require('../src/index.js');

async function main() {
  const binaryPath = resolveBinary();
  const client = await connect({ binaryPath });
  await client.close();
  const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: 'inherit', windowsHide: true, shell: false });
  if (result.error) throw result.error;
  if (result.signal) process.kill(process.pid, result.signal);
  else process.exitCode = result.status ?? 1;
}
main().catch((error) => {
  console.error(error.message.startsWith('dctx') ? error.message : `dctx: ${error.message}`);
  process.exitCode = 1;
});
