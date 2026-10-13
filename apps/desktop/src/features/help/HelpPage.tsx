import { useEffect, useMemo, useState } from "react";

import { Skeleton } from "@/components/ui";

import { DocView, docKind } from "./DocView";

// The whole documentation ships inside the app (works offline, also in the web UI).
const MD = import.meta.glob("../../../../../docs/**/*.md", { query: "?raw", import: "default" }) as Record<string, () => Promise<string>>;
const HTML = import.meta.glob("../../../../../guide.html", { query: "?raw", import: "default" }) as Record<string, () => Promise<string>>;

interface Entry {
  id: string;
  group: string;
  name: string;
  ext: string;
  load: () => Promise<string>;
}

function entries(): Entry[] {
  const all = { ...MD, ...HTML };
  return Object.entries(all)
    .map(([p, load]) => {
      const rel = p.replace(/^.*?\/(docs\/|guide\.html)/, (_m, d: string) => (d === "docs/" ? "" : "guide.html"));
      const parts = rel.split("/");
      const file = parts[parts.length - 1] ?? rel;
      const ext = file.split(".").pop() ?? "";
      const group = file === "guide.html" ? "Start here" : parts.length > 1 ? (parts[0] ?? "Docs") : "Overview";
      return { id: p, group, name: file.replace(/\.(md|html)$/, ""), ext, load };
    })
    .sort((a, b) => a.group.localeCompare(b.group) || a.name.localeCompare(b.name));
}

/** Help: the full documentation, bundled with the app. Markdown and HTML render; "Raw" shows the file text. */
export function HelpPage() {
  const list = useMemo(entries, []);
  const first = list.find((e) => e.name === "GUIDE-ASK-YOUR-BOOKS") ?? list[0];
  const [current, setCurrent] = useState<string | null>(first?.id ?? null);
  const [text, setText] = useState<{ id: string; value: string } | null>(null);
  const entry = list.find((e) => e.id === current);

  useEffect(() => {
    if (!entry) return;
    let alive = true;
    void entry.load().then((value) => alive && setText({ id: entry.id, value }));
    return () => {
      alive = false;
    };
  }, [entry]);

  const groups = [...new Set(list.map((e) => e.group))];
  return (
    <div className="flex h-full min-h-0 flex-col gap-3 p-6 md:flex-row">
      <nav aria-label="Documentation" className="flex max-h-48 shrink-0 flex-col gap-3 overflow-y-auto md:max-h-none md:w-64">
        <h1 className="text-title font-semibold">Help</h1>
        {groups.map((g) => (
          <div key={g} className="flex flex-col gap-0.5">
            <h2 className="text-small font-semibold text-muted capitalize">{g}</h2>
            {list
              .filter((e) => e.group === g)
              .map((e) => (
                <button
                  key={e.id}
                  type="button"
                  aria-current={e.id === current}
                  onClick={() => setCurrent(e.id)}
                  className={`rounded-control px-2 py-1 text-left text-small ${e.id === current ? "bg-accent/15" : "hover:bg-surface-2"}`}
                >
                  {e.name}
                </button>
              ))}
          </div>
        ))}
      </nav>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {entry && text?.id === entry.id ? <DocView key={entry.id} kind={docKind(entry.ext)} source={text.value} className="flex-1" /> : <Skeleton className="h-64 w-full" />}
      </div>
    </div>
  );
}
