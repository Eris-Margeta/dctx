// JavaScript fixture for SPEC-MAP-MODE sections 5 and 6.2.
// Used by parser fixture parity tests; imports are never executed.
// Covers ES modules, CommonJS, declarations, JSDoc and node:test.
import runner, { work as assist } from './support.js';
import * as helpers from './helpers.js';
import './setup.js';
import { test, describe } from 'node:test';
export { work as shared } from './support.js';
export * from './helpers.js';
const { check: verify, equal } = require('node:assert');
const fs = require('node:fs');
/** Run work. More detail belongs outside the first sentence. */
export function run() { assist(); }
/** Named arrow documentation.
 * Second line is deliberately ignored.
 */
export const arrow = () => run();
export class Worker {
  /** Execute work. */
  execute() { return arrow(); }
}
describe('suite', () => {
  test('runs', () => { run(); verify(); });
});
