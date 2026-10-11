import type { ConversationSummary } from "@/bindings";

export interface ConversationGroup {
  label: string;
  items: ConversationSummary[];
}

const DAY = 86_400_000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** Pinned first, then Today / Yesterday / Previous 7 days / Older (items arrive newest first). */
export function groupConversations(items: ConversationSummary[], now: number = Date.now()): ConversationGroup[] {
  const today = startOfDay(now);
  const buckets: ConversationGroup[] = [
    { label: "Pinned", items: [] },
    { label: "Today", items: [] },
    { label: "Yesterday", items: [] },
    { label: "Previous 7 days", items: [] },
    { label: "Older", items: [] },
  ];
  for (const c of items) {
    const idx = c.pinned ? 0 : c.updated_at >= today ? 1 : c.updated_at >= today - DAY ? 2 : c.updated_at >= today - 7 * DAY ? 3 : 4;
    buckets[idx]?.items.push(c);
  }
  return buckets.filter((b) => b.items.length > 0);
}
