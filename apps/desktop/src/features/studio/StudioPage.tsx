import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { toast } from "sonner";
import {
  Button, Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle, Input, Progress, Select, SelectContent, SelectItem,
  SelectTrigger, SelectValue, Skeleton, Tabs, TabsContent, TabsList, TabsTrigger, Textarea,
} from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import {
  SAMPLERS, type ImageBackend, type ImageFamily, type ImageRequest, type MediaStatus, type OutputKind, type OutputRecord,
  type TtsProgress,
} from "./types";

const STATUS_KEY = ["studio", "status"] as const;
const outputsKey = (kind: OutputKind) => ["studio", "outputs", kind] as const;

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="flex min-w-0 flex-col gap-1 text-small"><span>{label}</span>{children}</label>;
}
function Panel({ title, children }: { title: string; children: ReactNode }) {
  return <section className="flex flex-col gap-3 rounded-panel border border-border bg-surface-1 p-4"><h2 className="text-title font-semibold">{title}</h2>{children}</section>;
}
const message = (e: unknown) => toIpcException(e).message;
const str = (v: unknown) => (typeof v === "string" ? v : "");
const num = (v: unknown) => (typeof v === "number" ? v : 0);

/** Poll the engine status while something may be running. */
function useMediaStatus(fast: boolean) {
  return useQuery({ queryKey: STATUS_KEY, queryFn: api.media.status, retry: false, refetchInterval: fast ? 1000 : 5000 });
}

/** Thumbnail / player source read from USB outputs (never a file:// URL). */
function useOutputData(kind: OutputKind, name: string, mime: string) {
  const q = useQuery({ queryKey: ["studio", "data", kind, name], queryFn: () => api.media.readOutput(kind, name), staleTime: Infinity, retry: false });
  return q.data ? `data:${mime};base64,${q.data}` : null;
}
const imageMime = (file: string) => (file.endsWith(".jpg") ? "image/jpeg" : file.endsWith(".webp") ? "image/webp" : "image/png");

function useDeleteOutputs(kind: OutputKind) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (names: string[]) => api.media.deleteOutputs(kind, names),
    onSuccess: (n) => { toast.success(`Moved ${n} file${n === 1 ? "" : "s"} to the app Trash.`); void qc.invalidateQueries({ queryKey: outputsKey(kind) }); },
    onError: (e) => toast.error(message(e)),
  });
}

function ConfirmDelete({ target, onClose, onConfirm, pending }: { target: OutputRecord | null; onClose: () => void; onConfirm: (r: OutputRecord) => void; pending: boolean }) {
  return <Dialog open={target !== null} onOpenChange={(o) => { if (!o) onClose(); }}><DialogContent>
    <DialogTitle>Move to Trash?</DialogTitle>
    <DialogDescription>The file and its details move to the application Trash on this USB, where they can be recovered. Nothing is permanently deleted.</DialogDescription>
    <DialogFooter><Button variant="secondary" onClick={onClose}>Keep</Button><Button disabled={pending} onClick={() => { if (target) { onConfirm(target); onClose(); } }}>Move to Trash</Button></DialogFooter>
  </DialogContent></Dialog>;
}

function EngineNotice({ status, name }: { status: MediaStatus | undefined; name: "image" | "speech" | "voice" }) {
  const e = status?.engines.find((x) => x.name === name);
  if (!status || e?.installed) return null;
  return <p role="alert" className="rounded-panel border border-border p-3 text-small text-muted">
    The {name} engine is not installed in this distribution for {status.platform}. Add its prebuilt files under <span className="font-mono">runtimes/{status.platform}/</span>; NanoNexus never downloads or builds engines on this computer.
  </p>;
}

// ---- images ----------------------------------------------------------------------------------------------

const SIZES = [512, 640, 768, 1024];

