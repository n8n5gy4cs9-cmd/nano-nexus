import { describeChatEvent, logActivity } from "@/stores/activity";

import { tauriTransport } from "./tauriTransport";
import type { Transport } from "./types";

export type { CommandArgs, Transport, Unsubscribe } from "./types";

/** Log every command (names and timing only, never argument values) and streamed chat steps. */
function instrument(inner: Transport): Transport {
  const run = async <T,>(command: string, work: () => Promise<T>): Promise<T> => {
    const started = performance.now();
    logActivity("command", `→ ${command}`);
    try {
      const result = await work();
      logActivity("command", `← ${command} ${Math.round(performance.now() - started)} ms`);
      return result;
    } catch (e) {
      logActivity("error", `${command} failed: ${e instanceof Error ? e.message : String(e)}`);
      throw e;
    }
  };
  return {
    call: (command, args) => run(command, () => inner.call(command, args)),
    stream: (command, args, onEvent) =>
      run(command, () =>
        inner.stream(command, args, (event) => {
          const line = describeChatEvent(event);
          if (line) logActivity(line.kind, line.text);
          onEvent(event as never);
        }),
      ),
    on: (event, handler) => inner.on(event, handler),
  };
}

let active: Transport = instrument(tauriTransport);

/** The transport feature code runs on. */
export function getTransport(): Transport {
  return active;
}

/** Swap the transport (tests, the web build). */
export function setTransport(transport: Transport): void {
  active = instrument(transport);
}
