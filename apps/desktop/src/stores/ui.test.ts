import { beforeEach, expect, test } from "vitest";

import { useUi } from "./ui";

beforeEach(() => {
  window.localStorage.clear();
  useUi.setState({ theme: "dark", accent: "violet", sidebarOpen: true, inspectorOpen: false });
});

test("theme and accent persist to storage", () => {
  useUi.getState().setTheme("light");
  useUi.getState().setAccent("amber");
  expect(window.localStorage.getItem("nn.theme")).toBe("light");
  expect(window.localStorage.getItem("nn.accent")).toBe("amber");
  expect(useUi.getState().theme).toBe("light");
});

test("panels toggle", () => {
  useUi.getState().toggleSidebar();
  useUi.getState().toggleInspector();
  expect(useUi.getState().sidebarOpen).toBe(false);
  expect(useUi.getState().inspectorOpen).toBe(true);
});
