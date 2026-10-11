/** Mirrors the Rust `JobSummary` (payload of `job:update`, result of `jobs_list`). */
export type JobStatus = "queued" | "running" | "done" | "failed" | "cancelled";

export interface JobSummary {
  id: string;
  /** `scan | download | index | propose | watch` */
  kind: string;
  title: string;
  status: JobStatus;
  /** 0..1 when known. */
  progress: number | null;
  detail: string | null;
  /** In-app route that opens the result. */
  result_ref: string | null;
  error: string | null;
}

export const isActive = (j: JobSummary): boolean => j.status === "running" || j.status === "queued";

/** Replace the job with the same id (or add it) and keep running jobs first. */
export function upsertJob(list: JobSummary[], job: JobSummary): JobSummary[] {
  const next = list.some((j) => j.id === job.id) ? list.map((j) => (j.id === job.id ? job : j)) : [job, ...list];
  return [...next.filter(isActive), ...next.filter((j) => !isActive(j))];
}
