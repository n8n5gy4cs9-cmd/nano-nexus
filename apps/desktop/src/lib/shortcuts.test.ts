import { expect, test } from "vitest";

import { NAV } from "@/routes";

import { SHORTCUTS, shortcutLabel } from "./shortcuts";

test("labels follow the platform", () => {
  const k = SHORTCUTS.find((s) => s.action === "Search and commands");
  const t = SHORTCUTS.find((s) => s.keys === "Shift+T" && s.where === "Chat");
  const esc = SHORTCUTS.find((s) => s.keys === "Esc");
  if (!k || !t || !esc) throw new Error("missing entries");
  expect(shortcutLabel(k, "macos")).toBe("⌘K");
  expect(shortcutLabel(k, "windows")).toBe("Ctrl+K");
  expect(shortcutLabel(t, "macos")).toBe("⌘⇧T");
  expect(shortcutLabel(t, "linux")).toBe("Ctrl+Shift+T");
  expect(shortcutLabel(esc, "macos")).toBe("Esc");
});

test("the list matches the sections that have a digit shortcut", () => {
  const digits = NAV.filter((s) => s.digit !== undefined).map((s) => s.digit);
  expect(digits).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
  expect(SHORTCUTS.find((s) => s.keys === "1…9")?.action).toContain("Library");
});

test("no shortcut is listed twice for the same place", () => {
  const ids = SHORTCUTS.map((s) => `${s.where}:${s.bare ? "" : "mod+"}${s.keys}`);
  expect(new Set(ids).size).toBe(ids.length);
});
