import { expect, test } from "vitest";

import { baseName, describeRuntime } from "./runtime-view";

test("describes every supervisor state", () => {
  expect(describeRuntime(undefined).busy).toBe(true);
  expect(describeRuntime({ state: "stopped" }).tone).toBe("neutral");
  expect(describeRuntime({ state: "starting", attempt: 1 })).toMatchObject({ tone: "warning", busy: true, label: "Starting" });
  expect(describeRuntime({ state: "starting", attempt: 3 }).detail).toContain("attempt 3");
  expect(describeRuntime({ state: "loading" }).label).toBe("Loading");
  const ready = describeRuntime({ state: "ready", model_id: "m", model_path: "/a/b/model.gguf", ctx_size: 8192, gpu_layers: "all" });
  expect(ready).toMatchObject({ tone: "success", label: "Ready", detail: "model.gguf", busy: false });
  expect(describeRuntime({ state: "failed", reason: "exited 1", log_tail: [], can_retry: true })).toMatchObject({
    tone: "danger",
    detail: "exited 1",
  });
});

test("baseName handles both separators", () => {
  expect(baseName("C:\\models\\x.gguf")).toBe("x.gguf");
  expect(baseName("/a/b/c.gguf")).toBe("c.gguf");
});
