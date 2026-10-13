import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, test } from "vitest";

import type { AssistantCard } from "@/bindings";
import { TooltipProvider } from "@/components/ui";
import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { AssistantsPage, permissionChips } from "./AssistantsPage";
import { slugify, withDerivedPermissions, BLANK_SPEC } from "./BuilderSheet";

afterEach(() => setTransport(tauriTransport));

const card = (over: Partial<AssistantCard>): AssistantCard => ({
  id: "x",
  name: "X",
  icon: "sparkles",
  color: "violet",
  category: "General",
  tagline: "tag",
  description: "",
  version: null,
  source: "builtin",
  enabled: true,
  sort: 0,
  needs: [],
  tools: [],
  workflow: "chat",
  pipeline: null,
  starters: [],
  placeholder: null,
  available: true,
  unavailable_reason: null,
  ...over,
});

function mount(cards: AssistantCard[]) {
  const t = createMockTransport({ assistants_list: () => cards });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider>
        <MemoryRouter>
          <AssistantsPage />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return t;
}

test("the gallery lists cards, filters by search and category, and disables unbuilt pipelines", async () => {
  mount([
    card({ id: "nexus", name: "Nexus", needs: ["read"] }),
    card({ id: "scout", name: "Folder Scout", category: "Files", workflow: "pipeline", pipeline: "folder_report", available: false, unavailable_reason: "Available in M08" }),
    card({ id: "tldr", name: "TL;DR", category: "Writing", tagline: "summaries" }),
  ]);
  await screen.findByText("Nexus");
  expect(screen.getByText("Reads files")).toBeTruthy();
  const scoutCard = screen.getByText("Folder Scout").closest("div[class*='rounded-card']");
  expect(scoutCard?.querySelector("button[disabled]")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Search assistants"), { target: { value: "summar" } });
  await waitFor(() => expect(screen.queryByText("Nexus")).toBeNull());
  expect(screen.getByText("TL;DR")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Search assistants"), { target: { value: "" } });
  fireEvent.click(screen.getByRole("tab", { name: "Files" }));
  await waitFor(() => expect(screen.queryByText("TL;DR")).toBeNull());
  expect(screen.getByText("Folder Scout")).toBeTruthy();
});

test("helpers", () => {
  expect(permissionChips(["read", "write"])).toEqual(["Reads files", "Edits files"]);
  expect(slugify("Regex Smith!")).toBe("regex-smith");
  expect(slugify("3D Helper")).toBe("assistant-3d-helper");
  expect(slugify("  ")).toBe("");
  const router = { ...BLANK_SPEC, workflow: { kind: "router" }, tools: { allowed: ["read_file"] } };
  expect(withDerivedPermissions(router).permissions.needs).toEqual(["read"]);
  expect(withDerivedPermissions({ ...router, tools: { allowed: ["calculate"] } }).permissions.needs).toEqual([]);
  expect(withDerivedPermissions({ ...router, workflow: { kind: "chat" } }).permissions.needs).toEqual([]);
});
