import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import type { InstalledModel } from "@/bindings";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

const KEYS = { light: "duo.light_model_id", strong: "duo.strong_model_id" } as const;
const SETTINGS_KEY = ["settings", "duo"] as const;

function ModelSelect({ label, hint, value, models, onChange }: { label: string; hint: string; value: string; models: InstalledModel[]; onChange: (id: string) => void }) {
  return (
    <label className="flex min-w-48 flex-1 flex-col gap-1">
      <span className="text-small font-semibold">{label}</span>
      <select className="h-9 rounded-control border border-border bg-surface px-2 text-body" value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">Active model</option>
        {models.map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </select>
      <span className="text-caption text-muted">{hint}</span>
    </label>
  );
}

/** Duo assistant (v0.1): choose which installed model answers easy questions and which answers hard ones. */
export function DuoPanel({ models }: { models: InstalledModel[] }) {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: SETTINGS_KEY, queryFn: api.settings.get, retry: false });
  const read = (key: string) => String((settings.data as unknown as Record<string, unknown> | undefined)?.[key] ?? "");
  const usable = models.filter((m) => m.file_exists);
  const save = (key: string, id: string) =>
    void api.settings
      .set(key, id)
      .then(() => qc.invalidateQueries({ queryKey: SETTINGS_KEY }))
      .catch((e: unknown) => toast.error(toIpcException(e).message));
  if (usable.length === 0) return null;
  return (
    <section aria-label="Duo assistant" className="flex flex-col gap-3 rounded-card border border-border bg-surface p-4 shadow-1">
      <div>
        <h2 className="text-body font-semibold">Duo assistant</h2>
        <p className="text-small text-muted">Duo uses the light model for easy questions and the strong model for hard ones. Only one runs at a time, so the first answer after a switch is slower.</p>
      </div>
      <div className="flex flex-wrap gap-3">
        <ModelSelect label="Light model (easy)" hint="Quick questions, short chat." value={read(KEYS.light)} models={usable} onChange={(id) => save(KEYS.light, id)} />
        <ModelSelect label="Strong model (hard)" hint="Code, long text, big attachments, Think harder." value={read(KEYS.strong)} models={usable} onChange={(id) => save(KEYS.strong, id)} />
      </div>
    </section>
  );
}
