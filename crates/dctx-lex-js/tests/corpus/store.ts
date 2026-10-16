/** Typed store module. */
import type { Clock } from './clock';
import { EventBus, type Handler } from './bus';
export * from './shared';
export type { Snapshot } from './snapshot';

export enum Status {
  Idle = 'idle',
  Busy = 'busy',
}

export const enum Flags { A = 1, B = A << 1 }

/** Shape of an entry. */
export interface Entry<T = unknown> extends Base, Tagged<T> {
  id: string;
  value: T;
  /** Touches the entry. */
  touch(at: Date): Entry<T>;
  readonly tags?: string[];
}

export type Listener<T> = (entry: Entry<T>, previous?: Entry<T>) => void;
type Keys = keyof Entry;
type Mapped<T> = { readonly [K in keyof T]?: T[K] };

declare function external(name: string): number;

export function create(name: string): Store<string>;
export function create<T>(name: string, initial: T): Store<T>;
export function create<T>(name: string, initial?: T): Store<T> {
  return new Store<T>(name, initial as T);
}

export abstract class Repository<T extends Entry> {
  protected abstract find(id: string): Promise<T | undefined>;
  public async require(id: string): Promise<T> {
    const found = await this.find(id);
    if (found === undefined) {
      throw new Error(`missing ${id}`);
    }
    return found;
  }
}

export class Store<T> implements Iterable<T> {
  private readonly entries = new Map<string, T>();
  private listeners: Array<Listener<T>> = [];
  static readonly version: number = 2;

  constructor(private readonly name: string, initial?: T) {
    if (initial !== undefined) this.set('initial', initial);
  }

  set(key: string, value: T): this {
    this.entries.set(key, value);
    this.listeners.forEach((listen) => listen(value as unknown as Entry<T>));
    return this;
  }

  get<K extends string>(key: K): T | undefined {
    return this.entries.get(key);
  }

  onChange = (listener: Listener<T>): Unsubscribe => {
    this.listeners.push(listener);
    return () => this.off(listener);
  };

  private off(listener: Listener<T>): void {
    this.listeners = this.listeners.filter((l) => l !== listener);
  }

  *[Symbol.iterator](): Iterator<T> {
    yield* this.entries.values();
  }
}

export namespace Stores {
  export const registry: Record<string, Store<unknown>> = {};
  export function register<T>(store: Store<T>): void {
    registry[String(Date.now())] = store as Store<unknown>;
  }
}

const ratio = 10 / 2 / 5;
const generic = <T,>(value: T): T[] => [value];
export { ratio, generic };
