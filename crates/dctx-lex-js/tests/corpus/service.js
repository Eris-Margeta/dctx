#!/usr/bin/env node
'use strict';
/**
 * Order service. Handles orders.
 */
const { EventEmitter } = require('node:events');
const path = require('node:path');
const [first] = require('./list.js');
const { readFile: read, ...others } = require('node:fs/promises');
const { describe, it } = require('node:test');

const PATTERN = /^order-(\d+)\/[a-z]+$/i;
const ratio = total / count / 2;

/** Formats an order label. */
function label(order, prefix = `#${order.id}`) {
  const text = `${prefix}: ${order.items.map((item) => `${item.name} x${item.qty}`).join(', ')}`;
  return text.replace(/\s+/g, ' ').trim();
}

/** Order lifecycle events. */
class OrderService extends EventEmitter {
  #cache = new Map();
  static instances = 0;

  constructor(store) {
    super();
    this.store = store;
    OrderService.instances += 1;
  }

  /** Loads one order. */
  async load(id) {
    if (!PATTERN.test(id)) throw new TypeError(`bad id ${id}`);
    const order = await this.store.get(id);
    this.emit('loaded', order);
    return order;
  }

  get size() {
    return this.#cache.size;
  }

  handle = (event) => {
    this.emit('event', label(event));
  };

  *ids() {
    yield* this.#cache.keys();
  }
}

const helpers = {
  /** Sums totals. */
  sum(values) {
    return values.reduce((a, b) => a + b, 0);
  },
  average: (values) => helpers.sum(values) / values.length,
  'kebab-case': function () {
    return path.join('a', 'b');
  },
};

module.exports = { OrderService, label, helpers, first, read, others, ratio };

describe('OrderService', () => {
  it('labels orders', () => {
    const service = new OrderService({ get: async () => ({ id: 'order-1' }) });
    service.handle({ id: 1, items: [] });
    label({ id: 2, items: [] });
  });
  it.skip('skips', () => {});
});
