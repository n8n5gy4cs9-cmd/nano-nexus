import { IpcException, toIpcException } from "../ipc";
import type { CommandArgs, Transport, Unsubscribe } from "./types";

export type Connection = "connecting" | "open" | "reconnecting" | "ended";

interface Pending {
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
  onEvent?: (event: unknown) => void;
}

export interface WsTransport extends Transport {
  /** Current connection state. */
  readonly connection: Connection;
  /** Called on every state change. */
  onConnection(cb: (c: Connection) => void): Unsubscribe;
  /** Called after a reconnect or when the tab becomes visible again: refetch what may have been missed. */
  onResume(cb: () => void): Unsubscribe;
  close(): void;
}

interface Options {
  csrf: string;
  /** The server revoked this device (close code 4401). */
  onSessionEnded: () => void;
}

const BACKOFF_MS = [500, 1000, 2000, 5000, 10000];

/** The LAN Share web transport: JSON frames over one WebSocket (LAN-SHARE §4.2). */
export function createWsTransport(opts: Options): WsTransport {
  let socket: WebSocket | null = null;
  let connection: Connection = "connecting";
  let nextId = 1;
  let attempt = 0;
  let closed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const pending = new Map<number, Pending>();
  // Streams keep delivering events after their result arrived, until a terminal event.
  const streams = new Map<number, (event: unknown) => void>();
  const queue: string[] = [];
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const connListeners = new Set<(c: Connection) => void>();
  const resumeListeners = new Set<() => void>();

  const setConnection = (c: Connection) => {
    if (connection === c) return;
    connection = c;
    connListeners.forEach((l) => l(c));
  };

  const rawSend = (frame: unknown) => {
    const text = JSON.stringify(frame);
    if (socket?.readyState === WebSocket.OPEN) socket.send(text);
    else queue.push(text);
  };

  const failAll = (message: string) => {
    for (const [, p] of pending) p.reject(new IpcException({ code: "network", message, detail: null }));
    pending.clear();
    streams.clear();
  };

  const scheduleReconnect = () => {
    if (closed || timer) return;
    setConnection("reconnecting");
    const delay = BACKOFF_MS[Math.min(attempt, BACKOFF_MS.length - 1)] ?? 10000;
    attempt += 1;
    timer = setTimeout(() => {
      timer = undefined;
      open();
    }, delay);
  };

  function open() {
    if (closed) return;
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(`${proto}//${location.host}/api/ws?csrf=${encodeURIComponent(opts.csrf)}`);
    socket = ws;
    ws.onopen = () => {
      const wasReconnect = attempt > 0;
      attempt = 0;
      setConnection("open");
      for (const event of listeners.keys()) ws.send(JSON.stringify({ sub: event }));
      for (const text of queue.splice(0)) ws.send(text);
      if (wasReconnect) resumeListeners.forEach((l) => l());
    };
    ws.onmessage = (m: MessageEvent<string>) => {
      let frame: Record<string, unknown>;
      try {
        frame = JSON.parse(m.data) as Record<string, unknown>;
      } catch {
        return;
      }
      if (typeof frame.push === "string") {
        if (frame.push === "share:session_ended") return; // the 4401 close follows
        listeners.get(frame.push)?.forEach((l) => l(frame.data));
        return;
      }
      const id = frame.id;
      if (typeof id !== "number") return;
      if ("event" in frame) {
        const handler = streams.get(id) ?? pending.get(id)?.onEvent;
        handler?.(frame.event);
        const type = (frame.event as { type?: string } | null)?.type;
        if (type === "done" || type === "error") streams.delete(id);
        return;
      }
      const p = pending.get(id);
      if (!p) return;
      pending.delete(id);
      if ("err" in frame) p.reject(toIpcException(frame.err));
      else p.resolve(frame.ok);
    };
    ws.onclose = (e) => {
      socket = null;
      failAll("The connection to NanoNexus was lost.");
      if (e.code === 4401) {
        closed = true;
        setConnection("ended");
        opts.onSessionEnded();
        return;
      }
      scheduleReconnect();
    };
    ws.onerror = () => ws.close();
  }

  // iOS Safari kills background sockets: come back, reconnect if needed, and let the UI refetch.
  const onVisible = () => {
    if (document.visibilityState !== "visible" || closed) return;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      if (timer) {
        clearTimeout(timer);
        timer = undefined;
      }
      attempt = 1;
      open();
    }
    resumeListeners.forEach((l) => l());
  };
  document.addEventListener("visibilitychange", onVisible);
  open();

  const request = <T,>(command: string, args: CommandArgs | undefined, onEvent?: (e: unknown) => void) =>
    new Promise<T>((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve: resolve as (v: unknown) => void, reject, onEvent });
      if (onEvent) streams.set(id, onEvent);
      rawSend({ id, cmd: command, args: args ?? {} });
    });

  return {
    get connection() {
      return connection;
    },
    call: <T,>(command: string, args?: CommandArgs) => request<T>(command, args),
    stream: <E, T = void>(command: string, args: CommandArgs, onEvent: (event: E) => void) =>
      request<T>(command, args, (e) => onEvent(e as E)),
    async on<T>(event: string, handler: (payload: T) => void): Promise<Unsubscribe> {
      const wrapped = (p: unknown) => handler(p as T);
      let set = listeners.get(event);
      if (!set) {
        set = new Set();
        listeners.set(event, set);
        rawSend({ sub: event });
      }
      set.add(wrapped);
      return () => {
        const s = listeners.get(event);
        if (!s) return;
        s.delete(wrapped);
        if (s.size === 0) {
          listeners.delete(event);
          rawSend({ unsub: event });
        }
      };
    },
    onConnection(cb) {
      connListeners.add(cb);
      return () => connListeners.delete(cb);
    },
    onResume(cb) {
      resumeListeners.add(cb);
      return () => resumeListeners.delete(cb);
    },
    close() {
      closed = true;
      document.removeEventListener("visibilitychange", onVisible);
      if (timer) clearTimeout(timer);
      socket?.close();
    },
  };
}
