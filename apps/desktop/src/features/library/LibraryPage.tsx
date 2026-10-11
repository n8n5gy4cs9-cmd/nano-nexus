import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FilePlus2, FolderPlus, Library, RefreshCw, Search, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import { Badge, Button, EmptyState, Input, Progress, Skeleton, Switch } from "@/components/ui";
import { api } from "@/lib/api";
import { formatBytes, relativeTime } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";
import { IS_WEB } from "@/lib/target";

import { PassageDialog } from "./PassageDialog";
import type { KbLibrary, KbPassage, KbProgress } from "./types";

export const LIBRARIES_KEY = ["knowledge", "libraries"] as const;

/** Library page: add reference material, watch indexing, and check what the model would see. */
export function LibraryPage() {
  const qc = useQueryClient();
  const [progress, setProgress] = useState<Record<string, KbProgress>>({});
  const jobs = useRef<Record<string, string>>({});
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<KbPassage[] | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const [dropping, setDropping] = useState(false);

  const libraries = useQuery({ queryKey: LIBRARIES_KEY, queryFn: () => api.knowledge.list() });
  const refresh = () => void qc.invalidateQueries({ queryKey: LIBRARIES_KEY });
  const fail = (e: unknown) => {
    const err = toIpcException(e);
    if (err.code !== "cancelled") toast.error(err.message);
  };

  const reindex = useMutation({
    mutationFn: async (id: string) => {
      const job = crypto.randomUUID();
      jobs.current[id] = job;
      try {
        return await api.knowledge.reindex(job, id, (p) => setProgress((cur) => ({ ...cur, [id]: p })));
      } finally {
        delete jobs.current[id];
        setProgress((cur) => {
          const rest = { ...cur };
          delete rest[id];
          return rest;
        });
      }
    },
    onSuccess: (r) => {
      refresh();
      const bits = [`${r.indexed} indexed`, r.unchanged ? `${r.unchanged} unchanged` : "", r.skipped ? `${r.skipped} skipped` : "", r.failed ? `${r.failed} failed` : ""];
      toast.success(bits.filter(Boolean).join(", "));
      if (r.truncated) toast.warning("The library size limit was reached; some files were not indexed.");
    },
    onError: (e) => {
      refresh();
      fail(e);
    },
  });

  const add = useMutation({
    mutationFn: async (paths: string[]) => {
      const lib = await api.knowledge.add(null, paths, false);
      refresh();
      reindex.mutate(lib.id);
      return lib;
    },
    onError: fail,
  });

  const toggle = useMutation({
    mutationFn: (v: { id: string; enabled: boolean }) => api.knowledge.setEnabled(v.id, v.enabled),
    onSuccess: refresh,
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.knowledge.remove(id),
    onSuccess: refresh,
    onError: fail,
  });

  const addFolder = async () => {
    const p = await api.fs.pickFolder();
    if (p) add.mutate([p]);
  };
  const addFile = async () => {
    const p = await api.fs.pickAnyFile();
    if (p) add.mutate([p]);
  };

  // Drag-and-drop of files and folders (desktop only; the webview reports real paths).
  useEffect(() => {
    if (IS_WEB) return;
    let off: (() => void) | undefined;
    let alive = true;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((e) => {
          const p = e.payload;
          if (p.type === "enter" || p.type === "over") setDropping(true);
          else if (p.type === "leave") setDropping(false);
          else {
            setDropping(false);
            if (p.paths.length > 0) add.mutate(p.paths);
          }
        }),
      )
      .then((un) => {
        if (alive) off = un;
        else un();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      off?.();
    };
    // `add` is stable enough for a drop handler; re-subscribing on every render would lose events.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const runSearch = async () => {
    if (!query.trim()) return;
    try {
      setHits(await api.knowledge.search(query.trim()));
    } catch (e) {
      fail(e);
    }
  };

  const list = libraries.data ?? [];
  return (
    <div className="mx-auto flex h-full w-full max-w-4xl flex-col gap-6 overflow-y-auto p-6">
      <header>
        <h1 className="text-title font-semibold">Library</h1>
        <p className="text-small text-muted">
          Add manuals, books, and notes. Everything is indexed on this computer, and the assistant answers from them with citations. Nothing leaves your machine.
        </p>
      </header>

      <section
        className={`flex flex-wrap items-center gap-3 rounded-card border border-dashed p-4 ${dropping ? "border-accent bg-accent/5" : "border-border"}`}
        aria-label="Add to the library"
      >
        <Button onClick={() => void addFolder()} disabled={add.isPending || IS_WEB}>
          <FolderPlus /> Add folder…
        </Button>
        <Button variant="secondary" onClick={() => void addFile()} disabled={add.isPending || IS_WEB}>
          <FilePlus2 /> Add file…
        </Button>
        <span className="text-small text-muted">or drop files and folders here · md, txt, json, html, pdf, epub, docx, csv, source code</span>
      </section>

      {libraries.isLoading ? (
        <Skeleton className="h-24 w-full" />
      ) : list.length === 0 ? (
        <EmptyState icon={<Library />} title="No libraries yet" description="Add a folder of documents to ask questions about them offline." />
      ) : (
        <ul className="flex flex-col gap-3">
          {list.map((l) => (
            <LibraryRow
              key={l.id}
              lib={l}
              progress={progress[l.id]}
              busy={reindex.isPending && jobs.current[l.id] !== undefined}
              onToggle={(enabled) => toggle.mutate({ id: l.id, enabled })}
              onReindex={() => reindex.mutate(l.id)}
              onCancel={() => {
                const job = jobs.current[l.id];
                if (job) void api.knowledge.cancel(job);
              }}
              onRemove={() => remove.mutate(l.id)}
            />
          ))}
        </ul>
      )}

      <section className="flex flex-col gap-2" aria-label="Search passages">
        <h2 className="text-body font-semibold">What would the model see?</h2>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void runSearch();
          }}
        >
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Ask a question or type keywords…" aria-label="Search the libraries" />
          <Button type="submit" variant="secondary">
            <Search /> Search
          </Button>
        </form>
        {hits !== null && hits.length === 0 ? <p className="text-small text-muted">Not in the library: nothing relevant was found.</p> : null}
        <ul className="flex flex-col gap-2">
          {(hits ?? []).map((h, i) => (
            <li key={h.chunk_id}>
              <button type="button" className="w-full rounded-card border border-border p-3 text-left hover:bg-surface-2" onClick={() => setOpen(h.chunk_id)}>
                <div className="text-small font-medium">
                  [{i + 1}] {h.heading_path}
                  {h.page !== null ? ` · p. ${h.page}` : ""}
                </div>
                <div className="line-clamp-3 text-small text-muted">{h.text}</div>
              </button>
            </li>
          ))}
        </ul>
      </section>
      <PassageDialog chunkId={open} onClose={() => setOpen(null)} />
    </div>
  );
}

