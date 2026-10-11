import { Calculator, Clock, FileSearch, FileText, FolderOpen, Info, Loader2, Search, Wrench, type LucideIcon } from "lucide-react";

import { cn } from "@/lib/cn";
import type { ToolChip } from "@/stores/chat";

const ICON: Record<string, LucideIcon> = {
  read_file: FileText,
  list_dir: FolderOpen,
  search_files: FileSearch,
  grep_text: Search,
  file_info: Info,
  calculate: Calculator,
  now: Clock,
};

/** Parse a stored message's `tool_trace` (a JSON array of `{tool, summary, ok}`); bad data gives no chips. */
export function parseToolTrace(raw: string | null): ToolChip[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    return v.flatMap((x: unknown) => {
      if (typeof x !== "object" || x === null || !("tool" in x) || typeof x.tool !== "string") return [];
      const summary = "summary" in x && typeof x.summary === "string" ? x.summary : null;
      const ok = "ok" in x && typeof x.ok === "boolean" ? x.ok : null;
      return [{ tool: x.tool, summary, ok }];
    });
  } catch {
    return [];
  }
}

/** The tools an answer used: "Read notes.md", "Listed Downloads (42 items)", or a red chip when a tool failed. */
export function ToolChips({ chips }: { chips: ToolChip[] }) {
  if (chips.length === 0) return null;
  return (
    <ul className="mb-2 flex flex-wrap gap-1.5" aria-label="Tools used">
      {chips.map((c, i) => {
        const Icon = c.ok === null ? Loader2 : (ICON[c.tool] ?? Wrench);
        return (
          <li
            key={i}
            className={cn(
              "flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-1 text-small",
              c.ok === false ? "border-danger/40 bg-danger/10 text-danger" : "border-border bg-surface-2 text-muted",
            )}
          >
            <Icon className={cn("size-3.5 shrink-0", c.ok === null && "animate-spin")} />
            <span className="truncate">{c.summary ?? c.tool.replace("_", " ")}</span>
          </li>
        );
      })}
    </ul>
  );
}
