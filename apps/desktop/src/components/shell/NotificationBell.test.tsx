import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, test } from "vitest";

import type { NotificationPayload } from "@/bindings";
import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { NotificationBell } from "./NotificationBell";

afterEach(() => setTransport(tauriTransport));

const note = (over: Partial<NotificationPayload> = {}): NotificationPayload => ({
  id: "n1",
  level: "success",
  kind: "download",
  title: "Download finished",
  body: "MiniCPM5 is ready",
  action: { label: "Open Models", route: "/models" },
  read: false,
  created_at: Date.now() - 60_000,
  ...over,
});

function mount(items: NotificationPayload[]) {
  const t = createMockTransport({
    notifications_list: () => items,
    notifications_unread_count: () => items.filter((i) => !i.read).length,
    notifications_mark_read: () => null,
    notifications_clear: () => null,
  });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter>
        <NotificationBell />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return t;
}

test("shows the unread badge and lists notifications", async () => {
  mount([note(), note({ id: "n2", read: true, title: "Old one" })]);
  const bell = await screen.findByLabelText("Notifications, 1 unread");
  fireEvent.click(bell);
  expect(await screen.findByText("Download finished")).toBeTruthy();
  expect(screen.getByText("Old one")).toBeTruthy();
});

test("mark all read and clear call the backend", async () => {
  const t = mount([note()]);
  fireEvent.click(await screen.findByLabelText("Notifications, 1 unread"));
  fireEvent.click(await screen.findByRole("button", { name: "Mark all read" }));
  await waitFor(() => expect(t.calls.some((c) => c.command === "notifications_mark_read")).toBe(true));
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
  await waitFor(() => expect(t.calls.some((c) => c.command === "notifications_clear")).toBe(true));
});

test("a new notification event refreshes the list", async () => {
  const items = [note()];
  const t = mount(items);
  await screen.findByLabelText("Notifications, 1 unread");
  items.push(note({ id: "n3", title: "Another" }));
  act(() => t.emit("notification:new", items[1]));
  await screen.findByLabelText("Notifications, 2 unread");
});

test("empty state", async () => {
  mount([]);
  fireEvent.click(await screen.findByLabelText("Notifications"));
  expect(await screen.findByText("You're all caught up.")).toBeTruthy();
});
