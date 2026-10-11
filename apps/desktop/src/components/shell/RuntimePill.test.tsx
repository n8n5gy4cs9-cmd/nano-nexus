import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { RuntimePill } from "./RuntimePill";

afterEach(() => setTransport(tauriTransport));

function mount(handlers: Parameters<typeof createMockTransport>[0]) {
  const t = createMockTransport(handlers);
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RuntimePill />
    </QueryClientProvider>,
  );
  return t;
}

test("follows runtime:status events from loading to ready", async () => {
  const t = mount({ runtime_status: () => ({ state: "loading" }), runtime_logs: () => [] });
  await screen.findByLabelText("Model status: Loading");
  await waitFor(() => expect(t.calls.some((c) => c.command === "runtime_status")).toBe(true));
  act(() => t.emit("runtime:status", { state: "ready", model_id: "m", model_path: "/x/m.gguf", ctx_size: 8192, gpu_layers: "all" }));
  await screen.findByLabelText("Model status: Ready");
});

test("popover shows details and Restart calls the backend", async () => {
  const t = mount({
    runtime_status: () => ({ state: "ready", model_id: "m", model_path: "/x/model.gguf", ctx_size: 8192, gpu_layers: "all" }),
    runtime_logs: () => ["server listening"],
    runtime_restart: () => null,
  });
  fireEvent.click(await screen.findByLabelText("Model status: Ready"));
  await screen.findByText("8,192 tokens");
  await screen.findByText("server listening");
  fireEvent.click(screen.getByRole("button", { name: /Restart/ }));
  await waitFor(() => expect(t.calls.some((c) => c.command === "runtime_restart")).toBe(true));
});

test("a failed restart is shown", async () => {
  mount({
    runtime_status: () => ({ state: "failed", reason: "exited 1", log_tail: ["boom"], can_retry: true }),
    runtime_logs: () => [],
    runtime_restart: () => {
      throw { code: "model_missing", message: "No model installed.", detail: null };
    },
  });
  fireEvent.click(await screen.findByLabelText("Model status: Error"));
  fireEvent.click(screen.getByRole("button", { name: /Restart/ }));
  await screen.findByRole("alert");
  expect(screen.getByRole("alert").textContent).toContain("No model installed.");
});
