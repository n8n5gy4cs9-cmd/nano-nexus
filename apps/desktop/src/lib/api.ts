/**
 * The typed facade feature code uses to talk to the backend. It sits on the active `Transport`, so
 * it works on desktop (Tauri) and, later, in the LAN Share web build. Never call `invoke` directly
 * (an ESLint rule enforces this). Types come from the generated `bindings.ts`.
 */
import type {
  DirEntryInfo,
  FileStat,
  TextFile,
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
  ModelVerifyResult,
  PackExport,
  PackImport,
  PackProgress,
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
  ActivitySnapshot,
  ObserveView,
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

import type { EditorAiEvent, EditorAiRequest, EditorSaveResult } from "@/features/editor/types";
import type { PairingView, SharePolicy, ShareDevice, ShareStatus } from "@/features/share/types";
import type { KbIndexResult, KbLibrary, KbDocumentSource, KbLibraryDetail, KbPassage, KbProgress } from "@/features/library/types";

import { getTransport } from "./transport";
import type { ReportDelta, WatchSchedule, WatchView } from "@/features/analyzer/watches";
import type { JobSummary } from "@/features/jobs/types";
import type { Checker, Found, RenameItem, RenamePlan, RenamePlanOptions, RenameProgress, RenameSession, RenameVerifyResult, Usage } from "@/features/rename/types";
import type { Unsubscribe } from "./transport";

/** Value accepted by `settings.set`. */
export type SettingValue = null | boolean | number | string | string[] | Record<string, boolean> | Record<string, string[]>;

export const api = {
  app: {
    info: () => getTransport().call<AppInfo>("app_info"),
    /** Open an http(s) link in the OS browser. */
    /** Plain-text health report for the `/nano-nexus` chat command. */
    diagnostics: () => getTransport().call<string>("app_diagnostics"),
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
  rename: {
    /** Index a project; resolves with the new session. `jobId` is the handle for `cancel`. */
    createSession: (jobId: string, root: string, onProgress: (p: RenameProgress) => void) =>
      getTransport().stream<RenameProgress, RenameSession>("rename_session_create", { jobId, root }, onProgress),
    sessions: (limit = 50) => getTransport().call<RenameSession[]>("rename_sessions", { limit }),
    deleteSession: (sessionId: string) => getTransport().call<boolean>("rename_session_delete", { sessionId }),
    items: (sessionId: string, status: string | null = null) => getTransport().call<RenameItem[]>("rename_items", { sessionId, status }),
    /** Ask the model for names; resolves with how many proposals were made. */
    propose: (jobId: string, sessionId: string, symbolIds: string[] | null, tryAgain: boolean, onProgress: (p: RenameProgress) => void) =>
      getTransport().stream<RenameProgress, number>("rename_propose", { jobId, sessionId, symbolIds, tryAgain }, onProgress),
    updateItem: (sessionId: string, symbolId: string, patch: { finalName?: string | null; status?: string | null; includeStrings?: boolean | null }) =>
      getTransport().call<RenameItem>("rename_item_update", {
        sessionId,
        symbolId,
        finalName: patch.finalName ?? null,
        status: patch.status ?? null,
        includeStrings: patch.includeStrings ?? null,
      }),
    plan: (sessionId: string, options: RenamePlanOptions) => getTransport().call<RenamePlan>("rename_plan", { sessionId, options }),
    checkers: (sessionId: string) => getTransport().call<Checker[]>("rename_checkers", { sessionId }),
    verify: (jobId: string, sessionId: string, checkerId: string, onLine: (line: string) => void) =>
      getTransport().stream<string, RenameVerifyResult>("rename_verify", { jobId, sessionId, checkerId }, onLine),
    usages: (sessionId: string, symbolId: string) => getTransport().call<Usage[]>("rename_usages", { sessionId, symbolId }),
    find: (sessionId: string, query: string) => getTransport().call<Found[]>("rename_find", { sessionId, query }),
    cancel: (jobId: string) => getTransport().call<boolean>("rename_cancel", { jobId }),
  },
  watches: {
    list: () => getTransport().call<WatchView[]>("watches_list"),
    create: (root: string, schedule: WatchSchedule) => getTransport().call<WatchView>("watch_create", { root, schedule }),
    setEnabled: (id: string, enabled: boolean) => getTransport().call<null>("watch_set_enabled", { id, enabled }),
    setSchedule: (id: string, schedule: WatchSchedule) => getTransport().call<null>("watch_set_schedule", { id, schedule }),
    remove: (id: string) => getTransport().call<boolean>("watch_delete", { id }),
    /** Scan now; `null` for the first run (it saves the baseline). */
    runNow: (id: string) => getTransport().call<ReportDelta | null>("watch_run_now", { id }),
  },
  jobs: {
    list: (limit: number) => getTransport().call<JobSummary[]>("jobs_list", { limit }),
    cancel: (id: string) => getTransport().call<boolean>("job_cancel", { id }),
    /** Subscribe to `job:update`. */
    onUpdate: (handler: (job: JobSummary) => void): Promise<Unsubscribe> => getTransport().on<JobSummary>("job:update", handler),
  },
  observe: {
    snapshot: () => getTransport().call<ObserveView>("observe_snapshot"),
    /** The replay frame nearest before `atMs` (`null` when there is no history). */
    replay: (atMs: number) => getTransport().call<ActivitySnapshot | null>("observe_replay", { atMs }),
    demo: (on: boolean) => getTransport().call<ObserveView>("observe_demo", { on }),
    /** Tell the backend the page is open (live updates every 2 s) or closed. */
    watch: (on: boolean) => getTransport().call<null>("observe_watch", { on }),
    /** Subscribe to `observe:snapshot`. */
    onSnapshot: (handler: (v: ObserveView) => void): Promise<Unsubscribe> => getTransport().on<ObserveView>("observe:snapshot", handler),
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
    verifyBundled: () => getTransport().call<ModelVerifyResult[]>("models_verify_bundled"),
    pickFolder: () => getTransport().call<string | null>("models_pick_folder"),
    /** Copy installed models into a new model-pack folder inside `dest`; progress per chunk. */
    packExport: (jobId: string, ids: string[], dest: string, name: string | null, onProgress: (p: PackProgress) => void) =>
      getTransport().stream<PackProgress, PackExport>("models_pack_export", { jobId, ids, dest, name }, onProgress),
    /** Verify and install a model pack folder (offline). `copy = false` keeps the files where they are. */
    packImport: (jobId: string, path: string, copy: boolean, onProgress: (p: PackProgress) => void) =>
      getTransport().stream<PackProgress, PackImport>("models_pack_import", { jobId, path, copy }, onProgress),
    packCancel: (jobId: string) => getTransport().call<boolean>("models_pack_cancel", { jobId }),
    /** Subscribe to `models:list_changed` (models added in the background). */
    onListChanged: (handler: () => void): Promise<Unsubscribe> => getTransport().on<null>("models:list_changed", () => handler()),
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
    listDir: (path: string) => getTransport().call<DirEntryInfo[]>("fs_list_dir", { path }),
    readText: (path: string) => getTransport().call<TextFile>("fs_read_text", { path }),
    stat: (path: string) => getTransport().call<FileStat>("fs_stat", { path }),
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
  editor: {
    canWrite: (path: string) => getTransport().call<boolean>("editor_can_write", { path }),
    /** Saves through a change set; rejects with `conflict` if the file changed since `baseHash`. */
    save: (path: string, contents: string, baseHash: string | null) =>
      getTransport().call<EditorSaveResult>("editor_save", { path, contents, baseHash }),
    ai: (jobId: string, request: EditorAiRequest, onEvent: (e: EditorAiEvent) => void) =>
      getTransport().stream<EditorAiEvent>("editor_ai", { jobId, request }, onEvent),
    aiCancel: (jobId: string) => getTransport().call<boolean>("editor_ai_cancel", { jobId }),
  },
  knowledge: {
    list: () => getTransport().call<KbLibrary[]>("kb_list"),
    detail: (id: string) => getTransport().call<KbLibraryDetail>("kb_detail", { id }),
    /** Create a library from picked/dropped folders or files; index it next with `reindex`. */
    add: (name: string | null, paths: string[], reference: boolean) =>
      getTransport().call<KbLibrary>("kb_add", { name, paths, reference }),
    /** Index (incrementally); progress after every file. */
    reindex: (jobId: string, libraryId: string, onProgress: (p: KbProgress) => void) =>
      getTransport().stream<KbProgress, KbIndexResult>("kb_reindex", { jobId, libraryId }, onProgress),
    cancel: (jobId: string) => getTransport().call<boolean>("kb_cancel", { jobId }),
    setEnabled: (id: string, enabled: boolean) => getTransport().call<null>("kb_set_enabled", { id, enabled }),
    remove: (id: string) => getTransport().call<null>("kb_remove", { id }),
    search: (query: string, libraryIds: string[] | null = null, k = 6) =>
      getTransport().call<KbPassage[]>("kb_search", { query, libraryIds, k }),
    passage: (chunkId: number) => getTransport().call<KbPassage>("kb_passage", { chunkId }),
    /** Original text of the passage's document (html/md/txt...), for the rendered and raw views. */
    documentSource: (chunkId: number) => getTransport().call<KbDocumentSource>("kb_document_source", { chunkId }),
  },
  share: {
    status: () => getTransport().call<ShareStatus>("share_status"),
    start: () => getTransport().call<ShareStatus>("share_start"),
    stop: () => getTransport().call<ShareStatus>("share_stop"),
    setEnabled: (enabled: boolean) => getTransport().call<ShareStatus>("share_set_enabled", { enabled }),
    setSetting: (key: string, value: SettingValue) => getTransport().call<ShareStatus>("share_settings_set", { key, value }),
    pairingCode: () => getTransport().call<PairingView>("share_pairing_code"),
    devices: () => getTransport().call<ShareDevice[]>("share_devices_list"),
    updateDevice: (id: string, name: string | null, policy: SharePolicy | null) =>
      getTransport().call<null>("share_device_update", { id, name, policy }),
    revokeDevice: (id: string) => getTransport().call<boolean>("share_device_revoke", { id }),
    /** Subscribe to `share:status`. */
    onStatus: (handler: (s: ShareStatus) => void): Promise<Unsubscribe> =>
      getTransport().on<ShareStatus>("share:status", handler),
  },
  settings: {
    get: () => getTransport().call<Settings>("settings_get"),
    set: (key: string, value: SettingValue) => getTransport().call<Settings>("settings_set", { key, value }),
  },
};
