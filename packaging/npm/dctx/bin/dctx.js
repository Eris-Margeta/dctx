#!/usr/bin/env node
'use strict';

const { spawnSync } = require('node:child_process');
const supported = new Set(['linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64', 'win32-x64']);
const platform = `${process.platform}-${process.arch}`;
if (!supported.has(platform)) {
  console.error(`dctx: unsupported platform ${platform}`);
  process.exit(1);
}

let binary;
try {
  binary = require.resolve(`@dctx/cli-${platform}/bin/dctx${process.platform === 'win32' ? '.exe' : ''}`);
} catch {
  console.error(`dctx: missing @dctx/cli-${platform}; reinstall with npm install --include=optional dctx`);
  process.exit(1);
}
const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit', windowsHide: true });
if (result.error) {
  console.error(`dctx: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) process.kill(process.pid, result.signal);
else process.exit(result.status ?? 1);
