import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import type { AssistantSpec_Serialize, ValidationReport } from "@/bindings";
import {
  Badge,
  Button,
  Dialog,
  DialogDescription,
  DialogTitle,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Slider,
  Switch,
  Textarea,
} from "@/components/ui";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";
import { toIpcException } from "@/lib/ipc";
import { SheetContent } from "@/components/ui/dialog";

import { ASSISTANT_COLORS, ASSISTANT_ICONS, CATEGORIES } from "./icons";
import { TestChat } from "./TestChat";

export const BLANK_SPEC: AssistantSpec_Serialize = {
  schema_version: 1,
  id: "",
  name: "",
  icon: "sparkles",
  color: "violet",
  category: "Custom",
  tagline: "",
  description: "",
  version: "1.0.0",
  model: { thinking: false, temperature: 0.5, top_p: 0.95, min_p: 0, top_k: 40, repeat_penalty: 1, max_tokens: 900 },
  context: { attach: ["text"], max_attach_tokens: 3000, history_turns: 6 },
  permissions: { needs: [] },
  workflow: { kind: "chat" },
  tools: { allowed: [] },
  ui: { starters: [] },
  prompt: { system: "" },
};

const TOOLS = ["read_file", "list_dir", "search_files", "grep_text", "file_info", "calculate", "now"] as const;
const FILE_TOOLS = new Set(["read_file", "list_dir", "search_files", "grep_text", "file_info"]);

/** `Regex Smith!` → `regex-smith` (the id a new assistant gets until the user edits it). */
export function slugify(name: string): string {
  const s = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return s === "" || !/^[a-z]/.test(s) ? (s === "" ? "" : `assistant-${s}`) : s.slice(0, 40);
}

/** Keep `permissions.needs` in step with the tools chosen. */
export function withDerivedPermissions(spec: AssistantSpec_Serialize): AssistantSpec_Serialize {
  const reads = spec.workflow.kind === "router" && (spec.tools.allowed ?? []).some((t) => FILE_TOOLS.has(t));
  return { ...spec, permissions: { needs: reads ? ["read"] : (spec.permissions.needs ?? []).filter((n) => n !== "read") } };
}

const LEVEL_TONE = { green: "success", amber: "warning", red: "danger" } as const;

function Field({ label, error, children, hint }: { label: string; error?: string | undefined; children: React.ReactNode; hint?: string }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-small font-medium">{label}</span>
      {children}
      {hint && !error ? <span className="text-caption text-faint">{hint}</span> : null}
      {error ? <span className="text-caption text-danger">{error}</span> : null}
    </label>
  );
}

