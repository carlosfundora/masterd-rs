// Type contracts for MASTERd

export type ApiResult<T> =
  | {
      ok: true;
      data: T;
      requestId: string;
      receivedAt: string;
    }
  | {
      ok: false;
      error: ApiError;
      requestId: string;
      receivedAt: string;
    };

export type ApiError = {
  code: string;
  message: string;
  details?: Record<string, unknown>;
  recoverable: boolean;
};

export type SystemStatus = {
  engine: "offline" | "starting" | "online" | "degraded" | "error";
  database: "offline" | "connected" | "degraded" | "error";
  watcher: "offline" | "active" | "paused" | "error";
  models: ModelStatus[];
  queues: {
    pending: number;
    processing: number;
    review: number;
    completeToday: number;
    errors: number;
  };
  storage: {
    indexedFiles: number;
    totalBytes: number;
    savedBytes?: number;
  };
};

export type SystemHealth = {
  cpuUsage: number;
  memoryUsage: number;
  diskFreeBytes: number;
  dbLatencyMs: number;
  activeThreads: number;
};

export type ModelStatus = {
  id: string;
  name: string;
  role: "embedding" | "classification" | "ocr" | "summarization" | "reranking";
  status: "offline" | "loading" | "online" | "error";
  loadedAt?: string;
};

export type IntakeItem = {
  id: string;
  fileName: string;
  path: string;
  extension: string;
  sizeBytes: number;
  status: "queued" | "hashing" | "extracting" | "classifying" | "review" | "complete" | "error";
  progress: number;
  duplicateStatus?: "unknown" | "unique" | "exact_duplicate" | "near_duplicate" | "possible_duplicate";
  createdAt: string;
  updatedAt: string;
};

export type WatchFolder = {
  id: string;
  path: string;
  enabled: boolean;
  profileId: string;
  fileCount: number;
  createdAt: string;
};

export type QueueQuery = {
  status?: string;
  limit?: number;
  offset?: number;
};

export type Paginated<T> = {
  items: T[];
  total: number;
  limit: number;
  offset: number;
};

export type DocumentSearchQuery = {
  text?: string;
  category?: string;
  tag?: string;
  status?: ProcessingStatus;
  limit?: number;
  offset?: number;
};

export type DocumentPreview = {
  documentId: string;
  textPreview: string;
  pageCount: number;
  thumbnailUrl?: string;
  mimeType: string;
};

export type ExtractedTextResult = {
  documentId: string;
  fullText: string;
  language?: string;
  entities: Array<{
    text: string;
    label: string; // PERSON, ORG, DATE, AMOUNT, etc.
    confidence: number;
  }>;
};

export type ReprocessOptions = {
  ocr?: boolean;
  classify?: boolean;
  tags?: boolean;
  rename?: boolean;
};

export type ActionResult = {
  success: boolean;
  message: string;
  documentId: string;
  details?: Record<string, unknown>;
};

export type JobQuery = {
  status?: string;
  limit?: number;
  offset?: number;
};

export type ReviewQuery = {
  severity?: "info" | "warning" | "critical";
  resolved?: boolean;
  limit?: number;
  offset?: number;
};

export type ReviewResolution = {
  approved: boolean;
  editedFields?: Record<string, unknown>;
  notes?: string;
};

export type AutomationRuleDraft = Omit<AutomationRule, "id" | "createdAt" | "updatedAt">;

export type RuleTestResult = {
  matched: boolean;
  actionsEvaluated: Array<{
    type: string;
    applied: boolean;
    resultSummary: string;
  }>;
};

export type AuditQuery = {
  action?: string;
  actor?: string;
  documentId?: string;
  limit?: number;
  offset?: number;
};

export type ClassificationResult = {
  category: string;
  subcategory?: string;
  confidence: number;
  explanation?: string;
  model?: string;
  candidates?: Array<{
    category: string;
    confidence: number;
  }>;
};

export type DuplicateStatus =
  | "unknown"
  | "unique"
  | "exact_duplicate"
  | "near_duplicate"
  | "possible_duplicate";

export type ProcessingStatus =
  | "new"
  | "queued"
  | "processing"
  | "needs_review"
  | "complete"
  | "warning"
  | "error";

