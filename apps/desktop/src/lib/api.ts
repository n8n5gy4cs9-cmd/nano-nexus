/**
 * The typed facade feature code uses to talk to the backend. It sits on the active `Transport`, so
 * it works on desktop (Tauri) and, later, in the LAN Share web build. Never call `invoke` directly
 * (an ESLint rule enforces this). Types come from the generated `bindings.ts`.
 */
import type { AppInfo, NotificationPayload, RuntimeState, Settings } from "@/bindings";

import { getTransport } from "./transport";
import type { Unsubscribe } from "./transport";

/** Value accepted by `settings.set`. */
export type SettingValue = null | boolean | number | string | string[] | Record<string, boolean>;

export const api = {
  app: {
    info: () => getTransport().call<AppInfo>("app_info"),
  },
  runtime: {
    status: () => getTransport().call<RuntimeState>("runtime_status"),
    restart: () => getTransport().call<null>("runtime_restart"),
    logs: (lines: number) => getTransport().call<string[]>("runtime_logs", { lines }),
    /** Subscribe to `runtime:status`. */
    onStatus: (handler: (state: RuntimeState) => void): Promise<Unsubscribe> =>
      getTransport().on<RuntimeState>("runtime:status", handler),
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
