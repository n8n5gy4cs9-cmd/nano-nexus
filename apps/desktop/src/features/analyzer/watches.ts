/** Mirrors the Rust `WatchView` / `ReportDelta` (switch to the `bindings.ts` types after it regenerates). */
export type WatchSchedule = "daily" | "weekly" | "on_change";

export interface WatchView {
  id: string;
  root: string;
  schedule: WatchSchedule;
  enabled: boolean;
  last_report_id: string | null;
  last_run_at: number | null;
}

export interface ReportDelta {
  root: string;
  bytes_delta: number;
  files_delta: number;
  new_duplicate_groups: number;
}

export const SCHEDULE_LABEL: Record<WatchSchedule, string> = {
  daily: "Every day",
  weekly: "Every week",
  on_change: "When it changes",
};
