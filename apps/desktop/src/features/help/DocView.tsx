import { useState } from "react";

import { Markdown } from "@/features/chat/Markdown";
import { cn } from "@/lib/cn";

export type DocKind = "md" | "html" | "text";

/** File extension -> how to render it. */
export function docKind(ext: string): DocKind {
  const e = ext.toLowerCase();
  if (e === "md" || e === "markdown") return "md";
  if (e === "html" || e === "htm" || e === "xhtml") return "html";
  return "text";
}

/** No scripts, no network, no navigation: the page can only show what is inside it. */
const CSP = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:">`;

/** Render a document (Markdown or sandboxed HTML) with a switch to its raw text. Raw is the only view for plain text. */
export function DocView({ kind, source, className }: { kind: DocKind; source: string; className?: string }) {
  const [raw, setRaw] = useState(false);
  const canRender = kind !== "text";
  const showRaw = raw || !canRender;
  return (
    <div className={cn("flex min-h-0 flex-col gap-2", className)}>
      {canRender ? (
        <div className="flex gap-1" role="group" aria-label="View">
          {[
            { id: false, label: "Rendered" },
            { id: true, label: "Raw" },
          ].map((o) => (
            <button
              key={o.label}
              type="button"
              aria-pressed={raw === o.id}
              onClick={() => setRaw(o.id)}
              className={cn("rounded-control px-2 py-1 text-small", raw === o.id ? "bg-accent/15 text-text" : "text-muted hover:bg-surface-2")}
            >
              {o.label}
            </button>
          ))}
        </div>
      ) : null}
      {showRaw ? (
        <pre className="min-h-0 flex-1 overflow-auto rounded-card bg-surface-2 p-3 text-small whitespace-pre-wrap">{source}</pre>
      ) : kind === "md" ? (
        <div className="min-h-0 flex-1 overflow-auto rounded-card border border-border p-4">
          <Markdown source={source} />
        </div>
      ) : (
        <iframe title="Document" sandbox="" srcDoc={CSP + source} className="min-h-[50vh] w-full flex-1 rounded-card border border-border bg-white" />
      )}
    </div>
  );
}
