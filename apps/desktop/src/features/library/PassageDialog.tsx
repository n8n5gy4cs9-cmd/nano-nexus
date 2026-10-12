import { useEffect, useState } from "react";

import { Dialog, DialogContent } from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

import type { KbPassage } from "./types";

/** Shows one library passage in full (a clicked citation or a search result). No query cache needed. */
export function PassageDialog({ chunkId, onClose }: { chunkId: number | null; onClose: () => void }) {
  // The answer remembers which chunk it is for, so a new chunk shows "Loading…" without an effect-time reset.
  const [result, setResult] = useState<{ id: number; passage: KbPassage | null; error: string | null } | null>(null);
  const current = result && result.id === chunkId ? result : null;
  const passage = current?.passage ?? null;
  const error = current?.error ?? null;

  useEffect(() => {
    if (chunkId === null) return;
    let alive = true;
    api.knowledge
      .passage(chunkId)
      .then((p) => alive && setResult({ id: chunkId, passage: p, error: null }))
      .catch((e: unknown) => alive && setResult({ id: chunkId, passage: null, error: toIpcException(e).message }));
    return () => {
      alive = false;
    };
  }, [chunkId]);

  return (
    <Dialog open={chunkId !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-[min(720px,calc(100vw-32px))]">
        {error ? (
          <p className="text-small text-danger">{error}</p>
        ) : passage ? (
          <div className="flex max-h-[70vh] flex-col gap-2">
            <h2 className="pr-6 text-body font-semibold">{passage.heading_path}</h2>
            <p className="text-small text-muted">
              {passage.library_name} · {passage.doc_path}
              {passage.page !== null ? ` · page ${passage.page}` : ""}
            </p>
            <pre className="overflow-y-auto rounded-card bg-surface-2 p-3 text-small whitespace-pre-wrap">{passage.text}</pre>
          </div>
        ) : (
          <p className="text-small text-muted">Loading…</p>
        )}
      </DialogContent>
    </Dialog>
  );
}
