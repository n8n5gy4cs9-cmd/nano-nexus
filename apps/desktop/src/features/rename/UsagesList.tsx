import { useQuery } from "@tanstack/react-query";

import { Skeleton } from "@/components/ui";
import { api } from "@/lib/api";

/** Every place a symbol is used, with a one-line preview (the "occurrence drawer"). */
export function UsagesList({ sessionId, symbolId }: { sessionId: string; symbolId: string }) {
  const usages = useQuery({ queryKey: ["rename", "usages", sessionId, symbolId], queryFn: () => api.rename.usages(sessionId, symbolId), retry: false });
  if (usages.isPending) return <Skeleton className="h-10 w-full" />;
  if (usages.isError) return <p className="text-small text-danger">Could not load the usages.</p>;
  const list = usages.data ?? [];
  if (list.length === 0) return <p className="text-small text-muted">No other usages found.</p>;
  return (
    <ul className="flex max-h-64 flex-col gap-1 overflow-y-auto font-mono text-small">
      {list.map((u, n) => (
        <li key={`${u.path}:${u.line}:${n}`} className="rounded-control bg-surface-2 px-2 py-1">
          <div className="flex items-center justify-between gap-2 text-muted">
            <span className="truncate">
              {u.path}:{u.line}
            </span>
            <span className="shrink-0 text-caption">
              {u.is_def ? "definition" : u.kind}
              {u.ambiguous ? " · ambiguous" : ""}
            </span>
          </div>
          <pre className="truncate text-text">{u.preview}</pre>
        </li>
      ))}
    </ul>
  );
}
