import { RotateCcw, Send } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { ChatEvent } from "@/bindings";
import { Button, IconButton, Textarea } from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { Markdown } from "@/features/chat/Markdown";

interface Line {
  role: "user" | "assistant";
  text: string;
  error?: boolean;
}

/**
 * A scratch chat for the builder: sends the unsaved TOML as `draft_spec`, so the assistant can be
 * tried before it exists. The throwaway conversation is deleted on reset and on close.
 */
export function TestChat({ toml, valid, starters }: { toml: string; valid: boolean; starters: string[] }) {
  const [lines, setLines] = useState<Line[]>([]);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const conversation = useRef<string | null>(null);
  const assistantId = useRef<string | null>(null);
  const bottom = useRef<HTMLDivElement | null>(null);

  const dropConversation = () => {
    const id = conversation.current;
    conversation.current = null;
    if (id) void api.conversations.delete(id).catch(() => undefined);
  };
  useEffect(() => dropConversation, []);
  useEffect(() => bottom.current?.scrollIntoView?.({ block: "end" }), [lines]);

  const send = async (msg: string) => {
    if (!msg.trim() || busy || !valid) return;
    setText("");
    setBusy(true);
    setLines((l) => [...l, { role: "user", text: msg }, { role: "assistant", text: "" }]);
    const patch = (fn: (last: Line) => Line) =>
      setLines((l) => (l.length === 0 ? l : [...l.slice(0, -1), fn(l[l.length - 1] as Line)]));
    const onEvent = (e: ChatEvent) => {
      if (e.type === "started") assistantId.current = e.assistant_message_id;
      else if (e.type === "content") patch((x) => ({ ...x, text: x.text + e.delta }));
      else if (e.type === "error") patch((x) => ({ ...x, text: e.message, error: true }));
    };
    try {
      const r = await api.chat.send(
        { conversation_id: conversation.current, assistant_id: "nexus", text: msg, attachments: [], think: false, draft_spec: toml },
        onEvent,
      );
      conversation.current = r.conversation_id;
    } catch (e) {
      patch((x) => ({ ...x, text: toIpcException(e).message, error: true }));
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    if (assistantId.current) void api.chat.stop(assistantId.current).catch(() => undefined);
    dropConversation();
    setLines([]);
    setBusy(false);
  };

  return (
    <section aria-label="Test chat" className="flex h-full min-h-0 flex-col rounded-card border border-border bg-surface">
      <header className="flex items-center justify-between border-b border-border px-3 py-2">
        <h3 className="text-body font-semibold">Test chat</h3>
        <IconButton aria-label="Start over" size="sm" onClick={reset} disabled={lines.length === 0}>
          <RotateCcw />
        </IconButton>
      </header>
      <div className="min-h-0 flex-1 space-y-3 overflow-auto p-3">
        {lines.length === 0 ? (
          <div className="space-y-2">
            <p className="text-small text-muted">Try the assistant with your current, unsaved settings.</p>
            {starters.map((s) => (
              <Button key={s} size="sm" className="h-auto w-full justify-start py-1.5 text-left whitespace-normal" disabled={!valid} onClick={() => void send(s)}>
                {s}
              </Button>
            ))}
            {!valid ? <p className="text-small text-warning">Fix the errors on the left to test.</p> : null}
          </div>
        ) : (
          lines.map((l, i) =>
            l.role === "user" ? (
              <div key={i} className="ml-auto max-w-[85%] rounded-modal bg-surface-2 px-3 py-2 text-body whitespace-pre-wrap">
                {l.text}
              </div>
            ) : (
              <div key={i} className={l.error ? "text-danger" : ""}>
                {l.text ? <Markdown source={l.text} onOpenLink={() => undefined} /> : <p className="animate-pulse text-muted">…</p>}
              </div>
            ),
          )
        )}
        <div ref={bottom} />
      </div>
      <form
        className="flex items-end gap-2 border-t border-border p-2"
        onSubmit={(e) => {
          e.preventDefault();
          void send(text);
        }}
      >
        <Textarea
          aria-label="Test message"
          placeholder="Type a test message"
          value={text}
          maxHeight={96}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void send(text);
            }
          }}
        />
        <Button type="submit" variant="primary" disabled={busy || !valid || !text.trim()} aria-label="Send test message">
          <Send />
        </Button>
      </form>
    </section>
  );
}
