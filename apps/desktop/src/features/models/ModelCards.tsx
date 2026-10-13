import { Check, Download, Info, Pause, Play, Trash2, X } from "lucide-react";
import type { ReactNode } from "react";

import type { Badge as FitBadgeKind, CatalogCard, DownloadView, Fit, InstalledModel } from "@/bindings";
import { Badge, Button, IconButton, Progress, ProgressRing, Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui";
import { formatBytes, formatDuration } from "@/lib/format";

const GB = 1_073_741_824;

/** "needs ≈ 1.6 GB of 11.2 GB usable" */
export function describeFit(f: Fit): string {
  return `needs ≈ ${(f.need_bytes / GB).toFixed(1)} GB of ${(f.budget_bytes / GB).toFixed(1)} GB usable`;
}

const FIT: Record<FitBadgeKind, { label: string; tone: "success" | "warning" | "danger" }> = {
  great: { label: "Runs great", tone: "success" },
  tight: { label: "Tight fit", tone: "warning" },
  too_big: { label: "Too big", tone: "danger" },
};

export function FitBadge({ fit }: { fit: Fit }) {
  const f = FIT[fit.badge];
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span tabIndex={0}>
          <Badge tone={f.tone}>{f.label}</Badge>
        </span>
      </TooltipTrigger>
      <TooltipContent>{describeFit(fit)}</TooltipContent>
    </Tooltip>
  );
}

const CAP_LABEL: Record<string, string> = {
  chat: "Chat",
  code: "Code",
  thinking: "Thinking",
  "tools-native": "Tools",
  "tools-xml": "Tools",
  vision: "Vision",
  multilingual: "Multilingual",
  "long-context": "Long context",
};

export function CapabilityChips({ caps }: { caps: string[] }) {
  const labels = [...new Set(caps.map((c) => CAP_LABEL[c] ?? c))];
  return (
    <div className="flex flex-wrap gap-1">
      {labels.map((l) => (
        <Badge key={l}>{l}</Badge>
      ))}
    </div>
  );
}

export function speedText(d: DownloadView): string {
  const bps = d.bytes_per_sec ?? 0;
  const parts: string[] = [];
  if (bps > 0) parts.push(`${formatBytes(bps)}/s`);
  if (d.eta_secs !== null && d.status === "running") parts.push(`${formatDuration(d.eta_secs * 1000)} left`);
  return parts.join(" · ");
}

const PHASE: Record<string, string> = { preparing: "Checking what you already have…", verifying: "Verifying…", finalizing: "Finishing…" };

/** Progress ring + bar + controls for one download. */
export function DownloadStrip({ d, onPause, onResume, onCancel }: { d: DownloadView; onPause: () => void; onResume: () => void; onCancel: () => void }) {
  const frac = d.bytes > 0 ? d.done_bytes / d.bytes : 0;
  const running = d.status === "running";
  const phase = running && d.phase && d.phase !== "downloading" ? PHASE[d.phase] : null;
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <ProgressRing value={frac} size={22} label={`${d.file} progress`} />
        <div className="min-w-0 flex-1">
          <Progress value={frac} label={`${d.file} download`} />
        </div>
        <span className="tnum w-10 text-right text-small text-muted">{Math.round(frac * 100)}%</span>
        {running || d.status === "queued" ? (
          <IconButton aria-label={`Pause ${d.file}`} size="sm" onClick={onPause}>
            <Pause />
          </IconButton>
        ) : (
          <IconButton aria-label={`Resume ${d.file}`} size="sm" onClick={onResume}>
            <Play />
          </IconButton>
        )}
        <IconButton aria-label={`Cancel ${d.file}`} size="sm" onClick={onCancel}>
          <X />
        </IconButton>
      </div>
      <p className="text-caption text-faint">
        {d.status === "queued" ? "Waiting for the current download…" : d.status === "paused" ? `Paused · ${formatBytes(d.done_bytes)} of ${formatBytes(d.bytes)}` : (phase ?? `${formatBytes(d.done_bytes)} of ${formatBytes(d.bytes)} · ${speedText(d)}`)}
      </p>
      {d.status === "failed" && d.error ? <p className="text-small text-danger">{d.error}</p> : null}
    </div>
  );
}

function Card({ children, active }: { children: ReactNode; active?: boolean }) {
  return (
    <div className={`flex flex-col gap-3 rounded-card border bg-surface p-4 shadow-1 ${active ? "border-accent shadow-[0_0_0_1px_var(--accent)]" : "border-border"}`}>{children}</div>
  );
}

