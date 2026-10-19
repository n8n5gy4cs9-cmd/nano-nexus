/**
 * Deterministic layout of the Activity graph (OBSERVE.md, M19): one hub per source, projects around
 * it, agents around their project. Positions depend only on the order things appeared, never on
 * timing, so a node stays where it is while others come and go.
 */
import type { ActivitySnapshot, ActivitySource, AgentStatus } from "@/bindings";

export type NodeKind = "hub" | "project" | "agent";

export interface GNode {
  id: string;
  kind: NodeKind;
  x: number;
  y: number;
  r: number;
  label: string;
  /** Second label line (hubs and projects). */
  sub: string;
  /** Third label line (hubs). */
  sub2: string;
  source: ActivitySource;
  /** Agents only. */
  status: AgentStatus | null;
  /** Project (agents) or hub (projects). */
  parent: string | null;
  /** Events per second flowing out of this node (drives particles). */
  rate: number;
  /** Anything in this node's cluster is running. */
  live: boolean;
  isMain: boolean;
}

export interface GEdge {
  from: string;
  to: string;
  rate: number;
  live: boolean;
}

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export interface Layout {
  nodes: GNode[];
  byId: Map<string, GNode>;
  edges: GEdge[];
  bounds: Bounds;
}

export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
const HUB_R = 34;
const PROJECT_R = 15;

/** FNV-1a: a small stable hash for phases. */
export function hash32(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** Radius of the cloud of `n` agents around their project. */
export function clusterRadius(n: number): number {
  return 60 + 32 * Math.sqrt(Math.max(1, n));
}

const clampRate = (v: number | null | undefined, max = 8) => Math.min(max, Math.max(0, v ?? 0));

export const SOURCE_NAME: Record<ActivitySource, string> = {
  nano_nexus: "NanoNexus",
  claude_code: "Claude Code",
  codex: "Codex",
};

function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

export function buildLayout(snap: ActivitySnapshot): Layout {
  const nodes: GNode[] = [];
  const edges: GEdge[] = [];
  const hubs = snap.sources.filter((s) => s.agents > 0);
  const ring = hubs.length > 1 ? 760 : 0;
  hubs.forEach((src, hi) => {
    const a = -Math.PI / 2 + (hi * 2 * Math.PI) / Math.max(1, hubs.length);
    const hx = ring * Math.cos(a);
    const hy = ring * Math.sin(a);
    const hubId = `hub:${src.source}`;
    const projects = snap.projects.filter((p) => p.source === src.source).sort((p, q) => p.id.localeCompare(q.id));
    const hubRate: number[] = [];
    projects.forEach((p, pi) => {
      const agents = snap.agents
        .filter((g) => g.project_id === p.id)
        .sort((g, h) => g.started_ms - h.started_ms || g.id.localeCompare(h.id));
      const rc = clusterRadius(agents.length);
      const d = HUB_R + 110 + rc * 1.2;
      const pa = pi * GOLDEN_ANGLE + (hubs.length > 1 ? a : 0.6);
      const px = hx + d * Math.cos(pa);
      const py = hy + d * Math.sin(pa);
      const projectId = p.id;
      const phase = (hash32(p.id) % 628) / 100;
      let projectRate = 0;
      agents.forEach((g, gi) => {
        const ga = gi * GOLDEN_ANGLE + phase;
        const gr = 50 + 32 * Math.sqrt(gi + 1);
        const rate = clampRate(g.events_per_s) + (g.status === "running" ? 0.6 : 0);
        projectRate += rate;
        nodes.push({
          id: g.id,
          kind: "agent",
          x: px + gr * Math.cos(ga),
          y: py + gr * Math.sin(ga),
          r: g.parent ? 8 : 10.5,
          label: g.label,
          sub: "",
          sub2: "",
          source: g.source,
          status: g.status,
          parent: projectId,
          rate,
          live: g.status === "running",
          isMain: !g.parent,
        });
        edges.push({ from: g.id, to: projectId, rate, live: g.status === "running" });
      });
      hubRate.push(projectRate);
      nodes.push({
        id: projectId,
        kind: "project",
        x: px,
        y: py,
        r: PROJECT_R,
        label: p.label,
        sub: `${plural(p.agents, "agent")} · ${p.running} running`,
        sub2: "",
        source: p.source,
        status: null,
        parent: hubId,
        rate: projectRate,
        live: p.running > 0,
        isMain: false,
      });
      edges.push({ from: projectId, to: hubId, rate: projectRate, live: p.running > 0 });
    });
    nodes.push({
      id: hubId,
      kind: "hub",
      x: hx,
      y: hy,
      r: HUB_R,
      label: SOURCE_NAME[src.source].toUpperCase(),
      sub: plural(src.agents, "AGENT").toUpperCase() + ` · ${plural(projects.length, "PROJECT")}`.toUpperCase(),
      sub2: `${src.running} running`,
      source: src.source,
      status: null,
      parent: null,
      rate: hubRate.reduce((a2, b) => a2 + b, 0),
      live: src.running > 0,
      isMain: false,
    });
  });
  const byId = new Map(nodes.map((n) => [n.id, n]));
  return { nodes, byId, edges, bounds: boundsOf(nodes) };
}

export function boundsOf(nodes: GNode[]): Bounds {
  if (nodes.length === 0) return { minX: -200, minY: -150, maxX: 200, maxY: 150 };
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const n of nodes) {
    // Leave room for the labels beside agents.
    const lw = n.kind === "agent" ? 150 : 70;
    minX = Math.min(minX, n.x - n.r - lw);
    maxX = Math.max(maxX, n.x + n.r + lw);
    minY = Math.min(minY, n.y - n.r - 20);
    maxY = Math.max(maxY, n.y + n.r + 56);
  }
  return { minX, minY, maxX, maxY };
}

