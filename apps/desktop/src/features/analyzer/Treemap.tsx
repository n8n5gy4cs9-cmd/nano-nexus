import { ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";

import type { TreeNode } from "@/bindings";
import { cn } from "@/lib/cn";
import { formatBytes } from "@/lib/format";
import { squarify } from "@/lib/treemap";

import { categoryBackground, categoryLabel } from "./categories";

const W = 1000;
const H = 520;

/** Click a folder to zoom in; the breadcrumb zooms back out. Tiny tiles are labelled only when they fit. */
export function Treemap({ root }: { root: TreeNode }) {
  const [path, setPath] = useState<TreeNode[]>([]);
  const current = path[path.length - 1] ?? root;
  const tiles = useMemo(
    () => squarify(current.children.map((c) => ({ node: c, weight: c.bytes })), { x: 0, y: 0, w: W, h: H }),
    [current],
  );
  return (
    <div className="flex flex-col gap-2">
      <nav aria-label="Treemap path" className="flex flex-wrap items-center gap-1 text-small text-muted">
        <button type="button" className="rounded-control px-1 hover:bg-surface-2" onClick={() => setPath([])}>
          {root.name || "Folder"}
        </button>
        {path.map((n, i) => (
          <span key={n.rel_path} className="flex items-center gap-1">
            <ChevronRight className="size-3" aria-hidden />
            <button type="button" className="rounded-control px-1 hover:bg-surface-2" onClick={() => setPath(path.slice(0, i + 1))}>
              {n.name}
            </button>
          </span>
        ))}
      </nav>
      <div className="relative w-full overflow-hidden rounded-card border border-border" style={{ aspectRatio: `${W} / ${H}` }} role="group" aria-label="Folder sizes">
        {tiles.map(({ item, rect }) => {
          const n = item.node;
          const zoomable = n.children.length > 0 && !n.other;
          const showLabel = rect.w > 90 && rect.h > 34;
          return (
            <button
              key={n.rel_path + n.name}
              type="button"
              disabled={!zoomable}
              onClick={() => setPath([...path, n])}
              title={`${n.name} · ${formatBytes(n.bytes)} · ${n.files} files · ${categoryLabel(n.category)}`}
              className={cn(
                "absolute overflow-hidden border border-bg p-1 text-left text-caption text-accent-fg",
                zoomable ? "cursor-zoom-in hover:brightness-110" : "cursor-default",
              )}
              style={{
                left: `${(rect.x / W) * 100}%`,
                top: `${(rect.y / H) * 100}%`,
                width: `${(rect.w / W) * 100}%`,
                height: `${(rect.h / H) * 100}%`,
                background: categoryBackground(n.category),
              }}
            >
              {showLabel ? (
                <>
                  <span className="block truncate font-medium">{n.name}</span>
                  <span className="tnum block truncate opacity-80">{formatBytes(n.bytes)}</span>
                </>
              ) : null}
            </button>
          );
        })}
      </div>
    </div>
  );
}
