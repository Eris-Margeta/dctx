/** Price in cents to a display string, e.g. 250 -> "2.50 EUR". */
export function formatPrice(cents: number, currency = "EUR"): string {
  return `${(cents / 100).toFixed(2)} ${currency}`;
}

/** The customer's name as the screen shows it. */
export function formatName(name: string): string {
  return name.trim() === "" ? "guest" : name.trim();
}
