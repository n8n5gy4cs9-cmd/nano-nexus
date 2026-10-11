import { useMutation, useQuery } from "@tanstack/react-query";
import { ArrowLeft, Download, Sparkles } from "lucide-react";
import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { toast } from "sonner";

import type { ExportFormat, FolderReportView } from "@/bindings";
import {
  Badge,
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  EmptyState,
  Skeleton,
  StatCard,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";

import { REPORTS_KEY } from "./AnalyzerPage";
import { BodyReport } from "./BodyReport";
import { categoryBackground, categoryLabel, download } from "./categories";
import { Treemap } from "./Treemap";

const fail = (e: unknown) => void toast.error(toIpcException(e).message);

const EXPORTS: { format: ExportFormat; label: string }[] = [
  { format: "markdown", label: "Report (Markdown)" },
  { format: "json", label: "Everything (JSON)" },
  { format: "csv_largest", label: "Largest files (CSV)" },
  { format: "csv_duplicates", label: "Duplicates (CSV)" },
  { format: "csv_junk", label: "Junk (CSV)" },
];

/** `/analyzer/:id`: a saved report. */
export function ReportPage() {
  const { id = "" } = useParams();
  const nav = useNavigate();
  const list = useQuery({ queryKey: REPORTS_KEY, queryFn: () => api.analyzer.reports(null, 100) });
  const row = list.data?.find((r) => r.id === id);
  return (
    <div className="mx-auto flex h-full w-full max-w-5xl flex-col gap-4 overflow-y-auto p-6">
      <Button variant="ghost" className="self-start" onClick={() => nav("/analyzer")}>
        <ArrowLeft /> All reports
      </Button>
      {list.isPending ? (
        <Skeleton className="h-40 w-full" />
      ) : !row ? (
        <EmptyState title="Report not found" description="It may have been deleted." />
      ) : row.kind === "folder" ? (
        <FolderReportBody id={id} />
      ) : (
        <BodyReport id={id} kind={row.kind} title={row.title} />
      )}
    </div>
  );
}

function FolderReportBody({ id }: { id: string }) {
  const nav = useNavigate();
  const q = useQuery({ queryKey: ["analyzer", "folder", id], queryFn: () => api.analyzer.folderReport(id) });
  if (q.isPending) return <Skeleton className="h-64 w-full" />;
  if (q.isError) return <EmptyState title="Could not open the report" description={toIpcException(q.error).message} />;
  return <FolderReportView view={q.data} id={id} onPlanned={() => nav("/changes")} />;
}

function FolderReportView({ view, id, onPlanned }: { view: FolderReportView; id: string; onPlanned: () => void }) {
  const r = view.report;
  const [dups, setDups] = useState(true);
  const [junk, setJunk] = useState(true);
  const exportIt = useMutation({
    mutationFn: (f: ExportFormat) => api.analyzer.exportReport(id, f),
    onSuccess: download,
    onError: fail,
  });
  const plan = useMutation({
    mutationFn: () => api.analyzer.cleanupPlan(id, dups, junk),
    onSuccess: (p) => {
      toast.success(`Planned: ${p.files} files (${formatBytes(p.bytes)}) to the Trash. Review it before anything happens.`);
      onPlanned();
    },
    onError: fail,
  });
  const safeJunk = r.junk.filter((j) => j.safe_to_trash && !j.regenerable);
  return (
    <>
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h1 className="truncate text-title font-semibold">{view.shown_root}</h1>
          <p className="text-small text-muted">
            Scanned {new Date(r.scanned_at).toLocaleString()} in {(r.duration_ms / 1000).toFixed(1)} s
            {r.truncated ? " · stopped early (very large folder)" : ""}
          </p>
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="secondary">
              <Download /> Export
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {EXPORTS.map((e) => (
              <DropdownMenuItem key={e.format} onSelect={() => exportIt.mutate(e.format)}>
                {e.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </header>

      {view.narrative ? (
        <section className="rounded-card border border-border bg-surface p-4 shadow-1" aria-label="Summary">
          <div className="mb-1 flex items-center gap-2 text-small text-muted">
            <Sparkles className="size-4" aria-hidden /> Summary written by the assistant
          </div>
          <p className="whitespace-pre-wrap text-body">{view.narrative}</p>
        </section>
      ) : null}

      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <StatCard label="Files" value={r.totals.files.toLocaleString()} hint={`${r.totals.dirs.toLocaleString()} folders`} />
        <StatCard label="Total size" value={formatBytes(r.totals.bytes)} />
        <StatCard label="Wasted by copies" value={formatBytes(r.wasted_by_duplicates)} hint={`${r.duplicates.length} groups`} />
        <StatCard label="Safe to clear" value={formatBytes(safeJunk.reduce((s, j) => s + j.bytes, 0))} hint={`${safeJunk.length} items`} />
      </div>

      <section aria-label="Where the space went" className="flex flex-col gap-2">
        <h2 className="text-lead font-semibold">Where the space went</h2>
        <Treemap root={r.treemap} />
        <ul className="flex flex-wrap gap-x-4 gap-y-1 text-small">
          {r.by_category.map((c) => (
            <li key={c.category} className="flex items-center gap-1.5">
              <span className="size-3 rounded-chip" style={{ background: categoryBackground(c.category) }} aria-hidden />
              {categoryLabel(c.category)} <span className="tnum text-muted">{formatBytes(c.bytes)}</span>
            </li>
          ))}
        </ul>
      </section>

      <section className="flex flex-wrap items-center gap-4 rounded-card border border-border bg-surface p-4">
        <div className="min-w-0 flex-1">
          <div className="text-body font-medium">Plan a cleanup</div>
          <div className="text-small text-muted">Moves extra copies (the oldest is kept) and safe junk to the Trash. You review it first; nothing is deleted for good.</div>
        </div>
        <label className="flex items-center gap-2 text-small">
          <input type="checkbox" checked={dups} onChange={(e) => setDups(e.target.checked)} /> Duplicates
        </label>
        <label className="flex items-center gap-2 text-small">
          <input type="checkbox" checked={junk} onChange={(e) => setJunk(e.target.checked)} /> Safe junk
        </label>
        <Button variant="primary" disabled={(!dups && !junk) || plan.isPending} onClick={() => plan.mutate()}>
          Plan cleanup
        </Button>
      </section>

      <Tabs defaultValue="largest">
        <TabsList>
          <TabsTrigger value="largest">Largest</TabsTrigger>
          <TabsTrigger value="dups">Duplicates ({r.duplicates.length})</TabsTrigger>
          <TabsTrigger value="junk">Junk ({r.junk.length})</TabsTrigger>
          <TabsTrigger value="naming">Names ({r.naming_issues.length})</TabsTrigger>
          <TabsTrigger value="empty">Empty ({r.empty_dirs.length + r.empty_files.length})</TabsTrigger>
        </TabsList>
        <TabsContent value="largest">
          <Rows rows={r.largest_files.map((f) => ({ key: f.rel_path, main: f.rel_path, side: formatBytes(f.bytes), tag: categoryLabel(f.category) }))} />
        </TabsContent>
        <TabsContent value="dups">
          {r.duplicates.length === 0 ? (
            <EmptyState title="No duplicates found" />
          ) : (
            <ul className="flex flex-col gap-2">
              {r.duplicates.map((g) => (
                <li key={g.hash} className="rounded-card border border-border bg-surface p-3">
                  <div className="mb-1 flex items-center gap-2 text-small">
                    <span className="tnum font-medium">
                      {g.paths.length} copies × {formatBytes(g.bytes_each)}
                    </span>
                    <Badge tone={g.confirmed ? "success" : "warning"}>{g.confirmed ? "identical" : "probably identical"}</Badge>
                  </div>
                  <ul className="text-small text-muted">
                    {g.paths.map((p) => (
                      <li key={p} className="truncate">
                        {p}
                      </li>
                    ))}
                  </ul>
                </li>
              ))}
            </ul>
          )}
        </TabsContent>
        <TabsContent value="junk">
          <Rows
            rows={r.junk.map((j) => ({
              key: j.rel_path,
              main: j.rel_path,
              side: formatBytes(j.bytes),
              tag: j.regenerable ? "rebuilt by tools" : j.safe_to_trash ? "safe to trash" : j.rule,
            }))}
          />
        </TabsContent>
        <TabsContent value="naming">
          <Rows rows={r.naming_issues.map((n) => ({ key: n.kind + n.rel_path, main: n.rel_path, side: n.detail, tag: n.kind.replace(/_/g, " ") }))} />
        </TabsContent>
        <TabsContent value="empty">
          <Rows
            rows={[...r.empty_dirs.map((p) => ({ key: "d" + p, main: p, side: "", tag: "empty folder" })), ...r.empty_files.map((p) => ({ key: "f" + p, main: p, side: "", tag: "empty file" }))]}
          />
        </TabsContent>
      </Tabs>

      {r.warnings.length > 0 ? (
        <p className="text-small text-muted">{r.warnings.length} items could not be read and were skipped.</p>
      ) : null}
    </>
  );
}

function Rows({ rows }: { rows: { key: string; main: string; side: string; tag: string }[] }) {
  if (rows.length === 0) return <EmptyState title="Nothing here" />;
  return (
    <ul className="flex flex-col divide-y divide-border rounded-card border border-border bg-surface">
      {rows.slice(0, 500).map((r) => (
        <li key={r.key} className="flex items-center gap-3 px-3 py-1.5 text-small">
          <span className="min-w-0 flex-1 truncate" title={r.main}>
            {r.main}
          </span>
          <span className="tnum shrink-0 text-muted">{r.side}</span>
          <Badge tone="neutral">{r.tag}</Badge>
        </li>
      ))}
      {rows.length > 500 ? <li className="px-3 py-1.5 text-small text-muted">…and {rows.length - 500} more (export to see all)</li> : null}
    </ul>
  );
}
