/**
 * The typed facade feature code uses to talk to the backend. It sits on the active `Transport`, so
 * it works on desktop (Tauri) and, later, in the LAN Share web build. Never call `invoke` directly
 * (an ESLint rule enforces this). Types come from the generated `bindings.ts`.
 */
import type {
  CleanupPlan,
  ExportFile,
  ExportFormat,
  FolderReportView,
  ReportSummary,
  ScanProgress,
  ActivateResult,
  CatalogCard,
  DownloadView,
  InstalledModel,
  MemoryInfo,
  RepoFiles,
  SearchHit,
  VerifyResult,
  AppInfo,
  AssistantCard,
  AssistantDetail,
  AssistantSpec,
  AssistantSpec_Serialize,
  ValidationReport,
  AttachmentInput,
  ChatEvent,
  ChatSendArgs,
  ChatSendResult,
  ConversationSummary,
  ConversationUpdate,
  ConversationWithMessages,
  NotificationPayload,
  RuntimeState,
  Settings,
} from "@/bindings";

import type {
  AuditRecord,
  ChangePreview,
  ChangeSetSummary,
  Decision,
  DenyRuleView,
  Grant,
  GrantLevel,
  OsPermissionsStatus,
  PermissionRequest,
  UndoDecision,
  UndoOutcome,
} from "@/features/permissions/types";

import { getTransport } from "./transport";
import type { Unsubscribe } from "./transport";

/** Value accepted by `settings.set`. */
export type SettingValue = null | boolean | number | string | string[] | Record<string, boolean>;