/** Create or edit an assistant: form (left), live validation and token meter, test chat (right). */
function BuilderForm({
  editId,
  initial,
  idTouchedAtStart,
  onClose,
  onSaved,
}: {
  editId: string | null;
  initial: AssistantSpec_Serialize;
  idTouchedAtStart: boolean;
  onClose: () => void;
  onSaved: (id: string) => void;
}) {
  const [spec, setSpec] = useState<AssistantSpec_Serialize>(initial);
  const [idTouched, setIdTouched] = useState(idTouchedAtStart);
  const [toml, setToml] = useState("");
  const [report, setReport] = useState<ValidationReport | null>(null);
  const [saving, setSaving] = useState(false);

  const [starterText, setStarterText] = useState<string[]>([0, 1, 2, 3].map((i) => initial.ui.starters?.[i] ?? ""));

  const patch = (f: (s: AssistantSpec_Serialize) => AssistantSpec_Serialize) => setSpec((s) => withDerivedPermissions(f(s)));

  // Live validation, debounced: render to TOML in Rust (so what is validated is what is saved).
  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(() => {
      api.assistants
        .renderToml(spec)
        .then(async (text) => {
          const r = await api.assistants.validate(text);
          if (!cancelled) {
            setToml(text);
            setReport(r);
          }
        })
        .catch(() => undefined);
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [spec]);

  const err = (path: string) => report?.errors.find((e) => e.path === path)?.message;
  const valid = report?.ok === true;
  const tools = spec.tools.allowed ?? [];

  const save = async () => {
    setSaving(true);
    try {
      const saved = await api.assistants.save(toml, editId);
      onSaved(saved.card.id);
    } catch (e) {
      toast.error(toIpcException(e).message);
    } finally {
      setSaving(false);
    }
  };

  const setStarter = (i: number, v: string) => {
    const next = starterText.slice();
    next[i] = v;
    setStarterText(next);
    patch((s) => ({ ...s, ui: { ...s.ui, starters: next.map((x) => x.trim()).filter(Boolean) } }));
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-[min(1040px,100vw)] overflow-hidden p-0" aria-describedby="builder-desc">
        <div className="flex h-full flex-col">
          <header className="border-b border-border px-5 py-3 pr-12">
            <DialogTitle>{editId ? "Edit assistant" : "New assistant"}</DialogTitle>
            <DialogDescription id="builder-desc">Write a short, clear prompt: small models follow short instructions best.</DialogDescription>
          </header>
          <div className="grid min-h-0 flex-1 gap-4 overflow-auto p-5 lg:grid-cols-[1fr_340px]">
            <div className="flex min-w-0 flex-col gap-4">
              <div className="grid gap-3 sm:grid-cols-2">
                <Field label="Name" error={err("/name")}>
                  <Input
                    value={spec.name}
                    maxLength={32}
                    onChange={(e) => {
                      const name = e.target.value;
                      patch((s) => ({ ...s, name, id: idTouched ? s.id : slugify(name) }));
                    }}
                  />
                </Field>
                <Field label="Id" error={err("/id")} hint="Lowercase letters, numbers, and dashes.">
                  <Input
                    value={spec.id}
                    onChange={(e) => {
                      setIdTouched(true);
                      patch((s) => ({ ...s, id: e.target.value }));
                    }}
                  />
                </Field>
              </div>
              <Field label="Tagline" error={err("/tagline")}>
                <Input value={spec.tagline} maxLength={100} onChange={(e) => patch((s) => ({ ...s, tagline: e.target.value }))} />
              </Field>
              <div className="grid gap-3 sm:grid-cols-2">
                <Field label="Category">
                  <Select value={spec.category} onValueChange={(v) => patch((s) => ({ ...s, category: v }))}>
                    <SelectTrigger aria-label="Category">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {CATEGORIES.map((c) => (
                        <SelectItem key={c} value={c}>
                          {c}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
                <Field label="Works by">
                  <Select
                    value={spec.workflow.kind === "pipeline" ? "pipeline" : spec.workflow.kind}
                    onValueChange={(v) => patch((s) => ({ ...s, workflow: { kind: v, ...(v === "router" ? { max_hops: 3 } : {}) } }))}
                  >
                    <SelectTrigger aria-label="Works by">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="chat">Chatting only</SelectItem>
                      <SelectItem value="router">Chatting and reading files</SelectItem>
                    </SelectContent>
                  </Select>
                </Field>
              </div>
              <fieldset className="flex flex-col gap-2">
                <legend className="text-small font-medium">Icon and color</legend>
                <div className="flex flex-wrap gap-1" role="radiogroup" aria-label="Icon">
                  {Object.entries(ASSISTANT_ICONS).map(([name, Icon]) => (
                    <button
                      key={name}
                      type="button"
                      role="radio"
                      aria-checked={spec.icon === name}
                      aria-label={name}
                      onClick={() => patch((s) => ({ ...s, icon: name }))}
                      className={cn("grid size-8 place-items-center rounded-button border text-muted hover:bg-surface-2", spec.icon === name ? "border-accent text-text" : "border-border")}
                    >
                      <Icon className="size-4" />
                    </button>
                  ))}
                </div>
                <div className="flex gap-1.5" role="radiogroup" aria-label="Color">
                  {ASSISTANT_COLORS.map((c) => (
                    <button
                      key={c}
                      type="button"
                      role="radio"
                      aria-checked={spec.color === c}
                      aria-label={c}
                      onClick={() => patch((s) => ({ ...s, color: c }))}
                      className={cn("size-6 rounded-full border-2", spec.color === c ? "border-text" : "border-transparent")}
                      style={{ backgroundImage: `var(--hue-${c})` }}
                    />
                  ))}
                </div>
              </fieldset>
              <Field label="System prompt" error={err("/prompt/system")}>
                <Textarea
                  value={spec.prompt.system}
                  maxHeight={360}
                  className="min-h-40 py-2 font-mono text-small"
                  onChange={(e) => patch((s) => ({ ...s, prompt: { system: e.target.value } }))}
                />
              </Field>
              <div className="flex items-center gap-2 text-small" aria-live="polite">
                <Badge tone={report ? LEVEL_TONE[report.level] : "neutral"}>{report?.prompt_tokens ?? 0} tokens</Badge>
                <span className="text-muted">Green up to 350, amber up to 450, red above that.</span>
              </div>
              {spec.workflow.kind === "router" ? (
                <fieldset className="flex flex-col gap-1.5">
                  <legend className="text-small font-medium">Tools it may use</legend>
                  <div className="flex flex-wrap gap-x-4 gap-y-1">
                    {TOOLS.map((t) => (
                      <label key={t} className="flex items-center gap-1.5 text-small">
                        <input
                          type="checkbox"
                          checked={tools.includes(t)}
                          onChange={(e) =>
                            patch((s) => ({ ...s, tools: { allowed: e.target.checked ? [...(s.tools.allowed ?? []), t] : (s.tools.allowed ?? []).filter((x) => x !== t) } }))
                          }
                        />
                        {t}
                      </label>
                    ))}
                  </div>
                  <span className="text-caption text-faint">Tools only read. Folders still need your permission.</span>
                </fieldset>
              ) : null}
              <fieldset className="flex flex-col gap-2">
                <legend className="text-small font-medium">Starter questions (up to 4)</legend>
                {starterText.map((s, i) => (
                  <Input key={i} aria-label={`Starter ${i + 1}`} value={s} maxLength={120} onChange={(e) => setStarter(i, e.target.value)} />
                ))}
                {err("/ui/starters") ? <span className="text-caption text-danger">{err("/ui/starters")}</span> : null}
              </fieldset>
              <fieldset className="grid gap-3 sm:grid-cols-2">
                <legend className="mb-1 text-small font-medium">Answer style</legend>
                <Field label={`Creativity: ${(spec.model.temperature ?? 0.5).toFixed(2)}`} error={err("/model/temperature")}>
                  <Slider
                    aria-label="Creativity"
                    min={0}
                    max={1.2}
                    step={0.05}
                    value={[spec.model.temperature ?? 0.5]}
                    onValueChange={([v]) => patch((s) => ({ ...s, model: { ...s.model, temperature: v ?? 0.5 } }))}
                  />
                </Field>
                <Field label={`Longest answer: ${spec.model.max_tokens ?? 900} tokens`} error={err("/model/max_tokens")}>
                  <Slider
                    aria-label="Longest answer"
                    min={64}
                    max={4096}
                    step={32}
                    value={[spec.model.max_tokens ?? 900]}
                    onValueChange={([v]) => patch((s) => ({ ...s, model: { ...s.model, max_tokens: v ?? 900 } }))}
                  />
                </Field>
                <label className="flex items-center gap-2 text-small">
                  <Switch aria-label="Think harder by default" checked={spec.model.thinking ?? false} onCheckedChange={(thinking) => patch((s) => ({ ...s, model: { ...s.model, thinking } }))} />
                  Think harder by default
                </label>
                <Field label={`Remembers the last ${spec.context.history_turns ?? 6} turns`}>
                  <Slider
                    aria-label="History turns"
                    min={0}
                    max={20}
                    step={1}
                    value={[spec.context.history_turns ?? 6]}
                    onValueChange={([v]) => patch((s) => ({ ...s, context: { ...s.context, history_turns: v ?? 6 } }))}
                  />
                </Field>
              </fieldset>
              {report && !report.ok ? (
                <ul className="rounded-card border border-danger/40 bg-danger/10 p-3 text-small" aria-label="Problems">
                  {report.errors.map((e, i) => (
                    <li key={i}>
                      {e.path ? <code className="mr-1">{e.path}</code> : null}
                      {e.message}
                    </li>
                  ))}
                </ul>
              ) : null}
            </div>
            <div className="min-h-[360px] lg:sticky lg:top-0 lg:h-[calc(100vh-190px)]">
              <TestChat toml={toml} valid={valid} starters={spec.ui.starters ?? []} />
            </div>
          </div>
          <footer className="flex justify-end gap-2 border-t border-border px-5 py-3">
            <Button onClick={onClose}>Cancel</Button>
            <Button variant="primary" disabled={!valid || saving} onClick={() => void save()}>
              Save assistant
            </Button>
          </footer>
        </div>
      </SheetContent>
    </Dialog>
  );
}

/** Loads what to edit (a saved assistant, a draft from chat, or a blank one), then shows the form. */
export function BuilderSheet({
  editId,
  initialToml = null,
  onClose,
  onSaved,
}: {
  editId: string | null;
  /** TOML to start from (for example Prompt Architect's answer). */
  initialToml?: string | null;
  onClose: () => void;
  onSaved: (id: string) => void;
}) {
  const source = useQuery({
    queryKey: ["assistant-source", editId, initialToml],
    queryFn: async (): Promise<AssistantSpec_Serialize> => {
      if (editId !== null) return (await api.assistants.get(editId)).spec as AssistantSpec_Serialize;
      if (initialToml) return withDerivedPermissions(await api.assistants.draft(initialToml));
      return BLANK_SPEC;
    },
    staleTime: 0,
    gcTime: 0,
    retry: false,
  });
  useEffect(() => {
    if (source.error) toast.error(toIpcException(source.error).message);
  }, [source.error]);
  if (source.data) {
    return <BuilderForm editId={editId} initial={source.data} idTouchedAtStart={editId !== null || Boolean(initialToml)} onClose={onClose} onSaved={onSaved} />;
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-[min(1040px,100vw)]" aria-describedby="builder-loading">
        <DialogTitle>{editId ? "Edit assistant" : "New assistant"}</DialogTitle>
        <DialogDescription id="builder-loading">{source.error ? "Could not open this assistant." : "Loading…"}</DialogDescription>
      </SheetContent>
    </Dialog>
  );
}
