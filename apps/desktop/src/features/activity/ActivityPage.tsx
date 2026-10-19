import { Activity as ActivityIcon, Minus, Plus, SlidersHorizontal } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { ActivitySnapshot, ObserveView } from "@/bindings";
import { Button, Popover, PopoverContent, PopoverTrigger, Skeleton } from "@/components/ui";
import { type GNode, SOURCE_NAME, buildLayout } from "@/lib/activity-layout";
import { api } from "@/lib/api";
import { useUi } from "@/stores/ui";

import { AgentCard, AttentionCard, GroupCard } from "./DetailPanel";
import { GraphCanvas, type GraphHandle } from "./GraphCanvas";
import { ReplayBar } from "./ReplayBar";
import { SourcesPanel } from "./SourcesPanel";
import { STATUS_LABEL, STATUS_VAR, attentionItems, clock, formatTokens, formatUptime } from "./types";
import { OBSERVE_KEY, useObserve } from "./useObserve";

const label = "font-mono text-[10px] uppercase tracking-[0.14em] text-faint";
const REPLAY_START_BACK_MS = 30 * 60_000;

function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() => typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true);
  useEffect(() => {
    const mq = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!mq) return;
    const on = () => setReduced(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return reduced;
}

function Metric({ name, value, of }: { name: string; value: string | number; of?: string | number }) {
  return (
    <div className="min-w-0">
      <div className={label}>{name}</div>
      <div className="font-mono text-[22px] leading-7 tabular-nums text-text">
        {value}
        {of === undefined ? null : <span className="ml-1 text-small text-faint">/ {of}</span>}
      </div>
    </div>
  );
}

/** The live graph of what is running: NanoNexus, and (opt-in) Claude Code and Codex. */
export function ActivityPage() {
  const observe = useObserve();
  const qc = useQueryClient();
  const view = observe.data;
  const reduced = useReducedMotion();
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  const graph = useRef<GraphHandle>(null);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [hover, setHover] = useState<{ node: GNode; x: number; y: number } | null>(null);
  const [scale, setScale] = useState(1);
  const [replay, setReplay] = useState<{ t: number; playing: boolean; speed: number } | null>(null);
  const [frame, setFrame] = useState<ActivitySnapshot | null>(null);
  const seq = useRef(0);

  // Replay: fetch the frame for the chosen time (stale answers are dropped).
  const replayT = replay?.t;
  useEffect(() => {
    if (replayT === undefined) return;
    const mine = ++seq.current;
    const id = window.setTimeout(() => {
      api.observe
        .replay(replayT)
        .then((f) => {
          if (seq.current === mine) setFrame(f);
        })
        .catch(() => undefined);
    }, 60);
    return () => window.clearTimeout(id);
  }, [replayT]);

  // Replay: play forward in real time × speed; reaching the end returns to live.
  const playing = replay?.playing === true;
  const speed = replay?.speed ?? 1;
  const replayTo = view?.replay_to ?? null;
  useEffect(() => {
    if (!playing || replayTo === null) return;
    const id = window.setInterval(() => {
      setReplay((r) => {
        if (!r) return r;
        const t = r.t + 250 * r.speed;
        return t >= replayTo ? null : { ...r, t };
      });
    }, 250);
    return () => window.clearInterval(id);
  }, [playing, speed, replayTo]);

  const live = view?.snapshot;
  const snap = replay && frame ? frame : live;
  const layout = useMemo(() => (snap ? buildLayout(snap) : null), [snap]);
  const attention = useMemo(() => (snap ? attentionItems(snap) : []), [snap]);
  const onHover = useCallback((h: { node: GNode; x: number; y: number } | null) => setHover(h), []);
  const onSelect = useCallback((id: string | null) => setSelectedId(id), []);
  const setDemo = useCallback(
    (on: boolean) => {
      setReplay(null);
      setSelectedId(null);
      api.observe
        .demo(on)
        .then((v) => qc.setQueryData<ObserveView>(OBSERVE_KEY, v))
        .catch(() => undefined);
    },
    [qc],
  );

  if (observe.isPending) return <Skeleton className="m-6 h-96 w-[min(60rem,100%)]" />;
  if (!view || !snap || !layout) return <p className="p-6 text-muted">Activity is not available here.</p>;

  const now = snap.at_ms;
  const empty = snap.agents.length === 0;
  const selected = selectedId ? snap.agents.find((a) => a.id === selectedId) : undefined;
  const selectedProject = selectedId ? snap.projects.find((p) => p.id === selectedId) : undefined;
  const selectedHub = selectedId?.startsWith("hub:") ? selectedId.slice(4) : null;
  const driftBad = snap.drift.lines > 100 && snap.drift.bad * 20 > snap.drift.lines;
  const resolvedDark = typeof document === "undefined" || document.documentElement.dataset.theme !== "light";
  const hasHistory = view.replay_from !== null && view.replay_to !== null && view.replay_to > view.replay_from;

  const startReplay = () => {
    if (!hasHistory) return;
    const to = view.replay_to as number;
    const from = view.replay_from as number;
    setFrame(null);
    setReplay({ t: Math.max(from, to - REPLAY_START_BACK_MS), playing: false, speed: 4 });
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 p-3">
      {/* Header */}
      <header className="flex flex-wrap items-center gap-x-4 gap-y-2 px-1">
        <div className="flex items-center gap-2">
          <ActivityIcon className="size-4 text-accent" aria-hidden />
          <h1 className="font-mono text-small font-semibold uppercase tracking-[0.14em]">Live activity</h1>
        </div>
        <p className="font-mono text-caption text-muted" aria-live="polite">
          {snap.projects.length} projects · {snap.totals.agents} agents · {snap.totals.active} running
        </p>
        {snap.conflicts.length > 0 ? (
          <span role="status" className="rounded-chip border border-danger/60 bg-danger/10 px-2 py-0.5 font-mono text-[10px] uppercase tracking-[0.12em] text-danger">
            {snap.conflicts.length} file conflict{snap.conflicts.length === 1 ? "" : "s"}
          </span>
        ) : null}
        <div className="ml-auto flex items-center gap-2">
          {view.demo ? (
            <button type="button" onClick={() => setDemo(false)} title="Back to real activity" className="rounded-chip border border-accent/50 bg-accent/10 px-2 py-0.5 font-mono text-[10px] uppercase tracking-[0.12em] text-accent hover:bg-accent/20">
              Demo data · exit
            </button>
          ) : null}
          <span className="flex items-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.12em] text-muted">
            <span aria-hidden className={`size-1.5 rounded-full ${replay ? "bg-faint" : "bg-act-running"}`} />
            {replay ? "Replay" : "Live"} · read-only
          </span>
          <Button size="sm" variant={replay ? "secondary" : "ghost"} disabled={!hasHistory && !replay} onClick={() => (replay ? setReplay(null) : startReplay())} aria-pressed={replay !== null}>
            Replay
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setTheme(resolvedDark ? "light" : "dark")} aria-label="Switch light and dark" title={theme === "system" ? "Following the system theme" : undefined}>
            {resolvedDark ? "Dark" : "Light"}
          </Button>
          <Popover>
            <PopoverTrigger asChild>
              <Button size="sm" variant="ghost" aria-label="Choose what to watch">
                <SlidersHorizontal /> Sources
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-[26rem]">
              <SourcesPanel view={view} />
            </PopoverContent>
          </Popover>
        </div>
      </header>

      {driftBad ? (
        <p role="note" className="rounded-card border border-warning/40 bg-warning/10 px-3 py-1.5 text-small text-warning">
          {snap.drift.bad} of {snap.drift.lines} log lines could not be read. The log format of Claude Code or Codex may have changed, so some activity can be missing.
        </p>
      ) : null}

      {/* Canvas */}
      <div className="relative min-h-[320px] flex-1 overflow-hidden rounded-card border border-border bg-act-bg bg-[radial-gradient(ellipse_at_40%_45%,color-mix(in_oklch,var(--act-bg),var(--text)_6%),transparent_70%)]">
        <GraphCanvas ref={graph} snapshot={snap} selectedId={selectedId} onSelect={onSelect} onHover={onHover} onScale={setScale} reducedMotion={reduced} />

        {/* Top left: numbers */}
        <dl className="pointer-events-none absolute top-3 left-4 flex gap-6 border-l border-border pl-4">
          <Metric name="Tasks" value={snap.totals.tasks_done} of={snap.totals.tasks} />
          <Metric name="Events" value={snap.totals.events} />
          <Metric name="Active" value={snap.totals.active} of={snap.totals.agents} />
          <Metric name="Uptime" value={formatUptime(snap.totals.uptime_ms)} />
        </dl>

        {/* Top right: zoom and tokens */}
        <div className="absolute top-3 right-4 flex flex-col items-end gap-3">
          <div className="flex items-center gap-1 font-mono text-caption text-muted">
            <button type="button" aria-label="Zoom out" className="rounded-chip p-1 hover:bg-surface-2 hover:text-text" onClick={() => graph.current?.zoomBy(0.8)}>
              <Minus className="size-3.5" />
            </button>
            <span className="w-11 text-center tabular-nums" aria-live="off">
              {Math.round(scale * 100)}%
            </span>
            <button type="button" aria-label="Zoom in" className="rounded-chip p-1 hover:bg-surface-2 hover:text-text" onClick={() => graph.current?.zoomBy(1.25)}>
              <Plus className="size-3.5" />
            </button>
            <button type="button" className="ml-1 rounded-chip px-1.5 py-0.5 uppercase tracking-[0.12em] text-faint hover:bg-surface-2 hover:text-text" onClick={() => graph.current?.fit()}>
              Fit
            </button>
          </div>
          <div className="pointer-events-none text-right">
            <div className={label}>Tokens</div>
            <div className="font-mono text-[26px] leading-8 tabular-nums">
              {formatTokens(snap.totals.new_tokens)}
              <span className="ml-1.5 text-small text-muted">new tok</span>
            </div>
            <div className="mt-1 ml-auto h-px w-28 bg-gradient-to-r from-transparent via-act-running to-transparent opacity-70" />
            <div className="mt-1 font-mono text-caption text-faint">{formatTokens(snap.totals.cache_read_tokens)} from cache</div>
          </div>
        </div>

        {/* Side panel */}
        <div className="pointer-events-none absolute top-40 right-4 bottom-12 flex w-72 flex-col items-end gap-2 overflow-hidden">
          {selected ? (
            <AgentCard agent={selected} now={now} onClose={() => setSelectedId(null)} />
          ) : selectedProject ? (
            <GroupCard title={selectedProject.label} subtitle={`${selectedProject.agents} agents · ${selectedProject.running} running`} agents={snap.agents.filter((a) => a.project_id === selectedProject.id)} now={now} onSelect={setSelectedId} onClose={() => setSelectedId(null)} />
          ) : selectedHub ? (
            <GroupCard title={SOURCE_NAME[selectedHub as keyof typeof SOURCE_NAME] ?? selectedHub} subtitle="All agents of this tool" agents={snap.agents.filter((a) => a.source === selectedHub)} now={now} onSelect={setSelectedId} onClose={() => setSelectedId(null)} />
          ) : (
            <AttentionCard items={attention} onSelect={setSelectedId} />
          )}
        </div>

        {/* Hover tooltip */}
        {hover && hover.node.kind === "agent" ? (
          <div role="tooltip" className="pointer-events-none absolute z-10 max-w-64 rounded-chip border border-border bg-surface px-2 py-1 font-mono text-caption shadow-2" style={{ left: Math.min(hover.x + 14, 9999), top: hover.y + 14 }}>
            <div className="text-text">{hover.node.label}</div>
            <div className="text-faint">{hover.node.status ? STATUS_LABEL[hover.node.status] : ""}</div>
          </div>
        ) : null}

        {/* Bottom: legend and clock */}
        <div className="pointer-events-none absolute right-4 bottom-3 left-4 flex items-center justify-between font-mono text-[10px] uppercase tracking-[0.12em] text-muted">
          <ul className="flex gap-4" aria-label="Legend">
            {(["running", "idle", "stalled", "errored"] as const).map((s) => (
              <li key={s} className="flex items-center gap-1.5">
                <span aria-hidden className="size-2 rounded-full" style={{ background: `var(${STATUS_VAR[s]})` }} />
                {s}
              </li>
            ))}
          </ul>
          <span className="flex items-center gap-1.5 text-faint">
            <span aria-hidden className="size-1.5 rounded-full bg-faint" />
            {replay ? "Replaying" : "Updated"} {clock(now)}
          </span>
        </div>

        {/* Empty / first run */}
        {empty ? <EmptyOverlay view={view} onDemo={() => setDemo(true)} /> : null}
      </div>

      {replay && hasHistory ? (
        <ReplayBar
          from={view.replay_from as number}
          to={view.replay_to as number}
          value={replay.t}
          playing={replay.playing}
          speed={replay.speed}
          onChange={(t) => setReplay((r) => (r ? { ...r, t, playing: false } : r))}
          onPlaying={(p) => setReplay((r) => (r ? { ...r, playing: p } : r))}
          onSpeed={(s) => setReplay((r) => (r ? { ...r, speed: s } : r))}
          onExit={() => setReplay(null)}
        />
      ) : null}

      {/* Footer */}
      <footer className="flex flex-wrap items-center gap-x-5 gap-y-1 px-1 font-mono text-[10px] uppercase tracking-[0.12em] text-muted">
        {snap.sources.map((s) => (
          <span key={s.source} className={`flex items-center gap-1.5 ${s.enabled ? "" : "opacity-50"}`}>
            <span aria-hidden className="size-1.5 rounded-full" style={{ background: s.running > 0 ? "var(--act-running)" : s.enabled ? "var(--act-idle)" : "transparent", border: s.enabled ? "none" : "1px solid var(--act-idle)" }} />
            {s.label} {s.enabled ? s.agents : "off"}
          </span>
        ))}
        <span className="ml-auto text-faint">
          {snap.totals.tasks} tasks · {snap.totals.events} events
        </span>
      </footer>
    </div>
  );
}

function EmptyOverlay({ view, onDemo }: { view: ObserveView; onDemo: () => void }) {
  return (
    <div className="absolute inset-0 flex items-center justify-center p-6">
      <div className="w-[min(30rem,100%)] rounded-card border border-border bg-surface/90 p-5 shadow-2 backdrop-blur">
        <h2 className="text-lead font-semibold">Nothing is running right now</h2>
        <p className="mt-1 text-body text-muted">
          This page draws every chat, tool call and job as it happens. Turn on Claude Code or Codex to watch them too (read-only, never your prompts or code), or look at demo data first.
        </p>
        <div className="mt-4">
          <SourcesPanel view={view} compact />
        </div>
        <div className="mt-4 flex justify-end">
          <Button variant="secondary" onClick={onDemo}>
            Show demo data
          </Button>
        </div>
      </div>
    </div>
  );
}
