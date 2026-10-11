import { Trash2, X } from "lucide-react";
import { useEffect, useRef } from "react";

import { IconButton, Input } from "@/components/ui";
import { cn } from "@/lib/cn";
import { ACTIVITY_KINDS, useActivity, visibleEntries, type ActivityKind } from "@/stores/activity";

const TONE: Record<ActivityKind, string> = {
  chat: "text-text",
  tool: "text-accent-2",
  runtime: "text-muted",
  command: "text-faint",
  error: "text-danger",
};

const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour12: false });

/** A simple live log of what the app is doing. Hidden until opened (⌘J or the title bar button). */
export function ConsolePanel() {
  const { entries, shown, query, toggle, toggleKind, setQuery, clear } = useActivity();
  const rows = visibleEntries(entries, shown, query);
  const endRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [rows.length]);
  return (
    <section aria-label="Activity console" className="flex h-56 shrink-0 flex-col border-t border-border bg-code">
      <div className="flex items-center gap-2 border-b border-border px-2 py-1">
        <span className="text-small font-semibold">Activity</span>
        <div className="flex gap-1">
          {ACTIVITY_KINDS.map((k) => (
            <button
              key={k.kind}
              type="button"
              aria-pressed={shown.includes(k.kind)}
              onClick={() => toggleKind(k.kind)}
              className={cn(
                "rounded-full border border-border px-2 text-caption",
                shown.includes(k.kind) ? "bg-surface-3 text-text" : "text-faint hover:bg-surface-2",
              )}
            >
              {k.label}
            </button>
          ))}
        </div>
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter…"
          aria-label="Filter activity"
          className="ml-auto h-6 w-48 text-small"
        />
        <IconButton aria-label="Clear activity" size="sm" onClick={clear}>
          <Trash2 />
        </IconButton>
        <IconButton aria-label="Close console" size="sm" onClick={toggle}>
          <X />
        </IconButton>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-2 py-1 font-mono text-caption leading-5">
        {rows.length === 0 ? (
          <p className="text-faint">Nothing yet. Ask something and the steps show up here.</p>
        ) : (
          rows.map((r) => (
            <div key={r.id} className={cn("whitespace-pre-wrap break-words", TONE[r.kind])}>
              <span className="text-faint">{time(r.at)} </span>
              {r.text}
            </div>
          ))
        )}
        <div ref={endRef} />
      </div>
    </section>
  );
}
