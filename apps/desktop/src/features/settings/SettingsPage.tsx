import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { toast } from "sonner";

import type { Settings } from "@/bindings";
import { Skeleton } from "@/components/ui";
import { api, type SettingValue } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { IS_WEB } from "@/lib/target";
import { ACCENTS, isAccent, isThemePreference } from "@/lib/theme";
import { useUi } from "@/stores/ui";

import { AboutSection } from "./AboutSection";
import { Row, Section, SelectRow, TextRow, ToggleRow } from "./fields";

export const SETTINGS_KEY = ["settings"] as const;

/** Kinds of notifications the app can raise, for the per-kind switches. */
export const NOTIFICATION_KINDS: { kind: string; label: string }[] = [
  { kind: "download", label: "Model downloads" },
  { kind: "job", label: "Finished jobs (scans, rename proposals)" },
  { kind: "watch", label: "Folder watches" },
  { kind: "permission", label: "Access requests" },
  { kind: "share", label: "LAN Share (pairing)" },
];

const MB = 1024 * 1024;

type Bag = Record<string, unknown>;
const str = (b: Bag, k: string, d = ""): string => (typeof b[k] === "string" ? (b[k] as string) : d);
const num = (b: Bag, k: string, d = 0): number => (typeof b[k] === "number" ? (b[k] as number) : d);
const bool = (b: Bag, k: string, d = false): boolean => (typeof b[k] === "boolean" ? (b[k] as boolean) : d);

/** A whole number >= `min`, or `null` when the text is not one. */
function parseWhole(text: string, min = 0): number | null {
  const n = Number(text.trim());
  return text.trim() !== "" && Number.isInteger(n) && n >= min ? n : null;
}