function ImageGallery({ onReuse }: { onReuse: (r: OutputRecord) => void }) {
  const list = useQuery({ queryKey: outputsKey("images"), queryFn: () => api.media.outputs("images"), retry: false });
  const del = useDeleteOutputs("images");
  const [target, setTarget] = useState<OutputRecord | null>(null);
  const [open, setOpen] = useState<OutputRecord | null>(null);
  if (list.isPending) return <Skeleton className="h-32" />;
  if (list.isError) return <p role="alert" className="text-small text-muted">Gallery unavailable: {message(list.error)}</p>;
  if (list.data.length === 0) return <p className="text-small text-muted">No images yet. Generated images are saved on the USB with their settings.</p>;
  return <>
    <ul className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">{list.data.map((r) => <li key={r.file}><Thumb record={r} onOpen={() => setOpen(r)} /></li>)}</ul>
    <Dialog open={open !== null} onOpenChange={(o) => { if (!o) setOpen(null); }}><DialogContent>
      {open ? <ImageDetail record={open} onReuse={() => { onReuse(open); setOpen(null); }} onDelete={() => { setTarget(open); setOpen(null); }} /> : null}
    </DialogContent></Dialog>
    <ConfirmDelete target={target} pending={del.isPending} onClose={() => setTarget(null)} onConfirm={(r) => del.mutate([r.file])} />
  </>;
}
function Thumb({ record, onOpen }: { record: OutputRecord; onOpen: () => void }) {
  const src = useOutputData("images", record.file, imageMime(record.file));
  return <button type="button" onClick={onOpen} className="block aspect-square w-full overflow-hidden rounded-panel border border-border bg-surface-2">
    {src ? <img src={src} alt={str(record.details.prompt) || "Generated image"} className="h-full w-full object-cover" /> : <Skeleton className="h-full w-full" />}
  </button>;
}
function ImageDetail({ record, onReuse, onDelete }: { record: OutputRecord; onReuse: () => void; onDelete: () => void }) {
  const src = useOutputData("images", record.file, imageMime(record.file));
  const d = record.details;
  return <div className="flex flex-col gap-3">
    <DialogTitle>Image</DialogTitle>
    {src ? <img src={src} alt={str(d.prompt)} className="max-h-[60vh] w-full rounded-panel object-contain" /> : <Skeleton className="h-64" />}
    <DialogDescription className="break-words">{str(d.prompt)}</DialogDescription>
    <p className="text-small text-muted">{num(d.width)}×{num(d.height)} · {num(d.steps)} steps · CFG {num(d.cfgScale)} · {str(d.sampler)} · seed {num(d.seed)} · {str(d.model)} · {(num(d.durationMs) / 1000).toFixed(1)} s</p>
    <DialogFooter>
      {src ? <a className="inline-flex items-center rounded-button border border-border px-3 py-1.5 text-small" href={src} download={record.file}>Save a copy</a> : null}
      <Button variant="secondary" onClick={onReuse}>Reuse settings</Button>
      <Button variant="ghost" onClick={onDelete}>Move to Trash</Button>
    </DialogFooter>
  </div>;
}

