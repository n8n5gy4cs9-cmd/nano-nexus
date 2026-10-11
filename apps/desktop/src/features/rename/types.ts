/** Mirrors the Rust types of `commands/rename.rs` (switch to `bindings.ts` after it regenerates). */
export interface RenameProgress {
  /** `indexing | detecting | proposing | done` */
  phase: string;
  done: number;
  total: number;
}

export interface RenameSession {
  id: string;
  root: string;
  status: string;
  langs: string[];
  files: number;
  symbols: number;
  issues: number;
  created_at: number;
}

export type ItemStatus = "candidate" | "proposed" | "accepted" | "rejected" | "needs_manual" | "applied";

export interface RenameItem {
  symbol_id: string;
  old_name: string;
  kind: string;
  path: string;
  line: number;
  proposed: string | null;
  final_name: string | null;
  confidence: number | null;
  reason: string | null;
  issues: string[];
  flags: string[];
  status: ItemStatus;
  include_strings: boolean;
  usages: number;
  /** Why the last edit was refused (stored values unchanged). */
  error: string | null;
}

export interface RenamePlan {
  changeset_id: string;
  edits: number;
  skipped: string[];
  dirty: string[];
}

export interface RenamePlanOptions {
  docs: boolean;
  comments: boolean;
  ambiguous: boolean;
  public_api: boolean;
}

export interface Usage {
  path: string;
  line: number;
  kind: string;
  ambiguous: boolean;
  is_def: boolean;
  preview: string;
}

export interface Found {
  symbol: number;
  name: string;
  kind: string;
  path: string;
  line: number;
  signature: string;
  doc: string | null;
  matched: string;
  usages: { code: number; import: number; config: number; string: number; doc: number; comment: number };
}

export interface Checker {
  id: string;
  label: string;
  program: string;
  args: string[];
  dir: string;
  available: boolean;
}

export interface RenameVerifyResult {
  ok: boolean;
  exit_code: number | null;
  cancelled: boolean;
  timed_out: boolean;
  missing: boolean;
  first_errors: string[];
}

export const isFileKind = (kind: string): boolean => kind === "file" || kind === "folder";

/** The name that will be used: the user's edit, else the model's proposal. */
export const chosenName = (i: RenameItem): string | null => i.final_name ?? i.proposed;
