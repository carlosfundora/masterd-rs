import {
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
  ActionResult,
  JobQuery,
  ReviewItem,
  ReviewQuery,
  ReviewResolution,
  AutomationRule,
  AutomationRuleDraft,
  RuleTestResult,
  AuditEntry,
  AuditQuery,
  MasterdEvent,
  ModelStatus
} from "../contracts/api";

// Helper to create safe, predictable IDs
const makeId = () => Math.random().toString(36).substring(2, 11).toUpperCase();

// Helper to get formatted timestamps
const getTimestamp = (offsetMinutes = 0) => {
  const d = new Date(Date.now() + offsetMinutes * 60 * 1000);
  return d.toISOString();
};

// Initial dense state
class StatefulMockBackend {
  status: SystemStatus = {
    engine: "online",
    database: "connected",
    watcher: "active",
    models: [
      { id: "m-embed", name: "Gemini-Text-Embed-v2", role: "embedding", status: "online", loadedAt: getTimestamp(-360) },
      { id: "m-classify", name: "MASTERd-Classifier-v3.5", role: "classification", status: "online", loadedAt: getTimestamp(-350) },
      { id: "m-ocr", name: "Tesseract-OCR-Local-Engine", role: "ocr", status: "online", loadedAt: getTimestamp(-340) },
      { id: "m-summary", name: "Gemini-3.5-Flash-Local", role: "summarization", status: "online", loadedAt: getTimestamp(-330) },
      { id: "m-rank", name: "BGE-Reranker-Large", role: "reranking", status: "offline" }
    ],
    queues: {
      pending: 4,
      processing: 2,
      review: 3,
      completeToday: 24,
      errors: 1
    },
    storage: {
      indexedFiles: 142,
      totalBytes: 812400000, // ~812 MB
      savedBytes: 124000000  // ~124 MB from deduplication
    }
  };

  health: SystemHealth = {
    cpuUsage: 14.2,
    memoryUsage: 45.8,
    diskFreeBytes: 148200000000, // ~148 GB
    dbLatencyMs: 4,
    activeThreads: 4
  };

  watchFolders: WatchFolder[] = [
    { id: "wf-1", path: "/Users/username/Desktop/Tax2025", enabled: true, profileId: "Receipts & Financial Docs", fileCount: 28, createdAt: getTimestamp(-12000) },
    { id: "wf-2", path: "/Users/username/Downloads/Invoices", enabled: true, profileId: "Fast Scan", fileCount: 114, createdAt: getTimestamp(-10000) },
    { id: "wf-3", path: "/Users/username/Documents/ScannedCorrespondence", enabled: false, profileId: "Full Analysis", fileCount: 8, createdAt: getTimestamp(-8000) }
  ];

  config: AppConfig = {
    ocrLanguage: "eng",
    safetyConfidencePct: 85,
    chatModel: "auto",
    searxngUrl: "http://127.0.0.1:9265",
    bm25TopK: 8,
    ragTopK: 8,
    generationTemp: 0.7,
    generationMaxTokens: 1024,
    embeddingBackend: "http",
    colbertUrl: "http://127.0.0.1:11450",
    jinaUrl: "http://127.0.0.1:11447",
    qwen3Url: "http://127.0.0.1:11502",
    intakeMaxDepth: 3,
    intakeExtensions: ["txt", "md", "rst", "log", "pdf"],
    ollamaUrl: "http://127.0.0.1:11434",
    ollamaModel: "llama3.2",
  };

  intakeQueue: IntakeItem[] = [
    { id: "int-1", fileName: "Scan_2026_05_26_1120.pdf", path: "/Users/username/Desktop/Tax2025/Scan_2026_05_26_1120.pdf", extension: "pdf", sizeBytes: 2450000, status: "classifying", progress: 65, duplicateStatus: "unique", createdAt: getTimestamp(-3), updatedAt: getTimestamp() },
    { id: "int-2", fileName: "chase_credit_statement_apr.pdf", path: "/Users/username/Downloads/Invoices/chase_credit_statement_apr.pdf", extension: "pdf", sizeBytes: 1540000, status: "extracting", progress: 20, duplicateStatus: "unknown", createdAt: getTimestamp(-6), updatedAt: getTimestamp(-1) },
    { id: "int-3", fileName: "photograph_invoice_copy.jpeg", path: "/Users/username/Downloads/Invoices/photograph_invoice_copy.jpeg", extension: "jpeg", sizeBytes: 4200000, status: "queued", progress: 0, duplicateStatus: "unknown", createdAt: getTimestamp(-12), updatedAt: getTimestamp(-12) },
    { id: "int-4", fileName: "internal_notes_draft_v1.txt", path: "/Users/username/Documents/ScannedCorrespondence/internal_notes_draft_v1.txt", extension: "txt", sizeBytes: 12000, status: "review", progress: 100, duplicateStatus: "possible_duplicate", createdAt: getTimestamp(-15), updatedAt: getTimestamp(-5) },
    { id: "int-5", fileName: "corrupted_archive_payload.zip", path: "/Users/username/Downloads/Invoices/corrupted_archive_payload.zip", extension: "zip", sizeBytes: 25000000, status: "error", progress: 12, duplicateStatus: "unknown", createdAt: getTimestamp(-30), updatedAt: getTimestamp(-25) }
  ];