export type DocumentRecord = {
  id: string;
  originalName: string;
  currentName: string;
  suggestedName?: string;
  originalPath: string;
  currentPath: string;
  extension: string;
  mimeType: string;
  sizeBytes: number;
  hash: string;
  classification?: ClassificationResult;
  tags: string[];
  extractedText?: string;
  summary?: string;
  confidence: number;
  duplicateStatus: DuplicateStatus;
  processingStatus: ProcessingStatus;
  createdAt: string;
  updatedAt: string;
};

export type PipelineStage =
  | "ingest"
  | "normalize"
  | "hash"
  | "dedupe"
  | "extract_text"
  | "ocr"
  | "classify"
  | "extract_entities"
  | "suggest_tags"
  | "suggest_rename"
  | "route_storage"
  | "write_audit"
  | "complete";

export type PipelineLogEntry = {
  id: string;
  level: "debug" | "info" | "warning" | "error";
  message: string;
  createdAt: string;
  details?: Record<string, unknown>;
};

export type PipelineJob = {
  id: string;
  documentId: string;
  fileName: string;
  stage: PipelineStage;
  status: "queued" | "running" | "complete" | "warning" | "error";
  progress: number;
  startedAt?: string;
  finishedAt?: string;
  errorMessage?: string;
  workerId?: string;
  logs: PipelineLogEntry[];
};

export type ReviewItem = {
  id: string;
  documentId: string;
  reason:
    | "low_confidence_classification"
    | "low_confidence_rename"
    | "possible_duplicate"
    | "destructive_action"
    | "storage_conflict"
    | "extraction_warning";
  severity: "info" | "warning" | "critical";
  title: string;
  explanation: string;
  proposedAction?: ProposedAction;
  createdAt: string;
  resolved?: boolean;
};

export type ProposedAction = {
  type:
    | "rename"
    | "move"
    | "delete"
    | "tag"
    | "classify"
    | "merge_duplicate"
    | "mark_unique";
  before?: Record<string, unknown>;
  after?: Record<string, unknown>;
  confidence?: number;
};

export type AutomationRule = {
  id: string;
  name: string;
  description?: string;
  enabled: boolean;
  priority: number;
  trigger: RuleTrigger;
  conditions: RuleCondition[];
  actions: RuleAction[];
  safetyLevel: "safe" | "review_required" | "destructive";
  createdAt: string;
  updatedAt: string;
};

export type RuleTrigger = {
  event:
    | "file_imported"
    | "hash_complete"
    | "classification_complete"
    | "duplicate_detected"
    | "extraction_complete"
    | "manual";
};

export type RuleCondition = {
  field: string;
  operator:
    | "equals"
    | "not_equals"
    | "contains"
    | "starts_with"
    | "ends_with"
    | "greater_than"
    | "less_than"
    | "exists";
  value?: unknown;
};

export type RuleAction = {
  type:
    | "suggest_tag"
    | "suggest_rename"
    | "route_storage"
    | "require_review"
    | "mark_duplicate"
    | "set_classification";
  [key: string]: unknown;
};

export type AuditEntry = {
  id: string;
  documentId?: string;
  action:
    | "imported"
    | "hashed"
    | "classified"
    | "tagged"
    | "renamed"
    | "moved"
    | "duplicate_detected"
    | "review_created"
    | "review_resolved"
    | "error"
    | "reverted";
  actor: "system" | "user" | "rule";
  summary: string;
  before?: Record<string, unknown>;
  after?: Record<string, unknown>;
  reversible: boolean;
  createdAt: string;
};

export type MasterdEvent =
  | {
      type: "system.status.changed";
      payload: SystemStatus;
      timestamp: string;
    }
  | {
      type: "intake.item.updated";
      payload: IntakeItem;
      timestamp: string;
    }
  | {
      type: "document.updated";
      payload: DocumentRecord;
      timestamp: string;
    }
  | {
      type: "pipeline.job.updated";
      payload: PipelineJob;
      timestamp: string;
    }
  | {
      type: "review.item.created";
      payload: ReviewItem;
      timestamp: string;
    }
  | {
      type: "audit.entry.created";
      payload: AuditEntry;
      timestamp: string;
    }
  | {
      type: "error.created";
      payload: ApiError;
      timestamp: string;
    };

