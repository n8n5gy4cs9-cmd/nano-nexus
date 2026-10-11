import { useMutation, useQuery } from "@tanstack/react-query";
import { Play, Square } from "lucide-react";
import { useRef, useState } from "react";
import { toast } from "sonner";

import { Badge, Button } from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

import type { VerifyResult } from "./types";

const MAX_LINES = 400;

/** Run the project's own build or test command to check a rename. Never runs by itself. */
export function VerifyPanel({ sessionId }: { sessionId: string }) {
  const checkers = useQuery({ queryKey: ["rename", "checkers", sessionId], queryFn: () => api.rename.checkers(sessionId), retry: false });
  const [lines, setLines] = useState<string[]>([]);
  const [result, setResult] = useState<VerifyResult | null>(null);
  const job = useRef<string | null>(null);

  const run = useMutation({
    mutationFn: async (checkerId: string) => {
      const id = crypto.randomUUID();
      job.current = id;
      setLines([]);
      setResult(null);
      try {
        return await api.rename.verify(id, sessionId, checkerId, (l) => setLines((old) => [...old.slice(-(MAX_LINES - 1)), l]));
      } finally {
        job.current = null;
      }
    },
    onSuccess: setResult,
    onError: (e) => toast.error(toIpcException(e).message),
  });
  const list = checkers.data ?? [];
  return (
    <div className="flex flex-col gap-3">
      <p className="text-small text-muted">After you apply the renames, check that the project still builds. This runs your project's own command in the project folder.</p>
      {list.length === 0 ? <p className="text-small text-muted">No build or test command was found for this project.</p> : null}
      <div className="flex flex-wrap items-center gap-2">
        {list.map((c) => (
          <Button key={c.id} disabled={!c.available || run.isPending} title={c.available ? `${c.program} ${c.args.join(" ")}` : `${c.program} is not installed`} onClick={() => run.mutate(c.id)}>
            <Play /> {c.label}
          </Button>
        ))}
        {run.isPending ? (
          <Button variant="danger" onClick={() => job.current && void api.rename.cancel(job.current)}>
            <Square /> Stop
          </Button>
        ) : null}
        {result ? (
          <Badge tone={result.ok ? "success" : "danger"}>
            {result.ok ? "Passed" : result.cancelled ? "Stopped" : result.timed_out ? "Timed out" : result.missing ? "Not installed" : `Failed (exit ${result.exit_code ?? "?"})`}
          </Badge>
        ) : null}
      </div>
      {result && !result.ok && result.first_errors.length > 0 ? (
        <ul className="rounded-card border border-border bg-surface-2 p-2 font-mono text-small text-danger">
          {result.first_errors.map((l, n) => (
            <li key={n}>{l}</li>
          ))}
        </ul>
      ) : null}
      {lines.length > 0 ? <pre className="max-h-64 overflow-auto rounded-card border border-border bg-surface-2 p-2 font-mono text-caption">{lines.join("\n")}</pre> : null}
    </div>
  );
}