/** Settings: every key of the app's settings, grouped (DATA-AND-IPC §1). */
export function SettingsPage() {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: SETTINGS_KEY, queryFn: api.settings.get, retry: false });
  const setTheme = useUi((s) => s.setTheme);
  const setAccent = useUi((s) => s.setAccent);

  const save = useMutation({
    mutationFn: ({ key, value }: { key: string; value: SettingValue }) => api.settings.set(key, value),
    onSuccess: (next: Settings) => qc.setQueryData(SETTINGS_KEY, next),
    onError: (e) => toast.error(toIpcException(e).message),
  });
  const set = (key: string, value: SettingValue) => save.mutate({ key, value });
  const setWhole = (key: string, text: string, min = 0) => {
    const n = parseWhole(text, min);
    if (n === null) toast.error(`Enter a whole number${min > 0 ? ` of at least ${min}` : ""}.`);
    else set(key, n);
  };

  if (settings.isPending) return <Skeleton className="m-6 h-64 w-[min(42rem,100%)]" />;
  if (!settings.data) return <p className="p-6 text-muted">Settings are not available here.</p>;
  const s = settings.data as unknown as Bag;
  const kinds = (s["notifications.kinds"] ?? {}) as Record<string, boolean>;
  const extraDirs = Array.isArray(s["models.extra_dirs"]) ? (s["models.extra_dirs"] as string[]) : [];
  const threads = s["runtime.threads"];

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 p-6">
      <h1 className="text-title font-semibold">Settings</h1>

      <Section title="Appearance">
        <SelectRow
          label="Theme"
          value={str(s, "ui.theme", "system")}
          options={[
            { value: "system", label: "Match system" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
          onChange={(v) => {
            if (isThemePreference(v)) setTheme(v);
            set("ui.theme", v);
          }}
        />
        <SelectRow
          label="Accent color"
          value={str(s, "ui.accent", "violet")}
          options={ACCENTS.map((a) => ({ value: a, label: a.charAt(0).toUpperCase() + a.slice(1) }))}
          onChange={(v) => {
            if (isAccent(v)) setAccent(v);
            set("ui.accent", v);
          }}
        />
        <ToggleRow label="Show speed under answers" hint="Tokens per second and time to first token." checked={bool(s, "ui.show_perf_stats", true)} onChange={(v) => set("ui.show_perf_stats", v)} />
        {IS_WEB ? null : (
          <ToggleRow label="Quit when the window closes" hint="Off keeps NanoNexus running in the background (macOS default)." checked={bool(s, "app.quit_on_window_close")} onChange={(v) => set("app.quit_on_window_close", v)} />
        )}
      </Section>

      <Section title="Runtime" hint="Changes apply the next time the model server restarts.">
        <TextRow label="Context size (tokens)" hint="Longer remembers more but uses more memory." type="number" value={String(num(s, "runtime.ctx_size", 8192))} onCommit={(v) => setWhole("runtime.ctx_size", v, 512)} />
        <TextRow label="GPU layers" hint={'"all" or a number. 0 runs on the CPU.'} value={str(s, "runtime.gpu_layers", "all")} onCommit={(v) => (v.trim() === "all" || parseWhole(v) !== null ? set("runtime.gpu_layers", v.trim()) : toast.error('Enter "all" or a whole number.'))} />
        <SelectRow
          label="Flash attention"
          value={str(s, "runtime.flash_attn", "auto")}
          options={[
            { value: "auto", label: "Auto" },
            { value: "on", label: "On" },
            { value: "off", label: "Off" },
          ]}
          onChange={(v) => set("runtime.flash_attn", v)}
        />
        <TextRow
          label="CPU threads"
          hint="Empty = automatic."
          type="number"
          placeholder="auto"
          value={typeof threads === "number" ? String(threads) : ""}
          onCommit={(v) => (v.trim() === "" ? set("runtime.threads", null) : setWhole("runtime.threads", v, 1))}
        />
        <TextRow label="Unload model after idle (minutes)" hint="0 = never unload." type="number" value={String(num(s, "runtime.idle_unload_minutes"))} onCommit={(v) => setWhole("runtime.idle_unload_minutes", v)} />
      </Section>

      <Section title="Models">
        <Row label="Active model" hint={str(s, "models.active_model_id", "builtin")}>
          <Link className="text-body font-medium text-accent hover:underline" to="/models">
            Manage models
          </Link>
        </Row>
        <Row label="Duo assistant" hint="Choose the light and strong models on the Models page.">
          <Link className="text-body font-medium text-accent hover:underline" to="/models">
            Open
          </Link>
        </Row>
        {IS_WEB ? null : (
          <Row label="Extra model folders" hint="One folder per line. Models found there are listed too.">
            <textarea
              aria-label="Extra model folders"
              className="h-20 w-72 rounded-control border border-border bg-surface px-2 py-1 text-small"
              defaultValue={extraDirs.join("\n")}
              key={extraDirs.join("\n")}
              onBlur={(e) => {
                const dirs = e.target.value.split("\n").map((d) => d.trim()).filter(Boolean);
                if (dirs.join("\n") !== extraDirs.join("\n")) set("models.extra_dirs", dirs);
              }}
            />
          </Row>
        )}
      </Section>

      <Section title="Privacy">
        <ToggleRow label="Allow network downloads" hint="Needed to download models and read from Hugging Face. Chat never leaves your computer." checked={bool(s, "privacy.allow_network_downloads", true)} onChange={(v) => set("privacy.allow_network_downloads", v)} />
        <ToggleRow label="Use library passages in chats" hint="Adds matching passages from your offline libraries to ordinary chats." checked={bool(s, "knowledge.chat_context")} onChange={(v) => set("knowledge.chat_context", v)} />
        <TextRow label="Library passage budget (tokens)" type="number" value={String(num(s, "knowledge.max_tokens", 1200))} onCommit={(v) => setWhole("knowledge.max_tokens", v, 100)} />
      </Section>

      <Section title="Notifications">
        <ToggleRow label="Show system notifications when NanoNexus is in the background" checked={bool(s, "notifications.os_when_unfocused", true)} onChange={(v) => set("notifications.os_when_unfocused", v)} />
        {NOTIFICATION_KINDS.map(({ kind, label }) => (
          <ToggleRow key={kind} label={label} checked={kinds[kind] ?? true} onChange={(v) => set("notifications.kinds", { ...kinds, [kind]: v })} />
        ))}
        <TextRow label="Quiet hours start" hint="No system notifications between start and end. Empty = off." type="time" value={str(s, "notifications.quiet_start")} onCommit={(v) => set("notifications.quiet_start", v)} />
        <TextRow label="Quiet hours end" type="time" value={str(s, "notifications.quiet_end")} onCommit={(v) => set("notifications.quiet_end", v)} />
      </Section>

      {IS_WEB ? null : (
        <Section title="Changes and undo" hint="Backups of files you changed, so every change can be undone.">
          <TextRow label="Keep backups for (days)" type="number" value={String(num(s, "journal.retention_days", 30))} onCommit={(v) => setWhole("journal.retention_days", v, 1)} />
          <TextRow label="Backup size limit (MB)" type="number" value={String(Math.round(num(s, "journal.max_bytes", 2_147_483_648) / MB))} onCommit={(v) => {
            const n = parseWhole(v, 1);
            if (n === null) toast.error("Enter a whole number of MB.");
            else set("journal.max_bytes", n * MB);
          }} />
        </Section>
      )}

      <AboutSection />

      {IS_WEB ? null : (
        <Section title="LAN Share">
          <Row label="Share with phones and other computers" hint={bool(s, "share.enabled") ? "Sharing is on." : "Sharing is off."}>
            <Link className="text-body font-medium text-accent hover:underline" to="/settings/sharing">
              Open sharing settings
            </Link>
          </Row>
        </Section>
      )}
    </div>
  );
}
