import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";

import type { ObserveView, Settings } from "@/bindings";
import { Button, Dialog, DialogContent, Switch } from "@/components/ui";
import { api, type SettingValue } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

/** Same key as Settings uses, so both screens share one cache entry. */
const SETTINGS_KEY = ["settings"] as const;

type Bag = Record<string, unknown>;
const flag = (b: Bag, k: string, d: boolean) => (typeof b[k] === "boolean" ? (b[k] as boolean) : d);

interface External {
  key: "observe.claude_code" | "observe.codex";
  name: string;
  folder: string;
  found: (v: ObserveView | undefined) => boolean | undefined;
}

const EXTERNAL: External[] = [
  { key: "observe.claude_code", name: "Claude Code", folder: "~/.claude/projects", found: (v) => v?.claude_found },
  { key: "observe.codex", name: "Codex", folder: "~/.codex/sessions", found: (v) => v?.codex_found },
];

const WINDOWS = [1, 3, 6, 12, 24, 48];

/** Which things the Activity page watches, with the consent step for other tools' logs (ADR-017). */
export function SourcesPanel({ view, compact = false }: { view?: ObserveView | undefined; compact?: boolean }) {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: SETTINGS_KEY, queryFn: api.settings.get, retry: false });
  const [asking, setAsking] = useState<External | null>(null);
  const save = useMutation({
    mutationFn: ({ key, value }: { key: string; value: SettingValue }) => api.settings.set(key, value),
    onSuccess: (next: Settings) => {
      qc.setQueryData(SETTINGS_KEY, next);
      void qc.invalidateQueries({ queryKey: ["observe"] });
    },
    onError: (e) => toast.error(toIpcException(e).message),
  });
  const s = (settings.data ?? {}) as unknown as Bag;
  const set = (key: string, value: SettingValue) => save.mutate({ key, value });

  return (
    <div className="flex flex-col gap-3 text-body">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="font-medium">NanoNexus</div>
          {compact ? null : <div className="text-small text-muted">Your chats, tool calls and background jobs in this app.</div>}
        </div>
        <Switch aria-label="Show NanoNexus activity" checked={flag(s, "observe.nanonexus", true)} onCheckedChange={(v) => set("observe.nanonexus", v)} />
      </div>
      {EXTERNAL.map((x) => {
        const on = flag(s, x.key, false);
        const found = x.found(view);
        return (
          <div key={x.key} className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="font-medium">
                {x.name} <span className="text-small font-normal text-faint">{found === false ? "· no logs found on this computer" : found ? "· logs found" : ""}</span>
              </div>
              {compact ? null : (
                <div className="text-small text-muted">
                  Read-only. Reads <code className="font-mono">{x.folder}</code> for model, token counts, tool names and edited file names. Never your prompts, replies, commands or file contents.
                </div>
              )}
            </div>
            <Switch
              aria-label={`Show ${x.name} activity`}
              checked={on}
              onCheckedChange={(v) => (v ? setAsking(x) : set(x.key, false))}
            />
          </div>
        );
      })}
      {compact ? null : (
        <>
          <div className="flex items-center justify-between gap-3">
            <div>
              <div className="font-medium">Tell me when an agent needs attention</div>
              <div className="text-small text-muted">A notification when an agent looks stuck, hits an error, or two agents edit one file.</div>
            </div>
            <Switch aria-label="Activity alerts" checked={flag(s, "observe.alerts", true)} onCheckedChange={(v) => set("observe.alerts", v)} />
          </div>
          <div className="flex items-center justify-between gap-3">
            <div className="font-medium">How far back to look</div>
            <select
              aria-label="Hours of history"
              className="h-9 rounded-control border border-border bg-surface px-2 text-body"
              value={typeof s["observe.window_hours"] === "number" ? (s["observe.window_hours"] as number) : 6}
              onChange={(e) => set("observe.window_hours", Number(e.target.value))}
            >
              {WINDOWS.map((h) => (
                <option key={h} value={h}>
                  {h} {h === 1 ? "hour" : "hours"}
                </option>
              ))}
            </select>
          </div>
        </>
      )}
      <Dialog open={asking !== null} onOpenChange={(o) => !o && setAsking(null)}>
        <DialogContent>
          <h2 className="text-lead font-semibold">Show {asking?.name} activity?</h2>
          <p className="mt-2 text-body text-muted">
            NanoNexus will read the log files {asking?.name} already writes in <code className="font-mono">{asking?.folder}</code>, only to show what is running.
          </p>
          <ul className="mt-3 list-disc space-y-1 pl-5 text-body">
            <li>It keeps event types, counts, model names, token counts, tool names and the names of files that edit tools touch.</li>
            <li>It never keeps your prompts, the replies, shell commands or file contents.</li>
            <li>It only reads: nothing is written, nothing is sent anywhere, and the page is not available over LAN Share.</li>
          </ul>
          <div className="mt-5 flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setAsking(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                if (asking) set(asking.key, true);
                setAsking(null);
              }}
            >
              Turn on
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
