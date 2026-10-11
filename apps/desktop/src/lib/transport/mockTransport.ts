import { toIpcException } from "../ipc";
import type { CommandArgs, Transport, Unsubscribe } from "./types";

type Handler = (args: CommandArgs | undefined, emit: (event: unknown) => void) => unknown;

/** In-memory transport for tests and storybook-style demos. */
export interface MockTransport extends Transport {
  /** Deliver a global event to subscribers. */
  emit(event: string, payload: unknown): void;
  /** Every call made, in order. */
  readonly calls: ReadonlyArray<{ command: string; args: CommandArgs | undefined }>;
}

export function createMockTransport(handlers: Record<string, Handler>): MockTransport {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const calls: Array<{ command: string; args: CommandArgs | undefined }> = [];

  const run = async (command: string, args: CommandArgs | undefined, emit: (e: unknown) => void) => {
    calls.push({ command, args });
    const handler = handlers[command];
    if (!handler) throw toIpcException({ code: "not_found", message: `no mock for ${command}`, detail: null });
    try {
      return await handler(args, emit);
    } catch (e) {
      throw toIpcException(e);
    }
  };

  return {
    calls,
    call: async <T,>(command: string, args?: CommandArgs) => (await run(command, args, () => {})) as T,
    stream: async <E, T = void>(command: string, args: CommandArgs, onEvent: (event: E) => void) =>
      (await run(command, args, (e) => onEvent(e as E))) as T,
    on: async <T,>(event: string, handler: (payload: T) => void): Promise<Unsubscribe> => {
      const set = listeners.get(event) ?? new Set();
      const wrapped = (p: unknown) => handler(p as T);
      set.add(wrapped);
      listeners.set(event, set);
      return () => set.delete(wrapped);
    },
    emit: (event, payload) => listeners.get(event)?.forEach((l) => l(payload)),
  };
}
