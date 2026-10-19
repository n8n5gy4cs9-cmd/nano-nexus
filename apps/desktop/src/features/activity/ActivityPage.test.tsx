import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, test } from "vitest";

import type { ActivitySnapshot, AgentView, ObserveView } from "@/bindings";
import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { ActivityPage } from "./ActivityPage";

afterEach(() => setTransport(tauriTransport));

const agent = (id: string, status: AgentView["status"], over: Partial<AgentView> = {}): AgentView => ({
  id,
  label: id,
  source: "claude_code",
  project_id: "claude_code:/w/app",
  parent: null,
  model: "claude-sonnet-5-5",
  status,
  tool: status === "running" ? "Edit" : null,
  tokens: { input: 10, output: 20, cache_read: 300, cache_write: 5 },
  started_ms: 1_000,
  last_ms: 5_000,
  events: 12,
  messages: 3,
  errors: status === "errored" ? 2 : 0,
  events_per_s: status === "running" ? 2 : 0,
  ...over,
});

function snapshot(agents: AgentView[], conflicts: ActivitySnapshot["conflicts"] = []): ActivitySnapshot {
  const running = agents.filter((a) => a.status === "running").length;
  return {
    at_ms: 60_000,
    totals: { tasks: agents.length, tasks_done: agents.length - running, events: 99, active: running, agents: agents.length, uptime_ms: 59_000, new_tokens: 4_200_000, cache_read_tokens: 1_000 },
    sources: [
      { source: "nano_nexus", label: "NanoNexus", enabled: true, agents: 0, running: 0, last_ms: null },
      { source: "claude_code", label: "Claude Code", enabled: true, agents: agents.length, running, last_ms: 5_000 },
      { source: "codex", label: "Codex", enabled: false, agents: 0, running: 0, last_ms: null },
    ],
    projects: agents.length ? [{ id: "claude_code:/w/app", label: "w/app", source: "claude_code", agents: agents.length, running, new_tokens: 1, last_ms: 5_000 }] : [],
    agents,
    conflicts,
    drift: { lines: 0, bad: 0 },
  };
}

const view = (snap: ActivitySnapshot, over: Partial<ObserveView> = {}): ObserveView => ({
  snapshot: snap,
  replay_from: null,
  replay_to: null,
  demo: false,
  claude_found: true,
  codex_found: false,
  ...over,
});

function mount(v: ObserveView, extra: Record<string, () => unknown> = {}) {
  const t = createMockTransport({
    observe_snapshot: () => v,
    observe_watch: () => null,
    observe_demo: () => view(snapshot([agent("demo-1", "running")]), { demo: true }),
    observe_replay: () => v.snapshot,
    settings_get: () => ({ "observe.nanonexus": true, "observe.claude_code": false, "observe.codex": false, "observe.alerts": true, "observe.window_hours": 6 }),
    settings_set: () => ({ "observe.claude_code": true }),
    ...extra,
  });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter>
        <ActivityPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return t;
}

test("shows the headline numbers, the conflict chip and what needs attention", async () => {
  const snap = snapshot(
    [agent("main", "running"), agent("broken", "errored"), agent("stuck", "stalled"), agent("quiet", "idle")],
    [{ path_label: "src/cart.ts", agents: ["claude_code:main", "claude_code:stuck"], since_ms: 1 }],
  );
  const t = mount(view(snap));
  expect(await screen.findByText(/4 agents · 1 running/)).toBeTruthy();
  expect(screen.getByText("4.2M")).toBeTruthy();
  expect(screen.getByRole("status").textContent).toContain("1 file conflict");
  const attention = screen.getByRole("region", { name: "Needs attention" });
  expect(attention.textContent).toContain("src/cart.ts");
  expect(attention.textContent).toContain("2 errors");
  expect(attention.textContent).toContain("broken");
  expect(attention.textContent).toContain("stuck");
  expect(t.calls.some((c) => c.command === "observe_watch")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: /broken/ }));
  expect((await screen.findByRole("region", { name: "Agent details" })).textContent).toContain("claude-sonnet-5-5");
});

test("pushed snapshots replace the picture", async () => {
  const t = mount(view(snapshot([agent("a", "idle")])));
  expect(await screen.findByText(/1 agents · 0 running/)).toBeTruthy();
  act(() => t.emit("observe:snapshot", view(snapshot([agent("a", "running"), agent("b", "running")]))));
  expect(await screen.findByText(/2 agents · 2 running/)).toBeTruthy();
});

test("an empty picture offers the demo and the sources", async () => {
  const t = mount(view(snapshot([])));
  expect(await screen.findByText("Nothing is running right now")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Show demo data" }));
  await waitFor(() => expect(t.calls.some((c) => c.command === "observe_demo")).toBe(true));
  expect(await screen.findByRole("button", { name: /Demo data · exit/ })).toBeTruthy();
});

test("turning on another tool's logs asks first and only then saves", async () => {
  const t = mount(view(snapshot([])));
  const sw = await screen.findByRole("switch", { name: "Show Claude Code activity" });
  fireEvent.click(sw);
  expect(await screen.findByText("Show Claude Code activity?")).toBeTruthy();
  expect(t.calls.some((c) => c.command === "settings_set")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Turn on" }));
  await waitFor(() => expect(t.calls.find((c) => c.command === "settings_set")?.args).toEqual({ key: "observe.claude_code", value: true }));
});

test("turning a tool off needs no question", async () => {
  const t = mount(view(snapshot([])), { settings_get: () => ({ "observe.claude_code": true }) });
  const sw = await screen.findByRole("switch", { name: "Show Claude Code activity" });
  await waitFor(() => expect(sw.getAttribute("aria-checked")).toBe("true"));
  fireEvent.click(sw);
  await waitFor(() => expect(t.calls.find((c) => c.command === "settings_set")?.args).toEqual({ key: "observe.claude_code", value: false }));
});

test("replay needs history, then scrubs through frames", async () => {
  const snap = snapshot([agent("a", "running")]);
  mount(view(snap));
  expect((await screen.findByRole("button", { name: "Replay" })).hasAttribute("disabled")).toBe(true);
});

test("replay starts from the recent past and can return to live", async () => {
  const snap = snapshot([agent("a", "running")]);
  const t = mount(view(snap, { replay_from: 0, replay_to: 3_600_000 }));
  const btn = await screen.findByRole("button", { name: "Replay" });
  await waitFor(() => expect(btn.hasAttribute("disabled")).toBe(false));
  fireEvent.click(btn);
  expect(await screen.findByRole("group", { name: "Replay controls" })).toBeTruthy();
  await waitFor(() => expect(t.calls.some((c) => c.command === "observe_replay")).toBe(true));
  fireEvent.click(screen.getByRole("button", { name: "Back to live" }));
  await waitFor(() => expect(screen.queryByRole("group", { name: "Replay controls" })).toBeNull());
});

test("warns when many log lines could not be read", async () => {
  const snap = { ...snapshot([agent("a", "idle")]), drift: { lines: 1000, bad: 400 } };
  mount(view(snap));
  expect((await screen.findByRole("note")).textContent).toContain("400 of 1000 log lines could not be read");
});
