import { formatName, formatPrice } from "./format";

interface Line {
  item: string;
  cents: number;
  qty: number;
}

/** The greeting banner of the kiosk web screen. */
export function renderGreeting(name: string): string {
  return `Hello, ${formatName(name)}!`;
}

/** One receipt line per order line, then the total. */
export function renderReceipt(lines: Line[]): string[] {
  const rows = lines.map((l) => `${l.qty} x ${l.item}  ${formatPrice(l.cents * l.qty)}`);
  const total = lines.reduce((sum, l) => sum + l.cents * l.qty, 0);
  return [...rows, `total  ${formatPrice(total)}`];
}
