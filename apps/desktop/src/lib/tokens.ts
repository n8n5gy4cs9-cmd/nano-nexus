/** `3.1k`, `8k`, `950`. */
export function formatTokens(n: number): string {
  if (n < 1000) return String(Math.round(n));
  const k = n / 1000;
  return `${k >= 10 || Number.isInteger(k) ? Math.round(k) : k.toFixed(1)}k`;
}