export const api = {
  app: {
    info: () => getTransport().call<AppInfo>("app_info"),
    /** Open an http(s) link in the OS browser. */
    openExternal: (url: string) => getTransport().call<null>("open_external", { url }),
  },
  runtime: {
    status: () => getTransport().call<RuntimeState>("runtime_status"),
    restart: () => getTransport().call<null>("runtime_restart"),
    logs: (lines: number) => getTransport().call<string[]>("runtime_logs", { lines }),
    /** Subscribe to `runtime:status`. */
    onStatus: (handler: (state: RuntimeState) => void): Promise<Unsubscribe> =>
      getTransport().on<RuntimeState>("runtime:status", handler),
  },
  attachments: {
    /** Read dropped files (text only, <= 256 KB each). */
    read: (paths: string[]) => getTransport().call<AttachmentInput[]>("attachments_read", { paths }),
    /** Native file picker; empty when cancelled. */
    pick: () => getTransport().call<AttachmentInput[]>("attachments_pick"),
    /** OS file drag-and-drop over the window: hover state and the dropped paths. */
    onDrag: async (handlers: { enter: () => void; leave: () => void; drop: (paths: string[]) => void }): Promise<Unsubscribe> => {
      const t = getTransport();
      const offs = await Promise.all([
        t.on("tauri://drag-enter", handlers.enter),
        t.on("tauri://drag-leave", handlers.leave),
        t.on<{ paths: string[] }>("tauri://drag-drop", (p) => handlers.drop(p.paths)),
      ]);
      return () => offs.forEach((off) => off());
    },
  },
  conversations: {
    list: (query: string | null, limit: number, before: number | null = null) =>
      getTransport().call<ConversationSummary[]>("conversations_list", { query, limit, before }),
    get: (id: string) => getTransport().call<ConversationWithMessages>("conversation_get", { id }),
    update: (id: string, update: ConversationUpdate) => getTransport().call<null>("conversation_update", { id, update }),
    delete: (id: string) => getTransport().call<null>("conversation_delete", { id }),
    exportMd: (id: string) => getTransport().call<string>("conversation_export_md", { id }),
    /** Subscribe to `conversation:updated` (a title or summary changed). */
    onUpdated: (handler: (id: string) => void): Promise<Unsubscribe> =>
      getTransport().on<string>("conversation:updated", handler),
  },
  chat: {
    send: (args: ChatSendArgs, onEvent: (e: ChatEvent) => void) =>
      getTransport().stream<ChatEvent, ChatSendResult>("chat_send", { args }, onEvent),
    stop: (assistantMessageId: string) => getTransport().call<boolean>("chat_stop", { assistantMessageId }),
    regenerate: (assistantMessageId: string, think: boolean, onEvent: (e: ChatEvent) => void) =>
      getTransport().stream<ChatEvent, ChatSendResult>("chat_regenerate", { assistantMessageId, think }, onEvent),
    editResend: (userMessageId: string, text: string, think: boolean, onEvent: (e: ChatEvent) => void) =>
      getTransport().stream<ChatEvent, ChatSendResult>("chat_edit_resend", { userMessageId, text, think }, onEvent),
  },
  notifications: {
    list: (limit: number) => getTransport().call<NotificationPayload[]>("notifications_list", { limit }),
    unreadCount: () => getTransport().call<number>("notifications_unread_count"),
    /** Empty `ids` marks everything read. */
    markRead: (ids: string[]) => getTransport().call<null>("notifications_mark_read", { ids }),
    clear: () => getTransport().call<null>("notifications_clear"),
    sendTest: () => getTransport().call<NotificationPayload>("notifications_send_test"),
    /** Subscribe to `notification:new`. */
    onNew: (handler: (n: NotificationPayload) => void): Promise<Unsubscribe> =>
      getTransport().on<NotificationPayload>("notification:new", handler),
  },
  assistants: {
    list: () => getTransport().call<AssistantCard[]>("assistants_list"),
    get: (id: string) => getTransport().call<AssistantDetail>("assistant_get", { id }),
    validate: (toml: string) => getTransport().call<ValidationReport>("assistant_validate", { toml }),
    save: (toml: string, previousId: string | null = null) =>
      getTransport().call<AssistantDetail>("assistant_save", { toml, previousId }),
    delete: (id: string) => getTransport().call<null>("assistant_delete", { id }),
    duplicate: (id: string, name: string | null = null) => getTransport().call<AssistantDetail>("assistant_duplicate", { id, name }),
    setEnabled: (id: string, enabled: boolean) => getTransport().call<AssistantCard>("assistant_set_enabled", { id, enabled }),
    exportToml: (id: string) => getTransport().call<string>("assistant_export", { id }),
    importToml: (toml: string) => getTransport().call<AssistantDetail>("assistant_import", { toml }),
    draft: (toml: string) => getTransport().call<AssistantSpec_Serialize>("assistant_draft", { toml }),
    renderToml: (spec: AssistantSpec) => getTransport().call<string>("assistant_render_toml", { spec }),
  },
  analyzer: {
    /** Scan a folder; resolves with the saved report's id. `duplicates`: off | fast | full. */
    analyze: (jobId: string, root: string, duplicates: "off" | "fast" | "full", onProgress: (p: ScanProgress) => void) =>
      getTransport().stream<ScanProgress, string>("analyze_folder", { jobId, root, duplicates }, onProgress),
    cancel: (jobId: string) => getTransport().call<boolean>("analyze_cancel", { jobId }),
    reports: (kind: string | null = null, limit = 100) => getTransport().call<ReportSummary[]>("reports_list", { kind, limit }),
    folderReport: (id: string) => getTransport().call<FolderReportView>("report_folder", { id }),
    /** JSON body of a file / secrets / logs report. */
    body: (id: string) => getTransport().call<string>("report_body", { id }),
    remove: (id: string) => getTransport().call<boolean>("report_delete", { id }),
    saveNarrative: (id: string, text: string) => getTransport().call<null>("report_narrative_save", { id, text }),
    exportReport: (id: string, format: ExportFormat) => getTransport().call<ExportFile>("report_export", { id, format }),
    fileReport: (path: string) => getTransport().call<string>("file_report", { path }),
    cleanupPlan: (id: string, includeDuplicates: boolean, includeJunk: boolean) =>
      getTransport().call<CleanupPlan>("report_cleanup_plan", { id, includeDuplicates, includeJunk }),
  },
  models: {
    list: () => getTransport().call<InstalledModel[]>("models_list"),
    catalog: () => getTransport().call<CatalogCard[]>("models_catalog"),
    memory: () => getTransport().call<MemoryInfo>("models_memory"),
    search: (query: string) => getTransport().call<SearchHit[]>("models_search", { query }),
    repoFiles: (repo: string) => getTransport().call<RepoFiles>("models_repo_files", { repo }),
    downloads: () => getTransport().call<DownloadView[]>("models_downloads"),
    downloadCatalog: (catalogId: string) => getTransport().call<DownloadView>("models_download_catalog", { catalogId }),
    downloadHf: (repo: string, file: string) => getTransport().call<DownloadView>("models_download_hf", { repo, file }),
    pause: (id: string) => getTransport().call<null>("models_download_pause", { id }),
    resume: (id: string) => getTransport().call<null>("models_download_resume", { id }),
    cancel: (id: string) => getTransport().call<null>("models_download_cancel", { id }),
    activate: (id: string) => getTransport().call<ActivateResult>("models_activate", { id }),
    delete: (id: string) => getTransport().call<null>("models_delete", { id }),
    pickFile: () => getTransport().call<string | null>("models_pick_file"),
    import: (path: string, copy: boolean) => getTransport().call<InstalledModel>("models_import", { path, copy }),
    rescan: () => getTransport().call<number>("models_rescan"),
    verifyBundled: () => getTransport().call<VerifyResult[]>("models_verify_bundled"),
    /** Subscribe to `download:progress`. */
    onProgress: (handler: (d: DownloadView) => void): Promise<Unsubscribe> => getTransport().on<DownloadView>("download:progress", handler),
    /** Subscribe to `model:changed`. */
    onChanged: (handler: (id: string) => void): Promise<Unsubscribe> => getTransport().on<string>("model:changed", handler),
  },
  fs: {
    grants: () => getTransport().call<Grant[]>("fs_grants_list"),
    grantAdd: (root: string, level: GrantLevel, recursive: boolean, label: string | null = null, expiresAt: number | null = null) =>
      getTransport().call<Grant>("fs_grant_add", { root, level, recursive, label, expiresAt }),
    grantUpdate: (id: string, patch: { level?: GrantLevel; recursive?: boolean; label?: string; expiresAt?: number }) =>
      getTransport().call<Grant>("fs_grant_update", {
        id,
        level: patch.level ?? null,
        recursive: patch.recursive ?? null,
        label: patch.label ?? null,
        expiresAt: patch.expiresAt ?? null,
      }),
    grantRevoke: (id: string) => getTransport().call<boolean>("fs_grant_revoke", { id }),
    denyList: () => getTransport().call<DenyRuleView[]>("fs_deny_list"),
    denyAdd: (root: string) => getTransport().call<null>("fs_deny_add", { root }),
    denyRemove: (root: string) => getTransport().call<boolean>("fs_deny_remove", { root }),
    /** Native folder picker; `null` when cancelled. */
    pickFolder: () => getTransport().call<string | null>("fs_pick_folder"),
    /** Native picker for any one file (the Analyzer); `null` when cancelled. */
    pickAnyFile: () => getTransport().call<string | null>("fs_pick_any_file"),
    audit: (limit: number, before: number | null = null) => getTransport().call<AuditRecord[]>("audit_list", { limit, before }),
  },
  os: {
    permissions: () => getTransport().call<OsPermissionsStatus>("os_permissions_status"),
    openPrivacySettings: (pane: "files_and_folders" | "full_disk") => getTransport().call<null>("os_open_privacy_settings", { pane }),
  },
  permissions: {
    pending: () => getTransport().call<PermissionRequest[]>("permission_requests"),
    respond: (requestId: string, decision: Decision) => getTransport().call<boolean>("permission_respond", { requestId, decision }),
    /** Subscribe to `permission:request`. */
    onRequest: (handler: (r: PermissionRequest) => void): Promise<Unsubscribe> =>
      getTransport().on<PermissionRequest>("permission:request", handler),
  },
  changes: {
    list: (limit: number) => getTransport().call<ChangeSetSummary[]>("changesets_list", { limit }),
    preview: (id: string) => getTransport().call<ChangePreview>("changeset_preview", { id }),
    apply: (id: string) => getTransport().call<ChangeSetSummary>("changeset_apply", { id }),
    undo: (id: string, decisions: UndoDecision[]) => getTransport().call<UndoOutcome>("changeset_undo", { id, decisions }),
    discard: (id: string) => getTransport().call<null>("changeset_discard", { id }),
    /** Dev builds only. */
    makeTest: () => getTransport().call<string>("debug_make_test_changeset"),
    /** Subscribe to `changeset:update`. */
    onUpdate: (handler: (u: { id: string; status: string }) => void): Promise<Unsubscribe> =>
      getTransport().on<{ id: string; status: string }>("changeset:update", handler),
  },
  settings: {
    get: () => getTransport().call<Settings>("settings_get"),
    set: (key: string, value: SettingValue) => getTransport().call<Settings>("settings_set", { key, value }),
  },
};