export type AppConfig = {
  archivePath?: string;
  ocrLanguage: string;
  safetyConfidencePct: number;
  chatModel: string;
  searxngUrl: string;
  bm25TopK: number;
  ragTopK: number;
  generationTemp: number;
  generationMaxTokens: number;
  embeddingBackend: string;
  colbertUrl: string;
  jinaUrl: string;
  qwen3Url: string;
  intakeMaxDepth: number;
  intakeExtensions: string[];
  /** Ollama daemon URL — fallback when embedded engine is unavailable. */
  ollamaUrl: string;
  /** Ollama model name (e.g. "llama3.2", "mistral"). */
  ollamaModel: string;
};

export interface MasterdFrontendBridge {
  system: {
    getStatus(): Promise<ApiResult<SystemStatus>>;
    getHealth(): Promise<ApiResult<SystemHealth>>;
  };

  intake: {
    addFiles(paths: string[], profileId?: string): Promise<ApiResult<IntakeItem[]>>;
    addWatchFolder(path: string, profileId?: string): Promise<ApiResult<WatchFolder>>;
    removeWatchFolder(id: string): Promise<ApiResult<void>>;
    listWatchFolders(): Promise<ApiResult<WatchFolder[]>>;
    listQueue(params: QueueQuery): Promise<ApiResult<Paginated<IntakeItem>>>;
    retryItem(id: string): Promise<ApiResult<IntakeItem>>;
    cancelItem(id: string): Promise<ApiResult<IntakeItem>>;
  };

  documents: {
    search(params: DocumentSearchQuery): Promise<ApiResult<Paginated<DocumentRecord>>>;
    getById(id: string): Promise<ApiResult<DocumentRecord>>;
    getPreview(id: string): Promise<ApiResult<DocumentPreview>>;
    getExtractedText(id: string): Promise<ApiResult<ExtractedTextResult>>;
    updateTags(id: string, tags: string[]): Promise<ApiResult<DocumentRecord>>;
    reprocess(id: string, options?: ReprocessOptions): Promise<ApiResult<PipelineJob>>;
  };

  actions: {
    approveRename(documentId: string, suggestedName?: string): Promise<ApiResult<ActionResult>>;
    rejectRename(documentId: string, reason?: string): Promise<ApiResult<ActionResult>>;
    approveMove(documentId: string, destinationPath: string): Promise<ApiResult<ActionResult>>;
    markDuplicate(documentId: string, duplicateOfId: string): Promise<ApiResult<ActionResult>>;
    markUnique(documentId: string): Promise<ApiResult<ActionResult>>;
  };

  pipeline: {
    listJobs(params: JobQuery): Promise<ApiResult<Paginated<PipelineJob>>>;
    getJob(id: string): Promise<ApiResult<PipelineJob>>;
    retryJob(id: string): Promise<ApiResult<PipelineJob>>;
    cancelJob(id: string): Promise<ApiResult<PipelineJob>>;
  };

  review: {
    list(params: ReviewQuery): Promise<ApiResult<Paginated<ReviewItem>>>;
    resolve(id: string, resolution: ReviewResolution): Promise<ApiResult<ReviewItem>>;
  };

  rules: {
    list(): Promise<ApiResult<AutomationRule[]>>;
    getById(id: string): Promise<ApiResult<AutomationRule>>;
    create(rule: AutomationRuleDraft): Promise<ApiResult<AutomationRule>>;
    update(id: string, patch: Partial<AutomationRuleDraft>): Promise<ApiResult<AutomationRule>>;
    delete(id: string): Promise<ApiResult<void>>;
    test(rule: AutomationRuleDraft, documentId?: string): Promise<ApiResult<RuleTestResult>>;
  };

  audit: {
    list(params: AuditQuery): Promise<ApiResult<Paginated<AuditEntry>>>;
    getForDocument(documentId: string): Promise<ApiResult<AuditEntry[]>>;
    revert(entryId: string): Promise<ApiResult<ActionResult>>;
  };

  settings: {
    get(): Promise<ApiResult<AppConfig>>;
    save(config: AppConfig): Promise<ApiResult<void>>;
  };

  events: {
    subscribe(callback: (event: MasterdEvent) => void): () => void;
  };
}
