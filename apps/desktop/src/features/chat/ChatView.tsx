import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useParams } from "react-router-dom";

import type { Message } from "@/bindings";
import { api } from "@/lib/api";
import type { LiveTurn } from "@/stores/chat";

import { Composer } from "./Composer";
import { EmptyChat } from "./EmptyChat";
import { AssistantMessage, UserMessage, type AssistantView } from "./Message";
import { MessageList, type ListItem } from "./MessageList";
import { NEXUS_PLACEHOLDER, NEXUS_STARTERS } from "./starters";
import { conversationKey, useChatStream } from "./useChatStream";

const ASSISTANT_NAME = "Nexus";
const TAGLINE = "Your everyday local copilot. It can read the folders you share.";

function storedView(m: Message): AssistantView {
  return {
    id: m.id,
    content: m.content,
    reasoning: m.reasoning,
    streaming: m.status === "streaming",
    stopped: m.status === "stopped",
    error: m.status === "error" ? { code: "internal", message: m.error ?? "Something went wrong." } : null,
    stats: m.tps !== null || m.ttft_ms !== null ? { tps: m.tps, ttftMs: m.ttft_ms, tokensOut: m.tokens_out } : null,
  };
}

function liveView(t: LiveTurn): AssistantView {
  return {
    id: t.assistantId ?? "live",
    content: t.content,
    reasoning: t.reasoning || null,
    streaming: t.status === "starting" || t.status === "streaming",
    thinkingStartedAt: t.firstReasoningAt ?? t.startedAt,
    stopped: t.status === "stopped",
    error: t.error,
    stats: null,
    statusText: t.statusText,
  };
}

/** Whether the optimistic user bubble is still needed (the stored message has not arrived yet). */
export function needsOptimisticUser(messages: Message[], t: LiveTurn): boolean {
  if (!t.userText) return false;
  return !messages.some((m) => m.role === "user" && m.content === t.userText && m.created_at >= t.startedAt - 2000);
}

/** One conversation: messages, composer, and the live (streaming) turn. */
export function ChatView() {
  const { conversationId } = useParams();
  const { turn, streaming, send, regenerate, editResend, stop } = useChatStream(conversationId);
  const [think, setThink] = useState(false);
  const restart = () => void api.runtime.restart().catch(() => undefined);

  const conv = useQuery({
    queryKey: conversationKey(conversationId ?? "none"),
    queryFn: () => api.conversations.get(conversationId ?? ""),
    enabled: conversationId !== undefined,
    retry: false,
  });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings.get, retry: false, staleTime: 60_000 });
  const ctxSize = settings.data?.["runtime.ctx_size"] ?? 8192;

  const messages = useMemo(() => conv.data?.messages ?? [], [conv.data]);
  const liveSlot = turn;

  const items: ListItem[] = useMemo(() => {
    const out: ListItem[] = [];
    const lastUser = [...messages].reverse().find((m) => m.role === "user");
    for (const m of messages) {
      if (m.role === "user") {
        out.push({
          key: m.id,
          node: <UserMessage m={m} canEdit={!streaming && lastUser?.id === m.id} onEdit={(text) => void editResend(m.id, text, think)} />,
        });
      } else if (m.role === "assistant") {
        if (liveSlot?.assistantId === m.id) continue; // rendered live below
        out.push({
          key: m.id,
          node: <AssistantMessage v={storedView(m)} name={ASSISTANT_NAME} onRegenerate={streaming ? undefined : () => void regenerate(m.id, think)} onRestart={restart} />,
        });
      }
    }
    if (liveSlot) {
      if (needsOptimisticUser(messages, liveSlot)) {
        out.push({ key: "live-user", node: <UserMessage m={{ id: "live-user", content: liveSlot.userText, attachments: [] }} canEdit={false} onEdit={() => undefined} /> });
      }
      out.push({
        key: liveSlot.assistantId ?? "live-assistant",
        node: <AssistantMessage v={liveView(liveSlot)} name={ASSISTANT_NAME} onRestart={restart} onRegenerate={() => undefined} />,
      });
    }
    return out;
  }, [messages, liveSlot, streaming, think, editResend, regenerate]);

  const last = [...messages].reverse().find((m) => m.role === "assistant" && m.tokens_prompt !== null);
  const usedTokens = (last?.tokens_prompt ?? 0) + (last?.tokens_out ?? 0);
  const empty = items.length === 0;

  return (
    <div className="flex h-full flex-col">
      {conv.data ? (
        <header className="flex h-[52px] shrink-0 items-center border-b border-border px-5 backdrop-blur">
          <h2 className="truncate text-body font-semibold">{conv.data.conversation.title}</h2>
        </header>
      ) : null}
      {empty ? (
        <div className="min-h-0 flex-1">
          <EmptyChat name={ASSISTANT_NAME} tagline={TAGLINE} starters={NEXUS_STARTERS} onPick={(t) => void send(t, [], think)} />
        </div>
      ) : (
        <MessageList items={items} version={`${liveSlot?.content.length ?? 0}:${liveSlot?.reasoning.length ?? 0}:${messages.length}`} />
      )}
      <Composer
        key={conversationId ?? "new"}
        placeholder={NEXUS_PLACEHOLDER}
        streaming={streaming}
        think={think}
        onThinkChange={setThink}
        onSend={(text) => void send(text, [], think)}
        onStop={() => void stop()}
        usedTokens={usedTokens}
        ctxSize={ctxSize}
      />
    </div>
  );
}