function ImagesTab({ status }: { status: MediaStatus | undefined }) {
  const qc = useQueryClient();
  const [req, setReq] = useState<ImageRequest>({ prompt: "", negative_prompt: "", width: 512, height: 512, steps: 20, cfg_scale: 7, seed: null, batch: 1, sampler: "euler_a" });
  const [family, setFamily] = useState<ImageFamily>("sd15");
  const [backend, setBackend] = useState<ImageBackend | "auto">("auto");
  const [note, setNote] = useState("");
  const job = useRef<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const eng = status?.image;

  const start = useMutation({
    mutationFn: () => api.media.imageStart({
      backend: backend === "auto" ? null : backend, family, steps: req.steps, cfg_scale: req.cfg_scale, sampler: req.sampler,
      threads: null, flash_attn: false, vae_tiling: req.width * req.height > 768 * 768, vae_on_cpu: false,
    }),
    onSuccess: (n) => { setNote(n); void qc.invalidateQueries({ queryKey: STATUS_KEY }); },
    onError: (e) => { setNote(message(e)); void qc.invalidateQueries({ queryKey: STATUS_KEY }); },
  });
  const stop = useMutation({ mutationFn: api.media.imageStop, onSuccess: () => void qc.invalidateQueries({ queryKey: STATUS_KEY }) });
  const generate = useMutation({
    mutationFn: async () => {
      const id = crypto.randomUUID();
      job.current = id;
      setGenerating(true);
      try { return await api.media.imageGenerate(id, { request: req, family, init_image: null, denoising_strength: null }); }
      finally { job.current = null; setGenerating(false); }
    },
    onSuccess: () => void qc.invalidateQueries({ queryKey: outputsKey("images") }),
    onError: (e) => { toast.error(message(e)); void qc.invalidateQueries({ queryKey: STATUS_KEY }); },
  });
  useEffect(() => () => { if (job.current) void api.media.cancel(job.current).catch(() => {}); }, []);
  const set = <K extends keyof ImageRequest>(k: K, v: ImageRequest[K]) => setReq((o) => ({ ...o, [k]: v }));
  const g = eng?.generation;
  const ready = eng?.ready === true;

  return <div className="flex flex-col gap-4">
    <EngineNotice status={status} name="image" />
    <Panel title="Create an image">
      <Field label="Prompt"><Textarea maxLength={4000} value={req.prompt} onChange={(e) => set("prompt", e.target.value)} placeholder="A lighthouse at dusk, oil painting" /></Field>
      <Field label="Negative prompt"><Input maxLength={4000} value={req.negative_prompt} onChange={(e) => set("negative_prompt", e.target.value)} /></Field>
      <div className="grid gap-3 sm:grid-cols-3">
        <Field label="Model family"><Select value={family} onValueChange={(v) => setFamily(v === "sdxl" ? "sdxl" : "sd15")}><SelectTrigger aria-label="Model family"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="sd15">Stable Diffusion 1.5</SelectItem><SelectItem value="sdxl">Stable Diffusion XL</SelectItem></SelectContent></Select></Field>
        <Field label="Width"><Select value={String(req.width)} onValueChange={(v) => set("width", Number(v))}><SelectTrigger aria-label="Width"><SelectValue /></SelectTrigger><SelectContent>{SIZES.map((s) => <SelectItem key={s} value={String(s)}>{s}</SelectItem>)}</SelectContent></Select></Field>
        <Field label="Height"><Select value={String(req.height)} onValueChange={(v) => set("height", Number(v))}><SelectTrigger aria-label="Height"><SelectValue /></SelectTrigger><SelectContent>{SIZES.map((s) => <SelectItem key={s} value={String(s)}>{s}</SelectItem>)}</SelectContent></Select></Field>
        <Field label="Steps"><Input type="number" min={1} max={150} value={req.steps} onChange={(e) => set("steps", Number(e.target.value))} /></Field>
        <Field label="Guidance (CFG)"><Input type="number" min={0} max={30} step={0.5} value={req.cfg_scale} onChange={(e) => set("cfg_scale", Number(e.target.value))} /></Field>
        <Field label="Sampler"><Select value={req.sampler} onValueChange={(v) => set("sampler", v)}><SelectTrigger aria-label="Sampler"><SelectValue /></SelectTrigger><SelectContent>{SAMPLERS.map((s) => <SelectItem key={s} value={s}>{s}</SelectItem>)}</SelectContent></Select></Field>
        <Field label="Seed (empty = random)"><Input type="number" value={req.seed ?? ""} onChange={(e) => set("seed", e.target.value === "" ? null : Number(e.target.value))} /></Field>
        <Field label="Engine build"><Select value={backend} onValueChange={(v) => setBackend(v === "auto" ? "auto" : (["cpu", "vulkan", "cuda", "rocm", "metal"] as const).find((b) => b === v) ?? "auto")}><SelectTrigger aria-label="Engine build"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="auto">Automatic{status?.recommended_backend ? ` (${status.recommended_backend})` : ""}</SelectItem>{["cpu", "vulkan", "cuda", "rocm", "metal"].map((b) => <SelectItem key={b} value={b}>{b}</SelectItem>)}</SelectContent></Select></Field>
      </div>
      <div className="flex flex-wrap gap-2">
        {ready ? <Button variant="secondary" disabled={generating} onClick={() => stop.mutate()}>Stop engine</Button> : <Button variant="secondary" disabled={start.isPending || eng?.load.active === true} onClick={() => start.mutate()}>Start engine</Button>}
        <Button disabled={!ready || generating || !req.prompt.trim()} onClick={() => generate.mutate()}>Generate</Button>
        {generating ? <Button variant="ghost" onClick={() => { if (job.current) void api.media.cancel(job.current); }}>Cancel (stops the engine)</Button> : null}
      </div>
      {eng?.load.active || start.isPending ? <Progress label="Loading model" value={eng && eng.load.progress > 0 ? eng.load.progress / 100 : undefined} /> : null}
      {generating ? <Progress label="Generating" value={g && g.steps > 0 ? Math.min(0.99, g.step / g.steps) : undefined} /> : null}
      <p role="status" aria-live="polite" className="text-small text-muted">
        {eng?.error ?? (note || (ready ? `Engine ready${eng?.device ? ` on ${eng.device}` : ""}.` : eng?.load.active ? eng.load.phase : "Engine stopped. Start it to load the selected image model."))}
        {generating && g ? (g.decoding ? " Decoding image…" : g.steps > 0 ? ` Step ${g.step}/${g.steps} ${g.speed}` : " Starting…") : ""}
      </p>
      <p className="text-small text-muted">Pick the image model in Models. Larger sizes use much more memory; cancelling stops the engine and the next run reloads the model.</p>
    </Panel>
    <Panel title="Gallery"><ImageGallery onReuse={(r) => setReq((o) => ({ ...o, prompt: str(r.details.prompt), negative_prompt: str(r.details.negativePrompt), width: num(r.details.width) || o.width, height: num(r.details.height) || o.height, steps: num(r.details.steps) || o.steps, cfg_scale: num(r.details.cfgScale) || o.cfg_scale, sampler: str(r.details.sampler) || o.sampler, seed: typeof r.details.seed === "number" ? r.details.seed : null }))} /></Panel>
  </div>;
}

