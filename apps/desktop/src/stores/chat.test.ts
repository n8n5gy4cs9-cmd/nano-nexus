import { beforeEach, expect, test, vi } from "vitest";

import { NEW, bufferDelta, flushDeltas, handleChatEvent, useChat } from "./chat";

beforeEach(() => {
  useChat.setState({ turns: {} });
});

test("deltas are merged into one update per frame", () => {
  const spy = vi.fn();
  const unsub = useChat.subscribe(spy);
  useChat.getState().begin("c1", "hi");
  spy.mockClear();
  for (const d of ["a", "b", "c", "d"]) bufferDelta("c1", "content", d);
  bufferDelta("c1", "reasoning", "why");
  expect(useChat.getState().turns["c1"]?.content).toBe("");
  flushDeltas();
  expect(spy).toHaveBeenCalledTimes(1);
  expect(useChat.getState().turns["c1"]).toMatchObject({ content: "abcd", reasoning: "why", status: "streaming" });
  unsub();
});

test("events drive the turn lifecycle", () => {
  const st = useChat.getState();
  st.begin(NEW, "question");
  const key = () => NEW;
  handleChatEvent(key, { type: "started", assistant_message_id: "m1" });
  handleChatEvent(key, { type: "reasoning", delta: "hm" });
  handleChatEvent(key, { type: "content", delta: "Hello" });
  handleChatEvent(key, { type: "done", finish_reason: "stop", tokens_out: 3, tps: 40, ttft_ms: 100 });
  const t = useChat.getState().turns[NEW];
  expect(t).toMatchObject({ assistantId: "m1", content: "Hello", reasoning: "hm", status: "done" });
  expect(t?.firstContentAt).not.toBeNull();
});

test("stopped and error endings", () => {
  const st = useChat.getState();
  st.begin("a", "q");
  handleChatEvent(() => "a", { type: "content", delta: "partial" });
  handleChatEvent(() => "a", { type: "done", finish_reason: "stopped", tokens_out: 0, tps: null, ttft_ms: 0 });
  expect(useChat.getState().turns["a"]).toMatchObject({ status: "stopped", content: "partial" });
  st.begin("b", "q");
  handleChatEvent(() => "b", { type: "error", code: "runtime_unavailable", message: "no model" });
  expect(useChat.getState().turns["b"]?.error).toEqual({ code: "runtime_unavailable", message: "no model" });
});

test("rekey moves a new chat's turn to its real id", () => {
  const st = useChat.getState();
  st.begin(NEW, "q");
  st.rekey(NEW, "conv-1");
  expect(useChat.getState().turns[NEW]).toBeUndefined();
  expect(useChat.getState().turns["conv-1"]?.userText).toBe("q");
});
