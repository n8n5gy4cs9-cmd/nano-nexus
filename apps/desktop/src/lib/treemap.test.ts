import { describe, expect, it } from "vitest";

import { squarify } from "./treemap";

describe("squarify", () => {
  it("fills the box exactly and keeps areas proportional", () => {
    const items = [{ weight: 6 }, { weight: 3 }, { weight: 1 }];
    const out = squarify(items, { x: 0, y: 0, w: 100, h: 50 });
    const area = out.reduce((s, o) => s + o.rect.w * o.rect.h, 0);
    expect(area).toBeCloseTo(5000, 3);
    const big = out.find((o) => o.item.weight === 6)!.rect;
    expect(big.w * big.h).toBeCloseTo(3000, 3);
  });

  it("skips empty items and handles an empty box", () => {
    expect(squarify([{ weight: 0 }], { x: 0, y: 0, w: 10, h: 10 })).toEqual([]);
    expect(squarify([{ weight: 1 }], { x: 0, y: 0, w: 0, h: 10 })).toEqual([]);
  });
});
