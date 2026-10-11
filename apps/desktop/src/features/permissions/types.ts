/** Permission and change-set types come from the generated bindings; this file adds UI labels. */
import type { Access, GrantLevel } from "@/bindings";

export type {
  Access,
  AuditRecord,
  ChangePreview,
  ChangeSetSummary,
  Conflict,
  ConflictKind,
  Decision,
  DenyRuleView,
  FilePreview,
  Grant,
  GrantLevel,
  GrantScope,
  MissingGrant,
  OsFolderStatus,
  OsPermissionsStatus,
  PermissionRequest,
  Totals,
  UndoChoice,
  UndoConflictView,
  UndoDecision,
  UndoOutcome,
} from "@/bindings";

export type ChangeStatus = "planned" | "applied" | "rolled_back" | "undone" | "discarded" | "failed";

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
