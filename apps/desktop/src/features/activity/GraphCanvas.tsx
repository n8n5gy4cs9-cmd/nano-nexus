import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef } from "react";

import type { ActivitySnapshot } from "@/bindings";
import { type GNode, type Insets, type Layout, type View, buildLayout, fitView, hitTest, toWorld, zoomAt } from "@/lib/activity-layout";
import { Particles, edgeGeometry, mulberry32 } from "@/lib/activity-particles";

import { STATUS_VAR } from "./types";

export interface GraphHandle {
  zoomBy: (factor: number) => void;
  fit: () => void;
}

interface Props {
  snapshot: ActivitySnapshot;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onHover: (hover: { node: GNode; x: number; y: number } | null) => void;
  /** Called when the zoom changes, with the scale (1 = 100 %). */
  onScale: (scale: number) => void;
  /** No animation: static dashes instead of moving specks. */
  reducedMotion: boolean;
}

interface Palette {
  bg: string;
  text: string;
  muted: string;
  faint: string;
  ring: string;
  stream: string;
  running: string;
  idle: string;
  stalled: string;
  errored: string;
  mono: string;
  sans: string;
}

const PALETTE_FALLBACK: Palette = {
  bg: "#0d111c",
  text: "#e8eaf2",
  muted: "#a3a8ba",
  faint: "#6f758a",
  ring: "#454a5e",
  stream: "#f2efe4",
  running: "#e9b949",
  idle: "#7d8294",
  stalled: "#e08a3c",
  errored: "#e5574b",
  mono: "ui-monospace, Menlo, monospace",
  sans: "system-ui, sans-serif",
};

/** Resolve the design tokens once per theme change (the canvas cannot read `var()` itself). */
function readPalette(): Palette {
  if (typeof document === "undefined") return PALETTE_FALLBACK;
  const cs = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => cs.getPropertyValue(name).trim() || fallback;
  const f = PALETTE_FALLBACK;
  return {
    bg: v("--act-bg", f.bg),
    text: v("--text", f.text),
    muted: v("--text-muted", f.muted),
    faint: v("--text-faint", f.faint),
    ring: v("--act-ring", f.ring),
    stream: v("--act-stream", f.stream),
    running: v(STATUS_VAR.running, f.running),
    idle: v(STATUS_VAR.idle, f.idle),
    stalled: v(STATUS_VAR.stalled, f.stalled),
    errored: v(STATUS_VAR.errored, f.errored),
    mono: v("--font-mono", f.mono),
    sans: v("--font-sans", f.sans),
  };
}

function statusColor(p: Palette, n: GNode): string {
  if (n.status) return p[n.status];
  return n.live ? p.running : p.idle;
}

/** Keep the graph clear of the stats strip, the side panel and the legend. */
function insetsFor(w: number): Insets {
  return { top: 84, right: w >= 900 ? 312 : 24, bottom: 40, left: 24 };
}

interface Rect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

const collides = (r: Rect, others: Rect[]) => others.some((o) => r.x0 < o.x1 + 3 && r.x1 + 3 > o.x0 && r.y0 < o.y1 && r.y1 > o.y0);