function LibraryRow({
  lib,
  progress,
  busy,
  onToggle,
  onReindex,
  onCancel,
  onRemove,
}: {
  lib: KbLibrary;
  progress: KbProgress | undefined;
  busy: boolean;
  onToggle: (enabled: boolean) => void;
  onReindex: () => void;
  onCancel: () => void;
  onRemove: () => void;
}) {
  const indexing = progress !== undefined || busy;
  return (
    <li className="rounded-card border border-border p-4">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium">{lib.name}</span>
            {lib.source === "bundled" ? <Badge>Built in</Badge> : null}
          </div>
          <div className="text-small text-muted">
            {lib.doc_count} documents · {lib.chunk_count} passages · {formatBytes(lib.bytes)} ·{" "}
            {lib.indexed_at ? `indexed ${relativeTime(lib.indexed_at)}` : "not indexed yet"}
          </div>
        </div>
        <Switch checked={lib.enabled} onCheckedChange={onToggle} aria-label={`Use ${lib.name}`} />
        {indexing ? (
          <Button size="sm" variant="secondary" onClick={onCancel}>
            <X /> Cancel
          </Button>
        ) : (
          <Button size="sm" variant="secondary" onClick={onReindex}>
            <RefreshCw /> Re-index
          </Button>
        )}
        {lib.source === "user" ? (
          <Button size="sm" variant="ghost" aria-label={`Remove ${lib.name}`} onClick={onRemove}>
            <Trash2 />
          </Button>
        ) : null}
      </div>
      {progress ? (
        <div className="mt-3">
          <Progress
            label={`Indexing ${lib.name}`}
            value={progress.files_total > 0 ? progress.files_done / progress.files_total : undefined}
          />
          <div className="mt-1 truncate text-small text-muted">
            {progress.files_done}/{progress.files_total} · {progress.current}
          </div>
        </div>
      ) : null}
    </li>
  );
}