export interface View {
  scale: number;
  tx: number;
  ty: number;
}

export const MIN_SCALE = 0.2;
export const MAX_SCALE = 4;

export interface Insets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

const NO_INSETS: Insets = { top: 24, right: 24, bottom: 24, left: 24 };

/** The view that shows all of `b` inside a `w × h` canvas, clear of the overlays (`insets`). */
export function fitView(b: Bounds, w: number, h: number, insets: Insets = NO_INSETS): View {
  const bw = Math.max(1, b.maxX - b.minX);
  const bh = Math.max(1, b.maxY - b.minY);
  const aw = Math.max(50, w - insets.left - insets.right);
  const ah = Math.max(50, h - insets.top - insets.bottom);
  const scale = Math.min(1.4, Math.max(MIN_SCALE, Math.min(aw / bw, ah / bh)));
  return {
    scale,
    tx: insets.left + aw / 2 - ((b.minX + b.maxX) / 2) * scale,
    ty: insets.top + ah / 2 - ((b.minY + b.maxY) / 2) * scale,
  };
}

/** Zoom by `factor` keeping the world point under screen point (`sx`, `sy`) fixed. */
export function zoomAt(v: View, factor: number, sx: number, sy: number): View {
  const scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, v.scale * factor));
  const k = scale / v.scale;
  return { scale, tx: sx - (sx - v.tx) * k, ty: sy - (sy - v.ty) * k };
}

export function toWorld(v: View, sx: number, sy: number): { x: number; y: number } {
  return { x: (sx - v.tx) / v.scale, y: (sy - v.ty) / v.scale };
}

/** The topmost node under a world point (agents win over projects win over hubs). */
export function hitTest(layout: Layout, wx: number, wy: number, slack = 4): GNode | null {
  let best: GNode | null = null;
  const rank = { agent: 3, project: 2, hub: 1 } as const;
  for (const n of layout.nodes) {
    const d = Math.hypot(n.x - wx, n.y - wy);
    if (d <= n.r + slack && (!best || rank[n.kind] > rank[best.kind])) best = n;
  }
  return best;
}
