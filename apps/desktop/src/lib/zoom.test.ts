import { describe, expect, it } from "vitest";

import { applyZoom, parseZoom } from "./zoom";

describe("zoom", () => {
  it("accepts sane factors and falls back to 100%", () => {
    expect(parseZoom("1.25")).toBe(1.25);
    expect(parseZoom(null)).toBe(1);
    expect(parseZoom("9")).toBe(1);
    expect(parseZoom("abc")).toBe(1);
  });
  it("sets and clears the CSS zoom", () => {
    applyZoom(document.documentElement, 1.5);
    expect(document.documentElement.style.getPropertyValue("zoom")).toBe("1.5");
    applyZoom(document.documentElement, 1);
    expect(document.documentElement.style.getPropertyValue("zoom")).toBe("");
  });
});
