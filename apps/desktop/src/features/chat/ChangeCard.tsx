import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, History, Undo2, X } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

const LIST_KEY = ["changes", "list"] as const;

/** A proposed file change inside the chat, with Apply and Discard right there (Undo once applied). */
export function ChangeCard({ id, label }: { id: string; label: string | null }) {
  const qc = useQueryClient();
  const list = useQuery({ queryKey: LIST_KEY, queryFn: () => api.changes.list(100) });
  const row = list.data?.find((c) => c.id === id);
  const done = () => void qc.invalidateQueries({ queryKey: ["changes"] });
  const fail = (e: unknown) => {
    toast.error(toIpcException(e).message);
    done();
  };
  const apply = useMutation({
    mutationFn: () => api.changes.apply(id),
    onSuccess: () => {
      toast.success("Written to disk. You can undo it from Changes.");
      done();
    },
    onError: fail,
  });
  const discard = useMutation({ mutationFn: () => api.changes.discard(id), onSuccess: done, onError: fail });
  const undo = useMutation({
    mutationFn: () => api.changes.undo(id, []),
    onSuccess: (o) => {
      if (o.conflicts.length > 0) toast.warning("Some files changed since. Open Changes to choose what to restore.");
      done();
    },
    onError: fail,
  });
  const status = row?.status ?? "planned";
  const busy = apply.isPending || discard.isPending || undo.isPending;
  return (
    <div className="mb-2 flex flex-wrap items-center gap-2 rounded-card border border-accent/50 bg-accent/10 px-3 py-2 text-small">
      <History className="size-3.5 shrink-0" />
      <span className="min-w-0 flex-1 truncate">{row?.title ?? label ?? "Proposed change"}</span>
      {status === "planned" ? (
        <>
          <Button size="sm" variant="primary" disabled={busy} onClick={() => apply.mutate()}>
            <Check /> Apply
          </Button>
          <Button size="sm" disabled={busy} onClick={() => discard.mutate()}>
            <X /> Discard
          </Button>
        </>
      ) : status === "applied" ? (
        <>
          <span className="text-success">Applied</span>
          <Button size="sm" disabled={busy} onClick={() => undo.mutate()}>
            <Undo2 /> Undo
          </Button>
        </>
      ) : (
        <span className="text-muted">{status.replace("_", " ")}</span>
      )}
      <a href="#/changes" className="text-muted underline hover:text-text">
        Details
      </a>
    </div>
  );
}
