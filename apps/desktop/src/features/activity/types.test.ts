import { describe, expect, it } from "vitest";

import type { ActivitySnapshot } from "@/bindings";

import { attentionItems, formatTokens, formatUptime } from "./types";

describe("formatters", () => {
  it("formats tokens and uptime like the dashboard", () => {
    expect(formatTokens(812)).toBe("812");
    expect(formatTokens(13_500)).toBe("14k");
    expect(formatTokens(1_500)).toBe("1.5k");
    expect(formatTokens(4_200_000)).toBe("4.2M");
    expect(formatUptime(10_800_000)).toBe("3h 00m");
    expect(formatUptime(725_000)).toBe("12m 05s");
    expect(formatUptime(-1)).toBe("0s");
  });
});

describe("attentionItems", () => {
  it("lists conflicts, then errored, then stalled agents", () => {
    const base = { tokens: { input: 0, output: 0, cache_read: 0, cache_write: 0 }, project_id: "p", parent: null, model: null, tool: null, started_ms: 0, last_ms: 0, events: 0, messages: 0, events_per_s: 0, source: "claude_code" as const };
    const snap = {
      at_ms: 0,
      conflicts: [{ path_label: "cart/Cart.tsx", agents: ["a", "b"], since_ms: 0 }],
      agents: [
        { ...base, id: "s", label: "Stalled one", status: "stalled", errors: 0 },
        { ...base, id: "e", label: "Broken one", status: "errored", errors: 2 },
        { ...base, id: "r", label: "Fine", status: "running", errors: 0 },
      ],
    } as unknown as ActivitySnapshot;
    const items = attentionItems(snap);
    expect(items.map((i) => i.kind)).toEqual(["conflict", "errored", "stalled"]);
    expect(items[1]?.detail).toBe("2 errors");
  });
});
