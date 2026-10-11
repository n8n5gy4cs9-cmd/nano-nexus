import { useMemo } from "react";

import { cn } from "@/lib/cn";
import { parseUnifiedDiff, toSplitRows, type DiffLine } from "@/lib/diff";

export type DiffMode = "unified" | "split";

const TONE: Record<DiffLine["kind"], string> = {
  context: "",
  add: "bg-success/10",
  del: "bg-danger/10",
  hunk: "bg-surface-2 text-faint",
};
const MARK: Record<DiffLine["kind"], string> = { context: " ", add: "+", del: "−", hunk: "" };

function Num({ n }: { n: number | null }) {
  return <span className="tnum w-10 shrink-0 select-none px-1 text-right text-faint">{n ?? ""}</span>;
}

function Cell({ line }: { line: DiffLine | null }) {
  if (!line) return <div className="min-h-5 flex-1 bg-surface-2/40" />;
  return (
    <div className={cn("flex min-h-5 min-w-0 flex-1", TONE[line.kind])}>
      <Num n={line.kind === "add" ? line.newNo : (line.oldNo ?? line.newNo)} />
      <span className="w-4 shrink-0 select-none text-center text-faint">{MARK[line.kind]}</span>
      <span className="min-w-0 flex-1 whitespace-pre-wrap break-all pr-2">{line.text}</span>
    </div>
  );
}

/** Unified or side-by-side view of a unified diff (monospace, themed through tokens). */
export function DiffViewer({ diff, mode }: { diff: string; mode: DiffMode }) {
  const lines = useMemo(() => parseUnifiedDiff(diff), [diff]);
  const rows = useMemo(() => (mode === "split" ? toSplitRows(lines) : []), [lines, mode]);
  if (lines.length === 0) return <p className="p-3 text-small text-muted">No line changes to show.</p>;
  return (
    <div role="table" aria-label="Diff" className="overflow-x-auto rounded-card border border-border bg-surface font-mono text-small leading-5">
      {mode === "unified"
        ? lines.map((l, i) =>
            l.kind === "hunk" ? (
              <div key={i} className={cn("px-2", TONE.hunk)}>
                {l.text}
              </div>
            ) : (
              <div key={i} className={cn("flex", TONE[l.kind])}>
                <Num n={l.oldNo} />
                <Num n={l.newNo} />
                <span className="w-4 shrink-0 select-none text-center text-faint">{MARK[l.kind]}</span>
                <span className="min-w-0 flex-1 whitespace-pre-wrap break-all pr-2">{l.text}</span>
              </div>
            ),
          )
        : rows.map((r, i) =>
            r.hunk ? (
              <div key={i} className={cn("px-2", TONE.hunk)}>
                {r.hunk.text}
              </div>
            ) : (
              <div key={i} className="flex divide-x divide-border">
                <Cell line={r.left} />
                <Cell line={r.right} />
              </div>
            ),
          )}
    </div>
  );
}
