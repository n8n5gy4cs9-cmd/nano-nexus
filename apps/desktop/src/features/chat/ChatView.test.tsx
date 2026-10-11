import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { ChatEvent, Message } from "@/bindings";
import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";
import { useChat } from "@/stores/chat";

import { ChatView } from "./ChatView";

vi.mock("@/lib/highlight", () => ({ highlight: () => Promise.resolve(null) }));

afterEach(() => setTransport(tauriTransport));
beforeEach(() => useChat.setState({ turns: {} }));

const msg = (over: Partial<Message>): Message => ({
  id: "m",
  conversation_id: "c1",
  role: "user",
  content: "",
  reasoning: null,
  assistant_id: null,
  model_id: null,
  tool_trace: null,
  status: "done",
  error: null,
  tokens_prompt: null,
  tokens_out: null,
  tps: null,
  ttft_ms: null,
  created_at: Date.now(),
  attachments: [],
  ...over,
});

function mount(handlers: Parameters<typeof createMockTransport>[0], path = "/") {
  const t = createMockTransport({ settings_get: () => ({ "runtime.ctx_size": 8192 }), ...handlers });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/" element={<ChatView />} />
          <Route path="/c/:conversationId" element={<ChatView />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return t;
}

test("a new chat shows the starters and sends the picked one", async () => {
  const stored: Message[] = [];
  const t = mount({
    chat_send: (args, emit) => {
      const a = args as { args: { text: string } };
      stored.push(msg({ id: "u1", content: a.args.text }), msg({ id: "a1", role: "assistant", content: "Sure.", tokens_prompt: 100, tokens_out: 5, tps: 42, ttft_ms: 300 }));
      const events: ChatEvent[] = [
        { type: "started", assistant_message_id: "a1" },
        { type: "content", delta: "Sure." },
        { type: "done", finish_reason: "stop", tokens_out: 5, tps: 42, ttft_ms: 300 },
      ];
      events.forEach(emit);
      return { conversation_id: "c1", user_message_id: "u1", assistant_message_id: "a1" };
    },
    conversation_get: () => ({ conversation: { id: "c1", title: "Chat", assistant_id: "nexus" }, messages: stored }),
    conversations_list: () => [],
  });
  fireEvent.click(await screen.findByText("What's 18% of 2,450 and why?"));
  await screen.findByText("Sure.");
  expect(t.calls.find((c) => c.command === "chat_send")?.args).toMatchObject({ args: { conversation_id: null, assistant_id: "nexus", think: false } });
  // The stored transcript replaces the live one: user bubble and one answer.
  await waitFor(() => expect(screen.getAllByText("Sure.")).toHaveLength(1));
  expect(screen.getByText("What's 18% of 2,450 and why?")).toBeTruthy();
});

test("Enter sends, Shift+Enter does not", async () => {
  const t = mount({
    conversation_get: () => ({ conversation: { id: "c1", title: "T", assistant_id: "nexus" }, messages: [msg({ id: "u0", content: "earlier" })] }),
    chat_send: () => ({ conversation_id: "c1", user_message_id: "u", assistant_message_id: "a" }),
  }, "/c/c1");
  const box = await screen.findByLabelText("Message");
  fireEvent.change(box, { target: { value: "line one" } });
  fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
  expect(t.calls.filter((c) => c.command === "chat_send")).toHaveLength(0);
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(t.calls.filter((c) => c.command === "chat_send")).toHaveLength(1));
});

test("a failed send shows an actionable error card", async () => {
  mount({
    conversation_get: () => ({ conversation: { id: "c1", title: "T", assistant_id: "nexus" }, messages: [msg({ id: "u0", content: "earlier" })] }),
    chat_send: () => {
      throw { code: "runtime_unavailable", message: "no server", detail: null };
    },
    runtime_restart: () => null,
  }, "/c/c1");
  const box = await screen.findByLabelText("Message");
  fireEvent.change(box, { target: { value: "hello" } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect((await screen.findByRole("alert")).textContent).toContain("The model isn't running yet.");
  expect(screen.getByRole("button", { name: "Restart the model" })).toBeTruthy();
});

test("stored thoughts are collapsed and stats are shown on hover row", async () => {
  mount({
    conversation_get: () => ({
      conversation: { id: "c1", title: "T", assistant_id: "nexus" },
      messages: [msg({ id: "u0", content: "q" }), msg({ id: "a0", role: "assistant", content: "answer", reasoning: "deep thoughts", tps: 55, ttft_ms: 400 })],
    }),
  }, "/c/c1");
  await screen.findByText("answer");
  expect(screen.queryByText("deep thoughts")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Thoughts/ }));
  expect(screen.getByText("deep thoughts")).toBeTruthy();
  expect(screen.getByText("55 tok/s · 0.4 s")).toBeTruthy();
});

const twoMessages = () => ({
  conversation: { id: "c1", title: "T", assistant_id: "nexus" },
  messages: [msg({ id: "u0", content: "my question" }), msg({ id: "a0", role: "assistant", content: "the answer", tokens_prompt: 50, tokens_out: 4 })],
});

test("Ctrl+Up with an empty composer edits the last message and Ctrl+Shift+C copies the last answer", async () => {
  const writeText = vi.fn(() => Promise.resolve());
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  mount({ conversation_get: twoMessages }, "/c/c1");
  await screen.findByText("the answer");
  fireEvent.keyDown(window, { key: "c", ctrlKey: true, shiftKey: true });
  expect(writeText).toHaveBeenCalledWith("the answer");
  fireEvent.keyDown(window, { key: "ArrowUp", ctrlKey: true });
  expect(await screen.findByLabelText("Edit message")).toBeTruthy();
});

test("Escape stops a streaming answer", async () => {
  const t = mount(
    {
      conversation_get: twoMessages,
      chat_send: (_a, emit) => {
        emit({ type: "started", assistant_message_id: "a-live" });
        emit({ type: "content", delta: "working" });
        return { conversation_id: "c1", user_message_id: "u1", assistant_message_id: "a-live" };
      },
      chat_stop: () => true,
    },
    "/c/c1",
  );
  const box = await screen.findByLabelText("Message");
  fireEvent.change(box, { target: { value: "go" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await screen.findByLabelText("Stop generating");
  fireEvent.keyDown(window, { key: "Escape" });
  await waitFor(() => expect(t.calls.some((c) => c.command === "chat_stop" && (c.args as { assistantMessageId: string }).assistantMessageId === "a-live")).toBe(true));
});
