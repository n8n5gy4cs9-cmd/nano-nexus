/** Squarified treemap layout (Bruls et al.). Pure: nodes in, rectangles out. */

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Weighted {
  weight: number;
}

/** Lay `items` (any order) into `box`; the result keeps each item with its rectangle, largest first. */
export function squarify<T extends Weighted>(items: T[], box: Rect): { item: T; rect: Rect }[] {
  const sorted = items.filter((i) => i.weight > 0).sort((a, b) => b.weight - a.weight);
  const total = sorted.reduce((s, i) => s + i.weight, 0);
  const out: { item: T; rect: Rect }[] = [];
  if (total <= 0 || box.w <= 0 || box.h <= 0) return out;
  const scale = (box.w * box.h) / total;
  let free = { ...box };
  let row: T[] = [];

  const worst = (r: T[], side: number): number => {
    const sum = r.reduce((s, i) => s + i.weight * scale, 0);
    const max = Math.max(...r.map((i) => i.weight * scale));
    const min = Math.min(...r.map((i) => i.weight * scale));
    return Math.max((side * side * max) / (sum * sum), (sum * sum) / (side * side * min));
  };

  const flush = () => {
    if (row.length === 0) return;
    const sum = row.reduce((s, i) => s + i.weight * scale, 0);
    const horizontal = free.w >= free.h; // lay the row along the shorter side
    if (horizontal) {
      const rw = sum / free.h;
      let y = free.y;
      for (const item of row) {
        const h = (item.weight * scale) / rw;
        out.push({ item, rect: { x: free.x, y, w: rw, h } });
        y += h;
      }
      free = { x: free.x + rw, y: free.y, w: free.w - rw, h: free.h };
    } else {
      const rh = sum / free.w;
      let x = free.x;
      for (const item of row) {
        const w = (item.weight * scale) / rh;
        out.push({ item, rect: { x, y: free.y, w, h: rh } });
        x += w;
      }
      free = { x: free.x, y: free.y + rh, w: free.w, h: free.h - rh };
    }
    row = [];
  };

  for (const item of sorted) {
    const side = Math.min(free.w, free.h);
    if (row.length === 0 || worst([...row, item], side) <= worst(row, side)) {
      row.push(item);
    } else {
      flush();
      row.push(item);
    }
  }
  flush();
  return out;
}
