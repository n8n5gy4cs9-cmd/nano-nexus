import { X } from "lucide-react";

import type { AgentView } from "@/bindings";
import { SOURCE_NAME } from "@/lib/activity-layout";
import { formatDuration, relativeTime } from "@/lib/format";

import { type Attention, STATUS_LABEL, STATUS_VAR, formatTokens } from "./types";

function Stat({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-1">
      <dt className="font-mono text-[10px] uppercase tracking-[0.12em] text-faint">{label}</dt>
      <dd className="min-w-0 truncate text-right font-mono text-small text-text">{children}</dd>
    </div>
  );
}

export function StatusDot({ status }: { status: AgentView["status"] }) {
  return <span aria-hidden className="inline-block size-2 rounded-full" style={{ background: `var(${STATUS_VAR[status]})` }} />;
}

const cardCls = "pointer-events-auto w-72 rounded-card border border-border bg-surface/85 p-3 shadow-2 backdrop-blur";

export function AgentCard({ agent, now, onClose }: { agent: AgentView; now: number; onClose: () => void }) {
  const t = agent.tokens;
  return (
    <section aria-label="Agent details" className={cardCls}>
      <header className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-body font-semibold">{agent.label}</div>
          <div className="flex items-center gap-1.5 text-small text-muted">
            <StatusDot status={agent.status} /> {STATUS_LABEL[agent.status]} · {SOURCE_NAME[agent.source]}
          </div>
        </div>
        <button type="button" aria-label="Close details" className="rounded-chip p-1 text-muted hover:bg-surface-2 hover:text-text" onClick={onClose}>
          <X className="size-4" />
        </button>
      </header>
      <dl className="mt-2 divide-y divide-border">
        <Stat label="Model">{agent.model ?? "—"}</Stat>
        <Stat label="Tool now">{agent.tool ?? "—"}</Stat>
        <Stat label="New tokens">{formatTokens(t.input + t.output + t.cache_write)}</Stat>
        <Stat label="In / out">
          {formatTokens(t.input)} / {formatTokens(t.output)}
        </Stat>
        <Stat label="Cache read">{formatTokens(t.cache_read)}</Stat>
        <Stat label="Run time">{formatDuration(now - agent.started_ms)}</Stat>
        <Stat label="Last activity">{relativeTime(agent.last_ms, now)}</Stat>
        <Stat label="Events">{agent.events}</Stat>
        <Stat label="Errors">{agent.errors}</Stat>
      </dl>
    </section>
  );
}

export function GroupCard({ title, subtitle, agents, now, onSelect, onClose }: { title: string; subtitle: string; agents: AgentView[]; now: number; onSelect: (id: string) => void; onClose: () => void }) {
  return (
    <section aria-label={title} className={cardCls}>
      <header className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-body font-semibold">{title}</div>
          <div className="text-small text-muted">{subtitle}</div>
        </div>
        <button type="button" aria-label="Close details" className="rounded-chip p-1 text-muted hover:bg-surface-2 hover:text-text" onClick={onClose}>
          <X className="size-4" />
        </button>
      </header>
      <ul className="mt-2 max-h-64 divide-y divide-border overflow-auto">
        {agents.map((a) => (
          <li key={a.id}>
            <button type="button" onClick={() => onSelect(a.id)} className="flex w-full items-center gap-2 py-1.5 text-left hover:text-accent">
              <StatusDot status={a.status} />
              <span className="min-w-0 flex-1 truncate font-mono text-small">{a.label}</span>
              <span className="font-mono text-caption text-faint">{a.status === "running" ? (a.tool ?? "working") : relativeTime(a.last_ms, now)}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

const KIND_LABEL: Record<Attention["kind"], string> = { conflict: "File conflict", errored: "Error", stalled: "Stalled" };

export function AttentionCard({ items, onSelect }: { items: Attention[]; onSelect: (id: string) => void }) {
  if (items.length === 0) return null;
  return (
    <section aria-label="Needs attention" className={cardCls}>
      <h2 className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">Needs attention</h2>
      <ul className="mt-1 divide-y divide-border">
        {items.slice(0, 8).map((i) => (
          <li key={i.id}>
            <button type="button" onClick={() => i.targets[0] && onSelect(i.targets[0])} className="block w-full py-1.5 text-left hover:text-accent">
              <div className="flex items-center gap-2">
                <span aria-hidden className="inline-block size-2 rounded-full" style={{ background: i.kind === "stalled" ? "var(--act-stalled)" : "var(--act-errored)" }} />
                <span className="min-w-0 flex-1 truncate font-mono text-small">{i.title}</span>
                <span className="text-caption text-faint">{KIND_LABEL[i.kind]}</span>
              </div>
              <div className="pl-4 text-caption text-muted">{i.detail}</div>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
