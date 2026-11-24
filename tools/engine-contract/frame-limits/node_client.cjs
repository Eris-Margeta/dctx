// SPDX-License-Identifier: Apache-2.0
'use strict';

const { readFileSync } = require('node:fs');
const { connect } = require('../../../packaging/npm/dctx');

async function main() {
  const config = JSON.parse(readFileSync(0, 'utf8'));
  const client = await connect({ cwd: config.project, binaryPath: config.binary, env: config.env });
  try {
    const results = [];
    for (const test of config.cases) {
      const args = ['--help'];
      if (test.fill) args.push(test.fill.repeat(test.repeat) + test.suffix);
      let reply;
      try {
        reply = await client.request('languages', args);
      } catch (error) {
        if (!(error instanceof RangeError) || error.message !== 'dctx rpc: request exceeds 1048576 bytes') {
          throw error;
        }
      }
      if (reply && !reply.result) throw new Error(JSON.stringify(reply.error));
      results.push(reply ? reply.result.output : null);
    }
    await client.close();
    process.stdout.write(JSON.stringify(results) + '\n');
  } finally {
    await client.abort();
  }
}

main().catch((error) => {
  const diagnostic = JSON.stringify(String(error)).replace(/[\u007f-\uffff]/g, (char) =>
    '\\u' + char.charCodeAt(0).toString(16).padStart(4, '0'));
  process.stderr.write('Node frame limits: ' + diagnostic + '\n');
  process.exitCode = 1;
});
