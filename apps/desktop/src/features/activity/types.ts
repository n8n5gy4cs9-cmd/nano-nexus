import type { ActivitySnapshot, AgentStatus, AgentView, ConflictView } from "@/bindings";

export type { ActivitySnapshot, AgentView, ObserveView } from "@/bindings";

/** `4.2M`, `13.5k`, `812`. */
export function formatTokens(n: number): string {
  if (n < 1000) return String(Math.round(n));
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(n < 10_000_000 ? 1 : 0)}M`;
}

/** `3h 00m`, `12m 05s`, `42s` (whole seconds, for the stats strip). */
export function formatUptime(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s >= 3600) return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
  if (s >= 60) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  return `${s}s`;
}

export const STATUS_LABEL: Record<AgentStatus, string> = {
  running: "Running",
  idle: "Idle",
  stalled: "Stalled",
  errored: "Errored",
};

/** CSS variable (token) of each status colour. */
export const STATUS_VAR: Record<AgentStatus, string> = {
  running: "--act-running",
  idle: "--act-idle",
  stalled: "--act-stalled",
  errored: "--act-errored",
};

export interface Attention {
  kind: "conflict" | "errored" | "stalled";
  id: string;
  title: string;
  detail: string;
  /** Agent ids to jump to when the item is clicked. */
  targets: string[];
}

/** What needs the owner's eyes: conflicts first, then errors, then stalls. */
export function attentionItems(snap: ActivitySnapshot): Attention[] {
  const out: Attention[] = snap.conflicts.map((c: ConflictView) => ({
    kind: "conflict" as const,
    id: `conflict:${c.path_label}`,
    title: c.path_label,
    detail: `${c.agents.length} agents are editing this file`,
    targets: c.agents,
  }));
  const byLabel = (a: AgentView) => ({ id: a.id, title: a.label });
  for (const a of snap.agents.filter((x) => x.status === "errored")) {
    out.push({ kind: "errored", ...byLabel(a), detail: `${a.errors} error${a.errors === 1 ? "" : "s"}`, targets: [a.id] });
  }
  for (const a of snap.agents.filter((x) => x.status === "stalled")) {
    out.push({ kind: "stalled", ...byLabel(a), detail: "No activity for a few minutes", targets: [a.id] });
  }
  return out;
}

/** Time of day for the footer (`4:48:23 PM`). */
export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit", second: "2-digit" });
}
