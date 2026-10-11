import { create } from "zustand";

import type { ChatEvent, ErrorCode } from "@/bindings";

/** One tool call shown as a chip. `ok` is `null` while it runs. */
export interface ToolChip {
  tool: string;
  summary: string | null;
  ok: boolean | null;
  /** The call's arguments (a `report` chip carries `report_id` or `changeset_id`). */
  args?: Record<string, unknown>;
}

/** Parse a tool call's JSON arguments; anything that is not an object gives `undefined`. */
export function parseArgs(raw: string | null | undefined): Record<string, unknown> | undefined {
  if (!raw) return undefined;
  try {
    const v: unknown = JSON.parse(raw);
    return typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : undefined;
  } catch {
    return undefined;
  }
}

/** A turn that is (or just was) streaming. Lives in a store so it survives navigation. */
export interface LiveTurn {
  /** What the user typed (shown immediately, before the backend echoes it). */
  userText: string;
  assistantId: string | null;
  content: string;
  reasoning: string;
  startedAt: number;
  firstReasoningAt: number | null;
  firstContentAt: number | null;
  status: "starting" | "streaming" | "done" | "stopped" | "error";
  error: { code: ErrorCode; message: string } | null;
  statusText: string | null;
  tools: ToolChip[];
}

/** Key of the conversation a turn belongs to; `NEW` until the backend assigns an id. */
export const NEW = "new";

interface ChatStore {
  turns: Record<string, LiveTurn>;
  begin: (key: string, userText: string) => void;
  rekey: (from: string, to: string) => void;
  apply: (key: string, patch: Partial<LiveTurn>) => void;
  /** Add buffered text deltas in one update. */
  appendText: (key: string, content: string, reasoning: string) => void;
  clear: (key: string) => void;
}

export const useChat = create<ChatStore>((set) => ({
  turns: {},
  begin: (key, userText) =>
    set((s) => ({
      turns: {
        ...s.turns,
        [key]: {
          userText,
          assistantId: null,
          content: "",
          reasoning: "",
          startedAt: Date.now(),
          firstReasoningAt: null,
          firstContentAt: null,
          status: "starting",
          error: null,
          statusText: null,
          tools: [],
        },
      },
    })),
  rekey: (from, to) =>
    set((s) => {
      const t = s.turns[from];
      if (!t || from === to) return s;
      const { [from]: _gone, ...rest } = s.turns;
      void _gone;
      return { turns: { ...rest, [to]: t } };
    }),
  apply: (key, patch) =>
    set((s) => {
      const t = s.turns[key];
      return t ? { turns: { ...s.turns, [key]: { ...t, ...patch } } } : s;
    }),
  appendText: (key, content, reasoning) =>
    set((s) => {
      const t = s.turns[key];
      if (!t) return s;
      const now = Date.now();
      return {
        turns: {
          ...s.turns,
          [key]: {
            ...t,
            status: t.status === "starting" ? "streaming" : t.status,
            content: t.content + content,
            reasoning: t.reasoning + reasoning,
            firstReasoningAt: t.firstReasoningAt ?? (reasoning ? now : null),
            firstContentAt: t.firstContentAt ?? (content ? now : null),
          },
        },
      };
    }),
  clear: (key) =>
    set((s) => {
      const { [key]: _gone, ...rest } = s.turns;
      void _gone;
      return { turns: rest };
    }),
}));

// ---- delta buffering: render at most once per animation frame --------------------------------

const buffers = new Map<string, { content: string; reasoning: string }>();
let scheduled = false;

const raf: (cb: () => void) => void =
  typeof requestAnimationFrame === "function" ? (cb) => void requestAnimationFrame(cb) : (cb) => void setTimeout(cb, 16);

/** Apply everything buffered so far. Called once per frame, and before any non-text event. */
export function flushDeltas(): void {
  scheduled = false;
  for (const [key, b] of buffers) {
    if (b.content || b.reasoning) useChat.getState().appendText(key, b.content, b.reasoning);
  }
  buffers.clear();
}

/** Queue a text delta; the UI re-renders at most once per frame no matter how fast tokens arrive. */
export function bufferDelta(key: string, kind: "content" | "reasoning", delta: string): void {
  const b = buffers.get(key) ?? { content: "", reasoning: "" };
  b[kind] += delta;
  buffers.set(key, b);
  if (!scheduled) {
    scheduled = true;
    raf(flushDeltas);
  }
}

/** Move buffered text when a new chat gets its real id. */
export function rekeyBuffer(from: string, to: string): void {
  const b = buffers.get(from);
  if (b) {
    buffers.delete(from);
    buffers.set(to, b);
  }
}

/** Route one backend event into the store. */
export function handleChatEvent(getKey: () => string, e: ChatEvent): void {
  const key = getKey();
  const st = useChat.getState();
  switch (e.type) {
    case "reasoning":
      bufferDelta(key, "reasoning", e.delta);
      return;
    case "content":
      bufferDelta(key, "content", e.delta);
      return;
    case "started":
      flushDeltas();
      st.apply(key, { assistantId: e.assistant_message_id, status: "streaming" });
      return;
    case "status":
      st.apply(key, { statusText: e.text });
      return;
    case "done":
      flushDeltas();
      st.apply(key, { status: e.finish_reason === "stopped" ? "stopped" : "done", statusText: null });
      return;
    case "error":
      flushDeltas();
      st.apply(key, { status: "error", error: { code: e.code, message: e.message }, statusText: null });
      return;
    case "tool_call": {
      const t = st.turns[key];
      if (t) st.apply(key, { tools: [...t.tools, { tool: e.tool, summary: null, ok: null, args: parseArgs(e.args_json) }], statusText: toolStatus(e.tool) });
      return;
    }
    case "tool_result": {
      const t = st.turns[key];
      if (!t) return;
      const idx = t.tools.findLastIndex((c) => c.tool === e.tool && c.ok === null);
      const tools = t.tools.map((c, i) => (i === idx ? { ...c, summary: e.summary, ok: e.ok } : c));
      st.apply(key, { tools, statusText: null });
      return;
    }
    default:
      return; // permission prompts are shown by the app-level modal
  }
}

/** Status line while a tool runs. */
export function toolStatus(tool: string): string {
  const verbs: Record<string, string> = {
    read_file: "Reading a file…",
    list_dir: "Looking in a folder…",
    search_files: "Searching for files…",
    grep_text: "Searching inside files…",
    file_info: "Checking a file…",
    calculate: "Calculating…",
    now: "Checking the time…",
  };
  return verbs[tool] ?? "Working…";
}
