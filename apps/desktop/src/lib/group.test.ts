import { expect, test } from "vitest";

import type { ConversationSummary } from "@/bindings";

import { groupConversations } from "./group";

const mk = (id: string, updated_at: number, pinned = false): ConversationSummary => ({
  id,
  title: id,
  assistant_id: "nexus",
  pinned,
  updated_at,
  snippet: null,
});

test("groups by recency with pinned first", () => {
  const now = new Date(2026, 9, 11, 15, 0, 0).getTime();
  const h = 3_600_000;
  const groups = groupConversations(
    [mk("a", now - h), mk("b", now - 20 * h), mk("c", now - 3 * 86_400_000), mk("d", now - 30 * 86_400_000), mk("e", now - 40 * 86_400_000, true)],
    now,
  );
  expect(groups.map((g) => g.label)).toEqual(["Pinned", "Today", "Yesterday", "Previous 7 days", "Older"]);
  expect(groups.map((g) => g.items.map((i) => i.id))).toEqual([["e"], ["a"], ["b"], ["c"], ["d"]]);
});

test("empty buckets are omitted", () => {
  expect(groupConversations([], Date.now())).toEqual([]);
});
