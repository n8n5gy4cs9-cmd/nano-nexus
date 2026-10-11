// The only place (besides the generated bindings) allowed to import `invoke`.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { toIpcException } from "../ipc";
import type { CommandArgs, Transport, Unsubscribe } from "./types";

export const tauriTransport: Transport = {
  async call<T>(command: string, args?: CommandArgs): Promise<T> {
    try {
      return await invoke<T>(command, args);
    } catch (e) {
      throw toIpcException(e);
    }
  },

  async stream<E, T = void>(command: string, args: CommandArgs, onEvent: (event: E) => void): Promise<T> {
    const channel = new Channel<E>();
    channel.onmessage = onEvent;
    try {
      return await invoke<T>(command, { ...args, onEvent: channel });
    } catch (e) {
      throw toIpcException(e);
    }
  },

  async on<T>(event: string, handler: (payload: T) => void): Promise<Unsubscribe> {
    return listen<T>(event, (e) => handler(e.payload));
  },
};
