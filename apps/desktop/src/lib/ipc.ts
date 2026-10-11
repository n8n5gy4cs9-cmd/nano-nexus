import type { ErrorCode, IpcError } from "@/bindings";

export type { ErrorCode, IpcError };

/** A failed command. `code` is stable and machine-readable; `detail` is for the "Details" section. */
export class IpcException extends Error {
  readonly code: ErrorCode;
  readonly detail: string | null;

  constructor(error: IpcError) {
    super(error.message);
    this.name = "IpcException";
    this.code = error.code;
    this.detail = error.detail;
  }
}

export function isIpcError(value: unknown): value is IpcError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string" &&
    "message" in value &&
    typeof value.message === "string"
  );
}

/** Normalize anything a transport can throw into an IpcException. */
export function toIpcException(error: unknown): IpcException {
  if (error instanceof IpcException) return error;
  if (isIpcError(error)) return new IpcException({ ...error, detail: error.detail ?? null });
  const message = error instanceof Error ? error.message : String(error);
  return new IpcException({ code: "internal", message, detail: null });
}