const clip = (s: string, n: number) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/** The graph: hubs, projects, agents, and the streams between them. */
export const GraphCanvas = forwardRef<GraphHandle, Props>(function GraphCanvas(
  { snapshot, selectedId, onSelect, onHover, onScale, reducedMotion },
  ref,
) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const layout = useMemo(() => buildLayout(snapshot), [snapshot]);

  // Everything the animation loop reads lives in refs, so a new snapshot never restarts it.
  const layoutRef = useRef<Layout>(layout);
  const selectedRef = useRef<string | null>(selectedId);
  const hoverRef = useRef<string | null>(null);
  const viewRef = useRef<View>({ scale: 1, tx: 0, ty: 0 });
  const sizeRef = useRef({ w: 0, h: 0, dpr: 1 });
  const movedRef = useRef(false);
  const paletteRef = useRef<Palette>(PALETTE_FALLBACK);
  const particles = useRef(new Particles());
  const geoms = useRef(edgeGeometry(layout));
  const dirty = useRef(true);
  const onScaleRef = useRef(onScale);
  const reducedRef = useRef(reducedMotion);
  onScaleRef.current = onScale;
  reducedRef.current = reducedMotion;
  selectedRef.current = selectedId;

  const applyView = useCallback((v: View) => {
    viewRef.current = v;
    dirty.current = true;
    onScaleRef.current(v.scale);
  }, []);

  const fit = useCallback(() => {
    const { w, h } = sizeRef.current;
    if (w === 0) return;
    movedRef.current = false;
    applyView(fitView(layoutRef.current.bounds, w, h, insetsFor(w)));
  }, [applyView]);

  useImperativeHandle(
    ref,
    () => ({
      fit,
      zoomBy: (factor: number) => {
        const { w, h } = sizeRef.current;
        movedRef.current = true;
        applyView(zoomAt(viewRef.current, factor, w / 2, h / 2));
      },
    }),
    [fit, applyView],
  );

  useEffect(() => {
    layoutRef.current = layout;
    geoms.current = edgeGeometry(layout);
    dirty.current = true;
    // Until the owner pans or zooms, keep the whole graph in view as it grows.
    if (!movedRef.current) fit();
  }, [layout, fit]);

  useEffect(() => {
    dirty.current = true;
  }, [selectedId]);

  // Size, palette, input, and the animation loop.
  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return; // jsdom and very old webviews: the side panels still work.
    paletteRef.current = readPalette();

    const resize = () => {
      const r = wrap.getBoundingClientRect();
      const dpr = Math.min(2.5, window.devicePixelRatio || 1);
      sizeRef.current = { w: r.width, h: r.height, dpr };
      canvas.width = Math.max(1, Math.round(r.width * dpr));
      canvas.height = Math.max(1, Math.round(r.height * dpr));
      canvas.style.width = `${r.width}px`;
      canvas.style.height = `${r.height}px`;
      if (!movedRef.current) fit();
      dirty.current = true;
    };
    const ro = new ResizeObserver(resize);
    ro.observe(wrap);
    resize();

    const themeObserver = new MutationObserver(() => {
      paletteRef.current = readPalette();
      dirty.current = true;
    });
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-accent", "class", "style"] });

    // --- pointer input -------------------------------------------------------------------------
    let drag: { x: number; y: number; tx: number; ty: number; moved: boolean } | null = null;
    const local = (e: { clientX: number; clientY: number }) => {
      const r = canvas.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    const nodeAt = (x: number, y: number) => {
      const w = toWorld(viewRef.current, x, y);
      return hitTest(layoutRef.current, w.x, w.y, 4 / viewRef.current.scale);
    };
    const onDown = (e: PointerEvent) => {
      const p = local(e);
      drag = { x: p.x, y: p.y, tx: viewRef.current.tx, ty: viewRef.current.ty, moved: false };
      canvas.setPointerCapture(e.pointerId);
    };
    const onMove = (e: PointerEvent) => {
      const p = local(e);
      if (drag) {
        const dx = p.x - drag.x;
        const dy = p.y - drag.y;
        if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
        if (drag.moved) {
          movedRef.current = true;
          applyView({ ...viewRef.current, tx: drag.tx + dx, ty: drag.ty + dy });
          onHover(null);
        }
        return;
      }
      const n = nodeAt(p.x, p.y);
      const id = n?.id ?? null;
      if (id !== hoverRef.current) {
        hoverRef.current = id;
        dirty.current = true;
        canvas.style.cursor = n ? "pointer" : "grab";
      }
      onHover(n ? { node: n, x: p.x, y: p.y } : null);
    };
    const onUp = (e: PointerEvent) => {
      const d = drag;
      drag = null;
      if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
      if (d && !d.moved) {
        const p = local(e);
        onSelect(nodeAt(p.x, p.y)?.id ?? null);
      }
    };
    const onLeave = () => {
      hoverRef.current = null;
      onHover(null);
      dirty.current = true;
    };
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const p = local(e);
      movedRef.current = true;
      applyView(zoomAt(viewRef.current, Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0015)), p.x, p.y));
    };
    const onKey = (e: KeyboardEvent) => {
      const { w, h } = sizeRef.current;
      if (e.key === "+" || e.key === "=") {
        movedRef.current = true;
        applyView(zoomAt(viewRef.current, 1.25, w / 2, h / 2));
      } else if (e.key === "-") {
        movedRef.current = true;
        applyView(zoomAt(viewRef.current, 0.8, w / 2, h / 2));
      } else if (e.key === "0") fit();
      else if (e.key === "Escape") onSelect(null);
    };
    canvas.addEventListener("pointerdown", onDown);
    canvas.addEventListener("pointermove", onMove);
    canvas.addEventListener("pointerup", onUp);
    canvas.addEventListener("pointerleave", onLeave);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("dblclick", fit);
    canvas.addEventListener("keydown", onKey);

    // --- animation loop ------------------------------------------------------------------------
    let raf = 0;
    let last = 0;
    let ema = 8;
    let quality = 1;
    const rng = mulberry32(0x5eed);
    const frame = (now: number) => {
      raf = requestAnimationFrame(frame);
      if (document.visibilityState === "hidden") return;
      const dt = Math.min(0.1, last ? (now - last) / 1000 : 0.016);
      last = now;
      const animated = !reducedRef.current;
      if (!animated && !dirty.current) return;
      const t0 = performance.now();
      if (animated) particles.current.step(dt, geoms.current, quality, rng);
      draw(ctx, now, animated);
      dirty.current = false;
      const took = performance.now() - t0;
      ema = ema * 0.92 + took * 0.08;
      // Adapt: a slow frame sheds specks, a fast one wins them back.
      if (ema > 14) quality = Math.max(0.2, quality * 0.97);
      else if (ema < 7) quality = Math.min(1, quality * 1.01);
    };

    const draw = (c: CanvasRenderingContext2D, now: number, animated: boolean) => {
      const { w, h, dpr } = sizeRef.current;
      const v = viewRef.current;
      const p = paletteRef.current;
      const L = layoutRef.current;
      c.setTransform(1, 0, 0, 1, 0, 0);
      c.clearRect(0, 0, canvas.width, canvas.height);
      if (L.nodes.length === 0) return;
      const pulse = animated ? 0.5 + 0.5 * Math.sin(now / 650) : 0.6;

      // World space.
      c.setTransform(dpr * v.scale, 0, 0, dpr * v.scale, dpr * v.tx, dpr * v.ty);
      c.lineCap = "round";
      for (const e of L.edges) {
        const a = L.byId.get(e.from);
        const b = L.byId.get(e.to);
        if (!a || !b) continue;
        c.globalAlpha = e.live ? 0.22 : 0.09;
        c.strokeStyle = e.live ? p.running : p.ring;
        c.lineWidth = 1 / v.scale;
        if (!animated && e.live) c.setLineDash([3 / v.scale, 5 / v.scale]);
        c.beginPath();
        c.moveTo(a.x, a.y);
        c.lineTo(b.x, b.y);
        c.stroke();
        c.setLineDash([]);
      }
      if (animated) {
        const ps = particles.current;
        const size = 1.4 / Math.sqrt(v.scale);
        for (let i = 0; i < ps.count; i++) {
          const pt = ps.at(i, geoms.current);
          if (!pt) continue;
          const t = ps.t[i] ?? 1;
          const fade = Math.min(1, t * 8, (1 - t) * 8);
          c.globalAlpha = (ps.alpha[i] ?? 0) * fade;
          c.fillStyle = ps.tint[i] ? p.running : p.stream;
          c.fillRect(pt.x - size / 2, pt.y - size / 2, size, size);
        }
      }
      c.globalAlpha = 1;

      for (const n of L.nodes) drawNode(c, n, p, pulse, v.scale);

      // Screen space (labels stay crisp and the same size at every zoom). Hubs and projects first, then
      // agents that need attention, then the quiet ones; a label that would land on another is skipped.
      c.setTransform(dpr, 0, 0, dpr, 0, 0);
      const placed: Rect[] = [];
      const order: Record<string, number> = { hub: 0, project: 1, running: 2, errored: 2, stalled: 2, idle: 3 };
      const rank = (n: GNode) => order[n.kind === "agent" ? (n.status ?? "idle") : n.kind] ?? 3;
      const sorted = [...L.nodes].sort((a, b) => rank(a) - rank(b));
      for (const n of sorted) drawLabel(c, n, p, L, v, w, h, placed);
    };

    const drawNode = (c: CanvasRenderingContext2D, n: GNode, p: Palette, pulse: number, scale: number) => {
      const col = statusColor(p, n);
      const active = n.status === "running" || (n.kind !== "agent" && n.live);
      if (active || n.status === "errored" || n.status === "stalled") {
        const glowR = n.r * (n.kind === "hub" ? 2.6 : 2.6) * (0.85 + 0.15 * pulse);
        const g = c.createRadialGradient(n.x, n.y, n.r * 0.4, n.x, n.y, glowR);
        g.addColorStop(0, col);
        g.addColorStop(1, "transparent");
        c.globalAlpha = (n.status === "running" || n.kind !== "agent" ? 0.3 : 0.38) * (0.7 + 0.3 * pulse);
        c.fillStyle = g;
        c.beginPath();
        c.arc(n.x, n.y, glowR, 0, Math.PI * 2);
        c.fill();
      }
      c.globalAlpha = 1;
      if (n.kind === "agent") {
        // Dark disc with a coloured rim and core: idle agents read as quiet grey pebbles.
        c.fillStyle = p.bg;
        c.beginPath();
        c.arc(n.x, n.y, n.r, 0, Math.PI * 2);
        c.fill();
        c.lineWidth = 1.4 / scale;
        c.strokeStyle = n.status === "idle" ? p.ring : col;
        c.stroke();
        c.fillStyle = col;
        c.globalAlpha = n.status === "idle" ? 0.55 : 1;
        c.beginPath();
        c.arc(n.x, n.y, n.r * (n.status === "running" ? 0.58 + 0.08 * pulse : 0.5), 0, Math.PI * 2);
        c.fill();
        c.globalAlpha = 1;
      } else {
        const rim = n.kind === "hub" ? 2 : 1.6;
        c.fillStyle = p.bg;
        c.beginPath();
        c.arc(n.x, n.y, n.r, 0, Math.PI * 2);
        c.fill();
        c.lineWidth = rim / scale;
        c.strokeStyle = n.live ? col : p.ring;
        c.globalAlpha = n.live ? 0.9 : 1;
        c.stroke();
        c.globalAlpha = 1;
        c.fillStyle = col;
        c.beginPath();
        c.arc(n.x, n.y, n.r * (n.kind === "hub" ? 0.3 : 0.4), 0, Math.PI * 2);
        c.fill();
      }
      const sel = n.id === selectedRef.current;
      if (sel || n.id === hoverRef.current) {
        c.lineWidth = (sel ? 2 : 1.2) / scale;
        c.strokeStyle = p.text;
        c.globalAlpha = sel ? 0.95 : 0.5;
        c.beginPath();
        c.arc(n.x, n.y, n.r + 4 / scale, 0, Math.PI * 2);
        c.stroke();
        c.globalAlpha = 1;
      }
    };

    const drawLabel = (c: CanvasRenderingContext2D, n: GNode, p: Palette, L: Layout, v: View, w: number, h: number, placed: Rect[]) => {
      const sx = n.x * v.scale + v.tx;
      const sy = n.y * v.scale + v.ty;
      const sr = n.r * v.scale;
      if (sx < -200 || sy < -80 || sx > w + 200 || sy > h + 80) return;
      c.textBaseline = "alphabetic";
      if (n.kind === "agent") {
        if (v.scale < 0.42 && n.id !== selectedRef.current) return;
        const proj = n.parent ? L.byId.get(n.parent) : null;
        const right = !proj || n.x >= proj.x;
        c.font = `10px ${p.mono}`;
        const text = clip(n.label, 20);
        const tw = c.measureText(text).width;
        const x = sx + (right ? sr + 7 : -sr - 7);
        const keep = n.id === selectedRef.current || n.id === hoverRef.current;
        // Try the label where it belongs, then nudged up and down; give up if the spot stays taken.
        let dy = 0;
        let box: Rect = { x0: right ? x : x - tw, x1: right ? x + tw : x, y0: sy - 6, y1: sy + 6 };
        for (const nudge of [0, 12, -12, 24, -24]) {
          box = { x0: box.x0, x1: box.x1, y0: sy - 6 + nudge, y1: sy + 6 + nudge };
          dy = nudge;
          if (!collides(box, placed)) break;
          if (nudge === -24 && !keep) return;
        }
        placed.push(box);
        c.textAlign = right ? "left" : "right";
        c.fillStyle = n.status === "idle" ? p.faint : p.muted;
        c.globalAlpha = n.status === "idle" ? 0.85 : 1;
        c.fillText(text, x, sy + 3.5 + dy);
        c.globalAlpha = 1;
        return;
      }
      c.textAlign = "center";
      if (n.kind === "project") {
        // Name, then "7 agents · 2 running", under the node.
        c.font = `600 11px ${p.mono}`;
        const nw = c.measureText(n.label).width;
        c.fillStyle = p.text;
        c.fillText(n.label, sx, sy + sr + 15);
        c.font = `9px ${p.mono}`;
        const sw = c.measureText(n.sub).width;
        c.fillStyle = p.faint;
        c.fillText(n.sub, sx, sy + sr + 27);
        placed.push({ x0: sx - Math.max(nw, sw) / 2, x1: sx + Math.max(nw, sw) / 2, y0: sy + sr + 5, y1: sy + sr + 30 });
        return;
      }
      c.font = `600 10px ${p.mono}`;
      c.fillStyle = p.text;
      c.letterSpacing = "0.12em";
      c.fillText(n.label, sx, sy + sr + 17);
      c.font = `9px ${p.mono}`;
      c.fillStyle = p.muted;
      c.fillText(n.sub, sx, sy + sr + 30);
      c.fillStyle = p.faint;
      c.fillText(n.sub2, sx, sy + sr + 42);
      c.letterSpacing = "0px";
      placed.push({ x0: sx - 90, x1: sx + 90, y0: sy + sr + 5, y1: sy + sr + 46 });
    };

    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      themeObserver.disconnect();
      canvas.removeEventListener("pointerdown", onDown);
      canvas.removeEventListener("pointermove", onMove);
      canvas.removeEventListener("pointerup", onUp);
      canvas.removeEventListener("pointerleave", onLeave);
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("dblclick", fit);
      canvas.removeEventListener("keydown", onKey);
    };
  }, [applyView, fit, onHover, onSelect]);

  return (
    <div ref={wrapRef} className="absolute inset-0">
      <canvas
        ref={canvasRef}
        tabIndex={0}
        role="img"
        aria-label={`Activity graph: ${snapshot.totals.agents} agents in ${snapshot.projects.length} projects, ${snapshot.totals.active} running. Drag to pan, scroll to zoom, plus and minus keys zoom, zero fits.`}
        className="block touch-none cursor-grab outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
    </div>
  );
});
