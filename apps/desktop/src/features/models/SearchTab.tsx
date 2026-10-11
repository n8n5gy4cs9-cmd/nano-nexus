import { useMutation, useQuery } from "@tanstack/react-query";
import { Download, Heart, WifiOff } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import type { SearchHit } from "@/bindings";
import { Badge, Button, Input, Skeleton } from "@/components/ui";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { IpcException, toIpcException } from "@/lib/ipc";

/** Value that follows `value` after `ms` of quiet. */
export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

function Files({ repo, onStarted }: { repo: string; onStarted: () => void }) {
  const files = useQuery({ queryKey: ["models", "repo", repo], queryFn: () => api.models.repoFiles(repo), retry: false });
  const dl = useMutation({
    mutationFn: (file: string) => api.models.downloadHf(repo, file),
    onSuccess: () => {
      toast.success("Download started.");
      onStarted();
    },
    onError: (e) => toast.error(toIpcException(e).message),
  });
  if (files.isLoading) return <Skeleton className="h-10 w-full" />;
  if (files.error) return <p className="text-small text-danger">{toIpcException(files.error).message}</p>;
  const list = files.data?.files ?? [];
  if (list.length === 0) return <p className="text-small text-muted">No GGUF files in this repository.</p>;
  return (
    <ul className="flex flex-col gap-1">
      {files.data?.gated ? <li className="text-small text-warning">This repository is gated. Set the HF_TOKEN environment variable after accepting its terms.</li> : null}
      {list.map((f) => (
        <li key={f.name} className="flex items-center gap-2 rounded-chip bg-surface-2 px-2 py-1.5">
          <span className="min-w-0 flex-1 truncate text-small" title={f.name}>
            {f.name}
          </span>
          {f.quant ? <Badge>{f.quant}</Badge> : null}
          <span className="tnum w-20 text-right text-small text-muted">{formatBytes(f.bytes)}</span>
          <Button size="sm" variant="primary" disabled={dl.isPending || f.parts.length > 1} title={f.parts.length > 1 ? "Split models are not supported yet" : undefined} onClick={() => dl.mutate(f.name)}>
            <Download /> Download
          </Button>
        </li>
      ))}
    </ul>
  );
}

function Hit({ h, onStarted }: { h: SearchHit; onStarted: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <li className="rounded-card border border-border bg-surface p-3 shadow-1">
      <button type="button" className="flex w-full items-center justify-between gap-3 text-left" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <span className="min-w-0 truncate text-body font-medium">{h.id}</span>
        <span className="flex shrink-0 items-center gap-3 text-small text-muted">
          <span className="tnum">{h.downloads.toLocaleString()} downloads</span>
          <span className="tnum flex items-center gap-1">
            <Heart className="size-3.5" /> {h.likes}
          </span>
        </span>
      </button>
      {open ? (
        <div className="mt-2">
          <Files repo={h.id} onStarted={onStarted} />
        </div>
      ) : null}
    </li>
  );
}

/** Search Hugging Face for GGUF models (debounced; cached by the backend for 10 minutes). */
export function SearchTab({ onStarted }: { onStarted: () => void }) {
  const [text, setText] = useState("");
  const query = useDebounced(text.trim(), 400);
  const results = useQuery({ queryKey: ["models", "search", query], queryFn: () => api.models.search(query), enabled: query.length >= 2, retry: false });
  const offline = results.error instanceof IpcException && results.error.code === "network";
  return (
    <div className="flex flex-col gap-3">
      <Input aria-label="Search Hugging Face" placeholder="Search GGUF models on Hugging Face, e.g. “qwen coder”" value={text} onChange={(e) => setText(e.target.value)} />
      {offline ? (
        <div className="flex items-center gap-3 rounded-card border border-border bg-surface p-4">
          <WifiOff className="size-5 text-muted" />
          <div>
            <p className="text-body font-medium">You appear to be offline</p>
            <p className="text-small text-muted">Search needs the internet. The catalog and your installed models still work.</p>
          </div>
        </div>
      ) : results.error ? (
        <p className="text-small text-danger">{toIpcException(results.error).message}</p>
      ) : results.isFetching ? (
        <Skeleton className="h-20 w-full" />
      ) : query.length < 2 ? (
        <p className="text-small text-muted">Type at least two letters.</p>
      ) : (results.data ?? []).length === 0 ? (
        <p className="text-small text-muted">Nothing found for “{query}”.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {(results.data ?? []).map((h) => (
            <Hit key={h.id} h={h} onStarted={onStarted} />
          ))}
        </ul>
      )}
    </div>
  );
}
