import { fireEvent } from "@testing-library/react";
import { renderHook } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import { useChatShortcuts } from "./useChatShortcuts";

const actions = () => ({ streaming: false, stop: vi.fn(), editLast: vi.fn(), copyLastAnswer: vi.fn(), composerEmpty: true, toggleThink: vi.fn() });

test("mod+Shift+T flips Think harder; plain mod+T does not", () => {
  const a = actions();
  renderHook(() => useChatShortcuts("macos", a));
  fireEvent.keyDown(window, { key: "t", metaKey: true });
  expect(a.toggleThink).not.toHaveBeenCalled();
  fireEvent.keyDown(window, { key: "T", metaKey: true, shiftKey: true });
  expect(a.toggleThink).toHaveBeenCalledTimes(1);
});

test("Ctrl is the modifier on Windows, and Esc stops only while streaming", () => {
  const a = actions();
  renderHook(() => useChatShortcuts("windows", a));
  fireEvent.keyDown(window, { key: "T", ctrlKey: true, shiftKey: true });
  expect(a.toggleThink).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(a.stop).not.toHaveBeenCalled();
  const b = { ...actions(), streaming: true };
  renderHook(() => useChatShortcuts("windows", b));
  fireEvent.keyDown(window, { key: "Escape" });
  expect(b.stop).toHaveBeenCalledTimes(1);
});
