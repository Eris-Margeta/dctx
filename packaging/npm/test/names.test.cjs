// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
// The npm registry refused the unscoped name "dctx" (too similar to existing packages), and the
// publish order depends on these pins: every package is @tejl/..., all at one version.
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const npm = path.join(__dirname, '..');
const read = (dir) => JSON.parse(fs.readFileSync(path.join(npm, dir, 'package.json'), 'utf8'));

test('npm packages are scoped under @tejl and pinned to one version', () => {
  const wrapper = read('dctx');
  assert.equal(wrapper.name, '@tejl/dctx');
  assert.deepEqual(wrapper.bin, { dctx: 'bin/dctx.js' });
  assert.equal(wrapper.publishConfig && wrapper.publishConfig.access, 'public');
  const platforms = fs.readdirSync(npm).filter((d) => d.startsWith('cli-')).sort();
  assert.equal(platforms.length, 5);
  assert.equal(Object.keys(wrapper.optionalDependencies).length, 5);
  for (const dir of platforms) {
    const pkg = read(dir);
    assert.match(pkg.name, /^@tejl\/dctx-[a-z0-9]+-[a-z0-9]+$/, dir);
    assert.equal(pkg.version, wrapper.version, dir);
    assert.equal(wrapper.optionalDependencies[pkg.name], wrapper.version, dir);
  }
});
