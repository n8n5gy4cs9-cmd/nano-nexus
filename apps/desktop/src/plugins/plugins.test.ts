import { expect, test } from "vitest";

import { modelLabel } from "./builtin";
import { footerParts, parseDisabled, PLUGINS, serializeDisabled, type Plugin } from "./index";

const ctx = { modelPath: "/m/a.gguf", lastTps: 42.26 };

test("model label drops the folder and .gguf", () => {
  expect(modelLabel("/Users/me/models/gemma-4-E4B-Q5_K_M.gguf")).toBe("gemma-4-E4B-Q5_K_M");
  expect(modelLabel("C:\\m\\x.GGUF")).toBe("x");
  expect(modelLabel(undefined)).toBeNull();
});

test("built-in plugins show model and speed, and each can be turned off", () => {
  expect(footerParts(ctx, new Set())).toEqual(["a", "42.3 tok/s"]);
  expect(footerParts(ctx, new Set(["model-name"]))).toEqual(["42.3 tok/s"]);
  expect(footerParts(ctx, new Set(["speed"]))).toEqual(["a"]);
  expect(footerParts(ctx, new Set(["speed", "model-name"]))).toEqual([]);
  expect(footerParts({ modelPath: "/m/a.gguf", lastTps: null }, new Set())).toEqual(["a"]);
});

test("a throwing plugin is skipped", () => {
  const bad: Plugin = { id: "bad", name: "Bad", description: "", footer: () => { throw new Error("x"); } };
  const ok: Plugin = { id: "ok", name: "Ok", description: "", footer: () => "fine" };
  expect(footerParts(ctx, new Set(), [bad, ok])).toEqual(["fine"]);
});

test("plugin ids are unique and the disabled list round-trips", () => {
  expect(new Set(PLUGINS.map((p) => p.id)).size).toBe(PLUGINS.length);
  expect([...parseDisabled("speed, model-name,,")].sort()).toEqual(["model-name", "speed"]);
  expect(parseDisabled(undefined).size).toBe(0);
  expect(serializeDisabled(["speed", "model-name", "speed"])).toBe("model-name,speed");
});