  documents: DocumentRecord[] = [
    {
      id: "doc-101",
      originalName: "invoice_2405_acme.pdf",
      currentName: "invoice_2405_acme.pdf",
      suggestedName: "2026-05-24_invoice_acme_consulting.pdf",
      originalPath: "/Users/username/Downloads/Invoices/invoice_2405_acme.pdf",
      currentPath: "/Users/username/Downloads/Invoices/invoice_2405_acme.pdf",
      extension: "pdf",
      mimeType: "application/pdf",
      sizeBytes: 120400,
      hash: "7FAA982BB4102EEA998341392A",
      classification: {
        category: "Financial / Invoice",
        subcategory: "Consulting",
        confidence: 0.94,
        explanation: "Contains invoice headers, line items for technical consulting, tax identifier format, and billing totals.",
        model: "MASTERd-Classifier-v3.5",
        candidates: [
          { category: "Financial / Invoice", confidence: 0.94 },
          { category: "Financial / Receipt", confidence: 0.05 },
          { category: "Legal / Correspondence", confidence: 0.01 }
        ]
      },
      tags: ["finance", "acme", "consulting", "2026-q2"],
      extractedText: "ACME CORPORATION\nInvoice #INV-2026-904\nDate: May 24, 2026\nTo: SentSeven\nLine items: Senior software engineering consulting services.\nTotal Amount: $14,500.00\nPayment terms: net 30.",
      summary: "Invoice from ACME Corp for $14,500.00 for engineering consulting services.",
      confidence: 0.94,
      duplicateStatus: "unique",
      processingStatus: "needs_review",
      createdAt: getTimestamp(-120),
      updatedAt: getTimestamp(-110)
    },
    {
      id: "doc-102",
      originalName: "attention_is_all_you_need.pdf",
      currentName: "2017-06-12_research_waswani_attention_is-all-you-need.pdf",
      suggestedName: "2017-06-12_research_waswani_attention_is-all-you-need.pdf",
      originalPath: "/Users/username/Downloads/attention_is_all_you_need.pdf",
      currentPath: "/Users/username/Documents/Archive/Research/2017-06-12_research_waswani_attention_is-all-you-need.pdf",
      extension: "pdf",
      mimeType: "application/pdf",
      sizeBytes: 2410500,
      hash: "8BCC93112AA83E822EA88C31D2",
      classification: {
        category: "Academic / Research Paper",
        subcategory: "Machine Learning",
        confidence: 0.99,
        explanation: "Dense mathematical terminology, citation format, authors Waswani et al., and standard abstract describing neural network architectures.",
        model: "MASTERd-Classifier-v3.5",
        candidates: [
          { category: "Academic / Research Paper", confidence: 0.99 }
        ]
      },
      tags: ["deep-learning", "transformers", "research", "archive"],
      extractedText: "Attention Is All You Need\nAshish Vaswani, Noam Shazeer, Niki Parmar, Jakob Uszkoreit, Llion Jones, Aidan N. Gomez, Łukasz Kaiser, Illia Polosukhin\nAbstract: The dominant sequence transduction models are based on complex recurrent or convolutional neural networks...",
      summary: "The seminal paper introducing the Transformer neural network architecture based solely on self-attention mechanisms.",
      confidence: 0.99,
      duplicateStatus: "unique",
      processingStatus: "complete",
      createdAt: getTimestamp(-1440),
      updatedAt: getTimestamp(-1400)
    },
    {
      id: "doc-103",
      originalName: "Notice_Chase_Checking_Slashed.pdf",
      currentName: "Notice_Chase_Checking_Slashed.pdf",
      suggestedName: "2026-05-18_letter_chase_account-closure-warning.pdf",
      originalPath: "/Users/username/Downloads/Invoices/Notice_Chase_Checking_Slashed.pdf",
      currentPath: "/Users/username/Downloads/Invoices/Notice_Chase_Checking_Slashed.pdf",
      extension: "pdf",
      mimeType: "application/pdf",
      sizeBytes: 852000,
      hash: "11EFA99283AA1CCCDD221468",
      classification: {
        category: "Legal / Correspondence",
        subcategory: "Account Management",
        confidence: 0.48,
        explanation: "Low confidence because document contains checking account terminology alongside legal notice language.",
        model: "MASTERd-Classifier-v3.5",
        candidates: [
          { category: "Legal / Correspondence", confidence: 0.48 },
          { category: "Financial / Statement", confidence: 0.42 },
          { category: "Personal / Tax", confidence: 0.10 }
        ]
      },
      tags: ["chase", "checking", "unread", "attention"],
      extractedText: "CHASE BANK USA\nNotice of Account Review and Potential Limitations.\nDate: May 18, 2026\nDear Customer, we are writing to alert you regarding sudden transaction locks on checking route ending 9821. Action is required immediately.",
      summary: "Account warning letter from Chase Bank urging immediate checkup on checking account ending 9821 to prevent limits.",
      confidence: 0.48,
      duplicateStatus: "possible_duplicate",
      processingStatus: "needs_review",
      createdAt: getTimestamp(-60),
      updatedAt: getTimestamp(-58)
    },
    {
      id: "doc-104",
      originalName: "IMG_2391_grocery_receipt.JPG",
      currentName: "IMG_2391_grocery_receipt.JPG",
      suggestedName: "2026-05-25_receipt_wholefoods_grocery.jpg",
      originalPath: "/Users/username/Desktop/Tax2025/IMG_2391_grocery_receipt.JPG",
      currentPath: "/Users/username/Desktop/Tax2025/IMG_2391_grocery_receipt.JPG",
      extension: "jpg",
      mimeType: "image/jpeg",
      sizeBytes: 1540300,
      hash: "82FF0926B4EACD832D028B41AF03",
      classification: {
        category: "Financial / Receipt",
        subcategory: "Food & Beverage",
        confidence: 0.92,
        explanation: "OCR detected retailer WHOLE FOODS MARKET, subtotal, tax rate, and payment method of transaction.",
        model: "MASTERd-Classifier-v3.5",
        candidates: [
          { category: "Financial / Receipt", confidence: 0.92 },
          { category: "Financial / Invoice", confidence: 0.08 }
        ]
      },
      tags: ["groceries", "personal", "wholedfoods", "tax-deductible-potential"],
      extractedText: "WHOLE FOODS MARKET\nSTORE #10291 - BOSTON, MA\nORGANIC MILK   $5.99\nATLANTIC SALMON  $18.50\nSPINACH BG      $3.49\nSUBTOTAL: $27.98\nTAX: $0.00\nTOTAL: $27.98\nCARD: **** **** **** 4012",
      summary: "Grocery receipt from Whole Foods Market costing $27.98.",
      confidence: 0.92,
      duplicateStatus: "unique",
      processingStatus: "needs_review",
      createdAt: getTimestamp(-10),
      updatedAt: getTimestamp(-9)
    }
  ];