// ---- transcription ------------------------------------------------------------------------------------------

function TranscriptList() {
  const list = useQuery({ queryKey: outputsKey("transcripts"), queryFn: () => api.media.outputs("transcripts"), retry: false });
  const del = useDeleteOutputs("transcripts");
  const [target, setTarget] = useState<OutputRecord | null>(null);
  if (list.isPending) return <Skeleton className="h-24" />;
  if (list.isError) return <p role="alert" className="text-small text-muted">Transcripts unavailable: {message(list.error)}</p>;
  if (list.data.length === 0) return <p className="text-small text-muted">No saved transcripts yet.</p>;
  return <>
    <ul className="flex flex-col gap-2">{list.data.map((r) => <li key={r.file} className="flex flex-col gap-1 rounded-panel border border-border p-3">
      <div className="flex flex-wrap items-center justify-between gap-2"><span className="text-small font-medium">{str(r.details.sourceFile) || r.file}</span><span className="text-small text-muted">{r.created_at.slice(0, 16).replace("T", " ")} · {str(r.details.language)}</span></div>
      <p className="max-h-32 overflow-auto whitespace-pre-wrap text-small">{str(r.details.text)}</p>
      <div className="flex gap-2"><Button variant="secondary" onClick={() => void navigator.clipboard.writeText(str(r.details.text)).then(() => toast.success("Copied"))}>Copy</Button><Button variant="ghost" onClick={() => setTarget(r)}>Move to Trash</Button></div>
    </li>)}</ul>
    <ConfirmDelete target={target} pending={del.isPending} onClose={() => setTarget(null)} onConfirm={(r) => del.mutate([r.file])} />
  </>;
}

