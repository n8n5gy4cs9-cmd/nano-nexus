import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { type JobSummary, isActive, upsertJob } from "@/features/jobs/types";
import { api } from "@/lib/api";

export const JOBS_KEY = ["jobs"] as const;

/** Recent jobs, kept live by `job:update` events. */
export function useJobs() {
  const qc = useQueryClient();
  const list = useQuery({ queryKey: JOBS_KEY, queryFn: () => api.jobs.list(50), retry: false });
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    api.jobs
      .onUpdate((job) => qc.setQueryData<JobSummary[]>(JOBS_KEY, (old) => upsertJob(old ?? [], job)))
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else off = unsubscribe;
      })
      .catch(() => {
        /* no event bus (plain browser) */
      });
    return () => {
      cancelled = true;
      off?.();
    };
  }, [qc]);
  const jobs = list.data ?? [];
  return { jobs, active: jobs.filter(isActive) };
}
