/**
 * Permission and change-set types, mirroring the Rust definitions (`nexus_core::fs`,
 * `nexus_core::changes`, `service::fs`, `service::permissions`). Replace with imports from the
 * generated `@/bindings` once a debug launch on the Mac has regenerated it with the `fs_*` commands.
 */
export type Access = "read" | "write" | "create" | "move" | "trash";
export type GrantLevel = "read" | "read_write" | "full";
export type GrantScope = "persistent" | "session";

export interface Grant {
  id: string;
  root: string;
  level: GrantLevel;
  recursive: boolean;
  scope: GrantScope;
  expires_at: number | null;
  label: string | null;
  created_at: number;
}

export interface AuditRecord {
  id: number;
  ts: number;
  actor: string;
  action: string;
  path: string;
  allowed: boolean;
  detail: string | null;
}

export interface DenyRuleView {
  root: string;
  label: string;
  builtin: boolean;
}

export interface OsFolderStatus {
  path: string;
  state: string;
}

export interface OsPermissionsStatus {
  platform: string;
  folders: OsFolderStatus[];
  full_disk_access: string | null;
}

export interface PermissionRequest {
  id: string;
  assistant: string;
  path: string;
  needed: Access;
  reason: string | null;
  requested_by: string | null;
}

export type Decision = { decision: "deny" } | { decision: "allow_once" } | { decision: "allow_folder"; level: GrantLevel };

export interface Totals {
  files: number;
  insertions: number;
  deletions: number;
  creates: number;
  moves: number;
  trashes: number;
  bytes: number;
}

export type ChangeStatus = "planned" | "applied" | "rolled_back" | "undone" | "discarded" | "failed";

export interface ChangeSetSummary {
  id: string;
  title: string;
  origin: string;
  status: ChangeStatus;
  created_at: number;
  applied_at: number | null;
  undone_at: number | null;
  totals: Totals | null;
  error: string | null;
}

export interface FilePreview {
  op_index: number;
  kind: string;
  path: string;
  to: string | null;
  needs: Access;
  size_before: number | null;
  size_after: number | null;
  insertions: number;
  deletions: number;
  diff: string | null;
  binary: boolean;
}

export type ConflictKind = "hash_mismatch" | "missing" | "exists" | "denied" | "invalid";

export interface Conflict {
  op_index: number;
  path: string;
  kind: ConflictKind;
  message: string;
}

export interface MissingGrant {
  op_index: number;
  path: string;
  needed: Access;
}

export interface ChangePreview {
  files: FilePreview[];
  totals: Totals;
  conflicts: Conflict[];
  missing_grants: MissingGrant[];
}

export type UndoChoice = "keep_current" | "restore_anyway";

export interface UndoDecision {
  op_index: number;
  choice: UndoChoice;
}

export interface UndoConflictView {
  op_index: number;
  path: string;
  message: string;
}

export interface UndoOutcome {
  restored: number;
  skipped: number;
  conflicts: UndoConflictView[];
  problems: string[];
  status: string;
}

export const LEVEL_LABEL: Record<GrantLevel, string> = {
  read: "Read only",
  read_write: "Read & edit",
  full: "Full (create, move, trash)",
};

export const ACCESS_VERB: Record<Access, string> = {
  read: "read",
  write: "edit files in",
  create: "create files in",
  move: "move files in",
  trash: "move files to the Trash in",
};
