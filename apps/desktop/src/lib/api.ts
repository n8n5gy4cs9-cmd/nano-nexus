/**
 * The typed facade feature code uses to talk to the backend. It sits on the active `Transport`, so
 * it works on desktop (Tauri) and, later, in the LAN Share web build. Never call `invoke` directly
 * (an ESLint rule enforces this). Types come from the generated `bindings.ts`.
 */
import type {
  AppInfo,
  AttachmentInput,
  ChatEvent,
  ChatSendArgs,
  ChatSendResult,
  ConversationSummary,
  ConversationUpdate,
  ConversationWithMessages,
  NotificationPayload,
  RuntimeState,
  Settings,
} from "@/bindings";

import { getTransport } from "./transport";
import type { Unsubscribe } from "./transport";

/** Value accepted by `settings.set`. */
export type SettingValue = null | boolean | number | string | string[] | Record<string, boolean>;

export const api = {
  app: {
    info: () => getTransport().call<AppInfo>("app_info"),
    /** Open an http(s) link in the OS browser. */
    openExternal: (url: string) => getTransport().call<null>("open_external", { url }),
  },
  runtime: {
    status: () => getTransport().call<RuntimeState>("runtime_status"),
    restart: () => getTransport().call<null>("runtime_restart"),
    logs: (lines: number) => getTransport().call<string[]>("runtime_logs", { lines }),
    /** Subscribe to `runtime:status`. */
    onStatus: (handler: (state: RuntimeState) => void): Promise<Unsubscribe> =>
      getTransport().on<RuntimeState>("runtime:status", handler),
  },
  attachments: {
    /** Read dropped files (text only, <= 256 KB each). */
    read: (paths: string[]) => getTransport().call<AttachmentInput[]>("attachments_read", { paths }),
    /** Native file picker; empty when cancelled. */
    pick: () => getTransport().call<AttachmentInput[]>("attachments_pick"),
    /** OS file drag-and-drop over the window: hover state and the dropped paths. */
    onDrag: async (handlers: { enter: () => void; leave: () => void; drop: (paths: string[]) => void }): Promise<Unsubscribe> => {
      const t = getTransport();
      const offs = await Promise.all([
        t.on("tauri://drag-enter", handlers.enter),
        t.on("tauri://drag-leave", handlers.leave),
        t.on<{ paths: string[] }>("tauri://drag-drop", (p) => handlers.drop(p.paths)),
      ]);
      return () => offs.forEach((off) => off());
    },
  },
  conversations: {
    list: (query: string | null, limit: number, before: number | null = null) =>
      getTransport().call<ConversationSummary[]>("conversations_list", { query, limit, before }),
    get: (id: string) => getTransport().call<ConversationWithMessages>("conversation_get", { id }),
    update: (id: string, update: ConversationUpdate) => getTransport().call<null>("conversation_update", { id, update }),
    delete: (id: string) => getTransport().call<null>("conversation_delete", { id }),
    exportMd: (id: string) => getTransport().call<string>("conversation_export_md", { id }),
    /** Subscribe to `conversation:updated` (a title or summary changed). */
    onUpdated: (handler: (id: string) => void): Promise<Unsubscribe> =>
      getTransport().on<string>("conversation:updated", handler),
  },
  chat: {
    send: (args: ChatSendArgs, onEvent: (e: ChatEvent) => void) =>
      getTransport().stream<ChatEvent, ChatSendResult>("chat_send", { args }, onEvent),
    stop: (assistantMessageId: string) => getTransport().call<boolean>("chat_stop", { assistantMessageId }),
    regenerate: (assistantMessageId: string, think: boolean, onEvent: (e: ChatEvent) => void) =>
      getTransport().stream<ChatEvent, ChatSendResult>("chat_regenerate", { assistantMessageId, think }, onEvent),
    editResend: (userMessageId: string, text: string, think: boolean, onEvent: (e: ChatEvent) => void) =>
      getTransport().stream<ChatEvent, ChatSendResult>("chat_edit_resend", { userMessageId, text, think }, onEvent),
  },
  notifications: {
    list: (limit: number) => getTransport().call<NotificationPayload[]>("notifications_list", { limit }),
    unreadCount: () => getTransport().call<number>("notifications_unread_count"),
    /** Empty `ids` marks everything read. */
    markRead: (ids: string[]) => getTransport().call<null>("notifications_mark_read", { ids }),
    clear: () => getTransport().call<null>("notifications_clear"),
    sendTest: () => getTransport().call<NotificationPayload>("notifications_send_test"),
    /** Subscribe to `notification:new`. */
    onNew: (handler: (n: NotificationPayload) => void): Promise<Unsubscribe> =>
      getTransport().on<NotificationPayload>("notification:new", handler),
  },
  settings: {
    get: () => getTransport().call<Settings>("settings_get"),
    set: (key: string, value: SettingValue) => getTransport().call<Settings>("settings_set", { key, value }),
  },
};
