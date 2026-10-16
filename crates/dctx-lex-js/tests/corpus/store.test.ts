import { describe, it, expect } from 'vitest';
import { Store, create } from './store';

describe('Store', () => {
  it.concurrent('sets values', async () => {
    const store = create<number>('n', 1);
    store.set('a', 2);
    expect(store.get('a')).toBe(2);
  });
  it.todo('handles listeners');
  describe.skip('nested', () => {
    it('iterates', () => {
      expect([...new Store<number>('x')]).toEqual([]);
    });
  });
});
