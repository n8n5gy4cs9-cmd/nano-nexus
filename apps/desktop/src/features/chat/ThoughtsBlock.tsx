import { ChevronRight } from "lucide-react";
import { useEffect, useState } from "react";

import { cn } from "@/lib/cn";
import { formatDuration } from "@/lib/format";

/** The model's thinking: collapsed by default, with a live timer while it is still thinking. */
export function ThoughtsBlock({ text, streaming, startedAt }: { text: string; streaming: boolean; startedAt?: number | undefined }) {
  const [open, setOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!streaming) return;
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, [streaming]);
  const elapsed = startedAt ? now - startedAt : undefined;
  return (
    <div className="my-2 border-l-2 border-transparent pl-3 [border-image:var(--nexus-gradient)_1]">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-1 text-small text-muted hover:text-text"
      >
        <ChevronRight className={cn("size-3.5 transition-transform", open && "rotate-90")} />
        <span className={cn(streaming && "animate-pulse")}>{streaming ? "Thinking…" : "Thoughts"}</span>
        {elapsed !== undefined ? <span className="tnum font-mono text-caption text-faint">{formatDuration(elapsed)}</span> : null}
      </button>
      {open ? <p className="mt-1 text-small whitespace-pre-wrap text-muted">{text}</p> : null}
    </div>
  );
}
