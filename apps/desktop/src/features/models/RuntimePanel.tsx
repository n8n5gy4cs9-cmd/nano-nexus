import { useQuery } from "@tanstack/react-query";
import { RefreshCw, ScrollText } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { Badge, Button, Dialog, DialogContent, DialogDescription, DialogTitle, Dot, ScrollArea } from "@/components/ui";
import { useRuntime } from "@/hooks/useRuntime";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { baseName, describeRuntime } from "@/lib/runtime-view";

/** Runtime status at the top of the Models page: state, model, context, GPU layers, Restart, Logs. */
export function RuntimePanel() {
  const state = useRuntime();
  const view = describeRuntime(state);
  const [logsOpen, setLogsOpen] = useState(false);
  const logs = useQuery({ queryKey: ["runtime", "logs"], queryFn: () => api.runtime.logs(200), enabled: logsOpen, retry: false });
  const restart = () => void api.runtime.restart().catch((e: unknown) => toast.error(toIpcException(e).message));
  return (
    <section aria-label="Runtime" className="flex flex-wrap items-center gap-3 rounded-card border border-border bg-surface p-4 shadow-1">
      <Dot tone={view.tone} />
      <div className="min-w-0 flex-1">
        <p className="text-body font-semibold">
          {view.label} <span className="font-normal text-muted">· {view.detail}</span>
        </p>
        {state?.state === "ready" ? (
          <p className="text-small text-muted">
            {baseName(state.model_path)} · context {state.ctx_size.toLocaleString()} · GPU layers {state.gpu_layers}
          </p>
        ) : null}
        {state?.state === "failed" ? <Badge tone="danger">Could not start</Badge> : null}
      </div>
      <Button size="sm" onClick={restart}>
        <RefreshCw /> Restart
      </Button>
      <Button size="sm" onClick={() => setLogsOpen(true)}>
        <ScrollText /> Logs
      </Button>
      {logsOpen ? (
        <Dialog open onOpenChange={setLogsOpen}>
          <DialogContent className="w-[min(760px,calc(100vw-32px))]" aria-describedby="logs-desc">
            <DialogTitle>Runtime log</DialogTitle>
            <DialogDescription id="logs-desc">The last lines from the model server.</DialogDescription>
            <ScrollArea className="mt-3 h-80 rounded-card bg-surface-2 p-3">
              <pre className="font-mono text-caption whitespace-pre-wrap">{(logs.data ?? []).join("\n") || (logs.isLoading ? "Loading…" : "No log lines yet.")}</pre>
            </ScrollArea>
          </DialogContent>
        </Dialog>
      ) : null}
    </section>
  );
}