function TranscribeTab({ status }: { status: MediaStatus | undefined }) {
  const qc = useQueryClient();
  const [language, setLanguage] = useState("auto");
  const [translate, setTranslate] = useState(false);
  const [pct, setPct] = useState<number | null>(null);
  const [text, setText] = useState("");
  const job = useRef<string | null>(null);
  const run = useMutation({
    mutationFn: async () => {
      const path = await api.media.pickAudio();
      if (!path) return null;
      const id = crypto.randomUUID();
      job.current = id;
      setPct(null);
      setText("");
      try { return await api.media.transcribe(id, { path, language: language.trim() || "auto", translate_to_english: translate, threads: null, save: true }, setPct); }
      finally { job.current = null; }
    },
    onSuccess: (r) => { if (r) { setText(r.text); void qc.invalidateQueries({ queryKey: outputsKey("transcripts") }); } },
    onError: (e) => toast.error(message(e)),
  });
  useEffect(() => () => { if (job.current) void api.media.cancel(job.current).catch(() => {}); }, []);
  return <div className="flex flex-col gap-4">
    <EngineNotice status={status} name="speech" />
    <Panel title="Transcribe audio">
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Language (auto or a code such as en, fi)"><Input maxLength={3} value={language} onChange={(e) => setLanguage(e.target.value.toLowerCase())} /></Field>
        <label className="flex items-center gap-2 text-small"><input type="checkbox" checked={translate} onChange={(e) => setTranslate(e.target.checked)} /> Translate to English</label>
      </div>
      <div className="flex gap-2">
        <Button disabled={run.isPending} onClick={() => run.mutate()}>Choose WAV file…</Button>
        {run.isPending ? <Button variant="ghost" onClick={() => { if (job.current) void api.media.cancel(job.current); }}>Cancel</Button> : null}
      </div>
      {run.isPending ? <Progress label="Transcribing" value={pct === null ? undefined : pct / 100} /> : null}
      {text ? <Textarea readOnly value={text} maxHeight={320} aria-label="Transcript" /> : null}
      <p className="text-small text-muted">WAV audio only for now. You are asked to allow access to the file you choose. Pick the speech model in Models. Live microphone dictation is not available yet.</p>
    </Panel>
    <Panel title="Saved transcripts"><TranscriptList /></Panel>
  </div>;
}

// ---- voice -------------------------------------------------------------------------------------------------

function ClipPlayer({ record }: { record: OutputRecord }) {
  const src = useOutputData("audio", record.file, "audio/wav");
  return src ? <audio controls src={src} className="w-full" /> : <Skeleton className="h-10" />;
}
function AudioList() {
  const list = useQuery({ queryKey: outputsKey("audio"), queryFn: () => api.media.outputs("audio"), retry: false });
  const del = useDeleteOutputs("audio");
  const [target, setTarget] = useState<OutputRecord | null>(null);
  if (list.isPending) return <Skeleton className="h-24" />;
  if (list.isError) return <p role="alert" className="text-small text-muted">Clips unavailable: {message(list.error)}</p>;
  if (list.data.length === 0) return <p className="text-small text-muted">No saved clips yet.</p>;
  return <>
    <ul className="flex flex-col gap-2">{list.data.map((r) => <li key={r.file} className="flex flex-col gap-2 rounded-panel border border-border p-3">
      <span className="text-small">{str(r.details.voice)} · {(num(r.details.durationMs) / 1000).toFixed(1)} s · {str(r.details.text).slice(0, 80)}</span>
      <ClipPlayer record={r} />
      <div><Button variant="ghost" onClick={() => setTarget(r)}>Move to Trash</Button></div>
    </li>)}</ul>
    <ConfirmDelete target={target} pending={del.isPending} onClose={() => setTarget(null)} onConfirm={(r) => del.mutate([r.file])} />
  </>;
}

