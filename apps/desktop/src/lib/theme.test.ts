import { describe, expect, test } from "vitest";

import { applyTheme, isAccent, isThemePreference, resolveTheme } from "./theme";

describe("theme", () => {
  test("system follows the OS, explicit choices win", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  test("guards", () => {
    expect(isThemePreference("system")).toBe(true);
    expect(isThemePreference("neon")).toBe(false);
    expect(isAccent("rose")).toBe(true);
    expect(isAccent(3)).toBe(false);
  });

  test("applyTheme sets data attributes", () => {
    const el = document.createElement("html");
    applyTheme(el, "light", "cyan");
    expect(el.dataset.theme).toBe("light");
    expect(el.dataset.accent).toBe("cyan");
  });
});
