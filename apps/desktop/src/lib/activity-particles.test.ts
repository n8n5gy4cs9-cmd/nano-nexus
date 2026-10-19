import { describe, expect, it } from "vitest";

import type { Layout } from "./activity-layout";
import { MAX_PARTICLES, Particles, edgeGeometry, mulberry32 } from "./activity-particles";

function layout(rate: number): Layout {
  const mk = (id: string, x: number) => ({ id, kind: "agent" as const, x, y: 0, r: 8, label: id, sub: "", sub2: "", source: "claude_code" as const, status: null, parent: null, rate: 0, live: false, isMain: false });
  const a = mk("a", 0);
  const b = mk("b", 300);
  return { nodes: [a, b], byId: new Map([["a", a], ["b", b]]), edges: [{ from: "a", to: "b", rate, live: true }], bounds: { minX: 0, minY: 0, maxX: 300, maxY: 0 } };
}

describe("Particles", () => {
  it("spawns in proportion to the edge rate and removes finished specks", () => {
    const geoms = edgeGeometry(layout(4));
    const p = new Particles();
    const rng = mulberry32(1);
    for (let i = 0; i < 60; i++) p.step(1 / 60, geoms, 1, rng);
    expect(p.count).toBeGreaterThan(15);
    expect(p.count).toBeLessThan(60);
    for (let i = 0; i < 60 * 8; i++) p.step(1 / 60, geoms, 1, rng);
    // 6 s later the first ones crossed the 300-unit edge (at most 110 units/s).
    for (let i = 0; i < p.count; i++) expect(p.t[i]).toBeLessThan(1);
  });

  it("spawns nothing for a silent edge", () => {
    const p = new Particles();
    p.step(1, edgeGeometry(layout(0)), 1, mulberry32(2));
    expect(p.count).toBe(0);
  });

  it("respects the cap and the quality scale", () => {
    const geoms = edgeGeometry(layout(8));
    const rng = mulberry32(3);
    const full = new Particles();
    const low = new Particles();
    for (let i = 0; i < 600; i++) {
      full.step(1 / 60, geoms, 1, rng);
      low.step(1 / 60, geoms, 0.2, rng);
    }
    expect(full.count).toBeLessThanOrEqual(MAX_PARTICLES);
    expect(low.count).toBeLessThan(full.count);
  });

  it("keeps positions between the two ends", () => {
    const geoms = edgeGeometry(layout(6));
    const p = new Particles();
    const rng = mulberry32(4);
    for (let i = 0; i < 120; i++) p.step(1 / 60, geoms, 1, rng);
    for (let i = 0; i < p.count; i++) {
      const pt = p.at(i, geoms);
      if (!pt) throw new Error("no position");
      expect(pt.x).toBeGreaterThanOrEqual(-1);
      expect(pt.x).toBeLessThanOrEqual(301);
      expect(Math.abs(pt.y)).toBeLessThanOrEqual(12.5);
    }
  });

  it("is deterministic for a seed", () => {
    const run = () => {
      const p = new Particles();
      const rng = mulberry32(9);
      for (let i = 0; i < 100; i++) p.step(1 / 60, edgeGeometry(layout(3)), 1, rng);
      return [p.count, p.t[0], p.off[0]];
    };
    expect(run()).toEqual(run());
  });
});
