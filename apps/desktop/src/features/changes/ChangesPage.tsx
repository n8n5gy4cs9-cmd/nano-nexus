import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FilePlus2, FileText, FolderInput, History, Trash2, Undo2 } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import {
  Badge,
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  EmptyState,
  Skeleton,
} from "@/components/ui";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";
import { formatBytes, relativeTime } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";

import type {
  ChangePreview,
  ChangeSetSummary,
  ChangeStatus,
  FilePreview,
  MissingGrant,
  UndoChoice,
  UndoOutcome,
} from "../permissions/types";
import { DiffViewer, type DiffMode } from "./DiffViewer";

const LIST_KEY = ["changes", "list"] as const;
const fail = (e: unknown) => void toast.error(toIpcException(e).message);

const STATUS: Record<ChangeStatus, { label: string; tone: "neutral" | "accent" | "success" | "warning" | "danger" }> = {
  planned: { label: "Ready to review", tone: "accent" },
  applied: { label: "Applied", tone: "success" },
  rolled_back: { label: "Rolled back", tone: "warning" },
  undone: { label: "Undone", tone: "neutral" },
  discarded: { label: "Discarded", tone: "neutral" },
  failed: { label: "Failed", tone: "danger" },
};

const KIND_ICON: Record<string, typeof FileText> = { create: FilePlus2, move: FolderInput, trash: Trash2 };

const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
const dirName = (p: string) => p.replace(/[\\/][^\\/]*$/, "") || p;

