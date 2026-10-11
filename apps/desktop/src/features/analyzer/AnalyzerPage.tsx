import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FileSearch, FolderOpen, FolderSearch, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";

import type { ScanProgress } from "@/bindings";
import { Badge, Button, EmptyState, Progress, Skeleton } from "@/components/ui";
import { api } from "@/lib/api";
import { formatBytes, relativeTime } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";

export const REPORTS_KEY = ["analyzer", "reports"] as const;

const KIND_LABEL: Record<string, string> = { folder: "Folder", file: "File", secrets: "Secrets", logs: "Logs", data: "Data", rename: "Rename" };

const PHASE: Record<string, string> = { walking: "Scanning…", hashing: "Looking for duplicates…", summarizing: "Summing up…" };

/** Analyzer home: pick a folder or file, watch the scan, and reopen saved reports. */
export function AnalyzerPage() {
  const nav = useNavigate();
  const qc = useQueryClient();
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [deep, setDeep] = useState(false);
  const job = useRef<string | null>(null);

  const reports = useQuery({ queryKey: REPORTS_KEY, queryFn: () => api.analyzer.reports(null, 100) });

  const scan = useMutation({
    mutationFn: async (root: string) => {
      const id = crypto.randomUUID();
      job.current = id;
      setProgress({ files_seen: 0, bytes_seen: 0, current_dir: root, phase: "walking" });
      try {
        return await api.analyzer.analyze(id, root, deep ? "full" : "fast", setProgress);
      } finally {
        job.current = null;
        setProgress(null);
      }
    },
    onSuccess: (reportId) => {
      void qc.invalidateQueries({ queryKey: REPORTS_KEY });
      nav(`/analyzer/${reportId}`);
    },
    onError: (e) => {
      const err = toIpcException(e);
      if (err.code !== "cancelled") toast.error(err.message);
    },
  });

  const pickFolder = async () => {
    const p = await api.fs.pickFolder();
    if (p) scan.mutate(p);
  };

  const pickFile = useMutation({
    mutationFn: async () => {
      const path = await api.fs.pickAnyFile();
      if (!path) return null;
      return api.analyzer.fileReport(path);
    },
    onSuccess: (id) => {
      if (id) {
        void qc.invalidateQueries({ queryKey: REPORTS_KEY });
        nav(`/analyzer/${id}`);
      }
    },
    onError: (e) => void toast.error(toIpcException(e).message),
  });

  const remove = useMutation({
    mutationFn: (id: string) => api.analyzer.remove(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: REPORTS_KEY }),
  });

  return (
    <div className="mx-auto flex h-full w-full max-w-4xl flex-col gap-6 overflow-y-auto p-6">
      <header>
        <h1 className="text-title font-semibold">Analyzer</h1>
        <p className="text-muted">Scan a folder or a file. The numbers are computed on your computer; the AI only explains them.</p>
      </header>

      <section className="rounded-card border border-border bg-surface p-5 shadow-1">
        {progress ? (
          <div className="flex flex-col gap-3" role="status">
            <div className="text-body font-medium">{PHASE[progress.phase] ?? "Working…"}</div>
            <Progress label="Scan progress" value={undefined} />
            <div className="tnum text-small text-muted">
              {progress.files_seen.toLocaleString()} files · {formatBytes(progress.bytes_seen)}
            </div>
            <div className="truncate text-caption text-faint">{progress.current_dir}</div>
            <div>
              <Button variant="secondary" onClick={() => job.current && void api.analyzer.cancel(job.current)}>
                Cancel
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-4">
            <div className="flex flex-wrap gap-3">
              <Button onClick={() => void pickFolder()} disabled={pickFile.isPending}>
                <FolderOpen /> Scan a folder
              </Button>
              <Button variant="secondary" onClick={() => pickFile.mutate()} disabled={pickFile.isPending}>
                <FileSearch /> Look at a file
              </Button>
            </div>
            <label className="flex items-center gap-2 text-small text-muted">
              <input type="checkbox" checked={deep} onChange={(e) => setDeep(e.target.checked)} />
              Compare whole files when looking for duplicates (slower, certain)
            </label>
          </div>
        )}
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lead font-semibold">Saved reports</h2>
        {reports.isPending ? (
          <Skeleton className="h-16 w-full" />
        ) : (reports.data ?? []).length === 0 ? (
          <EmptyState icon={<FolderSearch />} title="No reports yet" description="Scan a folder to see where the space went." />
        ) : (
          <ul className="flex flex-col gap-1">
            {(reports.data ?? []).map((r) => (
              <li key={r.id} className="flex items-center gap-2 rounded-card border border-border bg-surface px-3 py-2">
                <button type="button" className="min-w-0 flex-1 text-left" onClick={() => nav(`/analyzer/${r.id}`)}>
                  <div className="flex items-center gap-2">
                    <Badge tone="neutral">{KIND_LABEL[r.kind] ?? r.kind}</Badge>
                    <span className="truncate text-body font-medium">{r.title}</span>
                  </div>
                  <div className="truncate text-small text-muted">
                    {r.summary ? `${r.summary} · ` : ""}
                    {relativeTime(r.created_at)}
                  </div>
                </button>
                <Button variant="ghost" aria-label={`Delete ${r.title}`} onClick={() => remove.mutate(r.id)}>
                  <Trash2 />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
