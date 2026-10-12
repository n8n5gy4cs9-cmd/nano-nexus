import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, beforeEach, expect, test } from "vitest";

import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";
import { useAssistantChoice } from "@/stores/assistant";
import { useUi } from "@/stores/ui";

import { CommandPalette } from "./CommandPalette";

const card = (id: string, name: string) => ({ id, name, icon: "hexagon", color: "violet", category: "General", tagline: `${name} tagline`, description: "", version: null, source: "builtin", enabled: true, available: true });

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}

function mount(handlers: Parameters<typeof createMockTransport>[0] = {}) {
  const t = createMockTransport({
    app_info: () => ({ os: "linux", version: "0.1.0", arch: "x86_64", data_dir: "", log_dir: "", build: null }),
    assistants_list: () => [card("nexus", "Nexus"), card("folder-scout", "Folder Scout")],
    models_list: () => [],
    conversations_list: () => [],
    ...handlers,
  });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter initialEntries={["/"]}>
        <Routes>
          <Route path="*" element={<Where />} />
        </Routes>
        <CommandPalette />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return t;
}

beforeEach(() => useUi.setState({ paletteOpen: false }));
afterEach(() => {
  setTransport(tauriTransport);
  useUi.setState({ paletteOpen: false });
});

test("closed until opened; then pages, assistants and actions are listed", async () => {
  mount();
  expect(screen.queryByLabelText("Type a command or search")).toBeNull();
  act(() => useUi.getState().setPaletteOpen(true));
  await screen.findByLabelText("Type a command or search");
  for (const text of ["Models", "Rename Forge", "Settings", "Folder Scout", "Restart the model runtime", "Show or hide the sidebar"]) {
    expect((await screen.findAllByText(text)).length).toBeGreaterThan(0);
  }
});

test("choosing a page navigates and closes the palette", async () => {
  mount();
  act(() => useUi.getState().setPaletteOpen(true));
  fireEvent.click(await screen.findByText("Rename Forge"));
  await waitFor(() => expect(screen.getByTestId("where").textContent).toBe("/rename"));
  expect(useUi.getState().paletteOpen).toBe(false);
});

test("New chat with an assistant selects it and opens the chat", async () => {
  mount();
  act(() => useUi.getState().setPaletteOpen(true));
  fireEvent.click(await screen.findByText("Folder Scout"));
  await waitFor(() => expect(useAssistantChoice.getState().selectedId).toBe("folder-scout"));
  expect(screen.getByTestId("where").textContent).toBe("/");
});

test("typing searches conversations and opens the hit", async () => {
  const t = mount({
    conversations_list: (args) => (args?.query === "invoice" ? [{ id: "c9", title: "March invoices", assistant_id: "nexus", pinned: false, updated_at: 1, snippet: "the invoice total" }] : []),
  });
  act(() => useUi.getState().setPaletteOpen(true));
  fireEvent.change(await screen.findByLabelText("Type a command or search"), { target: { value: "invoice" } });
  fireEvent.click(await screen.findByText("March invoices"));
  await waitFor(() => expect(screen.getByTestId("where").textContent).toBe("/c/c9"));
  expect(t.calls.some((c) => c.command === "conversations_list" && (c.args as { query?: string }).query === "invoice")).toBe(true);
});

test("an installed model can be activated from the palette", async () => {
  const t = mount({
    models_list: () => [{ id: "m1", name: "Small Model", quant: "Q4_K_M", file_exists: true, active: false }],
    models_activate: () => ({ id: "m1", warning: null }),
  });
  act(() => useUi.getState().setPaletteOpen(true));
  fireEvent.click(await screen.findByText("Small Model"));
  await waitFor(() => expect(t.calls.some((c) => c.command === "models_activate")).toBe(true));
});
