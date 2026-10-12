import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, beforeEach, expect, test } from "vitest";

import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";
import { useAssistantChoice } from "@/stores/assistant";
import { useUi } from "@/stores/ui";

import { OnboardingGate } from "./Onboarding";

const card = (id: string, name: string) => ({ id, name, icon: "hexagon", color: "violet", category: "General", tagline: `${name} tagline`, description: "", version: null, source: "builtin", enabled: true, available: true });

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}

function mount(opts: { onboarded?: boolean; chats?: number; runtime?: unknown; models?: unknown[]; extra?: Parameters<typeof createMockTransport>[0] } = {}) {
  const t = createMockTransport({
    settings_get: () => ({ "ui.onboarded": opts.onboarded ?? false }),
    settings_set: () => ({ "ui.onboarded": true }),
    conversations_list: () => Array.from({ length: opts.chats ?? 0 }, (_, i) => ({ id: `c${i}`, title: "t", assistant_id: "nexus", pinned: false, updated_at: 1, snippet: null })),
    runtime_status: () => opts.runtime ?? { state: "ready", model_id: "m", model_path: "/x/MiniCPM5-1B-Q8_0.gguf", ctx_size: 8192, gpu_layers: "all" },
    models_list: () => opts.models ?? [{ id: "m" }],
    assistants_list: () => [card("nexus", "Nexus"), card("duo", "Duo")],
    fs_pick_folder: () => "/Users/me/Downloads",
    fs_grant_add: () => ({}),
    ...opts.extra,
  });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter>
        <OnboardingGate />
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return t;
}

beforeEach(() => useUi.setState({ onboardingOpen: false }));
afterEach(() => {
  setTransport(tauriTransport);
  useUi.setState({ onboardingOpen: false });
});

test("a first run opens the tour; four steps lead to the first chat", async () => {
  const t = mount();
  await screen.findByText("Private AI on your computer");
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText(/MiniCPM5-1B-Q8_0 is ready/);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  fireEvent.click(await screen.findByRole("button", { name: "Choose a folder…" }));
  await screen.findByText(/Allowed to read \/Users\/me\/Downloads/);
  expect(t.calls.find((c) => c.command === "fs_grant_add")?.args).toMatchObject({ root: "/Users/me/Downloads", level: "read", recursive: true });
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  fireEvent.click(await screen.findByText("Duo"));
  await waitFor(() => expect(useAssistantChoice.getState().selectedId).toBe("duo"));
  expect(useUi.getState().onboardingOpen).toBe(false);
  expect(t.calls.some((c) => c.command === "settings_set" && (c.args as { key?: string }).key === "ui.onboarded")).toBe(true);
});

test("skip closes the tour and remembers it", async () => {
  const t = mount();
  fireEvent.click(await screen.findByRole("button", { name: "Skip" }));
  await waitFor(() => expect(useUi.getState().onboardingOpen).toBe(false));
  expect(t.calls.some((c) => c.command === "settings_set")).toBe(true);
});

test("it stays closed after the tour was done, or for people who already have chats", async () => {
  mount({ onboarded: true });
  await waitFor(() => expect(useUi.getState().onboardingOpen).toBe(false));
  expect(screen.queryByText("Private AI on your computer")).toBeNull();
});

test("existing users with chats are not interrupted", async () => {
  const t = mount({ chats: 3 });
  await waitFor(() => expect(t.calls.some((c) => c.command === "conversations_list")).toBe(true));
  await act(async () => {});
  expect(useUi.getState().onboardingOpen).toBe(false);
});

test("without a model the second step offers the download and goes to Models", async () => {
  mount({ models: [], runtime: { state: "failed", reason: "no model", log_tail: [], can_retry: false } });
  await screen.findByText("Private AI on your computer");
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  fireEvent.click(await screen.findByRole("button", { name: "Download a model" }));
  await waitFor(() => expect(screen.getByTestId("where").textContent).toBe("/models"));
});

test("the tour can be started again on request", async () => {
  mount({ onboarded: true });
  act(() => useUi.getState().setOnboardingOpen(true));
  await screen.findByText("Private AI on your computer");
});
