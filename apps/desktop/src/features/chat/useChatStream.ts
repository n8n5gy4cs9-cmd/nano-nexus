import { useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import { useNavigate } from "react-router-dom";

import type { AttachmentInput, ChatEvent } from "@/bindings";
import { CONVERSATIONS_KEY } from "@/hooks/useConversations";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { logActivity } from "@/stores/activity";
import { NEW, flushDeltas, handleChatEvent, rekeyBuffer, useChat } from "@/stores/chat";

export const conversationKey = (id: string) => ["conversation", id] as const;

export const NEXUS = "nexus";

/**
 * Chat actions for one conversation (`undefined` = a new chat). The live turn lives in the chat
 * store, so navigating away does not interrupt streaming and coming back shows the progress.
 */
export function useChatStream(conversationId: string | undefined, assistantId: string = NEXUS) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const key = conversationId ?? NEW;
  const turn = useChat((s) => s.turns[key]);

  const finish = useCallback(
    async (finalKey: () => string) => {
      // Keep the live view until the stored messages are loaded, so nothing flickers.
      const k = finalKey();
      const t = useChat.getState().turns[k];
      if (t && t.status !== "streaming" && t.status !== "starting") {
        await qc.invalidateQueries({ queryKey: conversationKey(k) });
        await qc.invalidateQueries({ queryKey: CONVERSATIONS_KEY });
        useChat.getState().clear(k);
      }
    },
    [qc],
  );

  const run = useCallback(
    async (userText: string, start: (onEvent: (e: ChatEvent) => void) => Promise<{ conversation_id: string }>) => {
      let current = key;
      useChat.getState().begin(current, userText);
      const onEvent = (e: ChatEvent) => {
        handleChatEvent(() => current, e);
        if (e.type === "done" || e.type === "error") {
          // Wait one tick so the final flush lands, then reload from the store.
          setTimeout(() => void finish(() => current), 0);
        }
      };
      try {
        const result = await start(onEvent);
        if (result.conversation_id !== current) {
          flushDeltas();
          useChat.getState().rekey(current, result.conversation_id);
          rekeyBuffer(current, result.conversation_id);
          current = result.conversation_id;
          void qc.invalidateQueries({ queryKey: CONVERSATIONS_KEY });
          void navigate(`/c/${result.conversation_id}`, { replace: true });
        }
        void qc.invalidateQueries({ queryKey: conversationKey(current) });
      } catch (e) {
        const err = toIpcException(e);
        useChat.getState().apply(current, { status: "error", error: { code: err.code, message: err.message } });
      }
    },
    [key, qc, navigate, finish],
  );

  const send = useCallback(
    (text: string, attachments: AttachmentInput[], think: boolean) =>
      run(text, (onEvent) =>
        api.chat.send({ conversation_id: conversationId ?? null, assistant_id: assistantId, text, attachments, think }, onEvent),
      ),
    [run, conversationId, assistantId],
  );

  const regenerate = useCallback(
    (assistantMessageId: string, think: boolean) => run("", (onEvent) => api.chat.regenerate(assistantMessageId, think, onEvent)),
    [run],
  );

  const editResend = useCallback(
    (userMessageId: string, text: string, think: boolean) =>
      run(text, (onEvent) => api.chat.editResend(userMessageId, text, think, onEvent)),
    [run],
  );

  // Stop shows immediately: the turn flips to "stopped" and late text is dropped, then the backend is told.
  const stop = useCallback(async () => {
    const st = useChat.getState();
    const t = st.turns[key];
    if (!t || (t.status !== "starting" && t.status !== "streaming")) return;
    flushDeltas();
    st.apply(key, { status: "stopped", statusText: null, stopRequested: true });
    logActivity("chat", t.assistantId ? "stop pressed" : "stop pressed (waiting for the answer to start)");
    if (!t.assistantId) return; // sent when the `started` event arrives
    try {
      const running = await api.chat.stop(t.assistantId);
      logActivity("chat", running ? "answer cancelled" : "answer had already finished");
    } catch (e) {
      logActivity("error", `stop failed: ${toIpcException(e).message}`);
    }
  }, [key]);

  const streaming = turn?.status === "starting" || turn?.status === "streaming";
  return { turn, streaming, send, regenerate, editResend, stop };
}
