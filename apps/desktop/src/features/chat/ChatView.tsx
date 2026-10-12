import { useQuery } from "@tanstack/react-query";
import { ChevronsUpDown } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";

import type { Message } from "@/bindings";
import { AssistantIcon, Kbd } from "@/components/ui";
import { FALLBACK_CARD, useAssistants } from "@/hooks/useAssistants";
import { usePlatform } from "@/hooks/usePlatform";
import { useChatShortcuts } from "@/hooks/useChatShortcuts";
import { toast } from "sonner";

import type { AttachmentInput } from "@/bindings";
import { useRuntime } from "@/hooks/useRuntime";
import { api } from "@/lib/api";
import { logActivity, useActivity } from "@/stores/activity";
import { modLabel } from "@/lib/platform";
import { useAssistantChoice } from "@/stores/assistant";
import type { LiveTurn } from "@/stores/chat";

import { Composer } from "./Composer";
import { EmptyChat } from "./EmptyChat";
import { AssistantMessage, UserMessage, type AssistantBadge, type AssistantView } from "./Message";
import { parseToolTrace } from "./ToolChips";
import { MessageList, type ListItem } from "./MessageList";
import { conversationKey, useChatStream } from "./useChatStream";


function storedView(m: Message): AssistantView {
  return {
    id: m.id,
    content: m.content,
    reasoning: m.reasoning,
    streaming: m.status === "streaming",
    stopped: m.status === "stopped",
    error: m.status === "error" ? { code: "internal", message: m.error ?? "Something went wrong." } : null,
    stats: m.tps !== null || m.ttft_ms !== null ? { tps: m.tps, ttftMs: m.ttft_ms, tokensOut: m.tokens_out } : null,
    tools: parseToolTrace(m.tool_trace),
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
    tools: t.tools,
  };
}

/** Whether the optimistic user bubble is still needed (the stored message has not arrived yet). */
export function needsOptimisticUser(messages: Message[], t: LiveTurn): boolean {
  if (!t.userText) return false;
  return !messages.some((m) => m.role === "user" && m.content === t.userText && m.created_at >= t.startedAt - 2000);
}

function SwitchDivider({ name }: { name: string }) {
  return (
    <div role="separator" className="my-1 flex items-center gap-3 text-caption text-faint">
      <span className="h-px flex-1 bg-border" />
      Switched to {name}
      <span className="h-px flex-1 bg-border" />
    </div>
  );
}

