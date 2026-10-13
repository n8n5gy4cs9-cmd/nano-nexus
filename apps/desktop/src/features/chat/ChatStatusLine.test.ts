import { expect, test } from "vitest";

import { modelLabel, statusParts } from "./ChatStatusLine";

test("model label drops the folder and .gguf", () => {
  expect(modelLabel("/Users/me/models/gemma-4-E4B-Q5_K_M.gguf")).toBe("gemma-4-E4B-Q5_K_M");
  expect(modelLabel("C:\\m\\x.GGUF")).toBe("x");
  expect(modelLabel(undefined)).toBeNull();
});

test("each part can be turned off", () => {
  const base = { modelPath: "/m/a.gguf", tps: 42.26, showModel: true, showSpeed: true };
  expect(statusParts(base)).toEqual(["a", "42.3 tok/s"]);
  expect(statusParts({ ...base, showModel: false })).toEqual(["42.3 tok/s"]);
  expect(statusParts({ ...base, showSpeed: false })).toEqual(["a"]);
  expect(statusParts({ ...base, showModel: false, showSpeed: false })).toEqual([]);
  expect(statusParts({ ...base, tps: null })).toEqual(["a"]);
});
