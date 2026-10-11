import { useMutation, useQuery } from "@tanstack/react-query";
import { ChevronDown, Loader2, RotateCw } from "lucide-react";
import { useState } from "react";

import { Button, Dot, Popover, PopoverContent, PopoverTrigger } from "@/components/ui";
import { useRuntime } from "@/hooks/useRuntime";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";
import { toIpcException } from "@/lib/ipc";
import { baseName, describeRuntime } from "@/lib/runtime-view";

const TONE_TEXT = { neutral: "text-muted", success: "text-success", warning: "text-warning", danger: "text-danger" } as const;

function Logs({ open }: { open: boolean }) {
  const logs = useQuery({
    queryKey: ["runtime-logs"],
    queryFn: () => api.runtime.logs(60),
    enabled: open,
    refetchInterval: open ? 2000 : false,
    retry: false,
  });
  const text = logs.data?.join("\n") ?? "";
  return (
    <pre
      aria-label="Server log"
      className="mt-2 max-h-40 overflow-auto rounded-chip bg-code p-2 text-caption leading-4 text-muted"
    >
      {text || "No output yet."}
    </pre>
  );
}

/** Title-bar pill with the model's live status; the popover shows details, logs, and Restart. */
export function RuntimePill() {
  const state = useRuntime();
  const view = describeRuntime(state);
  const [open, setOpen] = useState(false);
  const restart = useMutation({ mutationFn: api.runtime.restart });
  const ready = state?.state === "ready" ? state : undefined;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={`Model status: ${view.label}`}
          className="flex h-7 items-center gap-1.5 rounded-full border border-border bg-surface-2 px-2.5 text-small hover:bg-surface-3"
        >
          {view.busy ? <Loader2 className="size-3 animate-spin text-warning" /> : <Dot tone={view.tone} />}
          <span className={cn("font-medium", TONE_TEXT[view.tone])}>{view.label}</span>
          <ChevronDown className="size-3 text-faint" />
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-96">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="text-body font-semibold">{view.label}</div>
            <div className="truncate text-small text-muted" title={view.detail}>
              {view.detail}
            </div>
          </div>
          <Button size="sm" variant="secondary" disabled={restart.isPending} onClick={() => restart.mutate()}>
            <RotateCw className={cn(restart.isPending && "animate-spin")} />
            Restart
          </Button>
        </div>
        {ready ? (
          <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-small">
            <dt className="text-muted">Model</dt>
            <dd className="truncate">{baseName(ready.model_path)}</dd>
            <dt className="text-muted">Context</dt>
            <dd className="tnum">{ready.ctx_size.toLocaleString()} tokens</dd>
            <dt className="text-muted">GPU layers</dt>
            <dd>{ready.gpu_layers}</dd>
          </dl>
        ) : null}
        {restart.isError ? (
          <p role="alert" className="mt-2 text-small text-danger">
            {toIpcException(restart.error).message}
          </p>
        ) : null}
        {state?.state === "failed" && state.log_tail.length > 0 ? (
          <pre className="mt-2 max-h-28 overflow-auto rounded-chip bg-code p-2 text-caption leading-4 text-muted">
            {state.log_tail.join("\n")}
          </pre>
        ) : null}
        <Logs open={open} />
      </PopoverContent>
    </Popover>
  );
}
