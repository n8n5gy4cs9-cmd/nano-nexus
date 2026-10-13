import { useEffect, useState } from "react";

import { Dialog, DialogContent } from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

import { DocView, docKind } from "@/features/help/DocView";

import type { KbDocumentSource, KbPassage } from "./types";

/** Shows one library passage in full (a clicked citation or a search result). No query cache needed. */
export function PassageDialog({ chunkId, onClose }: { chunkId: number | null; onClose: () => void }) {
  // The answer remembers which chunk it is for, so a new chunk shows "Loading…" without an effect-time reset.
  const [result, setResult] = useState<{ id: number; passage: KbPassage | null; error: string | null } | null>(null);
  const current = result && result.id === chunkId ? result : null;
  const passage = current?.passage ?? null;
  const error = current?.error ?? null;

  // The original file (HTML renders in a sandbox, Markdown renders, "Raw" shows the text). Loaded on demand.
  const [orig, setOrig] = useState<{ id: number; src: KbDocumentSource | null; error: string | null } | null>(null);
  const [showOrig, setShowOrig] = useState<number | null>(null);
  const origNow = orig && orig.id === chunkId ? orig : null;
  const ext = passage?.doc_path.split(".").pop()?.toLowerCase() ?? "";
  const hasSource = ["html", "htm", "xhtml", "md", "markdown"].includes(ext);
  const viewOriginal = () => {
    if (chunkId === null) return;
    setShowOrig(chunkId);
    if (origNow) return;
    api.knowledge
      .documentSource(chunkId)
      .then((src) => setOrig({ id: chunkId, src, error: null }))
      .catch((e: unknown) => setOrig({ id: chunkId, src: null, error: toIpcException(e).message }));
  };

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
            {hasSource ? (
              <div className="flex gap-1" role="group" aria-label="Source view">
                <button type="button" aria-pressed={showOrig !== chunkId} className="rounded-control px-2 py-1 text-small hover:bg-surface-2 aria-pressed:bg-accent/15" onClick={() => setShowOrig(null)}>
                  Passage
                </button>
                <button type="button" aria-pressed={showOrig === chunkId} className="rounded-control px-2 py-1 text-small hover:bg-surface-2 aria-pressed:bg-accent/15" onClick={viewOriginal}>
                  Original document
                </button>
              </div>
            ) : null}
            {showOrig === chunkId && origNow ? (
              origNow.src ? (
                <>
                  <DocView kind={docKind(origNow.src.kind)} source={origNow.src.content} className="flex-1 overflow-hidden" />
                  {origNow.src.truncated ? <p className="text-small text-muted">Showing the first 2 MB of this file.</p> : null}
                </>
              ) : (
                <p className="text-small text-danger">{origNow.error}</p>
              )
            ) : showOrig === chunkId ? (
              <p className="text-small text-muted">Loading…</p>
            ) : (
              <pre className="overflow-y-auto rounded-card bg-surface-2 p-3 text-small whitespace-pre-wrap">{passage.text}</pre>
            )}
          </div>
        ) : (
          <p className="text-small text-muted">Loading…</p>
        )}
      </DialogContent>
    </Dialog>
  );
}
