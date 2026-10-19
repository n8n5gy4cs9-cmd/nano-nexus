import { describe, expect, it } from "vitest";

import type { ActivitySnapshot, AgentView } from "@/bindings";

import { MAX_SCALE, MIN_SCALE, boundsOf, buildLayout, fitView, hash32, hitTest, toWorld, zoomAt } from "./activity-layout";

function agent(id: string, project: string, over: Partial<AgentView> = {}): AgentView {
  return {
    id,
    label: id,
    source: "claude_code",
    project_id: project,
    parent: null,
    model: null,
    status: "idle",
    tool: null,
    tokens: { input: 0, output: 0, cache_read: 0, cache_write: 0 },
    started_ms: Number(hash32(id) % 1000),
    last_ms: 0,
    events: 1,
    messages: 0,
    errors: 0,
    events_per_s: 0,
    ...over,
  };
}

function snap(perProject: number[]): ActivitySnapshot {
  const agents: AgentView[] = [];
  const projects = perProject.map((n, pi) => {
    const id = `claude_code:/work/p${pi}`;
    for (let i = 0; i < n; i++) agents.push(agent(`claude_code:p${pi}a${i}`, id, { started_ms: i }));
    return { id, label: `work/p${pi}`, source: "claude_code" as const, agents: n, running: 0, new_tokens: 0, last_ms: 0 };
  });
  return {
    at_ms: 0,
    totals: { tasks: agents.length, tasks_done: 0, events: 0, active: 0, agents: agents.length, uptime_ms: 0, new_tokens: 0, cache_read_tokens: 0 },
    sources: [
      { source: "nano_nexus", label: "NanoNexus", enabled: true, agents: 0, running: 0, last_ms: null },
      { source: "claude_code", label: "Claude Code", enabled: true, agents: agents.length, running: 0, last_ms: 0 },
      { source: "codex", label: "Codex", enabled: false, agents: 0, running: 0, last_ms: null },
    ],
    projects,
    agents,
    conflicts: [],
    drift: { lines: 0, bad: 0 },
  };
}

describe("buildLayout", () => {
  it("makes one hub per source with agents, projects and agents under it", () => {
    const l = buildLayout(snap([7, 8, 4]));
    expect(l.nodes.filter((n) => n.kind === "hub")).toHaveLength(1);
    expect(l.nodes.filter((n) => n.kind === "project")).toHaveLength(3);
    expect(l.nodes.filter((n) => n.kind === "agent")).toHaveLength(19);
    expect(l.edges).toHaveLength(19 + 3);
    expect(l.byId.get("hub:claude_code")?.x).toBe(0);
    expect(l.nodes.every((n) => Number.isFinite(n.x) && Number.isFinite(n.y))).toBe(true);
  });

  it("never lets two nodes overlap, even with many agents", () => {
    for (const shape of [[7, 8, 4], [20, 3], [1], [30, 30, 30]]) {
      const l = buildLayout(snap(shape));
      for (let i = 0; i < l.nodes.length; i++) {
        for (let j = i + 1; j < l.nodes.length; j++) {
          const [a, b] = [l.nodes[i], l.nodes[j]];
          if (!a || !b) throw new Error("missing node");
          const d = Math.hypot(a.x - b.x, a.y - b.y);
          expect(d, `${a.id} vs ${b.id} in ${shape.join(",")}`).toBeGreaterThan(a.r + b.r);
        }
      }
    }
  });

  it("keeps existing nodes in place when an agent is added", () => {
    const a = buildLayout(snap([5, 5]));
    const next = snap([5, 5]);
    next.agents.push(agent("claude_code:new", "claude_code:/work/p0", { started_ms: 99 }));
    if (next.projects[0]) next.projects[0].agents = 6;
    const b = buildLayout(next);
    // Existing agents of the other project do not move; the grown project's own cluster can.
    const other = a.nodes.filter((n) => n.parent === "claude_code:/work/p1");
    for (const n of other) {
      const [m, pa, pb] = [b.byId.get(n.id), a.byId.get(n.parent ?? ""), b.byId.get(n.parent ?? "")];
      if (!m || !pa || !pb) throw new Error("missing node");
      expect(m.x - pb.x).toBeCloseTo(n.x - pa.x, 6);
      expect(m.y - pb.y).toBeCloseTo(n.y - pa.y, 6);
    }
  });

  it("spreads several sources on a ring", () => {
    const s = snap([2]);
    s.sources[2] = { source: "codex", label: "Codex", enabled: true, agents: 1, running: 0, last_ms: 0 };
    s.projects.push({ id: "codex:/x/y", label: "x/y", source: "codex", agents: 1, running: 0, new_tokens: 0, last_ms: 0 });
    s.agents.push(agent("codex:c1", "codex:/x/y", { source: "codex" }));
    const l = buildLayout(s);
    const hubs = l.nodes.filter((n) => n.kind === "hub");
    expect(hubs).toHaveLength(2);
    const [h0, h1] = hubs;
    if (!h0 || !h1) throw new Error("two hubs expected");
    expect(Math.hypot(h0.x - h1.x, h0.y - h1.y)).toBeGreaterThan(500);
  });

  it("is empty and still has finite bounds without agents", () => {
    const l = buildLayout(snap([]));
    expect(l.nodes).toHaveLength(0);
    expect(boundsOf(l.nodes).maxX).toBeGreaterThan(0);
  });
});

describe("view math", () => {
  it("fit centres the bounds and stays in the zoom limits", () => {
    const b = { minX: -100, minY: -50, maxX: 300, maxY: 150 };
    const v = fitView(b, 800, 400);
    expect(v.scale).toBeGreaterThanOrEqual(MIN_SCALE);
    expect(v.scale).toBeLessThanOrEqual(1.4);
    const c = toWorld(v, 400, 200);
    expect(c.x).toBeCloseTo(100, 6);
    expect(c.y).toBeCloseTo(50, 6);
  });

  it("fits inside the insets, not under the overlays", () => {
    const b = { minX: 0, minY: 0, maxX: 400, maxY: 200 };
    const v = fitView(b, 1000, 600, { top: 100, right: 300, bottom: 40, left: 20 });
    const left = b.minX * v.scale + v.tx;
    const right = b.maxX * v.scale + v.tx;
    expect(left).toBeGreaterThanOrEqual(20 - 1e-6);
    expect(right).toBeLessThanOrEqual(700 + 1e-6);
    expect(b.minY * v.scale + v.ty).toBeGreaterThanOrEqual(100 - 1e-6);
  });

  it("zooms around the cursor and clamps", () => {
    const v = { scale: 1, tx: 10, ty: 20 };
    const before = toWorld(v, 300, 200);
    const z = zoomAt(v, 2, 300, 200);
    const after = toWorld(z, 300, 200);
    expect(after.x).toBeCloseTo(before.x, 6);
    expect(after.y).toBeCloseTo(before.y, 6);
    expect(zoomAt(v, 1e6, 0, 0).scale).toBe(MAX_SCALE);
    expect(zoomAt(v, 1e-6, 0, 0).scale).toBe(MIN_SCALE);
  });

  it("hit-tests the smallest node first", () => {
    const l = buildLayout(snap([3]));
    const a = l.nodes.find((n) => n.kind === "agent");
    if (!a) throw new Error("no agent");
    expect(hitTest(l, a.x + 1, a.y)?.id).toBe(a.id);
    expect(hitTest(l, 99999, 99999)).toBeNull();
  });
});
