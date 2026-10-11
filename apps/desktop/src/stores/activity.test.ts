import { describe, expect, it } from "vitest";

import { describeChatEvent, visibleEntries, type ActivityEntry } from "./activity";

const e = (id: number, kind: ActivityEntry["kind"], text: string): ActivityEntry => ({ id, at: 0, kind, text });

describe("describeChatEvent", () => {
  it("names tool calls and results and skips text deltas", () => {
    expect(describeChatEvent({ type: "tool_call", tool: "list_dir", args_json: '{"path":"~/Downloads"}' })).toEqual({
      kind: "tool",
      text: '▶ list_dir {"path":"~/Downloads"}',
    });
    expect(describeChatEvent({ type: "tool_result", tool: "list_dir", summary: "42 items", ok: true })?.text).toBe("✓ list_dir: 42 items");
    expect(describeChatEvent({ type: "tool_result", tool: "x", summary: "boom", ok: false })?.kind).toBe("error");
    expect(describeChatEvent({ type: "content", delta: "Hel" })).toBeNull();
    expect(describeChatEvent("nope")).toBeNull();
  });
});

describe("visibleEntries", () => {
  const all = [e(1, "chat", "answer started"), e(2, "command", "→ app_info"), e(3, "error", "runtime_unavailable")];
  it("filters by kind and text", () => {
    expect(visibleEntries(all, ["chat", "error"], "").map((x) => x.id)).toEqual([1, 3]);
    expect(visibleEntries(all, ["chat", "command", "error"], "APP").map((x) => x.id)).toEqual([2]);
  });
});
