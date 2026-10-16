// TypeScript fixture for SPEC-MAP-MODE sections 5 and 6.2.
// Used by parser fixture parity tests; imports are never executed.
// Covers typed declarations, arrows, classes and vitest blocks.
import { it } from 'vitest';
import type { Input as Data } from './types';
export { Input } from './types';
/** A worker interface. */
export interface Worker { run(value: Data): Data; }
export type Result = Data;
/** Process one input. Extra detail. */
export function process(value: Data): Data { return value; }
export const arrow = (value: Data): Data => process(value);
export class Service {
  run(value: Data): Data { return arrow(value); }
}
it('processes', () => { process({}); });
