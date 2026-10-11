import { tauriTransport } from "./tauriTransport";
import type { Transport } from "./types";

export type { CommandArgs, Transport, Unsubscribe } from "./types";

let active: Transport = tauriTransport;

/** The transport feature code runs on. */
export function getTransport(): Transport {
  return active;
}

/** Swap the transport (tests, the web build). */
export function setTransport(transport: Transport): void {
  active = transport;
}
