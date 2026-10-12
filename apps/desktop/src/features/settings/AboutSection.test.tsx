import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { AboutSection, formatBuildDate } from "./AboutSection";

afterEach(() => setTransport(tauriTransport));

function mount(build: unknown) {
  setTransport(createMockTransport({ app_info: () => ({ version: "0.1.0", os: "macos", arch: "aarch64", data_dir: "/Users/me/Library/NanoNexus", log_dir: "", build }) }));
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AboutSection />
    </QueryClientProvider>,
  );
}

test("a built app shows its profile, commit and build date", async () => {
  mount({ profile: "studio", git_commit: "abc123def456", built_at: "2026-10-20T12:00:00Z", models: ["a", "b", "c"] });
  expect(await screen.findByText("studio")).toBeTruthy();
  expect(screen.getByText("abc123def456")).toBeTruthy();
  expect(screen.getByText("3 bundled models")).toBeTruthy();
  expect(screen.getByText(/2026/)).toBeTruthy();
  expect(screen.getByText("0.1.0")).toBeTruthy();
});

test("a dev build says so", async () => {
  mount(null);
  expect(await screen.findByText("dev")).toBeTruthy();
  expect(screen.queryByText("Commit")).toBeNull();
});

test("odd dates are shown as is", () => {
  expect(formatBuildDate("yesterday-ish")).toBe("yesterday-ish");
});
