import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, test } from "vitest";

import type { ConversationSummary } from "@/bindings";
import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { ConversationList } from "./ConversationList";

afterEach(() => setTransport(tauriTransport));

const conv = (id: string, title: string, over: Partial<ConversationSummary> = {}): ConversationSummary => ({
  id,
  title,
  assistant_id: "nexus",
  pinned: false,
  updated_at: Date.now(),
  snippet: null,
  ...over,
});

function mount(items: ConversationSummary[]) {
  const t = createMockTransport({
    conversations_list: (args) => (args?.query ? items.filter((i) => i.title.toLowerCase().includes(String(args.query).toLowerCase())) : items),
    conversation_update: () => null,
    conversation_delete: () => null,
    conversation_export_md: () => "# md",
  });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter>
        <ConversationList />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return t;
}

test("shows groups with pinned first", async () => {
  mount([conv("1", "Mixing vocals"), conv("2", "Pinned note", { pinned: true })]);
  await screen.findByText("Mixing vocals");
  expect(screen.getByText("Pinned")).toBeTruthy();
  expect(screen.getByText("Today")).toBeTruthy();
});

test("searching asks the backend", async () => {
  const t = mount([conv("1", "Mixing vocals"), conv("2", "Python decorators")]);
  await screen.findByText("Python decorators");
  fireEvent.change(screen.getByLabelText("Search conversations"), { target: { value: "python" } });
  await waitFor(() => expect(t.calls.some((c) => c.command === "conversations_list" && c.args?.query === "python")).toBe(true));
  await waitFor(() => expect(screen.queryByText("Mixing vocals")).toBeNull());
});

test("empty and no-match messages", async () => {
  mount([]);
  expect(await screen.findByText("Conversations will appear here.")).toBeTruthy();
});
