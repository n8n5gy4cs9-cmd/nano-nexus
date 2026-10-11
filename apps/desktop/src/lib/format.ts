/** Human-readable byte size (1024-based), e.g. `1.1 GB`. */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

/** `1.9s`, `2m 05s`, `1h 03m`. */
export function formatDuration(ms: number): string {
  const s = Math.max(0, ms) / 1000;
  if (s < 60) return `${s.toFixed(s < 10 ? 1 : 0)}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(Math.floor(s % 60)).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

/** `just now`, `5 min ago`, `2 h ago`, `yesterday`, `3 d ago`, then a date. */
export function relativeTime(thenMs: number | null | undefined, nowMs: number = Date.now()): string {
  if (thenMs == null) return "—";
  const diff = nowMs - thenMs;
  if (diff < 45_000) return "just now";
  const min = Math.round(diff / 60_000);
  if (min < 60) return `${min} min ago`;
  const h = Math.round(diff / 3_600_000);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(diff / 86_400_000);
  if (d === 1) return "yesterday";
  if (d < 7) return `${d} d ago`;
  return new Date(thenMs).toLocaleDateString();
}