/** One conversation: messages, composer, and the live (streaming) turn. */
export function ChatView() {
  const { conversationId } = useParams();
  const selectedId = useAssistantChoice((s) => s.selectedId);
  const setSwitcherOpen = useAssistantChoice((s) => s.setSwitcherOpen);
  const { byId } = useAssistants();
  const [think, setThink] = useState(false);
  const [composerEmpty, setComposerEmpty] = useState(true);
  const [editingId, setEditingId] = useState<string | null>(null);
  const platform = usePlatform();
  const restart = () => void api.runtime.restart().catch(() => undefined);

  const conv = useQuery({
    queryKey: conversationKey(conversationId ?? "none"),
    queryFn: () => api.conversations.get(conversationId ?? ""),
    enabled: conversationId !== undefined,
    retry: false,
  });
  const activeId = conv.data?.conversation.assistant_id ?? selectedId;
  const card = byId.get(activeId) ?? (activeId === FALLBACK_CARD.id ? FALLBACK_CARD : (byId.get("nexus") ?? FALLBACK_CARD));
  const { turn, streaming, send, regenerate, editResend, stop } = useChatStream(conversationId, card.id);
  // Duo: tell which model answered by comparing the stored model id with the two configured ones.
  const duoSettings = useQuery({ queryKey: ["settings", "duo"], queryFn: api.settings.get, enabled: card.id === "duo", retry: false });
  const duoTier = useCallback(
    (modelId: string | null): "light" | "strong" | null => {
      const s = duoSettings.data as unknown as Record<string, unknown> | undefined;
      const light = String(s?.["duo.light_model_id"] ?? "");
      const strong = String(s?.["duo.strong_model_id"] ?? "");
      if (!modelId || !light || !strong || light === strong) return null;
      return modelId === strong ? "strong" : modelId === light ? "light" : null;
    },
    [duoSettings.data],
  );
  const badgeFor = useCallback(
    (id: string | null): AssistantBadge => {
      const c = (id ? byId.get(id) : undefined) ?? card;
      return { name: c.name, icon: c.icon, color: c.color };
    },
    [byId, card],
  );
  const runtime = useRuntime();

  /** `/nano-nexus [question]`: attach a health report and ask the model; without a model, show it in the console. */
  const pendingQuestion = useAssistantChoice((s) => s.pendingQuestion);
  const setPendingQuestion = useAssistantChoice((s) => s.setPendingQuestion);
  useEffect(() => {
    if (!conversationId && pendingQuestion) {
      setPendingQuestion(null);
      void send(pendingQuestion, [], false);
    }
  }, [conversationId, pendingQuestion, setPendingQuestion, send]);

  const sendOrCommand = async (text: string, attachments: AttachmentInput[]) => {
    const m = /^\/nano-nexus\b\s*([\s\S]*)$/i.exec(text.trim());
    if (!m) return send(text, attachments, think);
    const question = (m[1] ?? "").trim() || "Check that NanoNexus is working and tell me anything that needs fixing.";
    let report: string;
    try {
      report = await api.app.diagnostics();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Could not build the diagnostics.");
      return;
    }
    if (runtime?.state !== "ready") {
      // The model cannot answer, so show the report where it can be read.
      for (const line of report.split("\n")) if (line.trim()) logActivity("runtime", line);
      useActivity.setState({ open: true });
      toast.info("The model is not ready. The diagnostics are in the Activity console.");
      return;
    }
    return send(question, [...attachments, { kind: "text", path: null, label: "NanoNexus diagnostics", text: report }], think);
  };
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings.get, retry: false, staleTime: 60_000 });
  const ctxSize = settings.data?.["runtime.ctx_size"] ?? 8192;

  const messages = useMemo(() => conv.data?.messages ?? [], [conv.data]);
  const liveSlot = turn;

  const items: ListItem[] = useMemo(() => {
    const out: ListItem[] = [];
    const lastUser = [...messages].reverse().find((m) => m.role === "user");
    let lastAssistant: string | null = null;
    for (const m of messages) {
      if (m.role === "user") {
        out.push({
          key: m.id,
          node: (
            <UserMessage
              key={`${m.id}:${editingId === m.id}`}
              m={m}
              canEdit={!streaming && lastUser?.id === m.id}
              startEditing={editingId === m.id}
              onEdit={(text) => {
                setEditingId(null);
                void editResend(m.id, text, think);
              }}
            />
          ),
        });
      } else if (m.role === "assistant") {
        if (liveSlot?.assistantId === m.id) continue; // rendered live below
        if (m.assistant_id && lastAssistant !== null && m.assistant_id !== lastAssistant) {
          out.push({ key: `switch-${m.id}`, node: <SwitchDivider name={badgeFor(m.assistant_id).name} /> });
        }
        if (m.assistant_id) lastAssistant = m.assistant_id;
        out.push({
          key: m.id,
          node: (
            <AssistantMessage
              v={storedView(m)}
              assistant={badgeFor(m.assistant_id)}
              onRegenerate={streaming ? undefined : () => void regenerate(m.id, think)}
              onRestart={restart}
              tier={m.assistant_id === "duo" ? duoTier(m.model_id) : null}
              // "Think harder" always routes Duo to the strong model.
              onUseStrong={streaming ? undefined : () => void regenerate(m.id, true)}
            />
          ),
        });
      }
    }
    if (liveSlot) {
      if (needsOptimisticUser(messages, liveSlot)) {
        out.push({ key: "live-user", node: <UserMessage m={{ id: "live-user", content: liveSlot.userText, attachments: [] }} canEdit={false} onEdit={() => undefined} /> });
      }
      out.push({
        key: liveSlot.assistantId ?? "live-assistant",
        node: <AssistantMessage v={liveView(liveSlot)} assistant={badgeFor(card.id)} onRestart={restart} onRegenerate={() => undefined} />,
      });
    }
    return out;
  }, [messages, liveSlot, streaming, think, duoTier, editResend, regenerate, editingId, badgeFor, card.id]);

  const lastUserMsg = [...messages].reverse().find((m) => m.role === "user");
  const lastAnswer = [...messages].reverse().find((m) => m.role === "assistant" && m.content);
  const shortcutActions = useMemo(
    () => ({
      streaming,
      stop: () => void stop(),
      composerEmpty,
      toggleThink: () => setThink((t) => !t),
      editLast: !streaming && lastUserMsg ? () => setEditingId(lastUserMsg.id) : undefined,
      copyLastAnswer: lastAnswer ? () => void navigator.clipboard.writeText(lastAnswer.content).catch(() => undefined) : undefined,
    }),
    [streaming, stop, composerEmpty, lastUserMsg, lastAnswer],
  );
  useChatShortcuts(platform, shortcutActions);
  const onEmptyChange = useCallback((v: boolean) => setComposerEmpty(v), []);

  const last = [...messages].reverse().find((m) => m.role === "assistant" && m.tokens_prompt !== null);
  const usedTokens = (last?.tokens_prompt ?? 0) + (last?.tokens_out ?? 0);
  const empty = items.length === 0;

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-[52px] shrink-0 items-center gap-3 border-b border-border px-5 backdrop-blur">
        <h2 className="min-w-0 flex-1 truncate text-body font-semibold">{conv.data?.conversation.title ?? ""}</h2>
        <button
          type="button"
          onClick={() => setSwitcherOpen(true)}
          aria-label={`Assistant: ${card.name}. Switch assistant`}
          className="flex h-8 shrink-0 items-center gap-2 rounded-button border border-border bg-surface px-2 text-small hover:bg-surface-2"
        >
          <AssistantIcon icon={card.icon} color={card.color} size={20} />
          <span className="font-medium">{card.name}</span>
          <ChevronsUpDown className="size-3.5 text-faint" />
          <Kbd>{modLabel(platform)}/</Kbd>
        </button>
      </header>
      {empty ? (
        <div className="min-h-0 flex-1">
          <EmptyChat name={card.name} icon={card.icon} color={card.color} tagline={card.tagline} starters={card.starters} onPick={(t) => void send(t, [], think)} />
        </div>
      ) : (
        <MessageList items={items} version={`${liveSlot?.content.length ?? 0}:${liveSlot?.reasoning.length ?? 0}:${messages.length}`} />
      )}
      <Composer
        key={conversationId ?? "new"}
        placeholder={card.placeholder ?? "Ask anything…"}
        streaming={streaming}
        think={think}
        onThinkChange={setThink}
        onSend={(text, attachments) => void sendOrCommand(text, attachments)}
        onStop={() => void stop()}
        onEmptyChange={onEmptyChange}
        usedTokens={usedTokens}
        ctxSize={ctxSize}
      />
    </div>
  );
}