export function ActivePill() {
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-accent/15 px-2 py-0.5 text-caption font-semibold text-accent shadow-[0_0_12px_var(--accent)]">
      <Check className="size-3" /> Active
    </span>
  );
}

const SOURCE_LABEL: Record<string, string> = { builtin: "Bundled", download: "Downloaded", import: "Imported", discovered: "Found in folder" };

export function InstalledCard({
  m,
  busy,
  onUse,
  onDelete,
  onDetails,
}: {
  m: InstalledModel;
  busy: boolean;
  onUse: () => void;
  onDelete: () => void;
  onDetails: () => void;
}) {
  return (
    <Card active={m.active}>
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 className="truncate text-body font-semibold" title={m.name}>
            {m.name}
          </h3>
          <p className="text-small text-muted">
            {[m.quant, formatBytes(m.bytes), m.license].filter(Boolean).join(" · ")}
          </p>
        </div>
        {m.active ? <ActivePill /> : null}
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge>{SOURCE_LABEL[m.source] ?? m.source}</Badge>
        <FitBadge fit={m.fit} />
        {!m.file_exists ? <Badge tone="danger">File missing</Badge> : null}
        {!m.has_chat_template ? <Badge tone="warning">No chat template</Badge> : null}
      </div>
      <CapabilityChips caps={m.capabilities} />
      <div className="mt-auto flex items-center justify-between gap-2">
        <IconButton aria-label={`Details of ${m.name}`} size="sm" onClick={onDetails}>
          <Info />
        </IconButton>
        <div className="flex gap-2">
          {m.removal !== "protected" ? (
            <Button size="sm" variant="ghost" onClick={onDelete} disabled={m.active} aria-label={m.removal === "unlink" ? `Remove ${m.name} from the list` : `Delete ${m.name}`}>
              <Trash2 /> {m.removal === "unlink" ? "Unlink" : "Delete"}
            </Button>
          ) : null}
          <Button size="sm" variant="primary" onClick={onUse} disabled={m.active || busy || !m.file_exists}>
            {m.active ? "In use" : "Use it"}
          </Button>
        </div>
      </div>
    </Card>
  );
}

export function CatalogCardView({
  c,
  onDownload,
  onUse,
  onPause,
  onResume,
  onCancel,
  activeId,
  busy,
}: {
  c: CatalogCard;
  onDownload: () => void;
  onUse: () => void;
  onPause: () => void;
  onResume: () => void;
  onCancel: () => void;
  activeId: string | null;
  busy: boolean;
}) {
  const e = c.entry;
  const installed = c.installed_id !== null;
  const isActive = installed && c.installed_id === activeId;
  return (
    <Card active={isActive}>
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 className="truncate text-body font-semibold" title={e.name}>
            {e.name}
          </h3>
          <p className="text-small text-muted">
            {e.params_b} B · {e.quant} · ≈ {formatBytes(e.approx_bytes)}
          </p>
        </div>
        {isActive ? <ActivePill /> : null}
      </div>
      {e.why ? <p className="text-small text-muted">{e.why}</p> : null}
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge tone="accent">{e.family}</Badge>
        <FitBadge fit={c.fit} />
        {!e.verified ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span tabIndex={0}>
                <Badge tone="warning">Community: unverified</Badge>
              </span>
            </TooltipTrigger>
            <TooltipContent>The repository was not confirmed when the catalog was written.</TooltipContent>
          </Tooltip>
        ) : null}
        {e.isDefault ? <Badge tone="success">Default</Badge> : null}
        {e.tags?.includes("librarian") ? <Badge tone="accent">Librarian</Badge> : null}
      </div>
      <CapabilityChips caps={e.capabilities} />
      <p className="truncate text-caption text-faint" title={e.license}>
        {e.license}
      </p>
      {c.download && c.download.status !== "done" ? (
        <DownloadStrip d={c.download} onPause={onPause} onResume={onResume} onCancel={onCancel} />
      ) : (
        <div className="mt-auto flex justify-end">
          {installed ? (
            <Button size="sm" variant="primary" onClick={onUse} disabled={isActive || busy}>
              {isActive ? "In use" : "Use it"}
            </Button>
          ) : (
            <Button size="sm" variant="primary" onClick={onDownload}>
              <Download /> Download
            </Button>
          )}
        </div>
      )}
    </Card>
  );
}
