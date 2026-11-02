import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { Button, Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle, Input, Progress, Select, SelectContent, SelectItem, SelectTrigger, SelectValue, Skeleton } from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { isModelKind, MODEL_KINDS, type ModelKind, type StudioModelInput, type StudioModelView, type StudioProgress } from "./studio-types";

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="flex min-w-0 flex-col gap-1 text-small"><span>{label}</span>{children}</label>;
}
function KindSelect({ value, onChange, disabled = false }: { value: ModelKind; onChange: (kind: ModelKind) => void; disabled?: boolean }) {
  return <Select value={value} disabled={disabled} onValueChange={(v) => { if (isModelKind(v)) onChange(v); }}>
    <SelectTrigger aria-label="Model category"><SelectValue /></SelectTrigger>
    <SelectContent>{MODEL_KINDS.map((kind) => <SelectItem key={kind} value={kind}>{kind}</SelectItem>)}</SelectContent>
  </Select>;
}
const size = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MiB`;

/** Shared USB assets. Selecting media records a choice; its engine integration is tracked separately. */
export function StudioModels() {
  const qc = useQueryClient();
  const models = useQuery({ queryKey: ["models", "studio"], queryFn: api.studio.list, retry: false });
  const [input, setInput] = useState<StudioModelInput>({ id: "", name: "", kind: "chat", role: "weights", variant: "universal" });
  const [url, setUrl] = useState("");
  const [progress, setProgress] = useState<StudioProgress | null>(null);
  const [status, setStatus] = useState("");
  const [remove, setRemove] = useState<StudioModelView | null>(null);
  const job = useRef<string | null>(null);
  const refresh = () => void qc.invalidateQueries({ queryKey: ["models"] });
  useEffect(() => () => { if (job.current) void api.studio.cancel(job.current, false).catch(() => {}); }, []);

  const transfer = useMutation({
    mutationFn: async (mode: "download" | "import") => {
      if (!input.id.trim() || !input.name.trim() || !input.role.trim()) throw new Error("Enter a model ID, name and asset role.");
      const path = mode === "import" ? await api.studio.pickAsset() : null;
      if (mode === "import" && !path) return null;
      const id = crypto.randomUUID();
      job.current = id;
      setProgress(null);
      setStatus(mode === "download" ? "Resolving model revision and checksum…" : "Preparing USB import…");
      try {
        const modelInput = { ...input, id: input.id.trim(), name: input.name.trim(), role: input.role.trim() };
        return mode === "download" ? await api.studio.download(id, url.trim(), modelInput, setProgress) : await api.studio.import(id, path ?? "", modelInput, setProgress);
      } finally { job.current = null; }
    },
    onSuccess: (model) => { if (model) setStatus(`Added ${model.name}. Identical assets share one stored file.`); refresh(); },
    onError: (error) => { setStatus(toIpcException(error).message); refresh(); },
  });
  const migration = useMutation({
    mutationFn: async (mode: "adopt" | "pack") => {
      const path = mode === "pack" ? await api.models.pickFolder() : null;
      if (mode === "pack" && !path) return null;
      const id = crypto.randomUUID();
      job.current = id;
      setProgress(null);
      setStatus("Verifying model sources; existing files will be retained…");
      try { return mode === "adopt" ? await api.studio.adoptLegacy(id, setProgress) : await api.studio.importPack(id, path ?? "", setProgress); }
      finally { job.current = null; }
    },
    onSuccess: (result) => {
      if (result) setStatus(`${result.imported.length} adopted/imported, ${result.skipped.length} already registered, ${result.failed.length} failed.${result.failed.length ? ` ${result.failed.slice(0, 10).join("; ")}${result.failed.length > 10 ? "; additional failures omitted" : ""}` : ""}`);
      refresh();
    },
    onError: (error) => { setStatus(toIpcException(error).message); refresh(); },
  });
  const action = useMutation({
    mutationFn: async (run: () => Promise<unknown>) => run(),
    onSuccess: () => { setRemove(null); refresh(); },
    onError: (error) => toast.error(toIpcException(error).message),
  });
  const stop = async (discard: boolean) => {
    if (!job.current) return;
    try {
      const cancelled = await api.studio.cancel(job.current, discard);
      setStatus(cancelled ? (discard ? "Cancelling; partial download will be removed." : "Pausing; download bytes stay on USB. Retry the same URL to resume.") : "The transfer has already finished.");
    } catch (error) { setStatus(toIpcException(error).message); }
  };
  const transferring = transfer.isPending || migration.isPending;
  const busy = transferring || action.isPending;
  const patch = (key: "id" | "name" | "role" | "variant", value: string) => setInput((old) => ({ ...old, [key]: value }));

  if (models.isPending) return <Skeleton className="h-48" />;
  if (models.isError) return <div role="status" className="rounded-panel border border-border p-4">
    <p>USB Studio storage is unavailable.</p><p className="text-small text-muted">{toIpcException(models.error).message}</p>
    <Button variant="secondary" onClick={() => void models.refetch()}>Retry</Button>
  </div>;
  return <div className="flex flex-col gap-4">
    <section className="flex flex-col gap-3 rounded-panel border border-border bg-surface-1 p-4">
      <h2 className="text-title font-semibold">Shared USB models</h2>
      <p className="text-small text-muted">Models stay in the USB folder and are shared by Mac and Windows. Add companion files to the same model ID with a distinct role, such as projector or voices.</p>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Model ID"><Input maxLength={160} value={input.id} disabled={busy} placeholder="my-coder" onChange={(e) => patch("id", e.target.value)} /></Field>
        <Field label="Display name"><Input maxLength={512} value={input.name} disabled={busy} onChange={(e) => patch("name", e.target.value)} /></Field>
        <Field label="Category"><KindSelect value={input.kind} disabled={busy} onChange={(kind) => setInput((old) => ({ ...old, kind }))} /></Field>
        <Field label="Asset role"><Input maxLength={160} value={input.role} disabled={busy} onChange={(e) => patch("role", e.target.value)} /></Field>
        <Field label="Backend variant"><Input maxLength={160} value={input.variant} disabled={busy} onChange={(e) => patch("variant", e.target.value)} /></Field>
      </div>
      <Field label="Hugging Face file URL"><Input type="url" maxLength={8192} disabled={busy} value={url} placeholder="https://huggingface.co/owner/repo/resolve/main/model.gguf" onChange={(e) => setUrl(e.target.value)} /></Field>
      <div className="flex flex-wrap gap-2">
        <Button disabled={busy || !url.trim()} onClick={() => transfer.mutate("download")}>Download / resume</Button>
        <Button variant="secondary" disabled={busy} onClick={() => transfer.mutate("import")}>Import file to USB</Button>
        {transferring ? <><Button variant="secondary" onClick={() => void stop(false)}>Pause</Button><Button variant="ghost" onClick={() => void stop(true)}>Cancel transfer</Button></> : null}
      </div>
      {transferring ? <Progress label="Model transfer" value={progress && progress.total > 0 ? progress.bytes / progress.total : undefined} /> : null}
      <p role="status" aria-live="polite" className="text-small text-muted">{status}{transferring && progress ? ` · ${progress.phase}: ${size(progress.bytes)} / ${size(progress.total)}` : ""}</p>
      <div className="flex flex-wrap gap-2">
        <Button variant="secondary" disabled={busy} onClick={() => migration.mutate("adopt")}>Adopt existing USB models</Button>
        <Button variant="secondary" disabled={busy} onClick={() => migration.mutate("pack")}>Import offline model pack</Button>
      </div>
      <p className="text-small text-muted">Unload chat before adoption. Adoption verifies registered files already inside this USB model folder without copying them. Pack import verifies and copies only missing content into shared storage. Source files remain unchanged.</p>
      <p className="text-small text-muted">Chat uses the existing runtime. Image, speech and voice selections are used by the Studio page when its engine is installed in this distribution. Format validation does not guarantee a model fits this computer.</p>
    </section>
    {(models.data ?? []).length === 0 ? <p className="p-4 text-muted">No shared Studio models yet. Download weights or import a local model to start.</p> : (models.data ?? []).map((view) => <section key={view.model.id} className="flex flex-col gap-2 rounded-panel border border-border bg-surface-1 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2"><h3 className="font-semibold">{view.model.name}</h3><span className="text-small text-muted">{view.model.kind} · {view.model.variant}{view.selected ? " · selected" : ""}</span></div>
      {!view.available ? <p role="alert" className="text-small text-muted">One or more assets are missing or changed. Re-import them before selecting this model.</p> : null}
      <ul className="flex flex-col gap-1 text-small text-muted">{view.model.assets.map(({ role, asset }) => <li key={role} className="break-all">{role}: {asset.path} · {size(asset.bytes)}<br /><span className="font-mono">SHA-256 {asset.sha256}</span></li>)}</ul>
      <div className="flex flex-wrap items-center gap-2">
        <Button disabled={busy || !view.available || view.selected} onClick={() => action.mutate(() => api.studio.select(view.model.id))}>{view.model.kind === "chat" ? "Use for chat" : "Select for workspace"}</Button>
        <Button variant="secondary" disabled={busy} onClick={() => action.mutate(() => api.studio.unload(view.model.kind))}>Unload {view.model.kind}</Button>
        <Button variant="ghost" disabled={busy || view.selected} onClick={() => { setInput({ id: view.model.id, name: view.model.name, kind: view.model.kind, variant: view.model.variant, role: "weights" }); }}>Add / repair asset</Button>
        <div className="w-32"><KindSelect value={view.model.kind} disabled={busy || view.selected} onChange={(kind) => action.mutate(() => api.studio.reclassify(view.model.id, kind))} /></div>
        <Button variant="ghost" disabled={busy || view.selected} onClick={() => setRemove(view)}>Remove entry</Button>
      </div>
    </section>)}
    <Dialog open={remove !== null} onOpenChange={(open) => { if (!open) setRemove(null); }}><DialogContent>
      <DialogTitle>Remove {remove?.model.name}?</DialogTitle><DialogDescription>This removes the model entry. Shared asset files stay on USB for other models and recovery; this action does not reclaim their disk space.</DialogDescription>
      <DialogFooter><Button variant="secondary" onClick={() => setRemove(null)}>Keep model</Button><Button disabled={action.isPending} onClick={() => { if (remove) action.mutate(() => api.studio.remove(remove.model.id)); }}>Remove entry</Button></DialogFooter>
    </DialogContent></Dialog>
  </div>;
}
