import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { AttachmentInput } from "@/bindings";
import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { Composer, estimateAttachmentTokens, isTruncated } from "./Composer";

afterEach(() => setTransport(tauriTransport));

const file = (label: string, text: string, path = `/x/${label}`): AttachmentInput => ({ kind: "file", path, label, text });

function mount(handlers: Parameters<typeof createMockTransport>[0], onSend = vi.fn()) {
  const t = createMockTransport(handlers);
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Composer placeholder="Ask" streaming={false} think={false} onThinkChange={() => undefined} onSend={onSend} onStop={() => undefined} usedTokens={0} ctxSize={8192} />
    </QueryClientProvider>,
  );
  return { t, onSend };
}

test("picked files appear as chips with a token estimate and are sent with the message", async () => {
  const { onSend } = mount({ attachments_pick: () => [file("notes.md", "x".repeat(320))] });
  fireEvent.click(screen.getByLabelText("Attach files"));
  expect(await screen.findByText(/notes\.md/)).toBeTruthy();
  expect(screen.getByText("~100 tok")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "summarize" } });
  fireEvent.click(screen.getByLabelText("Send message"));
  expect(onSend).toHaveBeenCalledWith("summarize", [expect.objectContaining({ label: "notes.md" })]);
  expect(screen.queryByText(/notes\.md/)).toBeNull();
});

test("a file alone can be sent, and chips can be removed", async () => {
  const { onSend } = mount({ attachments_pick: () => [file("a.txt", "hi"), file("b.txt", "yo")] });
  fireEvent.click(screen.getByLabelText("Attach files"));
  await screen.findByText(/a\.txt/);
  fireEvent.click(screen.getByLabelText("Remove a.txt"));
  expect(screen.queryByText(/a\.txt/)).toBeNull();
  fireEvent.click(screen.getByLabelText("Send message"));
  expect(onSend).toHaveBeenCalledWith("", [expect.objectContaining({ label: "b.txt" })]);
});

test("dropping files reads them and shows the drop hint while hovering", async () => {
  const { t } = mount({ attachments_read: (args) => (args as { paths: string[] }).paths.map((p) => file(p.split("/").pop() ?? p, "data", p)) });
  await waitFor(() => expect(screen.queryByText("Drop to attach")).toBeNull());
  await act(async () => {
    t.emit("tauri://drag-enter", {});
  });
  expect(await screen.findByText("Drop to attach")).toBeTruthy();
  await act(async () => {
    t.emit("tauri://drag-drop", { paths: ["/home/me/guide.md"] });
  });
  expect(await screen.findByText(/guide\.md/)).toBeTruthy();
  expect(screen.queryByText("Drop to attach")).toBeNull();
});

test("read errors are surfaced instead of attaching", async () => {
  const { t } = mount({
    attachments_pick: () => {
      throw { code: "invalid_input", message: "this looks like a binary file, not text", detail: null };
    },
  });
  fireEvent.click(screen.getByLabelText("Attach files"));
  await waitFor(() => expect(t.calls.some((c) => c.command === "attachments_pick")).toBe(true));
  expect(screen.queryByLabelText("Attachments")).toBeNull();
});

test("helpers", () => {
  expect(estimateAttachmentTokens(file("a", "x".repeat(32)))).toBe(10);
  expect(isTruncated(file("a", "text\n[… truncated: the file is 900 KB]"))).toBe(true);
  expect(isTruncated(file("a", "short"))).toBe(false);
});
