import { Dialog, DialogDescription, DialogTitle } from "@/components/ui";
import { SheetContent } from "@/components/ui/dialog";
import type { InstalledModel } from "@/bindings";
import { formatBytes } from "@/lib/format";

import { CapabilityChips, FitBadge, describeFit } from "./ModelCards";

function Row({ k, v }: { k: string; v: string | null | undefined }) {
  if (v === null || v === undefined || v === "") return null;
  return (
    <div className="flex justify-between gap-4 border-b border-border py-1.5 text-small">
      <dt className="text-muted">{k}</dt>
      <dd className="min-w-0 break-all text-right font-medium">{v}</dd>
    </div>
  );
}

/** Model detail: where it is, what it is, the GGUF header facts, and the RAM estimate. */
export function ModelDrawer({ m, onClose }: { m: InstalledModel; onClose: () => void }) {
  const g = m.gguf;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <SheetContent aria-describedby="model-drawer-desc" className="overflow-auto">
        <DialogTitle className="pr-8">{m.name}</DialogTitle>
        <DialogDescription id="model-drawer-desc">{[m.quant, formatBytes(m.bytes)].filter(Boolean).join(" · ")}</DialogDescription>
        <div className="mt-3 flex flex-col gap-3">
          <div className="flex items-center gap-2">
            <FitBadge fit={m.fit} />
            <span className="text-small text-muted">{describeFit(m.fit)}</span>
          </div>
          <CapabilityChips caps={m.capabilities} />
          <dl>
            <Row k="Source" v={m.source} />
            <Row k="Repository" v={m.repo} />
            <Row k="File" v={m.file} />
            <Row k="Location" v={m.path} />
            <Row k="License" v={m.license} />
            <Row k="Architecture" v={g?.architecture} />
            <Row k="Quantization" v={g?.quant ?? m.quant} />
            <Row k="Context length" v={g?.context_length ? `${g.context_length.toLocaleString()} tokens` : null} />
            <Row k="Layers" v={g?.block_count?.toString()} />
            <Row k="Embedding size" v={g?.embedding_length?.toString()} />
            <Row k="Attention heads" v={g?.head_count ? `${g.head_count} (${g.head_count_kv ?? g.head_count} for the cache)` : null} />
            <Row k="Chat template" v={g ? (g.has_chat_template ? "yes" : "missing: replies may be poor") : null} />
            <Row k="Thinking switch" v={m.supports_thinking_toggle ? "supported" : "not supported"} />
          </dl>
        </div>
      </SheetContent>
    </Dialog>
  );
}
