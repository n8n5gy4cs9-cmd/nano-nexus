import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { StorageActivity } from "./StorageActivity";

afterEach(() => setTransport(tauriTransport));

function mount(report: unknown) {
  setTransport(createMockTransport({ app_write_report: () => report }));
  render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><StorageActivity /></QueryClientProvider>);
}

test("shows recorded writes, outside-root warnings and the not-observed list", async () => {
  mount({
    recorded: [{ ts_ms: 1, subsystem: "outputs", operation: "create", destination: "outputs/a.png", outside_root: false, bytes: 2048, ok: true, error: null, count: 1 }],
    configured: [{ subsystem: "Worker HOME", path: "/host/home", outside_root: true }],
    gaps: ["Not a system-wide file monitor."],
    log_path: "logs/write-audit.jsonl",
  });
  expect(await screen.findByText("outputs/a.png")).toBeTruthy();
  expect(screen.getByText(/2\.0 KiB/)).toBeTruthy();
  expect(screen.getByText(/Configured outside the app drive/)).toBeTruthy();
  expect(screen.getByText("Not a system-wide file monitor.")).toBeTruthy();
});

test("an empty record says nothing was recorded", async () => {
  mount({ recorded: [], configured: [], gaps: [], log_path: null });
  expect(await screen.findByText("Nothing recorded yet this session.")).toBeTruthy();
});
