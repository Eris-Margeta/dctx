// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const manifest = require('../package.json');

const { version } = manifest;
// Tarballs built by packaging/npm/scripts/build-pack.mjs carry no optionalDependencies.
const optionalDependencies = manifest.optionalDependencies ?? {};

function cacheBinaryPath({ env = process.env } = {}) {
  const configuredHome = env[process.platform === 'win32' ? 'USERPROFILE' : 'HOME'];
  const home = configuredHome && path.isAbsolute(configuredHome) ? configuredHome : os.homedir();
  const base = process.platform === 'win32'
    ? env.LOCALAPPDATA && path.isAbsolute(env.LOCALAPPDATA)
      ? env.LOCALAPPDATA : path.join(home, 'AppData', 'Local')
    : process.platform === 'darwin'
      ? path.join(home, 'Library', 'Caches')
      : env.XDG_CACHE_HOME && path.isAbsolute(env.XDG_CACHE_HOME)
        ? env.XDG_CACHE_HOME : path.join(home, '.cache');
  return path.join(base, 'dctx', 'npm', version, `${process.platform}-${process.arch}`,
    process.platform === 'win32' ? 'dctx.exe' : 'dctx');
}

function executable(filename) {
  if (!fs.statSync(filename).isFile()) throw new Error('not a regular file');
  if (fs.realpathSync(filename) === fs.realpathSync(path.join(__dirname, '../bin/dctx.js'))) {
    throw new Error('the npm command shim cannot be used as the native engine');
  }
  fs.accessSync(filename, process.platform === 'win32' ? fs.constants.F_OK : fs.constants.X_OK);
  return filename;
}

function resolveBinary({ binaryPath, env = process.env, cwd = process.cwd() } = {}) {
  const explicit = binaryPath ?? env.DCTX_BIN;
  if (explicit !== undefined) {
    if (typeof explicit !== 'string' || explicit.length === 0) {
      throw new Error('dctx: binaryPath / DCTX_BIN must be a nonempty executable path');
    }
    const filename = path.resolve(cwd, explicit);
    try { return executable(filename); } catch (error) {
      throw new Error(`dctx: cannot execute ${filename}: ${error.message}. Set DCTX_BIN to a dctx ${version} executable.`, { cause: error });
    }
  }
  const platform = `${process.platform}-${process.arch}`;
  const packageName = `@tejl/dctx-${platform}`;
  if (Object.hasOwn(optionalDependencies, packageName)) {
    let filename;
    try {
      filename = require.resolve(`${packageName}/bin/dctx${process.platform === 'win32' ? '.exe' : ''}`);
    } catch (error) {
      if (error.code !== 'MODULE_NOT_FOUND') throw error;
    }
    if (filename) {
      try { return executable(filename); } catch (error) {
        throw new Error(`dctx: cannot execute ${filename}: ${error.message}. ` +
          `Reinstall with npm install ${packageName}@${version}, or set DCTX_BIN to a dctx ${version} executable.`, { cause: error });
      }
    }
  }
  const cached = cacheBinaryPath({ env });
  try { return executable(cached); } catch (error) {
    if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') {
      throw new Error(`dctx: cannot execute cached binary ${cached}: ${error.message}. ` +
        `Set DCTX_BIN to a dctx ${version} executable` +
        `${process.platform === 'win32' ? '.' : ' or restore executable permission on the cache file (chmod 755).'}`, { cause: error });
    }
  }
  const install = Object.hasOwn(optionalDependencies, packageName)
    ? `Install the matching platform package in the same project: npm install ${packageName}@${version}. ` +
      `npm install @tejl/dctx normally includes it as an optional dependency. ` : '';
  throw new Error(`dctx: no ${version} binary for ${platform}. ${install}` +
    `${install ? 'Or obtain' : 'Obtain'} dctx ${version} from https://github.com/Eris-Margeta/dctx/releases/tag/v${version}, ` +
    `verify its SHA256SUMS, and set DCTX_BIN to the executable's absolute path or place it at ${cached}` +
    `${process.platform === 'win32' ? '.' : ' with executable permission (chmod 755).'} No binary is downloaded automatically.`);
}

module.exports = { resolveBinary, cacheBinaryPath };
