/** Args of a command, by parameter name. */
export type CommandArgs = Record<string, unknown>;

export type Unsubscribe = () => void;

/**
 * How the UI talks to the backend. The desktop build uses Tauri (`tauriTransport`); the LAN Share
 * web build will use a WebSocket (`wsTransport`, M14). Feature code never sees which one is active.
 */
export interface Transport {
  /** Call a command; rejects with an `IpcException`. */
  call<T>(command: string, args?: CommandArgs): Promise<T>;
  /** Call a streaming command; `onEvent` receives each streamed message in order. */
  stream<E, T = void>(command: string, args: CommandArgs, onEvent: (event: E) => void): Promise<T>;
  /** Subscribe to a global event; resolves to an unsubscribe function. */
  on<T>(event: string, handler: (payload: T) => void): Promise<Unsubscribe>;
}
