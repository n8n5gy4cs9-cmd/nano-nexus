/**
 * Particle streams of the Activity graph: specks that travel from an agent to its project and on to
 * the hub, spawned at a rate that follows the events per second of each edge. Struct-of-arrays and a
 * hard cap keep it cheap; the spawn rate scales with `quality`, which the renderer lowers when
 * frames take too long (OBSERVE.md, M19).
 */
import type { GEdge, Layout } from "./activity-layout";

export const MAX_PARTICLES = 1500;
/** Specks per event per second (a busy agent draws a dense stream, an idle one draws none). */
const PER_EVENT = 9;
/** Travel speed in world units per second. */
const SPEED_MIN = 45;
const SPEED_MAX = 110;

export type Rng = () => number;

/** Deterministic generator for tests and a stable first frame. */
export function mulberry32(seed: number): Rng {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export interface EdgeGeom {
  edge: GEdge;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  len: number;
  /** Unit normal. */
  nx: number;
  ny: number;
}

export function edgeGeometry(layout: Layout): EdgeGeom[] {
  const out: EdgeGeom[] = [];
  for (const e of layout.edges) {
    const a = layout.byId.get(e.from);
    const b = layout.byId.get(e.to);
    if (!a || !b) continue;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy) || 1;
    out.push({ edge: e, x0: a.x, y0: a.y, x1: b.x, y1: b.y, len, nx: -dy / len, ny: dx / len });
  }
  return out;
}

export class Particles {
  /** Edge index, progress 0..1, speed (units/s), sideways offset, alpha, tint (0 = stream, 1 = running). */
  edge = new Int32Array(MAX_PARTICLES);
  t = new Float32Array(MAX_PARTICLES);
  speed = new Float32Array(MAX_PARTICLES);
  off = new Float32Array(MAX_PARTICLES);
  alpha = new Float32Array(MAX_PARTICLES);
  tint = new Uint8Array(MAX_PARTICLES);
  count = 0;
  /** Fractional specks owed to each edge. */
  private owed: number[] = [];

  clear() {
    this.count = 0;
    this.owed = [];
  }

  /** Advance by `dt` seconds. `quality` (0.2..1) scales both the spawn rate and the cap. */
  step(dt: number, geoms: EdgeGeom[], quality: number, rng: Rng) {
    // Move, and swap-remove the finished ones.
    for (let i = 0; i < this.count; ) {
      const g = geoms[this.edge[i] ?? -1];
      if (!g) {
        this.removeAt(i);
        continue;
      }
      const t = (this.t[i] ?? 1) + ((this.speed[i] ?? 0) * dt) / g.len;
      this.t[i] = t;
      if (t >= 1) this.removeAt(i);
      else i++;
    }
    const cap = Math.floor(MAX_PARTICLES * Math.min(1, Math.max(0.1, quality)));
    if (this.owed.length !== geoms.length) this.owed = new Array<number>(geoms.length).fill(0);
    for (let k = 0; k < geoms.length; k++) {
      const g = geoms[k];
      if (!g || g.edge.rate <= 0.01) continue;
      let owed = (this.owed[k] ?? 0) + g.edge.rate * PER_EVENT * dt * Math.min(1, quality + 0.15);
      while (owed >= 1) {
        owed -= 1;
        if (this.count >= cap) {
          owed = 0;
          break;
        }
        const i = this.count++;
        this.edge[i] = k;
        this.t[i] = rng() * 0.04;
        this.speed[i] = SPEED_MIN + rng() * (SPEED_MAX - SPEED_MIN);
        this.off[i] = (rng() - rng()) * 12;
        this.alpha[i] = 0.3 + rng() * 0.6;
        this.tint[i] = g.edge.live && rng() < 0.3 ? 1 : 0;
      }
      this.owed[k] = owed;
    }
  }

  /** Position of particle `i` in world space. */
  at(i: number, geoms: EdgeGeom[]): { x: number; y: number } | null {
    const g = geoms[this.edge[i] ?? -1];
    if (!g) return null;
    const t = this.t[i] ?? 0;
    // The stream fans out mid-way and gathers again at both ends.
    const side = (this.off[i] ?? 0) * Math.sin(Math.PI * t);
    return { x: g.x0 + (g.x1 - g.x0) * t + g.nx * side, y: g.y0 + (g.y1 - g.y0) * t + g.ny * side };
  }

  private removeAt(i: number) {
    const last = --this.count;
    if (i === last) return;
    this.edge[i] = this.edge[last] ?? 0;
    this.t[i] = this.t[last] ?? 1;
    this.speed[i] = this.speed[last] ?? 0;
    this.off[i] = this.off[last] ?? 0;
    this.alpha[i] = this.alpha[last] ?? 0;
    this.tint[i] = this.tint[last] ?? 0;
  }
}
