import { useMutation } from "@tanstack/react-query";
import { Search } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { Badge, Button, Input } from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

import { UsagesList } from "./UsagesList";

/** "Where is this used?": search symbols, files and folders by name, then see every usage. */
export function FindPanel({ sessionId }: { sessionId: string }) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<number | null>(null);
  const find = useMutation({
    mutationFn: (q: string) => api.rename.find(sessionId, q),
    onError: (e) => toast.error(toIpcException(e).message),
  });
  const run = () => {
    setOpen(null);
    if (query.trim()) find.mutate(query.trim());
  };
  return (
    <div className="flex flex-col gap-3">
      <div className="flex gap-2">
        <Input
          aria-label="Find a symbol, file, or folder"
          placeholder="Name of a function, class, file…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") run();
          }}
        />
        <Button onClick={run} disabled={find.isPending || !query.trim()}>
          <Search /> Find
        </Button>
      </div>
      {find.data && find.data.length === 0 ? <p className="text-small text-muted">No match.</p> : null}
      <ul className="flex flex-col gap-1.5">
        {(find.data ?? []).map((f) => (
          <li key={f.symbol} className="rounded-card border border-border bg-surface px-3 py-2">
            <button type="button" className="flex w-full flex-col text-left" onClick={() => setOpen(open === f.symbol ? null : f.symbol)}>
              <span className="flex items-center gap-2">
                <code className="text-body font-medium">{f.name}</code>
                <Badge>{f.kind}</Badge>
              </span>
              <span className="truncate text-small text-muted">
                {f.path}
                {f.line > 0 ? `:${f.line}` : ""} · {f.signature}
              </span>
              <span className="text-caption text-faint">
                {f.usages.code} in code · {f.usages.import} imports · {f.usages.string} in text · {f.usages.doc + f.usages.comment} in docs and comments
              </span>
            </button>
            {open === f.symbol ? (
              <div className="mt-2">
                <UsagesList sessionId={sessionId} symbolId={String(f.symbol)} />
              </div>
            ) : null}
          </li>
        ))}
      </ul>
    </div>
  );
}