  jobs: PipelineJob[] = [
    {
      id: "job-001",
      documentId: "doc-104",
      fileName: "IMG_2391_grocery_receipt.JPG",
      stage: "ocr",
      status: "complete",
      progress: 100,
      startedAt: getTimestamp(-10),
      finishedAt: getTimestamp(-9),
      workerId: "local-worker-1",
      logs: [
        { id: "l-1", level: "info", message: "Starting local OCR engine process on image", createdAt: getTimestamp(-10) },
        { id: "l-2", level: "debug", message: "Contrast enhancement complete", createdAt: getTimestamp(-9.8) },
        { id: "l-3", level: "info", message: "Extracted 28 tokens of text. Language confidence EN: 98%", createdAt: getTimestamp(-9.2) }
      ]
    },
    {
      id: "job-002",
      documentId: "doc-101",
      fileName: "invoice_2405_acme.pdf",
      stage: "suggest_rename",
      status: "complete",
      progress: 100,
      startedAt: getTimestamp(-120),
      finishedAt: getTimestamp(-118),
      workerId: "local-worker-2",
      logs: [
        { id: "lh-1", level: "info", message: "Analyzing invoice meta data", createdAt: getTimestamp(-120) },
        { id: "lh-2", level: "info", message: "Generating rename pattern with rule ID (ru-01)", createdAt: getTimestamp(-119) },
        { id: "lh-3", level: "info", message: "Suggested: 2026-05-24_invoice_acme_consulting.pdf", createdAt: getTimestamp(-118) }
      ]
    },
    {
      id: "job-003",
      documentId: "awaiting-doc-id",
      fileName: "Scan_2026_05_26_1120.pdf",
      stage: "dedupe",
      status: "running",
      progress: 42,
      startedAt: getTimestamp(-2),
      workerId: "local-worker-1",
      logs: [
        { id: "li-1", level: "info", message: "Reading stream buffer and calculating SHA-256", createdAt: getTimestamp(-2) },
        { id: "li-2", level: "debug", message: "Computed checksum: BAA310931CCED83901AC8D203", createdAt: getTimestamp(-1.5) },
        { id: "li-3", level: "info", message: "Searching indexed documents for duplicate check", createdAt: getTimestamp(-1) }
      ]
    }
  ];

  reviewQueue: ReviewItem[] = [
    {
      id: "rev-201",
      documentId: "doc-103",
      reason: "low_confidence_classification",
      severity: "warning",
      title: "Review low-confidence classification",
      explanation: "Document has 48% confidence of category 'Legal / Correspondence'. Highlighted keywords show balance of checking terms and warning notice terms.",
      proposedAction: {
        type: "classify",
        before: { category: "Uncategorized" },
        after: { category: "Legal / Correspondence" },
        confidence: 0.48
      },
      createdAt: getTimestamp(-58),
      resolved: false
    },
    {
      id: "rev-202",
      documentId: "doc-101",
      reason: "low_confidence_rename",
      severity: "info",
      title: "Review suggested document name",
      explanation: "Rule applied: '{date}_invoice_{sender}_{short_subject}.pdf'. Supplier matched 'acme' but was transcribed in lower case, causing medium confidence rating.",
      proposedAction: {
        type: "rename",
        before: { currentName: "invoice_2405_acme.pdf" },
        after: { suggestedName: "2026-05-24_invoice_acme_consulting.pdf" },
        confidence: 0.72
      },
      createdAt: getTimestamp(-110),
      resolved: false
    },
    {
      id: "rev-203",
      documentId: "doc-103",
      reason: "possible_duplicate",
      severity: "critical",
      title: "Review overlapping checksum / near duplicate",
      explanation: "The content matches 'Chase Checking Alert v2.pdf' by 91.2% in text similarity vector logs.",
      proposedAction: {
        type: "merge_duplicate",
        before: { isDuplicate: false },
        after: { duplicateOf: "doc-101" },
        confidence: 0.91
      },
      createdAt: getTimestamp(-40),
      resolved: false
    }
  ];

  rules: AutomationRule[] = [
    {
      id: "ru-01",
      name: "Legal correspondence naming",
      description: "Automatically formats name for correspondence of legal category",
      enabled: true,
      priority: 10,
      trigger: { event: "classification_complete" },
      conditions: [
        { field: "classification.category", operator: "equals", value: "Legal / Correspondence" }
      ],
      actions: [
        { type: "suggest_rename", template: "{date}_letter_{sender}_{short_subject}.{ext}" },
        { type: "require_review", threshold: 0.90 }
      ],
      safetyLevel: "review_required",
      createdAt: getTimestamp(-12000),
      updatedAt: getTimestamp(-11000)
    },
    {
      id: "ru-02",
      name: "Financial Invoices & Receipts tagger",
      description: "Autotags document with tags invoice or receipt",
      enabled: true,
      priority: 5,
      trigger: { event: "classification_complete" },
      conditions: [
        { field: "classification.category", operator: "contains", value: "Financial" }
      ],
      actions: [
        { type: "suggest_tag", tag: "finance" }
      ],
      safetyLevel: "safe",
      createdAt: getTimestamp(-10000),
      updatedAt: getTimestamp(-9500)
    },
    {
      id: "ru-03",
      name: "Immediate routing of tax receipts",
      description: "Direct routing for tax documents flagged with high priority status",
      enabled: false,
      priority: 1,
      trigger: { event: "extraction_complete" },
      conditions: [
        { field: "tags", operator: "contains", value: "tax" }
      ],
      actions: [
        { type: "route_storage", destinationFolder: "/Users/username/Documents/TaxArchive" }
      ],
      safetyLevel: "destructive", // moves files
      createdAt: getTimestamp(-8000),
      updatedAt: getTimestamp(-7900)
    }
  ];

