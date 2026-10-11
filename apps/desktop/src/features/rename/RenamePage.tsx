import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, FolderOpen, GitBranch, PenLine, Sparkles, Square, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { toast } from "sonner";

import { Badge, Button, EmptyState, Progress, Skeleton, Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui";
import { api } from "@/lib/api";
import { relativeTime } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";

import { FindPanel } from "./FindPanel";
import { ItemsTable } from "./ItemsTable";
import { type ItemStatus, type RenameItem, type RenamePlan, type RenamePlanOptions, type RenameProgress, isFileKind } from "./types";
import { VerifyPanel } from "./VerifyPanel";

const SESSIONS_KEY = ["rename", "sessions"] as const;
const itemsKey = (id: string) => ["rename", "items", id] as const;

const PHASE: Record<string, string> = { indexing: "Reading the project…", detecting: "Looking for naming problems…", proposing: "Thinking of better names…", done: "Done" };

const FILTERS: { value: ItemStatus | "all"; label: string }[] = [
  { value: "all", label: "All" },
  { value: "candidate", label: "No proposal" },
  { value: "proposed", label: "Proposed" },
  { value: "accepted", label: "Accepted" },
  { value: "needs_manual", label: "Needs you" },
  { value: "rejected", label: "Rejected" },
  { value: "applied", label: "Applied" },
];

const fail = (e: unknown) => {
  const err = toIpcException(e);
  if (err.code !== "cancelled") toast.error(err.message);
};

/** Rename Forge home: pick a project, or reopen a recent one. */
function StartView({ onOpen }: { onOpen: (id: string) => void }) {
  const qc = useQueryClient();
  const [progress, setProgress] = useState<RenameProgress | null>(null);
  const job = useRef<string | null>(null);
  const sessions = useQuery({ queryKey: SESSIONS_KEY, queryFn: () => api.rename.sessions(50), retry: false });

  const create = useMutation({
    mutationFn: async (root: string) => {
      const id = crypto.randomUUID();
      job.current = id;
      setProgress({ phase: "indexing", done: 0, total: 0 });
      try {
        return await api.rename.createSession(id, root, setProgress);
      } finally {
        job.current = null;
        setProgress(null);
      }
    },
    onSuccess: (s) => {
      void qc.invalidateQueries({ queryKey: SESSIONS_KEY });
      onOpen(s.id);
    },
    onError: fail,
  });
  const remove = useMutation({ mutationFn: (id: string) => api.rename.deleteSession(id), onSuccess: () => void qc.invalidateQueries({ queryKey: SESSIONS_KEY }), onError: fail });
  const pick = async () => {
    const root = await api.fs.pickFolder();
    if (root) create.mutate(root);
  };

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-6 p-6">
      <h1 className="text-title font-semibold">Rename Forge</h1>
      <section className="rounded-card border border-border bg-surface p-5 shadow-1">
        <h2 className="text-lead font-semibold">Fix confusing names in a project</h2>
        <p className="mt-1 text-small text-muted">
          Pick a code project. NanoNexus finds names that are unclear or inconsistent, suggests better ones, and updates every place they are used. You review everything before any file changes, and you can undo it.
        </p>
        {progress ? (
          <div className="mt-4 flex flex-col gap-2">
            <Progress label="Indexing" value={progress.total > 0 ? progress.done / progress.total : undefined} />
            <div className="flex items-center justify-between text-small text-muted">
              <span>{PHASE[progress.phase] ?? progress.phase}</span>
              <Button size="sm" variant="ghost" onClick={() => job.current && void api.rename.cancel(job.current)}>
                <Square /> Stop
              </Button>
            </div>
          </div>
        ) : (
          <Button variant="primary" className="mt-4" onClick={() => void pick()}>
            <FolderOpen /> Choose a project folder
          </Button>
        )}
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-lead font-semibold">Recent projects</h2>
        {sessions.isPending ? (
          <Skeleton className="h-14 w-full" />
        ) : (sessions.data ?? []).length === 0 ? (
          <EmptyState icon={<PenLine />} title="No projects yet" description="Choose a folder to start." />
        ) : (
          <ul className="flex flex-col gap-1">
            {(sessions.data ?? []).map((s) => (
              <li key={s.id} className="flex items-center gap-2 rounded-card border border-border bg-surface px-3 py-2">
                <button type="button" className="min-w-0 flex-1 text-left" onClick={() => onOpen(s.id)}>
                  <div className="truncate text-body font-medium">{s.root}</div>
                  <div className="text-small text-muted">
                    {s.issues} naming issue{s.issues === 1 ? "" : "s"} · {s.files} files · {relativeTime(s.created_at)}
                  </div>
                </button>
                <Button variant="ghost" aria-label={`Remove ${s.root} from the list`} onClick={() => remove.mutate(s.id)}>
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

/** One project: propose names, review them, plan the change, verify. */
function SessionView({ sessionId, onBack }: { sessionId: string; onBack: () => void }) {
  const qc = useQueryClient();
  const nav = useNavigate();
  const [filter, setFilter] = useState<ItemStatus | "all">("all");
  const [progress, setProgress] = useState<RenameProgress | null>(null);
  const [plan, setPlan] = useState<RenamePlan | null>(null);
  const [options, setOptions] = useState<RenamePlanOptions>({ docs: true, comments: true, ambiguous: false, public_api: false });
  const job = useRef<string | null>(null);

  const session = useQuery({ queryKey: SESSIONS_KEY, queryFn: () => api.rename.sessions(50), retry: false, select: (all) => all.find((s) => s.id === sessionId) });
  const items = useQuery({ queryKey: itemsKey(sessionId), queryFn: () => api.rename.items(sessionId), retry: false });
  const all = items.data ?? [];
  const refresh = () => void qc.invalidateQueries({ queryKey: itemsKey(sessionId) });
  const replace = (next: RenameItem) => qc.setQueryData<RenameItem[]>(itemsKey(sessionId), (old) => (old ?? []).map((i) => (i.symbol_id === next.symbol_id ? next : i)));

  const propose = useMutation({
    mutationFn: async (v: { ids: string[] | null; again: boolean }) => {
      const id = crypto.randomUUID();
      job.current = id;
      setProgress({ phase: "proposing", done: 0, total: 0 });
      try {
        return await api.rename.propose(id, sessionId, v.ids, v.again, setProgress);
      } finally {
        job.current = null;
        setProgress(null);
        refresh();
      }
    },
    onSuccess: (n) => toast.success(n === 0 ? "Nothing left to propose." : `${n} new name${n === 1 ? "" : "s"} suggested.`),
    onError: fail,
  });
  const update = useMutation({
    mutationFn: (v: { item: RenameItem; finalName?: string; status?: ItemStatus; includeStrings?: boolean }) =>
      api.rename.updateItem(sessionId, v.item.symbol_id, { finalName: v.finalName ?? null, status: v.status ?? null, includeStrings: v.includeStrings ?? null }),
    onSuccess: (next) => {
      replace(next);
      if (next.error) toast.error(next.error);
    },
    onError: fail,
  });
  const makePlan = useMutation({ mutationFn: () => api.rename.plan(sessionId, options), onSuccess: setPlan, onError: fail });

  const symbols = all.filter((i) => !isFileKind(i.kind));
  const files = all.filter((i) => isFileKind(i.kind));
  const visible = (list: RenameItem[]) => (filter === "all" ? list : list.filter((i) => i.status === filter));
  const accepted = all.filter((i) => i.status === "accepted").length;
  const proposed = all.filter((i) => i.status === "proposed");
  const todo = all.filter((i) => i.status === "candidate").length;

  const table = (list: RenameItem[]) => (
    <ItemsTable
      items={visible(list)}
      sessionId={sessionId}
      onEdit={(item, name) => update.mutate({ item, finalName: name })}
      onStatus={(item, status) => update.mutate({ item, status })}
      onStrings={(item, on) => update.mutate({ item, includeStrings: on })}
      onRetry={(item) => propose.mutate({ ids: [item.symbol_id], again: true })}
    />
  );
  const s = session.data;

  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-4 p-6">
      <div className="flex items-center gap-2">
        <Button variant="ghost" onClick={onBack}>
          <ArrowLeft /> Projects
        </Button>
        <h1 className="min-w-0 flex-1 truncate text-lead font-semibold">{s?.root ?? "Rename Forge"}</h1>
        {s ? (
          <span className="text-small text-muted">
            {s.langs.join(", ") || "no code found"} · {s.files} files · {s.symbols} names
          </span>
        ) : null}
      </div>

      <div className="flex flex-wrap items-center gap-2 rounded-card border border-border bg-surface p-3">
        {progress ? (
          <div className="flex min-w-60 flex-1 flex-col gap-1">
            <Progress label="Proposing names" value={progress.total > 0 ? progress.done / progress.total : undefined} />
            <span className="text-small text-muted">
              {PHASE[progress.phase] ?? progress.phase}
              {progress.total > 0 ? ` ${progress.done}/${progress.total}` : ""}
            </span>
          </div>
        ) : (
          <Button variant="primary" disabled={todo === 0 || propose.isPending} onClick={() => propose.mutate({ ids: null, again: false })}>
            <Sparkles /> Suggest names{todo > 0 ? ` (${todo})` : ""}
          </Button>
        )}
        {progress ? (
          <Button variant="danger" onClick={() => job.current && void api.rename.cancel(job.current)}>
            <Square /> Stop
          </Button>
        ) : null}
        <Button disabled={proposed.length === 0} onClick={() => proposed.forEach((item) => update.mutate({ item, status: "accepted" }))}>
          Accept all suggestions ({proposed.length})
        </Button>
        <select aria-label="Show" className="ml-auto h-8 rounded-control border border-border bg-surface px-2 text-small" value={filter} onChange={(e) => setFilter(e.target.value as ItemStatus | "all")}>
          {FILTERS.map((f) => (
            <option key={f.value} value={f.value}>
              {f.label}
            </option>
          ))}
        </select>
      </div>

      {items.isPending ? (
        <Skeleton className="h-40 w-full" />
      ) : all.length === 0 ? (
        <EmptyState icon={<PenLine />} title="No naming problems found" description="This project's names look consistent. You can still search for a name in the Find tab." />
      ) : null}

      <Tabs defaultValue="symbols">
        <TabsList>
          <TabsTrigger value="symbols">Names ({symbols.length})</TabsTrigger>
          <TabsTrigger value="files">Files and folders ({files.length})</TabsTrigger>
          <TabsTrigger value="find">Find</TabsTrigger>
          <TabsTrigger value="verify">Check</TabsTrigger>
        </TabsList>
        <TabsContent value="symbols">{table(symbols)}</TabsContent>
        <TabsContent value="files">{table(files)}</TabsContent>
        <TabsContent value="find">
          <FindPanel sessionId={sessionId} />
        </TabsContent>
        <TabsContent value="verify">
          <VerifyPanel sessionId={sessionId} />
        </TabsContent>
      </Tabs>

      <section aria-label="Apply" className="flex flex-col gap-3 rounded-card border border-border bg-surface p-4 shadow-1">
        <h2 className="text-lead font-semibold">Apply the accepted renames</h2>
        <div className="flex flex-wrap gap-x-5 gap-y-1 text-small">
          {(
            [
              ["docs", "Update docs (Markdown)"],
              ["comments", "Update comments"],
              ["ambiguous", "Include uses I'm not sure about"],
              ["public_api", "Include public API names"],
            ] as const
          ).map(([key, label]) => (
            <label key={key} className="flex items-center gap-2">
              <input type="checkbox" checked={options[key]} onChange={(e) => setOptions((o) => ({ ...o, [key]: e.target.checked }))} />
              {label}
            </label>
          ))}
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="primary" disabled={accepted === 0 || makePlan.isPending} onClick={() => makePlan.mutate()}>
            Preview {accepted} rename{accepted === 1 ? "" : "s"}
          </Button>
          <span className="text-small text-muted">Nothing changes until you apply the plan on the Changes page. After applying, choose the project again to refresh the list.</span>
        </div>
        {plan ? (
          <div className="flex flex-col gap-2 rounded-card border border-border bg-surface-2 p-3">
            <div className="text-body">
              Plan ready: {plan.edits} edit{plan.edits === 1 ? "" : "s"}.
            </div>
            {plan.dirty.length > 0 ? (
              <div className="flex items-start gap-2 text-small text-warning">
                <GitBranch className="mt-0.5 size-4 shrink-0" />
                <span>
                  {plan.dirty.length} file{plan.dirty.length === 1 ? " has" : "s have"} uncommitted git changes ({plan.dirty.slice(0, 3).join(", ")}
                  {plan.dirty.length > 3 ? "…" : ""}). Commit first so you can compare or revert with git too.
                </span>
              </div>
            ) : null}
            {plan.skipped.length > 0 ? (
              <details className="text-small text-muted">
                <summary>
                  <Badge tone="warning">{plan.skipped.length} skipped</Badge>
                </summary>
                <ul className="mt-1 list-disc pl-5">
                  {plan.skipped.map((x, n) => (
                    <li key={n}>{x}</li>
                  ))}
                </ul>
              </details>
            ) : null}
            <div>
              <Button variant="primary" onClick={() => nav("/changes")}>
                Review on the Changes page
              </Button>
            </div>
          </div>
        ) : null}
      </section>
    </div>
  );
}

/** Rename Forge (M09): find unclear names, get suggestions, review, plan, and verify. */
export function RenamePage() {
  const [params, setParams] = useSearchParams();
  const sessionId = params.get("session");
  const open = (id: string | null) => setParams(id ? { session: id } : {});
  return sessionId ? <SessionView key={sessionId} sessionId={sessionId} onBack={() => open(null)} /> : <StartView onOpen={open} />;
}
