import { create } from "zustand";

export type ActivityKind = "chat" | "tool" | "runtime" | "command" | "error";

export interface ActivityEntry {
  id: number;
  at: number;
  kind: ActivityKind;
  text: string;
}

export const ACTIVITY_KINDS: { kind: ActivityKind; label: string }[] = [
  { kind: "chat", label: "Chat" },
  { kind: "tool", label: "Tools" },
  { kind: "runtime", label: "Runtime" },
  { kind: "command", label: "Commands" },
  { kind: "error", label: "Errors" },
];

const MAX_ENTRIES = 500;
let nextId = 1;

interface ActivityState {
  /** The console is hidden until the user opens it. */
  open: boolean;
  entries: ActivityEntry[];
  /** Kinds currently shown. Raw commands are off by default because they are noisy. */
  shown: ActivityKind[];
  query: string;
  toggle: () => void;
  toggleKind: (kind: ActivityKind) => void;
  setQuery: (q: string) => void;
  clear: () => void;
}

export const useActivity = create<ActivityState>((set) => ({
  open: false,
  entries: [],
  shown: ["chat", "tool", "runtime", "error"],
  query: "",
  toggle: () => set((s) => ({ open: !s.open })),
  toggleKind: (kind) =>
    set((s) => ({ shown: s.shown.includes(kind) ? s.shown.filter((k) => k !== kind) : [...s.shown, kind] })),
  setQuery: (query) => set({ query }),
  clear: () => set({ entries: [] }),
}));

/** Add a line to the console (kept even while it is closed, newest last, capped). */
export function logActivity(kind: ActivityKind, text: string): void {
  const entry: ActivityEntry = { id: nextId++, at: Date.now(), kind, text };
  useActivity.setState((s) => ({ entries: [...s.entries.slice(-(MAX_ENTRIES - 1)), entry] }));
}

/** Entries that pass the kind and text filters. */
export function visibleEntries(entries: ActivityEntry[], shown: ActivityKind[], query: string): ActivityEntry[] {
  const q = query.trim().toLowerCase();
  return entries.filter((e) => shown.includes(e.kind) && (q === "" || e.text.toLowerCase().includes(q)));
}

const clip = (s: string, n = 240) => (s.length > n ? `${s.slice(0, n)}…` : s);

/** One readable line for a streamed chat event, or `null` for noise (text deltas). */
export function describeChatEvent(e: unknown): { kind: ActivityKind; text: string } | null {
  if (typeof e !== "object" || e === null || !("type" in e) || typeof e.type !== "string") return null;
  const o = e as Record<string, unknown>;
  switch (e.type) {
    case "started":
      return { kind: "chat", text: "answer started" };
    case "status":
      return { kind: "chat", text: String(o.text ?? "") };
    case "tool_call":
      return { kind: "tool", text: `▶ ${String(o.tool)} ${clip(String(o.args_json ?? ""))}` };
    case "tool_result":
      return { kind: o.ok === false ? "error" : "tool", text: `${o.ok === false ? "✗" : "✓"} ${String(o.tool)}: ${clip(String(o.summary ?? ""))}` };
    case "permission_needed":
      return { kind: "chat", text: `needs access to ${String(o.path)} (${String(o.needed)})` };
    case "error":
      return { kind: "error", text: `${String(o.code)}: ${String(o.message)}` };
    case "done":
      return { kind: "chat", text: `done: ${String(o.tokens_out)} tokens, ${Number(o.tps ?? 0).toFixed(1)} tok/s, first token ${String(o.ttft_ms)} ms` };
    default:
      return null; // content / reasoning deltas
  }
}