  auditLog: AuditEntry[] = [
    { id: "aud-01", documentId: "doc-102", action: "imported", actor: "system", summary: "Loaded file attention_is_all_you_need.pdf through active watcher on folder [Invoices]", reversible: false, createdAt: getTimestamp(-1440) },
    { id: "aud-02", documentId: "doc-102", action: "classified", actor: "system", summary: "Classified as Academic / Research Paper with 99% confidence using classifier model", reversible: true, before: { category: "Uncategorized" }, after: { category: "Academic / Research Paper" }, createdAt: getTimestamp(-1400) },
    { id: "aud-03", documentId: "doc-101", action: "imported", actor: "system", summary: "Imported invoice_2405_acme.pdf from manual dropzone selection", reversible: false, createdAt: getTimestamp(-120) },
    { id: "aud-04", documentId: "doc-101", action: "classified", actor: "system", summary: "Classified invoice_2405_acme.pdf as Financial / Invoice (94% confidence)", reversible: true, createdAt: getTimestamp(-118) },
    { id: "aud-05", documentId: "doc-101", action: "review_created", actor: "system", summary: "Triggered review item rev-202 (low confidence suggested rename score)", reversible: false, createdAt: getTimestamp(-110) },
    { id: "aud-06", documentId: "doc-103", action: "imported", actor: "system", summary: "Loaded file Notice_Chase_Checking_Slashed.pdf from Downloads", reversible: false, createdAt: getTimestamp(-60) },
    { id: "aud-07", documentId: "doc-103", action: "classified", actor: "system", summary: "Classified Notice_Chase_Checking_Slashed.pdf as Legal / Correspondence (48% confidence)", reversible: true, createdAt: getTimestamp(-58) }
  ];

  subscribers: Array<(e: MasterdEvent) => void> = [];

  subscribe(callback: (e: MasterdEvent) => void) {
    this.subscribers.push(callback);
    return () => {
      this.subscribers = this.subscribers.filter(cb => cb !== callback);
    };
  }

  emit(e: MasterdEvent) {
    this.subscribers.forEach(cb => {
      try {
        cb(e);
      } catch (err) {
        console.error("Error in subscriber callback", err);
      }
    });
  }

  // Local updates for testing and interaction
  updateQueueCounts() {
    const fresh = this.intakeQueue.filter(i => i.status === "queued").length;
    const proc = this.intakeQueue.filter(i => i.status === "hashing" || i.status === "extracting" || i.status === "classifying").length;
    const rev = this.reviewQueue.filter(r => !r.resolved).length;
    const errs = this.intakeQueue.filter(i => i.status === "error").length;

    this.status.queues = {
      pending: fresh,
      processing: proc,
      review: rev,
      completeToday: this.documents.filter(d => d.processingStatus === "complete").length + 20, // offset
      errors: errs
    };

    this.emit({
      type: "system.status.changed",
      payload: { ...this.status },
      timestamp: getTimestamp()
    });
  }
}

const db = new StatefulMockBackend();

