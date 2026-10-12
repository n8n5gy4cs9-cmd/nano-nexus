import { afterEach, expect, test, vi } from "vitest";

import { api } from "./api";
import { IpcException, toIpcException } from "./ipc";
import { getTransport, setTransport } from "./transport";
import { createMockTransport } from "./transport/mockTransport";
import { tauriTransport } from "./transport/tauriTransport";

afterEach(() => setTransport(tauriTransport));

test("facade maps methods to commands and args", async () => {
  const t = createMockTransport({
    runtime_logs: (args) => [`lines=${String(args?.lines)}`],
    settings_set: () => ({ ok: true }),
  });
  setTransport(t);
  expect(await api.runtime.logs(20)).toEqual(["lines=20"]);
  await api.settings.set("ui.theme", "light");
  expect(t.calls).toEqual([
    { command: "runtime_logs", args: { lines: 20 } },
    { command: "settings_set", args: { key: "ui.theme", value: "light" } },
  ]);
});

test("events reach subscribers until unsubscribed", async () => {
  const t = createMockTransport({});
  setTransport(t);
  const seen: unknown[] = [];
  const off = await api.runtime.onStatus((s) => seen.push(s));
  t.emit("runtime:status", { state: "loading" });
  off();
  t.emit("runtime:status", { state: "stopped" });
  expect(seen).toEqual([{ state: "loading" }]);
});

test("backend errors surface as IpcException", async () => {
  setTransport(
    createMockTransport({
      runtime_restart: () => {
        throw { code: "model_missing", message: "No model installed.", detail: "x" };
      },
    }),
  );
  const err = await api.runtime.restart().catch((e: unknown) => e);
  expect(err).toBeInstanceOf(IpcException);
  expect((err as IpcException).code).toBe("model_missing");
  expect((err as IpcException).detail).toBe("x");
});

test("unknown failures become internal errors", () => {
  expect(toIpcException(new Error("boom")).code).toBe("internal");
  expect(toIpcException("weird").message).toBe("weird");
});

test("streams deliver events in order", async () => {
  const t = createMockTransport({
    chat_demo: (_args, emit) => {
      emit("a");
      emit("b");
      return 7;
    },
  });
  const got: string[] = [];
  const result = await t.stream<string, number>("chat_demo", {}, (e) => got.push(e));
  expect(got).toEqual(["a", "b"]);
  expect(result).toBe(7);
});

test("default transport is Tauri", async () => {
  const spy = vi.spyOn(tauriTransport, "call").mockResolvedValue("from tauri");
  expect(await getTransport().call("anything")).toBe("from tauri");
  expect(spy).toHaveBeenCalledWith("anything", undefined);
  spy.mockRestore();
});
