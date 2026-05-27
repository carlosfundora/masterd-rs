/**
 * Real Tauri v2 bridge using @tauri-apps/api/core invoke.
 * Requires the live Tauri runtime; there is no mock fallback.
 */

import type {
  MasterdFrontendBridge,
  ApiResult,
  AppConfig,
  SystemStatus,
  SystemHealth,
  IntakeItem,
  WatchFolder,
  QueueQuery,
  Paginated,
  DocumentSearchQuery,
  DocumentRecord,
  DocumentPreview,
  ExtractedTextResult,
  ReprocessOptions,
  PipelineJob,
  JobQuery,
  ReviewQuery,
  ReviewItem,
  ReviewResolution,
  ActionResult,
  AutomationRule,
  AutomationRuleDraft,
  RuleTestResult,
  AuditQuery,
  AuditEntry,
  MasterdEvent,
} from "../contracts/api";

// ── Runtime detection ──────────────────────────────────────────────────────────
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// ── Tauri invoke helper ────────────────────────────────────────────────────────
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

// ── Tauri event helper ─────────────────────────────────────────────────────────
function listenTauri(callback: (event: MasterdEvent) => void): () => void {
  let unlisten: (() => void) | null = null;

  (async () => {
    const { listen } = await import("@tauri-apps/api/event");
    unlisten = await listen<MasterdEvent>("masterd://event", (e) => {
      callback(e.payload);
    });
  })();

  return () => {
    if (unlisten) unlisten();
  };
}

// ── Tauri real bridge ──────────────────────────────────────────────────────────
const tauriBridge: MasterdFrontendBridge = {
  system: {
    getStatus: () => invoke("system_get_status"),
    getHealth: () => invoke("system_get_health"),
  },
  intake: {
    addFiles: (paths, profileId) => invoke("intake_add_files", { paths, profileId }),
    addWatchFolder: (path, profileId) => invoke("intake_add_watch_folder", { path, profileId }),
    removeWatchFolder: (id) => invoke("intake_remove_watch_folder", { id }),
    listWatchFolders: () => invoke("intake_list_watch_folders"),
    listQueue: (params) => invoke("intake_list_queue", { params }),
    retryItem: (id) => invoke("intake_retry_item", { id }),
    cancelItem: (id) => invoke("intake_cancel_item", { id }),
  },
  documents: {
    search: (params) => invoke("documents_search", { params }),
    getById: (id) => invoke("documents_get_by_id", { id }),
    getPreview: (id) => invoke("documents_get_preview", { id }),
    getExtractedText: (id) => invoke("documents_get_extracted_text", { id }),
    updateTags: (id, tags) => invoke("documents_update_tags", { id, tags }),
    reprocess: (id, options) => invoke("documents_reprocess", { id, options }),
  },
  actions: {
    approveRename: (documentId, suggestedName) =>
      invoke("actions_approve_rename", { documentId, suggestedName }),
    rejectRename: (documentId, reason) =>
      invoke("actions_reject_rename", { documentId, reason }),
    approveMove: (documentId, destinationPath) =>
      invoke("actions_approve_move", { documentId, destinationPath }),
    markDuplicate: (documentId, duplicateOfId) =>
      invoke("actions_mark_duplicate", { documentId, duplicateOfId }),
    markUnique: (documentId) => invoke("actions_mark_unique", { documentId }),
  },
  pipeline: {
    listJobs: (params) => invoke("pipeline_list_jobs", { params }),
    getJob: (id) => invoke("pipeline_get_job", { id }),
    retryJob: (id) => invoke("pipeline_retry_job", { id }),
    cancelJob: (id) => invoke("pipeline_cancel_job", { id }),
  },
  review: {
    list: (params) => invoke("review_list", { params }),
    resolve: (id, resolution) => invoke("review_resolve", { id, resolution }),
  },
  rules: {
    list: () => invoke("rules_list"),
    getById: (id) => invoke("rules_get_by_id", { id }),
    create: (rule) => invoke("rules_create", { rule }),
    update: (id, patch) => invoke("rules_update", { id, patch }),
    delete: (id) => invoke("rules_delete", { id }),
    test: (rule, documentId) => invoke("rules_test", { rule, documentId }),
  },
  audit: {
    list: (params) => invoke("audit_list", { params }),
    getForDocument: (documentId) => invoke("audit_get_for_document", { documentId }),
    revert: (entryId) => invoke("audit_revert", { entryId }),
  },
  settings: {
    get: () => invoke("settings_get"),
    save: (config) => invoke("settings_save", { config }),
  },
  events: {
    subscribe: (callback) => listenTauri(callback),
  },
};

// ── Chat bridge (separate from main contract, streamed via Tauri events) ───────
export type ThinkMode = "Auto" | "Thinking" | "Instruct";
export type SearchMode = "LocalDocuments" | "WebSearch" | "Both";

export type ChatStreamToken =
  | { type: "think"; text: string }
  | { type: "response"; text: string }
  | { type: "done"; citations: Array<{ title: string; url: string }> }
  | { type: "error"; message: string };

export async function startChat(
  message: string,
  thinkMode: ThinkMode,
  searchMode: SearchMode,
  sessionId: string,
  onToken: (token: ChatStreamToken) => void
): Promise<() => void> {
  if (!isTauri()) {
    throw new Error("MASTERd chat requires the live Tauri desktop runtime.");
  }

  const { listen } = await import("@tauri-apps/api/event");
  const channelId = `chat:${sessionId}:${Date.now()}`;

  const unlisten = await listen<ChatStreamToken>(`masterd://chat/${channelId}`, (e) => {
    onToken(e.payload);
  });

  await invoke("chat_send_message", {
    message,
    thinkMode,
    searchMode,
    sessionId,
    channelId,
  });

  return () => unlisten();
}

// ── Exported bridge (live only) ────────────────────────────────────────────────
export async function getBridge(): Promise<MasterdFrontendBridge> {
  if (!isTauri()) {
    throw new Error("MASTERd requires the live Tauri desktop runtime.");
  }
  return tauriBridge;
}
