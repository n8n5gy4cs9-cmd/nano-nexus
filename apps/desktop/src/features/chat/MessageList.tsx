import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";

import { Button } from "@/components/ui";

/** Above this many items the list is virtualized. */
export const VIRTUALIZE_ABOVE = 200;
const STICK_THRESHOLD = 80;

export interface ListItem {
  key: string;
  node: ReactNode;
}

/**
 * Chat log. Sticks to the bottom while new content arrives unless the reader scrolled up, in which
 * case a "New messages" pill appears. Long conversations are virtualized.
 */
export function MessageList({ items, version }: { items: ListItem[]; version: unknown }) {
  const scroller = useRef<HTMLDivElement | null>(null);
  const [atBottom, setAtBottom] = useState(true);
  const [unseen, setUnseen] = useState(false);
  const stick = useRef(true);
  const virtual = items.length > VIRTUALIZE_ABOVE;

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns unmemoizable functions; used locally only
  const virtualizer = useVirtualizer({
    count: virtual ? items.length : 0,
    getScrollElement: () => scroller.current,
    estimateSize: () => 120,
    overscan: 8,
  });

  const toBottom = useCallback(() => {
    const el = scroller.current;
    if (!el) return;
    if (virtual) virtualizer.scrollToIndex(items.length - 1, { align: "end" });
    el.scrollTop = el.scrollHeight;
    stick.current = true;
    setAtBottom(true);
    setUnseen(false);
  }, [virtual, virtualizer, items.length]);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < STICK_THRESHOLD;
    stick.current = near;
    setAtBottom(near);
    if (near) setUnseen(false);
  };

  // New content: follow it when the reader is at the bottom, else flag it.
  useLayoutEffect(() => {
    if (stick.current) {
      const el = scroller.current;
      if (el) el.scrollTop = el.scrollHeight;
    } else {
      setUnseen(true);
    }
  }, [version, items.length]);

  // Start at the bottom when a conversation opens.
  const first = useRef(true);
  useEffect(() => {
    if (first.current && items.length > 0) {
      first.current = false;
      toBottom();
    }
  }, [items.length, toBottom]);

  return (
    <div className="relative min-h-0 flex-1">
      <div ref={scroller} onScroll={onScroll} role="log" aria-live="polite" aria-label="Conversation" className="h-full overflow-y-auto">
        <div className="mx-auto w-full max-w-[760px] px-4 py-6">
          {virtual ? (
            <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
              {virtualizer.getVirtualItems().map((v) => {
                const item = items[v.index];
                return item ? (
                  <div
                    key={item.key}
                    data-index={v.index}
                    ref={virtualizer.measureElement}
                    style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${v.start}px)` }}
                    className="pb-6"
                  >
                    {item.node}
                  </div>
                ) : null;
              })}
            </div>
          ) : (
            <div className="flex flex-col gap-6">
              {items.map((i) => (
                <div key={i.key}>{i.node}</div>
              ))}
            </div>
          )}
        </div>
      </div>
      {!atBottom && unseen ? (
        <Button variant="secondary" size="sm" className="absolute bottom-3 left-1/2 -translate-x-1/2 rounded-full shadow-2" onClick={toBottom}>
          <ArrowDown /> New messages
        </Button>
      ) : null}
    </div>
  );
}
