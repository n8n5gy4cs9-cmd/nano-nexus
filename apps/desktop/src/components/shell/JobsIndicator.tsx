import { useMutation } from "@tanstack/react-query";
import { Loader2, X } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";

import { IconButton, Popover, PopoverContent, PopoverTrigger } from "@/components/ui";
import { type JobSummary, isActive } from "@/features/jobs/types";
import { useJobs } from "@/hooks/useJobs";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";

const STATUS_LABEL: Record<JobSummary["status"], string> = {
  queued: "Waiting",
  running: "Running",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

function Row({ job, onOpen, onCancel }: { job: JobSummary; onOpen: (route: string) => void; onCancel: (id: string) => void }) {
  const pct = job.progress === null ? null : Math.round(job.progress * 100);
  return (
    <li className="flex flex-col gap-1 rounded-card px-3 py-2.5" data-job-id={job.id}>
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-body font-medium">{job.title}</span>
        {isActive(job) ? (
          <IconButton aria-label={`Cancel ${job.title}`} size="sm" onClick={() => onCancel(job.id)}>
            <X />
          </IconButton>
        ) : (
          <span className={cn("shrink-0 text-caption", job.status === "failed" ? "text-danger" : "text-faint")}>{STATUS_LABEL[job.status]}</span>
        )}
      </div>
      {isActive(job) ? (
        <div
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={pct ?? undefined}
          className="h-1.5 overflow-hidden rounded-full bg-surface-2"
        >
          <div className={cn("h-full bg-accent", pct === null && "w-1/3 animate-pulse")} style={pct === null ? undefined : { width: `${pct}%` }} />
        </div>
      ) : null}
      {job.status === "failed" && job.error ? <p className="line-clamp-2 text-small text-danger">{job.error}</p> : null}
      {job.detail && isActive(job) ? <p className="truncate text-small text-muted">{job.detail}</p> : null}
      {job.status === "done" && job.result_ref ? (
        <button type="button" className="self-start text-small font-medium text-accent hover:underline" onClick={() => onOpen(job.result_ref ?? "")}>
          Open result
        </button>
      ) : null}
    </li>
  );
}

/** Title-bar indicator: a spinner with the number of running jobs; the popover lists recent jobs (M11). */
export function JobsIndicator() {
  const { jobs, active } = useJobs();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const cancel = useMutation({ mutationFn: (id: string) => api.jobs.cancel(id) });
  if (jobs.length === 0) return null;
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <IconButton aria-label={active.length > 0 ? `Jobs, ${active.length} running` : "Jobs"} size="sm" className="relative">
          <Loader2 className={cn(active.length > 0 && "animate-spin")} />
          {active.length > 0 ? (
            <span className="absolute -top-0.5 -right-0.5 grid min-w-4 place-items-center rounded-full bg-accent px-1 text-[10px] leading-4 font-semibold text-accent-fg">
              {active.length}
            </span>
          ) : null}
        </IconButton>
      </PopoverTrigger>
      <PopoverContent className="w-[360px] p-0">
        <div className="border-b border-border px-3 py-2 text-body font-semibold">Jobs</div>
        <ul className="max-h-96 overflow-y-auto p-1">
          {jobs.map((j) => (
            <Row
              key={j.id}
              job={j}
              onCancel={(id) => cancel.mutate(id)}
              onOpen={(route) => {
                setOpen(false);
                void navigate(route);
              }}
            />
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  );
}
