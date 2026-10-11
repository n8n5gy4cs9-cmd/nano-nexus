import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, test } from "vitest";

import type { CatalogCard, DownloadView, Fit, InstalledModel } from "@/bindings";
import { TooltipProvider } from "@/components/ui";
import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { ModelsPage } from "./ModelsPage";
import { mergeProgress } from "./useModels";

afterEach(() => setTransport(tauriTransport));

const fit: Fit = { weights_bytes: 1, kv_cache_bytes: 1, need_bytes: 2_000_000_000, budget_bytes: 11_000_000_000, badge: "great" };

const installed = (over: Partial<InstalledModel>): InstalledModel => ({
  id: "m",
  source: "download",
  name: "Model",
  repo: null,
  file: "m.gguf",
  path: "/m.gguf",
  file_exists: true,
  bytes: 1_100_000_000,
  quant: "Q8_0",
  catalog_id: null,
  license: "apache-2.0",
  capabilities: ["chat", "thinking"],
  supports_thinking_toggle: true,
  has_chat_template: true,
  context_length: 131072,
  active: false,
  removal: "delete",
  fit,
  gguf: null,
  ...over,
});

const dl = (over: Partial<DownloadView>): DownloadView => ({
  id: "d1",
  catalog_id: "cat1",
  repo: "o/r",
  file: "f.gguf",
  bytes: 1000,
  done_bytes: 300,
  status: "running",
  error: null,
  bytes_per_sec: 2_000_000,
  eta_secs: 30,
  phase: "downloading",
  ...over,
});

function card(over: Partial<CatalogCard["entry"]>, extra: Partial<CatalogCard> = {}): CatalogCard {
  return {
    entry: {
      id: "cat1",
      name: "Cat Model",
      family: "MiniCPM5",
      params_b: 1,
      quant: "Q4_K_M",
      repo: "o/r",
      revision: "main",
      file: null,
      file_pattern: "x",
      approx_bytes: 657_000_000,
      license: "mit",
      context_max: null,
      context_default: 8192,
      capabilities: ["chat"],
      supports_thinking_toggle: false,
      sampling: { no_think: { temperature: 0.7, top_p: 0.95, top_k: null, min_p: 0, repeat_penalty: null }, think: null },
      min_ram_gb: 4,
      tier: "tiny",
      tags: [],
      isDefault: false,
      verified: false,
      why: "Small and quick.",
      notes: null,
      ...over,
    },
    installed_id: null,
    download: null,
    fit,
    ...extra,
  };
}

function mount(handlers: Parameters<typeof createMockTransport>[0]) {
  const t = createMockTransport({
    runtime_status: () => ({ state: "ready", model_id: "b", model_path: "/x/b.gguf", ctx_size: 8192, gpu_layers: "all" }),
    models_downloads: () => [],
    models_catalog: () => [],
    models_list: () => [],
    ...handlers,
  });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider>
        <MemoryRouter>
          <ModelsPage />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return t;
}

test("installed models are grouped, the active one is marked, and the bundle cannot be deleted", async () => {
  mount({
    models_list: () => [
      installed({ id: "b", name: "Bundled One", source: "builtin", removal: "protected", active: true }),
      installed({ id: "d", name: "Downloaded One" }),
      installed({ id: "f", name: "Found One", source: "discovered", removal: "unlink" }),
    ],
  });
  await screen.findByText("Bundled One");
  expect(screen.getByText("Active")).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Downloaded" })).toBeTruthy();
  expect(screen.queryByLabelText("Delete Bundled One")).toBeNull();
  expect(screen.getByLabelText("Delete Downloaded One")).toBeTruthy();
  expect(screen.getByLabelText("Remove Found One from the list")).toBeTruthy();
  expect(screen.getByText(/Ready/)).toBeTruthy();
});

test("Use it activates a model and deleting asks first", async () => {
  const t = mount({
    models_list: () => [installed({ id: "d", name: "Downloaded One" })],
    models_activate: () => ({ id: "d", warning: null }),
    models_delete: () => null,
  });
  fireEvent.click(await screen.findByRole("button", { name: "Use it" }));
  await waitFor(() => expect(t.calls.some((c) => c.command === "models_activate" && (c.args as { id: string }).id === "d")).toBe(true));
  fireEvent.click(screen.getByLabelText("Delete Downloaded One"));
  expect(await screen.findByText("Delete this model?")).toBeTruthy();
  expect(t.calls.some((c) => c.command === "models_delete")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(t.calls.some((c) => c.command === "models_delete")).toBe(true));
});

test("Discover lists the catalog offline, marks unverified models, and starts a download", async () => {
  const t = mount({ models_catalog: () => [card({}), card({ id: "cat2", name: "Running One" }, { download: dl({ catalog_id: "cat2" }) })], models_download_catalog: () => dl({}) });
  fireEvent.mouseDown(await screen.findByRole("tab", { name: "Discover" }), { button: 0 });
  fireEvent.click(await screen.findByRole("tab", { name: "Discover" }));
  await screen.findByText("Cat Model");
  expect(screen.getAllByText("Community: unverified").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Runs great").length).toBe(2);
  expect(screen.getByRole("progressbar", { name: "f.gguf download" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /Download/ }));
  await waitFor(() => expect(t.calls.some((c) => c.command === "models_download_catalog")).toBe(true));
});

test("search shows a friendly offline state", async () => {
  mount({
    models_search: () => {
      throw { code: "network", message: "offline", detail: null };
    },
  });
  fireEvent.mouseDown(await screen.findByRole("tab", { name: "Search" }), { button: 0 });
  fireEvent.change(await screen.findByLabelText("Search Hugging Face"), { target: { value: "qwen" } });
  expect(await screen.findByText("You appear to be offline", {}, { timeout: 3000 })).toBeTruthy();
});

test("live progress frames merge into the cached download list", () => {
  const list = [dl({ id: "a", repo: "o/r" }), dl({ id: "b" })];
  const merged = mergeProgress(list, dl({ id: "b", repo: "", file: "", done_bytes: 700, bytes: 1000, bytes_per_sec: 5, eta_secs: 2, phase: "verifying" }));
  expect(merged?.[1]).toMatchObject({ id: "b", done_bytes: 700, phase: "verifying", file: "f.gguf" });
  expect(merged?.[0]).toBe(list[0]);
  expect(mergeProgress(list, dl({ id: "zzz" }))).toBe(list);
  expect(mergeProgress(undefined, dl({}))).toBeUndefined();
});
