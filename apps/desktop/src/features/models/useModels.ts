import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import type { DownloadView } from "@/bindings";
import { api } from "@/lib/api";

export const INSTALLED_KEY = ["models", "installed"] as const;
export const CATALOG_KEY = ["models", "catalog"] as const;
export const DOWNLOADS_KEY = ["models", "downloads"] as const;
export const MEMORY_KEY = ["models", "memory"] as const;

/** Merge a live progress frame into the cached download list (frames carry no repo or file name). */
export function mergeProgress(list: DownloadView[] | undefined, frame: DownloadView): DownloadView[] | undefined {
  if (!list) return list;
  const i = list.findIndex((d) => d.id === frame.id);
  if (i < 0) return list;
  const cur = list[i] as DownloadView;
  const next: DownloadView = { ...cur, done_bytes: frame.done_bytes, bytes: frame.bytes || cur.bytes, bytes_per_sec: frame.bytes_per_sec, eta_secs: frame.eta_secs, phase: frame.phase, status: "running" };
  return [...list.slice(0, i), next, ...list.slice(i + 1)];
}

/** Installed models, the catalog, and downloads, kept fresh by `download:progress` and `model:changed`. */
export function useModels() {
  const qc = useQueryClient();
  const installed = useQuery({ queryKey: INSTALLED_KEY, queryFn: api.models.list, retry: false });
  const catalog = useQuery({ queryKey: CATALOG_KEY, queryFn: api.models.catalog, retry: false });
  const downloads = useQuery({ queryKey: DOWNLOADS_KEY, queryFn: api.models.downloads, retry: false });
  useEffect(() => {
    let cancelled = false;
    const offs: Array<() => void> = [];
    const keep = (p: Promise<() => void>) =>
      p.then((u) => (cancelled ? u() : offs.push(u))).catch(() => {
        /* no event bus (plain browser) */
      });
    const refresh = () => {
      void qc.invalidateQueries({ queryKey: ["models"] });
    };
    void keep(
      api.models.onProgress((frame) => {
        if (frame.repo !== "") refresh(); // a status change (queued, paused, done, failed, ...)
        else qc.setQueryData<DownloadView[]>(DOWNLOADS_KEY, (l) => mergeProgress(l, frame));
      }),
    );
    void keep(api.models.onChanged(refresh));
    void keep(api.models.onListChanged(refresh));
    return () => {
      cancelled = true;
      offs.forEach((o) => o());
    };
  }, [qc]);
  return { installed, catalog, downloads };
}
