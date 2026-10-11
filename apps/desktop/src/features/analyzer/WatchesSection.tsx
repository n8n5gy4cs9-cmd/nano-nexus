import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, Play, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { Button, EmptyState, Skeleton, Switch } from "@/components/ui";
import { api } from "@/lib/api";
import { relativeTime } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";

import { SCHEDULE_LABEL, type WatchSchedule } from "./watches";

export const WATCHES_KEY = ["analyzer", "watches"] as const;

/** Watched folders: re-scanned on a schedule, with a notification when something big changed (M11). */
export function WatchesSection() {
  const qc = useQueryClient();
  const [schedule, setSchedule] = useState<WatchSchedule>("weekly");
  const watches = useQuery({ queryKey: WATCHES_KEY, queryFn: api.watches.list, retry: false });
  const refresh = () => void qc.invalidateQueries({ queryKey: WATCHES_KEY });
  const onError = (e: unknown) => toast.error(toIpcException(e).message);

  const add = useMutation({
    mutationFn: async () => {
      const root = await api.fs.pickFolder();
      if (root) await api.watches.create(root, schedule);
    },
    onSuccess: refresh,
    onError,
  });
  const toggle = useMutation({ mutationFn: (v: { id: string; enabled: boolean }) => api.watches.setEnabled(v.id, v.enabled), onSuccess: refresh, onError });
  const remove = useMutation({ mutationFn: (id: string) => api.watches.remove(id), onSuccess: refresh, onError });
  const runNow = useMutation({
    mutationFn: (id: string) => api.watches.runNow(id),
    onSuccess: () => {
      toast.success("Scan finished. You will get a notification with what changed.");
      refresh();
    },
    onError,
  });

  return (
    <section aria-label="Watched folders" className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-lead font-semibold">Watched folders</h2>
        <div className="flex items-center gap-2">
          <select
            aria-label="Watch schedule"
            className="h-9 rounded-control border border-border bg-surface px-2 text-body"
            value={schedule}
            onChange={(e) => setSchedule(e.target.value as WatchSchedule)}
          >
            {(Object.keys(SCHEDULE_LABEL) as WatchSchedule[]).map((s) => (
              <option key={s} value={s}>
                {SCHEDULE_LABEL[s]}
              </option>
            ))}
          </select>
          <Button size="sm" onClick={() => add.mutate()} disabled={add.isPending}>
            <Eye /> Watch a folder
          </Button>
        </div>
      </div>
      {watches.isPending ? (
        <Skeleton className="h-12 w-full" />
      ) : (watches.data ?? []).length === 0 ? (
        <EmptyState icon={<Eye />} title="No watched folders" description="Watch Downloads to hear about big new files and duplicates." />
      ) : (
        <ul className="flex flex-col gap-1">
          {(watches.data ?? []).map((w) => (
            <li key={w.id} className="flex items-center gap-2 rounded-card border border-border bg-surface px-3 py-2">
              <div className="min-w-0 flex-1">
                <div className="truncate text-body font-medium">{w.root}</div>
                <div className="text-small text-muted">
                  {SCHEDULE_LABEL[w.schedule]} · {w.last_run_at ? `last scan ${relativeTime(w.last_run_at)}` : "not scanned yet"}
                </div>
              </div>
              <Switch checked={w.enabled} onCheckedChange={(enabled) => toggle.mutate({ id: w.id, enabled })} aria-label={`Watch ${w.root}`} />
              <Button variant="ghost" aria-label={`Run now: ${w.root}`} disabled={runNow.isPending} onClick={() => runNow.mutate(w.id)}>
                <Play />
              </Button>
              <Button variant="ghost" aria-label={`Stop watching ${w.root}`} onClick={() => remove.mutate(w.id)}>
                <Trash2 />
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