// This object implements MasterdFrontendBridge completely
export const mockBridge: MasterdFrontendBridge = {
  system: {
    async getStatus(): Promise<ApiResult<SystemStatus>> {
      return {
        ok: true,
        data: db.status,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },
    async getHealth(): Promise<ApiResult<SystemHealth>> {
      // Simulate slight variation in CPU
      db.health.cpuUsage = +(10 + Math.random() * 15).toFixed(1);
      db.health.memoryUsage = +(42 + Math.random() * 5).toFixed(1);
      return {
        ok: true,
        data: db.health,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    }
  },

  intake: {
    async addFiles(paths: string[], profileId?: string): Promise<ApiResult<IntakeItem[]>> {
      const addedItems: IntakeItem[] = paths.map(path => {
        const fileParts = path.split("/");
        const fileName = fileParts[fileParts.length - 1];
        const ext = fileName.split(".").pop() || "";
        const id = "int-" + makeId();

        const item: IntakeItem = {
          id,
          fileName,
          path,
          extension: ext,
          sizeBytes: Math.floor(200000 + Math.random() * 5000000),
          status: "queued",
          progress: 0,
          duplicateStatus: "unknown",
          createdAt: getTimestamp(),
          updatedAt: getTimestamp()
        };

        db.intakeQueue.unshift(item);

        // Add corresponding audit
        const audit: AuditEntry = {
          id: "aud-" + makeId(),
          action: "imported",
          actor: "user",
          summary: `Queued ${fileName} in file dropzone (Profile: ${profileId || "Full Analysis"})`,
          reversible: false,
          createdAt: getTimestamp()
        };
        db.auditLog.unshift(audit);
        db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });

        // Simulate progressive pipeline state in the background for high interaction feeling!
        setTimeout(() => {
          item.status = "hashing";
          item.progress = 10;
          db.emit({ type: "intake.item.updated", payload: { ...item }, timestamp: getTimestamp() });
          db.updateQueueCounts();

          // Create a pipeline job too
          const jp: PipelineJob = {
            id: "job-" + makeId(),
            documentId: "loading-id",
            fileName: item.fileName,
            stage: "hash",
            status: "running",
            progress: 25,
            startedAt: getTimestamp(),
            workerId: "local-worker-1",
            logs: [{ id: "l-" + makeId(), level: "info", message: `Hashing stream data for ${item.fileName}`, createdAt: getTimestamp() }]
          };
          db.jobs.unshift(jp);
          db.emit({ type: "pipeline.job.updated", payload: jp, timestamp: getTimestamp() });

          setTimeout(() => {
            item.status = "extracting";
            item.progress = 50;
            db.emit({ type: "intake.item.updated", payload: { ...item }, timestamp: getTimestamp() });
            jp.stage = "extract_text";
            jp.progress = 50;
            jp.logs.push({ id: "l-" + makeId(), level: "info", message: `Extracting plain text metadata from document layout`, createdAt: getTimestamp() });
            db.emit({ type: "pipeline.job.updated", payload: jp, timestamp: getTimestamp() });

            setTimeout(() => {
              // Now move it into the documents database!
              const docId = "doc-" + makeId();
              item.status = "complete";
              item.progress = 100;
              db.emit({ type: "intake.item.updated", payload: { ...item }, timestamp: getTimestamp() });

              // Build classification candidates dynamically if legal or finance
              const isReceipt = fileName.toLowerCase().includes("receipt") || fileName.toLowerCase().includes("img") || ext === "png";
              const cat = isReceipt ? "Financial / Receipt" : "Legal / Correspondence";
              
              const newDoc: DocumentRecord = {
                id: docId,
                originalName: item.fileName,
                currentName: item.fileName,
                suggestedName: `2026-05-26_${isReceipt ? 'receipt' : 'letter'}_${fileName.toLowerCase().replace("." + ext, "")}.${ext}`,
                originalPath: item.path,
                currentPath: item.path,
                extension: item.extension,
                mimeType: ext === "pdf" ? "application/pdf" : "image/jpeg",
                sizeBytes: item.sizeBytes,
                hash: "5DE2D" + makeId() + "2FF",
                classification: {
                  category: cat,
                  confidence: 0.88,
                  explanation: "Categorized based on document syntax patterns and metadata triggers.",
                  model: "MASTERd-Classifier-v3.5",
                  candidates: [
                    { category: cat, confidence: 0.88 },
                    { category: "Academic / Research Paper", confidence: 0.12 }
                  ]
                },
                tags: [isReceipt ? "tax" : "management", ext, "incoming"],
                extractedText: `AUTOMATIC EXTRACTION TEXT SAMPLE FOR: ${fileName}\nDate of Processing: 2026-05-26\nLine item values found: Total $124.50. Transaction authorized successfully.`,
                summary: `An automated parsing result for ${fileName}.`,
                confidence: 0.88,
                duplicateStatus: "unique",
                processingStatus: "needs_review",
                createdAt: getTimestamp(),
                updatedAt: getTimestamp()
              };

              db.documents.unshift(newDoc);
              db.emit({ type: "document.updated", payload: newDoc, timestamp: getTimestamp() });

              // Complete job
              jp.status = "complete";
              jp.progress = 100;
              jp.finishedAt = getTimestamp();
              jp.logs.push({ id: "l-" + makeId(), level: "info", message: `Document analysis completed successfully. Created record ${docId}`, createdAt: getTimestamp() });
              db.emit({ type: "pipeline.job.updated", payload: jp, timestamp: getTimestamp() });

              // Create review item
              const rId = "rev-" + makeId();
              const rev: ReviewItem = {
                id: rId,
                documentId: docId,
                reason: "low_confidence_rename",
                severity: "info",
                title: "Review suggested rename for auto-imported file",
                explanation: `Verify formatting mapping generated for manual input ${fileName}.`,
                proposedAction: {
                  type: "rename",
                  before: { currentName: fileName },
                  after: { suggestedName: newDoc.suggestedName },
                  confidence: 0.88
                },
                createdAt: getTimestamp(),
                resolved: false
              };
              db.reviewQueue.unshift(rev);
              db.emit({ type: "review.item.created", payload: rev, timestamp: getTimestamp() });

              db.updateQueueCounts();
            }, 3000);
          }, 2000);
        }, 1500);

        return item;
      });

      db.updateQueueCounts();

      return {
        ok: true,
        data: addedItems,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async addWatchFolder(path: string, profileId?: string): Promise<ApiResult<WatchFolder>> {
      const folder: WatchFolder = {
        id: "wf-" + makeId(),
        path,
        enabled: true,
        profileId: profileId || "Fast Scan",
        fileCount: 0,
        createdAt: getTimestamp()
      };
      
      db.watchFolders.push(folder);

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        action: "imported",
        actor: "user",
        summary: `Registered new active watch folder: ${path} (Watcher state: active)`,
        reversible: true,
        createdAt: getTimestamp()
      };
      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });

      return {
        ok: true,
        data: folder,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async removeWatchFolder(id: string): Promise<ApiResult<void>> {
      const folder = db.watchFolders.find(f => f.id === id);
      db.watchFolders = db.watchFolders.filter(f => f.id !== id);

      if (folder) {
        const audit: AuditEntry = {
          id: "aud-" + makeId(),
          action: "reverted",
          actor: "user",
          summary: `De-registered and disabled watch folder: ${folder.path}`,
          reversible: false,
          createdAt: getTimestamp()
        };
        db.auditLog.unshift(audit);
        db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      }

      return { ok: true, data: undefined, requestId: makeId(), receivedAt: getTimestamp() };
    },

    async listWatchFolders(): Promise<ApiResult<WatchFolder[]>> {
      return { ok: true, data: [...db.watchFolders], requestId: makeId(), receivedAt: getTimestamp() };
    },

    async listQueue(params: QueueQuery): Promise<ApiResult<Paginated<IntakeItem>>> {
      let items = [...db.intakeQueue];
      if (params.status) {
        items = items.filter(i => i.status === params.status);
      }
      return {
        ok: true,
        data: {
          items,
          total: items.length,
          limit: params.limit || 10,
          offset: params.offset || 0
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async retryItem(id: string): Promise<ApiResult<IntakeItem>> {
      const item = db.intakeQueue.find(i => i.id === id);
      if (!item) {
        return {
          ok: false,
          error: { code: "NOT_FOUND", message: `Intake item ${id} not found`, recoverable: true },
          requestId: makeId(),
          receivedAt: getTimestamp()
        };
      }

      item.status = "queued";
      item.progress = 0;
      db.updateQueueCounts();

      return {
        ok: true,
        data: item,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async cancelItem(id: string): Promise<ApiResult<IntakeItem>> {
      const item = db.intakeQueue.find(i => i.id === id);
      if (!item) {
        return {
          ok: false,
          error: { code: "NOT_FOUND", message: `Intake $id not found`, recoverable: true },
          requestId: makeId(),
          receivedAt: getTimestamp()
        };
      }

      db.intakeQueue = db.intakeQueue.filter(i => i.id !== id);
      db.updateQueueCounts();

      return {
        ok: true,
        data: item,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    }
  },

  documents: {
    async search(params: DocumentSearchQuery): Promise<ApiResult<Paginated<DocumentRecord>>> {
      let items = [...db.documents];

      if (params.text) {
        const query = params.text.toLowerCase();
        items = items.filter(
          doc =>
            doc.originalName.toLowerCase().includes(query) ||
            doc.currentName.toLowerCase().includes(query) ||
            doc.extractedText?.toLowerCase().includes(query) ||
            doc.tags.some(t => t.toLowerCase().includes(query))
        );
      }

      if (params.category) {
        items = items.filter(doc => doc.classification?.category === params.category);
      }

      if (params.tag) {
        items = items.filter(doc => doc.tags.includes(params.tag!));
      }

      if (params.status) {
        items = items.filter(doc => doc.processingStatus === params.status);
      }

      return {
        ok: true,
        data: {
          items,
          total: items.length,
          limit: params.limit || 20,
          offset: params.offset || 0
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async getById(id: string): Promise<ApiResult<DocumentRecord>> {
      const doc = db.documents.find(d => d.id === id);
      if (!doc) {
        return {
          ok: false,
          error: { code: "NOT_FOUND", message: `Document ${id} not found`, recoverable: false },
          requestId: makeId(),
          receivedAt: getTimestamp()
        };
      }
      return {
        ok: true,
        data: doc,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async getPreview(id: string): Promise<ApiResult<DocumentPreview>> {
      const doc = db.documents.find(d => d.id === id);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }
      return {
        ok: true,
        data: {
          documentId: doc.id,
          textPreview: doc.extractedText?.slice(0, 300) || "Empty preview.",
          pageCount: 1,
          mimeType: doc.mimeType
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async getExtractedText(id: string): Promise<ApiResult<ExtractedTextResult>> {
      const doc = db.documents.find(d => d.id === id);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }
      
      return {
        ok: true,
        data: {
          documentId: doc.id,
          fullText: doc.extractedText || "No text extracted.",
          language: "en",
          entities: [
            { text: doc.originalName.toLowerCase().includes("chase") ? "CHASE BANK" : "ACME CORP", label: "ORGANIZATION", confidence: 0.98 },
            { text: "May 2026", label: "DATE", confidence: 0.95 }
          ]
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async updateTags(id: string, tags: string[]): Promise<ApiResult<DocumentRecord>> {
      const doc = db.documents.find(d => d.id === id);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      const beforeTags = [...doc.tags];
      doc.tags = tags;
      doc.updatedAt = getTimestamp();

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        documentId: id,
        action: "tagged",
        actor: "user",
        summary: `Tags updated for document ${doc.currentName}`,
        before: { tags: beforeTags },
        after: { tags: tags },
        reversible: true,
        createdAt: getTimestamp()
      };
      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });

      return {
        ok: true,
        data: doc,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async reprocess(id: string, options?: ReprocessOptions): Promise<ApiResult<PipelineJob>> {
      const doc = db.documents.find(d => d.id === id);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      doc.processingStatus = "processing";
      
      const job: PipelineJob = {
        id: "job-" + makeId(),
        documentId: id,
        fileName: doc.currentName,
        stage: "ocr",
        status: "running",
        progress: 10,
        startedAt: getTimestamp(),
        workerId: "local-worker-2",
        logs: [{ id: "l-" + makeId(), level: "info", message: `Manual reprocess requested (Options: ${JSON.stringify(options || {})})`, createdAt: getTimestamp() }]
      };

      db.jobs.unshift(job);
      db.emit({ type: "pipeline.job.updated", payload: job, timestamp: getTimestamp() });

      // Run background timeout
      setTimeout(() => {
        job.progress = 60;
        job.stage = "classify";
        job.logs.push({ id: "l-" + makeId(), level: "info", message: `Executing classification pipeline re-evaluation`, createdAt: getTimestamp() });
        db.emit({ type: "pipeline.job.updated", payload: job, timestamp: getTimestamp() });

        setTimeout(() => {
          job.status = "complete";
          job.progress = 100;
          job.finishedAt = getTimestamp();
          job.logs.push({ id: "l-" + makeId(), level: "info", message: `Processing steps finished complete. Updated indexes.`, createdAt: getTimestamp() });
          
          doc.processingStatus = "complete";
          doc.updatedAt = getTimestamp();

          const audit: AuditEntry = {
            id: "aud-" + makeId(),
            documentId: doc.id,
            action: "classified",
            actor: "system",
            summary: `Automated re-indexing completed for ${doc.currentName}`,
            reversible: false,
            createdAt: getTimestamp()
          };
          db.auditLog.unshift(audit);

          db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
          db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });
          db.updateQueueCounts();
        }, 1500);
      }, 1500);

      db.updateQueueCounts();

      return {
        ok: true,
        data: job,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    }
  },

  actions: {
    async approveRename(documentId: string, suggestedName?: string): Promise<ApiResult<ActionResult>> {
      const doc = db.documents.find(d => d.id === documentId);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      const nameToApply = suggestedName || doc.suggestedName || doc.currentName;
      const beforeName = doc.currentName;
      doc.currentName = nameToApply;
      doc.processingStatus = "complete";
      doc.updatedAt = getTimestamp();

      // Resolve review
      const rev = db.reviewQueue.find(r => r.documentId === documentId && r.reason === "low_confidence_rename");
      if (rev) rev.resolved = true;

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        documentId,
        action: "renamed",
        actor: "user",
        summary: `Approved file rename from ${beforeName} to ${nameToApply}`,
        before: { currentName: beforeName },
        after: { currentName: nameToApply },
        reversible: true,
        createdAt: getTimestamp()
      };
      
      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });
      
      db.updateQueueCounts();

      return {
        ok: true,
        data: { success: true, message: "File renamed successfully in indices", documentId },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async rejectRename(documentId: string, reason?: string): Promise<ApiResult<ActionResult>> {
      const doc = db.documents.find(d => d.id === documentId);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      doc.suggestedName = undefined;
      doc.processingStatus = "complete";
      
      // Resolve review
      const rev = db.reviewQueue.find(r => r.documentId === documentId && r.reason === "low_confidence_rename");
      if (rev) rev.resolved = true;

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        documentId,
        action: "review_resolved",
        actor: "user",
        summary: `Rejected name rewrite suggestion for ${doc.currentName}. Kept original name. (Reason: ${reason || "User rejected suggestion"})`,
        reversible: false,
        createdAt: getTimestamp()
      };

      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });

      db.updateQueueCounts();

      return {
        ok: true,
        data: { success: true, message: "Rename suggestion rejected, original name preserved.", documentId },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async approveMove(documentId: string, destinationPath: string): Promise<ApiResult<ActionResult>> {
      const doc = db.documents.find(d => d.id === documentId);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      const before = doc.currentPath;
      doc.currentPath = destinationPath;
      doc.updatedAt = getTimestamp();

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        documentId,
        action: "moved",
        actor: "user",
        summary: `Moved index pointer from ${before} to ${destinationPath}`,
        before: { currentPath: before },
        after: { currentPath: destinationPath },
        reversible: true,
        createdAt: getTimestamp()
      };

      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });

      return {
        ok: true,
        data: { success: true, message: "Moved document successfully", documentId },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async markDuplicate(documentId: string, duplicateOfId: string): Promise<ApiResult<ActionResult>> {
      const doc = db.documents.find(d => d.id === documentId);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      doc.duplicateStatus = "exact_duplicate";
      doc.processingStatus = "complete";
      
      const rev = db.reviewQueue.find(r => r.documentId === documentId && r.reason === "possible_duplicate");
      if (rev) rev.resolved = true;

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        documentId,
        action: "duplicate_detected",
        actor: "user",
        summary: `Approved duplicate marking. Document linked to index record ${duplicateOfId}`,
        reversible: true,
        createdAt: getTimestamp()
      };

      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });

      db.updateQueueCounts();

      return {
        ok: true,
        data: { success: true, message: "Marked duplicate successfully", documentId },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async markUnique(documentId: string): Promise<ApiResult<ActionResult>> {
      const doc = db.documents.find(d => d.id === documentId);
      if (!doc) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      doc.duplicateStatus = "unique";
      doc.processingStatus = "complete";

      const rev = db.reviewQueue.find(r => r.documentId === documentId && r.reason === "possible_duplicate");
      if (rev) rev.resolved = true;

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        documentId,
        action: "review_resolved",
        actor: "user",
        summary: `Marked document similarity suspect as uniquely true: ${doc.currentName}`,
        reversible: true,
        createdAt: getTimestamp()
      };

      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });

      db.updateQueueCounts();

      return {
        ok: true,
        data: { success: true, message: "Resolved file as unique successfully", documentId },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    }
  },

  pipeline: {
    async listJobs(params: JobQuery): Promise<ApiResult<Paginated<PipelineJob>>> {
      let items = [...db.jobs];
      if (params.status) {
        items = items.filter(j => j.status === params.status);
      }
      return {
        ok: true,
        data: {
          items,
          total: items.length,
          limit: params.limit || 10,
          offset: params.offset || 0
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async getJob(id: string): Promise<ApiResult<PipelineJob>> {
      const job = db.jobs.find(j => j.id === id);
      if (!job) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }
      return { ok: true, data: job, requestId: makeId(), receivedAt: getTimestamp() };
    },

    async retryJob(id: string): Promise<ApiResult<PipelineJob>> {
      const job = db.jobs.find(j => j.id === id);
      if (!job) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      job.status = "queued";
      job.progress = 0;
      job.logs.push({ id: "l-" + makeId(), level: "info", message: "Manually retrying worker pipeline execution", createdAt: getTimestamp() });
      db.emit({ type: "pipeline.job.updated", payload: { ...job }, timestamp: getTimestamp() });
      db.updateQueueCounts();

      return {
        ok: true,
        data: job,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async cancelJob(id: string): Promise<ApiResult<PipelineJob>> {
      const job = db.jobs.find(j => j.id === id);
      if (!job) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      job.status = "error";
      job.errorMessage = "Cancelled by manual system supervisor request.";
      job.logs.push({ id: "l-" + makeId(), level: "warning", message: "Job process cancellation requested.", createdAt: getTimestamp() });
      db.emit({ type: "pipeline.job.updated", payload: { ...job }, timestamp: getTimestamp() });
      db.updateQueueCounts();

      return {
        ok: true,
        data: job,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    }
  },

  review: {
    async list(params: ReviewQuery): Promise<ApiResult<Paginated<ReviewItem>>> {
      let items = [...db.reviewQueue];
      
      // Filter by resolved parameter - by default show unresolved unless requested
      if (params.resolved !== undefined) {
        items = items.filter(r => r.resolved === params.resolved);
      } else {
        items = items.filter(r => !r.resolved);
      }

      if (params.severity) {
        items = items.filter(r => r.severity === params.severity);
      }

      return {
        ok: true,
        data: {
          items,
          total: items.length,
          limit: params.limit || 10,
          offset: params.offset || 0
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async resolve(id: string, resolution: ReviewResolution): Promise<ApiResult<ReviewItem>> {
      const item = db.reviewQueue.find(r => r.id === id);
      if (!item) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      item.resolved = true;
      
      // Apply mock doc classification updates if that is what was verified
      if (item.proposedAction && resolution.approved) {
        const doc = db.documents.find(d => d.id === item.documentId);
        if (doc) {
          if (item.proposedAction.type === "classify" && doc.classification) {
            doc.classification.category = (resolution.editedFields?.category as string) || doc.classification.category;
            doc.processingStatus = "complete";
          } else if (item.proposedAction.type === "rename") {
            const finalName = (resolution.editedFields?.suggestedName as string) || doc.suggestedName || doc.currentName;
            doc.currentName = finalName;
            doc.processingStatus = "complete";
          } else if (item.proposedAction.type === "merge_duplicate") {
            doc.duplicateStatus = "exact_duplicate";
            doc.processingStatus = "complete";
          }
          doc.updatedAt = getTimestamp();
          db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });
        }
      } else {
        // Disapprove or customized fields
        const doc = db.documents.find(d => d.id === item.documentId);
        if (doc) {
          if (item.proposedAction?.type === "classify" && resolution.editedFields?.category) {
            if (!doc.classification) {
              doc.classification = { category: "Uncategorized", confidence: 1.0 };
            }
            doc.classification.category = resolution.editedFields.category as string;
            doc.processingStatus = "complete";
            doc.updatedAt = getTimestamp();
            db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });
          }
        }
      }

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        documentId: item.documentId,
        action: "review_resolved",
        actor: "user",
        summary: `Resolved review docket item: ${item.title} (Approved: ${resolution.approved}) ${resolution.notes ? '- ' + resolution.notes : ''}`,
        reversible: false,
        createdAt: getTimestamp()
      };
      
      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      db.updateQueueCounts();

      return {
        ok: true,
        data: item,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    }
  },

  rules: {
    async list(): Promise<ApiResult<AutomationRule[]>> {
      return {
        ok: true,
        data: db.rules,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async getById(id: string): Promise<ApiResult<AutomationRule>> {
      const r = db.rules.find(rule => rule.id === id);
      if (!r) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Rule not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }
      return { ok: true, data: r, requestId: makeId(), receivedAt: getTimestamp() };
    },

    async create(rule: AutomationRuleDraft): Promise<ApiResult<AutomationRule>> {
      const newRule: AutomationRule = {
        ...rule,
        id: "ru-" + makeId(),
        createdAt: getTimestamp(),
        updatedAt: getTimestamp()
      };

      db.rules.push(newRule);

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        action: "review_resolved",
        actor: "user",
        summary: `Created active automation trigger rule: ${rule.name}`,
        reversible: true,
        createdAt: getTimestamp()
      };
      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });

      return {
        ok: true,
        data: newRule,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async update(id: string, patch: Partial<AutomationRuleDraft>): Promise<ApiResult<AutomationRule>> {
      const r = db.rules.find(rule => rule.id === id);
      if (!r) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Rule not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      Object.assign(r, patch);
      r.updatedAt = getTimestamp();

      const audit: AuditEntry = {
        id: "aud-" + makeId(),
        action: "review_resolved",
        actor: "user",
        summary: `Modified rule variables for trigger identifier: ${r.name}`,
        reversible: true,
        createdAt: getTimestamp()
      };
      db.auditLog.unshift(audit);
      db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });

      return {
        ok: true,
        data: r,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async delete(id: string): Promise<ApiResult<void>> {
      const r = db.rules.find(rule => rule.id === id);
      db.rules = db.rules.filter(rule => rule.id !== id);

      if (r) {
        const audit: AuditEntry = {
          id: "aud-" + makeId(),
          action: "review_resolved",
          actor: "user",
          summary: `Archived and deleted trigger rule: ${r.name}`,
          reversible: false,
          createdAt: getTimestamp()
        };
        db.auditLog.unshift(audit);
        db.emit({ type: "audit.entry.created", payload: audit, timestamp: getTimestamp() });
      }

      return {
        ok: true,
        data: undefined,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async test(rule: AutomationRuleDraft, documentId?: string): Promise<ApiResult<RuleTestResult>> {
      // Dry-run testing evaluation
      let matched = false;
      if (documentId) {
        const doc = db.documents.find(d => d.id === documentId);
        if (doc) {
          matched = true; // matching simulated
        }
      } else {
        matched = true;
      }

      return {
        ok: true,
        data: {
          matched,
          actionsEvaluated: rule.actions.map(act => ({
            type: act.type,
            applied: matched,
            resultSummary: matched ? `Simulated rule pass for document: action criteria resolved successfully.` : 'No match triggers found'
          }))
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    }
  },

  audit: {
    async list(params: AuditQuery): Promise<ApiResult<Paginated<AuditEntry>>> {
      let items = [...db.auditLog];

      if (params.documentId) {
        items = items.filter(a => a.documentId === params.documentId);
      }
      if (params.action) {
        items = items.filter(a => a.action === params.action);
      }
      if (params.actor) {
        items = items.filter(a => a.actor === params.actor);
      }

      return {
        ok: true,
        data: {
          items,
          total: items.length,
          limit: params.limit || 20,
          offset: params.offset || 0
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async getForDocument(documentId: string): Promise<ApiResult<AuditEntry[]>> {
      const items = db.auditLog.filter(a => a.documentId === documentId);
      return {
        ok: true,
        data: items,
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    },

    async revert(entryId: string): Promise<ApiResult<ActionResult>> {
      const entry = db.auditLog.find(a => a.id === entryId);
      if (!entry) {
        return { ok: false, error: { code: "NOT_FOUND", message: "Not found", recoverable: true }, requestId: makeId(), receivedAt: getTimestamp() };
      }

      if (!entry.reversible) {
        return {
          ok: false,
          error: { code: "NOT_REVERSIBLE", message: "This operation action cannot be automatically reverted. Please apply changes manually.", recoverable: false },
          requestId: makeId(),
          receivedAt: getTimestamp()
        };
      }

      // Mark the document change backwards if appropriate metadata is parsed
      if (entry.documentId && entry.before) {
        const doc = db.documents.find(d => d.id === entry.documentId);
        if (doc) {
          if (entry.action === "renamed" && entry.before.currentName) {
            doc.currentName = entry.before.currentName as string;
          } else if (entry.action === "tagged" && entry.before.tags) {
            doc.tags = entry.before.tags as string[];
          } else if (entry.action === "moved" && entry.before.currentPath) {
            doc.currentPath = entry.before.currentPath as string;
          }
          doc.updatedAt = getTimestamp();
          db.emit({ type: "document.updated", payload: { ...doc }, timestamp: getTimestamp() });
        }
      }

      entry.reversible = false; // can't revert twice

      const revertAudit: AuditEntry = {
        id: "aud-" + makeId(),
        documentId: entry.documentId,
        action: "reverted",
        actor: "user",
        summary: `Reverted action from audit item ${entryId}: ${entry.summary}`,
        reversible: false,
        createdAt: getTimestamp()
      };
      
      db.auditLog.unshift(revertAudit);
      db.emit({ type: "audit.entry.created", payload: revertAudit, timestamp: getTimestamp() });
      db.updateQueueCounts();

      return {
        ok: true,
        data: {
          success: true,
          message: `Successfully reverted action: ${entry.summary}`,
          documentId: entry.documentId || ""
        },
        requestId: makeId(),
        receivedAt: getTimestamp()
      };
    }
  },

  settings: {
    async get() {
      return { ok: true as const, data: { ...db.config }, requestId: makeId(), receivedAt: getTimestamp() };
    },
    async save(config: AppConfig) {
      db.config = { ...config };
      return { ok: true as const, data: undefined as void, requestId: makeId(), receivedAt: getTimestamp() };
    }
  },

  events: {
    subscribe(callback: (event: MasterdEvent) => void): () => void {
      return db.subscribe(callback);
    }
  }
};
