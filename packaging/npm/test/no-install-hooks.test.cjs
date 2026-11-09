// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const hooks = ['preinstall', 'install', 'postinstall', 'prepare'];

function* manifests(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) yield* manifests(filename);
    else if (entry.isFile() && entry.name === 'package.json') yield filename;
  }
}

for (const filename of manifests(root)) {
  const relative = path.relative(root, filename).split(path.sep).join('/');
  test(`${relative} has no install hooks`, () => {
    const manifest = JSON.parse(fs.readFileSync(filename, 'utf8'));
    for (const hook of hooks) {
      assert.equal(Object.prototype.hasOwnProperty.call(manifest.scripts ?? {}, hook), false,
        `${relative}: scripts.${hook} must be absent`);
    }
  });
}