function VoiceTab({ status }: { status: MediaStatus | undefined }) {
  const qc = useQueryClient();
  const voices = useQuery({ queryKey: ["studio", "voices"], queryFn: api.media.voices, retry: false });
  const [text, setText] = useState("");
  const [voice, setVoice] = useState("af_heart");
  const [speed, setSpeed] = useState(1);
  const [prog, setProg] = useState<TtsProgress | null>(null);
  const job = useRef<string | null>(null);
  const run = useMutation({
    mutationFn: async () => {
      const id = crypto.randomUUID();
      job.current = id;
      setProg(null);
      try { return await api.media.speak(id, { text, voice, speed }, setProg); } finally { job.current = null; }
    },
    onSuccess: () => void qc.invalidateQueries({ queryKey: outputsKey("audio") }),
    onError: (e) => toast.error(message(e)),
  });
  useEffect(() => () => { if (job.current) void api.media.cancel(job.current).catch(() => {}); }, []);
  const value = prog && prog.phase !== "loading" && prog.total > 0 ? Math.min(0.99, prog.done / prog.total) : undefined;
  return <div className="flex flex-col gap-4">
    <EngineNotice status={status} name="voice" />
    <Panel title="Speak text">
      <Field label="Text"><Textarea maxLength={5000} value={text} onChange={(e) => setText(e.target.value)} placeholder="Type something to hear it spoken." /></Field>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Voice"><Select value={voice} onValueChange={setVoice}><SelectTrigger aria-label="Voice"><SelectValue /></SelectTrigger><SelectContent>{(voices.data ?? [{ id: "af_heart", name: "Heart" }]).map((v) => <SelectItem key={v.id} value={v.id}>{v.name}</SelectItem>)}</SelectContent></Select></Field>
        <Field label={`Speed ${speed.toFixed(2)}×`}><Input type="range" min={0.5} max={2} step={0.05} value={speed} onChange={(e) => setSpeed(Number(e.target.value))} /></Field>
      </div>
      <div className="flex gap-2">
        <Button disabled={run.isPending || !text.trim()} onClick={() => run.mutate()}>Generate speech</Button>
        {run.isPending ? <Button variant="ghost" onClick={() => { if (job.current) void api.media.cancel(job.current); }}>Cancel</Button> : null}
      </div>
      {run.isPending ? <Progress label="Generating speech" value={value} /> : null}
      <p className="text-small text-muted">Up to 5000 characters per clip. The voice model must already be on the USB; nothing is downloaded while speaking. Pick the voice model in Models.</p>
    </Panel>
    <Panel title="Saved clips"><AudioList /></Panel>
  </div>;
}

/** Image, speech-to-text and text-to-speech workspaces (UP.041-044). */
export function StudioPage() {
  const [tab, setTab] = useState("images");
  const status = useMediaStatus(tab === "images");
  return <div className="mx-auto flex w-full max-w-4xl flex-col gap-4 p-6">
    <h1 className="text-title font-semibold">Studio</h1>
    {status.isError ? <p role="alert" className="rounded-panel border border-border p-3 text-small text-muted">{message(status.error)}</p> : null}
    {status.data?.busy ? <p role="status" className="text-small text-muted">{status.data.busy} is running; other jobs wait for it.</p> : null}
    <Tabs value={tab} onValueChange={setTab}>
      <TabsList><TabsTrigger value="images">Images</TabsTrigger><TabsTrigger value="speech">Transcribe</TabsTrigger><TabsTrigger value="voice">Voice</TabsTrigger></TabsList>
      <TabsContent value="images" className="pt-4"><ImagesTab status={status.data} /></TabsContent>
      <TabsContent value="speech" className="pt-4"><TranscribeTab status={status.data} /></TabsContent>
      <TabsContent value="voice" className="pt-4"><VoiceTab status={status.data} /></TabsContent>
    </Tabs>
  </div>;
}
