export const ZOOM_LEVELS = [0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2] as const;

/** Parse a stored zoom factor; anything outside 0.5-3 falls back to 1 (100%). */
export function parseZoom(v: unknown): number {
  const n = typeof v === "string" ? Number(v) : typeof v === "number" ? v : NaN;
  return Number.isFinite(n) && n >= 0.5 && n <= 3 ? n : 1;
}

/** Scale the whole UI (CSS `zoom` works in WKWebView, WebView2 and Chromium). */
export function applyZoom(root: HTMLElement, zoom: number): void {
  root.style.setProperty("zoom", zoom === 1 ? "" : String(zoom));
}