function ChangeList({ items, selected, onSelect }: { items: ChangeSetSummary[]; selected: string | null; onSelect: (id: string) => void }) {
  return (
    <ul className="flex flex-col gap-1">
      {items.map((c) => {
        const s = STATUS[c.status];
        return (
          <li key={c.id}>
            <button
              type="button"
              onClick={() => onSelect(c.id)}
              aria-current={c.id === selected}
              className={cn(
                "w-full rounded-card border px-3 py-2 text-left transition-colors hover:bg-surface-2",
                c.id === selected ? "border-accent bg-surface-2" : "border-transparent",
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="truncate text-body font-medium">{c.title}</span>
                <Badge tone={s.tone}>{s.label}</Badge>
              </div>
              <div className="mt-0.5 text-small text-muted">
                {c.origin} · {relativeTime(c.created_at)}
                {c.totals ? ` · ${c.totals.files} file${c.totals.files === 1 ? "" : "s"}` : ""}
              </div>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function FileRow({ file, active, onClick }: { file: FilePreview; active: boolean; onClick: () => void }) {
  const Icon = KIND_ICON[file.kind] ?? FileText;
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn("flex w-full items-center gap-2 rounded-chip px-2 py-1.5 text-left hover:bg-surface-2", active && "bg-surface-2")}
    >
      <Icon className="size-4 shrink-0 text-muted" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-body" title={file.path}>
          {baseName(file.path)}
          {file.to ? ` → ${baseName(file.to)}` : ""}
        </span>
        <span className="block truncate text-caption text-faint">{file.kind}</span>
      </span>
      {file.insertions > 0 ? <span className="tnum text-small text-success">+{file.insertions}</span> : null}
      {file.deletions > 0 ? <span className="tnum text-small text-danger">−{file.deletions}</span> : null}
    </button>
  );
}

function MissingGrants({ missing, onGranted }: { missing: MissingGrant[]; onGranted: () => void }) {
  const allow = useMutation({
    mutationFn: async (m: MissingGrant) => api.fs.grantAdd(dirName(m.path), m.needed === "read" ? "read" : m.needed === "write" ? "read_write" : "full", true),
    onSuccess: onGranted,
    onError: fail,
  });
  return (
    <div className="rounded-card border border-warning/40 bg-warning/10 p-3">
      <p className="text-body font-medium">Permission needed before this can be applied</p>
      <ul className="mt-1 flex flex-col gap-1">
        {missing.map((m) => (
          <li key={`${m.op_index}-${m.path}`} className="flex items-center justify-between gap-2 text-small">
            <span className="truncate" title={m.path}>
              {m.needed} · {m.path}
            </span>
            <Button size="sm" disabled={allow.isPending} onClick={() => allow.mutate(m)}>
              Allow folder
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function UndoConflictDialog({
  outcome,
  onClose,
  onConfirm,
  busy,
}: {
  outcome: UndoOutcome;
  onClose: () => void;
  onConfirm: (choices: Record<number, UndoChoice>) => void;
  busy: boolean;
}) {
  const [choices, setChoices] = useState<Record<number, UndoChoice>>({});
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle>Some files changed since</DialogTitle>
        <DialogDescription>Choose what to do with each file. Keeping the current version leaves it as it is now.</DialogDescription>
        <ul className="mt-3 flex max-h-64 flex-col gap-2 overflow-auto">
          {outcome.conflicts.map((c) => {
            const choice = choices[c.op_index] ?? "keep_current";
            return (
              <li key={c.op_index} className="rounded-card border border-border p-2">
                <div className="truncate text-body" title={c.path}>
                  {baseName(c.path)}
                </div>
                <div className="text-small text-muted">{c.message}</div>
                <div className="mt-1.5 flex gap-1.5">
                  {(["keep_current", "restore_anyway"] as const).map((k) => (
                    <Button
                      key={k}
                      size="sm"
                      variant={choice === k ? "primary" : "secondary"}
                      aria-pressed={choice === k}
                      onClick={() => setChoices((x) => ({ ...x, [c.op_index]: k }))}
                    >
                      {k === "keep_current" ? "Keep current" : "Restore anyway"}
                    </Button>
                  ))}
                </div>
              </li>
            );
          })}
        </ul>
        <DialogFooter>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            disabled={busy}
            onClick={() => onConfirm(Object.fromEntries(outcome.conflicts.map((c) => [c.op_index, choices[c.op_index] ?? "keep_current"])))}
          >
            Undo
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Detail({ summary, onChanged }: { summary: ChangeSetSummary; onChanged: () => void }) {
  const qc = useQueryClient();
  const [mode, setMode] = useState<DiffMode>("unified");
  const [sel, setSel] = useState(0);
  const [conflicts, setConflicts] = useState<UndoOutcome | null>(null);
  const preview = useQuery<ChangePreview>({
    queryKey: ["changes", "preview", summary.id, summary.status],
    queryFn: () => api.changes.preview(summary.id),
    enabled: summary.status === "planned" || summary.status === "applied",
  });
  const done = () => {
    void qc.invalidateQueries({ queryKey: ["changes"] });
    onChanged();
  };
  const apply = useMutation({
    mutationFn: () => api.changes.apply(summary.id),
    onSuccess: () => {
      toast.success("Changes applied. You can undo them from this page.");
      done();
    },
    onError: (e) => {
      fail(e);
      done();
    },
  });
  const discard = useMutation({ mutationFn: () => api.changes.discard(summary.id), onSuccess: done, onError: fail });
  const undo = useMutation({
    mutationFn: (decisions: { op_index: number; choice: UndoChoice }[]) => api.changes.undo(summary.id, decisions),
    onSuccess: (o, decisions) => {
      if (o.conflicts.length > 0 && decisions.length === 0) {
        setConflicts(o);
        return;
      }
      setConflicts(null);
      if (o.problems.length > 0) toast.warning(`Undo finished with ${o.problems.length} problem(s): ${o.problems[0]}`);
      else toast.success(`Restored ${o.restored} file${o.restored === 1 ? "" : "s"}.`);
      done();
    },
    onError: fail,
  });

  const p = preview.data;
  const file = p?.files[Math.min(sel, Math.max(0, (p?.files.length ?? 1) - 1))];
  const s = STATUS[summary.status];
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-3">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="truncate text-lead font-semibold">{summary.title}</h2>
          <p className="text-small text-muted">
            {summary.origin} · {relativeTime(summary.created_at)} · <Badge tone={s.tone}>{s.label}</Badge>
          </p>
        </div>
        <div className="flex gap-2">
          {summary.status === "planned" ? (
            <>
              <Button disabled={discard.isPending || apply.isPending} onClick={() => discard.mutate()}>
                Discard
              </Button>
              <Button variant="primary" disabled={!(p && p.conflicts.length === 0 && p.missing_grants.length === 0 && p.files.length > 0) || apply.isPending} onClick={() => apply.mutate()}>
                Apply changes
              </Button>
            </>
          ) : null}
          {summary.status === "applied" ? (
            <Button disabled={undo.isPending} onClick={() => undo.mutate([])}>
              <Undo2 /> Undo
            </Button>
          ) : null}
        </div>
      </header>
      {summary.error ? <p className="rounded-card border border-danger/40 bg-danger/10 p-3 text-small text-danger">{summary.error}</p> : null}
      {preview.isLoading ? <Skeleton className="h-32 w-full" /> : null}
      {p ? (
        <>
          <p className="text-small text-muted">
            {p.totals.files} file{p.totals.files === 1 ? "" : "s"} · <span className="text-success">+{p.totals.insertions}</span>{" "}
            <span className="text-danger">−{p.totals.deletions}</span> · {formatBytes(p.totals.bytes)}
          </p>
          {summary.status === "planned" && p.missing_grants.length > 0 ? (
            <MissingGrants missing={p.missing_grants} onGranted={() => void preview.refetch()} />
          ) : null}
          {summary.status === "planned" && p.conflicts.length > 0 ? (
            <div className="rounded-card border border-danger/40 bg-danger/10 p-3">
              <p className="text-body font-medium">This plan can't be applied as it is</p>
              <ul className="mt-1 list-disc pl-5 text-small">
                {p.conflicts.map((c) => (
                  <li key={`${c.op_index}-${c.kind}`}>
                    {baseName(c.path)}: {c.message}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          <div className="grid min-h-0 gap-3 lg:grid-cols-[240px_1fr]">
            <div className="flex flex-col gap-0.5">
              {p.files.map((f) => (
                <FileRow key={f.op_index} file={f} active={file?.op_index === f.op_index} onClick={() => setSel(p.files.indexOf(f))} />
              ))}
            </div>
            <div className="min-w-0">
              {file ? (
                <>
                  <div className="mb-2 flex items-center justify-between gap-2">
                    <span className="truncate text-small text-muted" title={file.path}>
                      {file.path}
                    </span>
                    <div className="flex gap-1">
                      {(["unified", "split"] as const).map((m) => (
                        <Button key={m} size="sm" variant={mode === m ? "primary" : "ghost"} aria-pressed={mode === m} onClick={() => setMode(m)}>
                          {m === "unified" ? "Unified" : "Side by side"}
                        </Button>
                      ))}
                    </div>
                  </div>
                  {file.diff ? (
                    <DiffViewer diff={file.diff} mode={mode} />
                  ) : (
                    <p className="rounded-card border border-border bg-surface p-3 text-small text-muted">
                      {file.binary
                        ? `Binary file (${formatBytes(file.size_before ?? 0)} → ${formatBytes(file.size_after ?? 0)}).`
                        : file.kind === "move"
                          ? `Move to ${file.to ?? "?"}`
                          : file.kind === "trash"
                            ? "Moves to the Trash. You can restore it with Undo."
                            : "No text changes."}
                    </p>
                  )}
                </>
              ) : null}
            </div>
          </div>
        </>
      ) : null}
      {conflicts ? (
        <UndoConflictDialog
          outcome={conflicts}
          busy={undo.isPending}
          onClose={() => setConflicts(null)}
          onConfirm={(choices) =>
            undo.mutate(Object.entries(choices).map(([op, choice]) => ({ op_index: Number(op), choice })))
          }
        />
      ) : null}
    </div>
  );
}

/** Changes page (UI-DESIGN §6.9): every planned or applied change set, with preview, apply, and undo. */
export function ChangesPage() {
  const qc = useQueryClient();
  const list = useQuery({ queryKey: LIST_KEY, queryFn: () => api.changes.list(100) });
  const [selected, setSelected] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    api.changes
      .onUpdate(() => void qc.invalidateQueries({ queryKey: ["changes"] }))
      .then((u) => {
        if (cancelled) u();
        else off = u;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      off?.();
    };
  }, [qc]);
  const makeTest = useMutation({
    mutationFn: api.changes.makeTest,
    onSuccess: (id) => {
      setSelected(id);
      void qc.invalidateQueries({ queryKey: ["changes"] });
    },
    onError: fail,
  });
  const items = list.data ?? [];
  const current = items.find((c) => c.id === selected) ?? items[0] ?? null;
  return (
    <div className="flex h-full flex-col gap-4 p-6">
      <header className="flex items-start justify-between gap-3">
        <div>
          <h1 className="text-lead font-semibold">Changes</h1>
          <p className="text-body text-muted">Nothing changes on disk until you apply it here, and every apply can be undone.</p>
        </div>
        {import.meta.env.DEV ? (
          <Button size="sm" disabled={makeTest.isPending} onClick={() => makeTest.mutate()}>
            New test change set
          </Button>
        ) : null}
      </header>
      {list.isLoading ? (
        <Skeleton className="h-40 w-full" />
      ) : items.length === 0 ? (
        <EmptyState icon={<History />} title="No changes yet" description="When an assistant plans edits, renames, or new files, they show up here for you to review." />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-4 md:flex-row">
          <aside className="w-full shrink-0 overflow-auto md:w-72">
            <ChangeList items={items} selected={current?.id ?? null} onSelect={setSelected} />
          </aside>
          {current ? <div className="min-h-0 min-w-0 flex-1 overflow-auto"><Detail key={current.id} summary={current} onChanged={() => void list.refetch()} /></div> : null}
        </div>
      )}
    </div>
  );
}
