# MASTERd Codebase Review
**Version:** 2.0.0 | **Date:** 2026-06-11 | **Scope:** MASTERd ↔ masterd-libs drift, Rust capability mapping, integration strategy

---

## Executive Summary

- **96% of shared files are byte-for-byte identical** (26/27 non-extraction files). All `utils/`, `core/`, `config/`, and `api.py` have diverged only in import-path namespace (`src.` → `masterd_extract.`). The two codebases are trivially re-unifiable.
- **masterd-ocr is strictly ahead** on the 3 extraction files that differ: it adds a 4th PDF extraction tier (isolated textract subprocess), a 3-tier HTML cascade (trafilatura → readabilipy → BS4), chat-log detection in text files, and fully-formed NLP/dedup modules (`dedup/`, `nlp/`, callbacks) absent from MASTERd entirely.
- **masterd-galactic is an entirely new capability layer** providing the full modern RAG stack — dual embeddings (LanceDB), BM25 (SQLite FTS5), ColBERT reranking, HyDE, Self-RAG, RRF fusion — none of which exist in MASTERd's current `search_service.py` (which does only SQLite LIKE/FTS).
- **MASTERd's legacy `src/masterd/` subtree is 100% dead code** — zero imports from the current `src/` layout reach it. It is safe to delete immediately.
- **The Rust workspace has direct replacements for every hot-path operation** (MinHash, BM25, cosine similarity, string matching, fingerprinting, chunking, dedup), all already built as PyO3 `cdylib` modules ready for Python bridging via `rs_pyo3_mesh`.

---

## Phase 1: Module Drift Analysis

### Summary Table

| File | Classification | Divergence Notes |
|---|---|---|
| `extraction/pdf.py` | **SIGNIFICANT** | masterd-ocr adds 4th extraction tier (isolated textract subprocess), minor whitespace cleanup |
| `extraction/pdf_decrypt.py` | **IDENTICAL** | — |
| `extraction/text.py` | **SIGNIFICANT** | masterd-ocr adds trafilatura→readabilipy→BS4 cascade for HTML; chat log detection for .txt; import path changes |
| `extraction/benchmark.py` | **MINOR** | Import paths only (`src.` → `masterd_extract.`) |
| `extraction/ocr/evaluate.py` | **IDENTICAL** | — |
| `extraction/ocr/preprocess.py` | **MINOR** | Import path only |
| `extraction/ocr/processor.py` | **MINOR** | Import paths only (4 changed lines) |
| `core/config_v2.py` | **IDENTICAL** | — |
| `core/logger.py` | **IDENTICAL** | — |
| `core/messages.py` | **IDENTICAL** | — |
| `core/validation.py` | **IDENTICAL** | — |
| `config/ml_config.py` | **IDENTICAL** | — |
| `api.py` | **IDENTICAL** | — |
| `utils/concurrency.py` | **IDENTICAL** | — |
| `utils/duplicate_detector.py` | **IDENTICAL** | — |
| `utils/generate_key.py` | **IDENTICAL** | — |
| `utils/hardware.py` | **IDENTICAL** | — |
| `utils/indexing.py` | **IDENTICAL** | — |
| `utils/interrupt.py` | **IDENTICAL** | — |
| `utils/io.py` | **IDENTICAL** | — |
| `utils/lru_cache.py` | **IDENTICAL** | — |
| `utils/metrics.py` | **IDENTICAL** | — |
| `utils/naming.py` | **IDENTICAL** | — |
| `utils/repository_detector.py` | **IDENTICAL** | — |
| `utils/security.py` | **IDENTICAL** | — |
| `utils/time.py` | **IDENTICAL** | — |
| `utils/zip_inspector.py` | **IDENTICAL** | — |

**Score: 24 IDENTICAL · 3 MINOR · 0 MODERATE · 2 SIGNIFICANT** (import-path changes in MINOR/SIGNIFICANT are mechanical namespace renames, not logic changes)

---

### Diffs for SIGNIFICANT Files

#### `extraction/pdf.py` — SIGNIFICANT

masterd-ocr adds a 4th PDF extraction tier as an isolated subprocess calling a `.venv-textract` binary. MASTERd stops at pdfminer.six (tier 3); masterd-ocr continues to textract as a last resort. Several trailing whitespace cleanups are also present.

```diff
30c30
<         Attempts text extraction in order (PyMuPDF -> pdfplumber -> pdfminer.six).
---
>         Attempts text extraction in order (PyMuPDF -> pdfplumber -> pdfminer.six -> Isolated Textract).
40c40
<                 self.logger.info(f"pdfplumber failed/empty for {file_path.name}, trying pdfminer.")
---
>                 self.logger.info(f"pdfplumber failed/empty for {file_path.name}, trying pdfminer.six.")
41a42,48
>         if not text:
>             if self.logger:
>                 self.logger.info(f"pdfminer.six failed/empty for {file_path.name}, trying isolated textract fallback.")
>             text = self.extract_text_with_textract_isolated(file_path)
>
>         return {"extracted_text": text}

86a94,135
>     def extract_text_with_textract_isolated(self, file_path: Path) -> str:
>         """
>         Falls back to an isolated textract virtual environment to avoid dependency conflicts.
>         Requires Scripts/setup_textract_venv.sh to have been built.
>         """
>         import subprocess
>         import os
>
>         current_dir = Path(__file__).resolve().parent
>         project_root = current_dir.parent.parent.parent
>         venv_bin = project_root / ".venv-textract" / "bin" / "textract"
>
>         if not venv_bin.exists():
>             if self.logger:
>                 self.logger.warning(f"Isolated textract not found at {venv_bin}. Run scripts/setup_textract.sh to enable this fallback.")
>             return ""
>
>         try:
>             result = subprocess.run(
>                 [str(venv_bin), str(file_path)],
>                 capture_output=True,
>                 text=True,
>                 timeout=60
>             )
>             if result.returncode == 0 and result.stdout:
>                 return result.stdout.strip()
>             else:
>                 if self.logger and result.stderr:
>                     self.logger.error(f"{file_path.name}: textract CLI error - {result.stderr}")
>                 return ""
>         except subprocess.TimeoutExpired:
>             if self.logger:
>                 self.logger.error(f"{file_path.name}: textract fallback timed out after 60s.")
>             return ""
>         except Exception as e:
>             if self.logger:
>                 self.logger.error(f"{file_path.name}: textract fallback exception - {e}")
>             return ""

126,127c175,176
<     from src.extraction.ocr import perform_ocr_pdf
---
>     from masterd_extract.extraction.ocr import perform_ocr_pdf
```

**Impact on MASTERd:** MASTERd will fail silently on documents that require textract. The isolated-venv subprocess pattern avoids the `textract` package's conflicting C-library dependencies at the cost of a setup script.

---

#### `extraction/text.py` — SIGNIFICANT

masterd-ocr replaces MASTERd's BeautifulSoup-only HTML path with a 3-tier cascade (trafilatura → readabilipy → BS4 fallback), adds chat-log detection for `.txt` files, and changes several import paths.

```diff
148c148,151
<     Extracts text and code from HTML or XML files.
---
>     Extracts text and code from HTML or XML files using cascading fallbacks:
>     1. Trafilatura
>     2. Readabilipy
>     3. BeautifulSoup
153,156d155
<     if not BS4_AVAILABLE:
<         logger.error("beautifulsoup4 library not found...", critical=True)
<         return result
<
160,162d158
<         # Optimization: For smaller files (< 5MB), read content for code extraction...
<
166d161
<                 # Store raw code/content
169c164,195
<                 # Parse from bytes (BS4 detects encoding)
---
>                 # Tier 1: Try Trafilatura
>                 try:
>                     import trafilatura
>                     extracted = trafilatura.extract(result['extracted_code'])
>                     if extracted and len(extracted.strip()) > 50:
>                         result['extracted_text'] = extracted
>                         logger.info(f"{file_path.name}: Extracted successfully using Trafilatura.")
>                         return result
>                 except ImportError:
>                     logger.warning("Trafilatura not installed.")
>                 except Exception as e:
>                     logger.debug(f"Trafilatura failed: {e}")
>
>                 # Tier 2: Try readabilipy
>                 try:
>                     from readabilipy import simple_json_from_html_string
>                     article = simple_json_from_html_string(result['extracted_code'], use_readability=True)
>                     if article and article.get('plain_text'):
>                         text = "\n".join([t['text'] for t in article['plain_text'] if t.get('text')])
>                         if len(text.strip()) > 50:
>                             result['extracted_text'] = text
>                             logger.info(f"{file_path.name}: Extracted successfully using Readabilipy.")
>                             return result
>                 except ImportError:
>                     logger.warning("Readabilipy not installed.")
>                 except Exception as e:
>                     logger.debug(f"Readabilipy failed: {e}")
>
>                 # Tier 3: Fallback to BeautifulSoup Strainer
>                 if not BS4_AVAILABLE:
>                     return result
...
202,205d225
<     except FileNotFoundError:
<         logger.error(f"File not found: {file_path.name}.", ...)
<     except PermissionError:
<         logger.error(f"Permission denied: {file_path.name}.", ...)
...
220a241,242
>         from .chat import extract_chat_log
>
225a248,254
>         # Try to parse text as a chat log (WhatsApp/SMS)
>         if file_size < MAX_TEXT_SIZE:
>             chat_result = extract_chat_log(file_path)
>             if chat_result:
>                 logger.info(f"{file_path.name}: Identified and parsed as structured chat log.")
>                 return chat_result

568,569c597,598
<     from src.extraction.ocr import perform_ocr_image
---
>     from masterd_extract.extraction.ocr import perform_ocr_image
```

**Impact on MASTERd:** HTML extraction will produce inferior output — BS4 strips boilerplate (navbars, ads) poorly compared to trafilatura. MASTERd also silently drops FileNotFoundError/PermissionError cases (the except blocks were removed in masterd-ocr, likely an oversight worth reviewing before merging).

---

## Phase 2: Features Only in masterd-libs

### masterd-ocr — New Modules

| Module | Summary | Significance |
|---|---|---|
| `dedup/minhash.py` | Near-duplicate detection using datasketch MinHash LSH. Computes 128-permutation MinHash signatures via isolated `.venv-datasketch` subprocess; falls back to simulated n-gram hashing. Returns `MinHashResult` with Jaccard similarity and `is_near_duplicate` flag. | Prevents DB bloat from near-duplicate documents — critical for large ingestion pipelines. Rust equivalent: `rs_minhash_dedup` (same algorithm). |
| `dedup/bloom.py` | O(1) hash existence check via Bloom filter (`BloomIndex` class). Uses `pybloom_live` if available, otherwise pure-Python double-hashing. Default 100K capacity, 0.1% false positive rate. | Pre-flight dedup gate before any DB query — avoids round-trips on already-processed files. Complement to `duplicate_detector.py`. |
| `nlp/language.py` | Language detection using fasttext LID-176 model via isolated `.venv-fasttext` subprocess. Returns ISO 639-1 code + confidence. Heuristic fallback (script detection + common word matching). | Required for per-language OCR engine selection and multilingual extraction pipelines. |
| `nlp/scrub.py` | PII scrubbing using scrubadub (isolated subprocess). Detects SSNs, emails, phone numbers, credit cards, IPs. Regex fallback. Returns `ScrubResult` with entity list and scrubbed text. | Compliance requirement for any pipeline handling sensitive documents. Rust partial equivalent: `rs_evidence_sanitizer` (scaffolded). |
| `nlp/fuzzy.py` | OCR spelling correction via rapidfuzz. Matches garbled words against a dictionary using configurable similarity threshold (default 80%). Checks for `rust_string_matching` PyO3 bridge first, then rapidfuzz, then pure Python. | Improves OCR text quality downstream. Note: explicitly checks for the Rust `rs_string_matching` bridge — the Rust integration point is already wired in. |
| `nlp/validation.py` | Agent-configurable Pydantic V2 validation for extraction results. Provides `ExtractedEntity`, `ExtractionSchema`, `ExtractionValidationResult` models. Validates dates, amounts, required fields. | Enables AI-agent-driven dataset creation where the agent defines the validation contract at runtime. |
| `extraction/chat.py` | Structured chat-log parser for WhatsApp exports and SMS backups. Handles timestamp normalization (ISO 8601), sender identity normalization (phone numbers, names), multi-format message bodies. | New document type support not in MASTERd. Required by `text.py`'s chat detection branch. |
| `extraction/lexers.py` | Pygments `RegexLexer` subclasses for WhatsApp (`WhatsAppLexer`) and SMS (`SMSLexer`) formats. Tokenizes timestamps, sender names, system events, message content. | Enables syntax-highlighted rendering and structured parsing of chat exports in downstream tools. |
| `core/callbacks.py` | HuggingFace Trainer-style event callback system. `ExtractionCallback` base class + `CallbackHandler` dispatcher. Events: `on_extraction_begin/end`, `on_step_begin/end`, `on_log`, `on_error`. `ExtractionState` dataclass carries file metadata, elapsed time, OCR flags. | Decouples observability from extraction logic — prerequisite for logging and future metrics backends. |
| `config/extraction_params.py` | Pydantic V2 `ExtractionParams` model exposing all 45+ extraction engine tunable parameters: concurrency flags, OCR engine mode/PSM/DPI, all 11 image preprocessing toggles, dynamic reprocessing limits, quality thresholds. | Single authoritative configuration model for programmatic API consumption (agents, integrations). |
| `integrations/` | Optional experiment-tracking callback layer (if ever needed). | **Omitted from the new Rust-first MASTERd app scope**. |

---

### masterd-galactic — Entirely Absent from MASTERd

#### `embeddings/` Submodule

| Module | Summary |
|---|---|
| `dual_embed.py` | `DualEmbedder` class producing two embedding vectors per document (Arctic-embed-L primary, configurable secondary) via HTTP to SGLang embedding server. Supports int8 quantization at ingest time. Batched async calls. |
| `embedding_cache.py` | SQLite-backed L2 embedding cache keyed by SHA-256 content hash. Thread-safe write-through with random TTL jitter to avoid cache stampede. Falls back gracefully on DB errors. |
| `embedding_analysis.py` | Dimensionality reduction (UMAP) and clustering (HDBSCAN) on embedding vectors for corpus analysis and visualization. |
| `embed_colbert_bench.py` | Benchmarking harness for ColBERT late-interaction scoring. |
| `gpu_cluster.py` | GPU-accelerated K-Means clustering via `torch_kmeans` on AMD RX 6700 XT (ROCm/gfx1030). Centroid caching for query routing. |

#### `extraction/` Submodule

| Module | Summary |
|---|---|
| `content_extractor.py` | 4-tool cascade content extractor: trafilatura → readabilipy → goose3 → boilerpy3, each in isolated venv subprocess. Returns `ExtractionResult` with text, title, author, date, URL, source tool. |
| `ingest_controller.py` | 7-step document write pipeline: (1) compute doc_id/content_hash, (2) insert `pending` row, (3) dual embed, (4) index in LanceDB, (5) update SQL with `index_receipt`, (6) save to filesystem, (7) create Neo4j `:Doc` node. Failure → `status='failed'`. |
| `ingest_router.py` | Routes incoming documents to appropriate extractor by MIME type and content analysis. |
| `data_quality.py` | Per-document data quality scoring: text density, language confidence, PII density, extraction tier used. |
| `archive_offload.py` | Archives non-ingested search results to Wayback Machine. Logs all accept/reject/blacklist decisions to Redis + JSONL for fine-tuning datasets. |

#### `retrieval/` Submodule

| Module | Summary |
|---|---|
| `bm25_search.py` | BM25 sparse search via SQLite FTS5 virtual table. `SearchQueryParser` converts natural-language queries to FTS5 syntax with proximity operators, boolean logic, negation shorthands, and quote balancing. |
| `dense_search.py` | ANN vector search on LanceDB using `DualEmbedder` for query encoding. `DenseSearchStrategy` implements the `SearchStrategy` interface. |
| `fusion.py` | Weighted Reciprocal Rank Fusion (RRF) combining BM25 + dense results. Per-source weight tuning, k=60 smoothing constant. |
| `reranker.py` | ColBERT reranker (LFM2-ColBERT-350M) via PyLate/FastAPI server (port 8082). Fallback chain: PyLate → RAGatouille in-process → SGLang pointwise → passthrough. |
| `hyde.py` | Hypothetical Document Embeddings: SGLang generates a hypothetical ideal answer, which is embedded instead of the raw query to bridge vocabulary gap. |
| `self_rag.py` | Self-Reflective RAG: iterative retrieval with LLM critique of result quality. Refines query up to `max_iterations=3` until `confidence >= 0.7`. |
| `query_intent.py` | Online query intent clustering via `MiniBatchKMeans`. Gates HyDE generation by intent cluster, adjusts Self-RAG thresholds, tracks distribution. |
| `source_trust.py` | YAML-backed source whitelist/blacklist for web retrieval. Whitelisted sources bypass quality checks; blacklisted sources are rejected immediately. |
| `diversify.py` | Result diversification to avoid retrieving near-duplicate passages. |
| `search_cache.py` / `retrieval_cache.py` / `generation_cache.py` | Multi-layer caching for search, retrieval, and generation results. |
| `analytics.py` / `analytics_intelligence.py` | Observer-pattern analytics for retrieval events (ported from EXHIBITRON). DuckDB/metrics emission. |
| `sglang_tools.py` | SGLang function-call tool definitions for retrieval operations. |
| `strategies.py` | Abstract base classes: `SearchStrategy`, `FusionStrategy`, `RerankStrategy`, `ResultSource`, `RetrievalResult`, `SearchFilters`. |

---

## Phase 3: Features Only in MASTERd

### MASTERd-Only Modules

| Component | Summary | Rust Mapping |
|---|---|---|
| **`src/ml_nlp/model_registry.py`** | Centralized model registry with tiered fallback chains (PREMIUM → STANDARD → FALLBACK → FAST). Manages model availability and graceful degradation per `ModelTask` (entity extraction, document classification, keyword extraction, folder suggestion, duplicate detection, name suggestion). | No direct Rust equivalent — application-level orchestration. |
| **`src/ml_nlp/classification_learner.py`** | Adaptive document-type classifier that learns from user corrections. Maintains keyword→type score mappings and regex filename patterns. Persists learned preferences to `config/learned_classifications.json`. | No Rust equivalent — custom incremental learner. |
| **`src/ml_nlp/preference_learner.py`** | Learns user renaming preferences (case style, separator, prefix/suffix patterns) from filename corrections. | No Rust equivalent. |
| **`src/ml_nlp/structure_learner.py`** | Learns folder structure preferences from user `move` operations. Suggests target directories for new documents. | No Rust equivalent. |
| **`src/ml_nlp/live_learner.py`** | Coordinator aggregating PreferenceLearner, StructureLearner, and ClassificationLearner. Dispatches `CorrectionEvent` objects to appropriate sub-learners. | `rs_agent_runtime` for event dispatch; learner logic has no Rust equivalent. |
| **`src/ml_nlp/transformers_impl.py`** | HuggingFace Transformers + Flair integration for premium-tier classification and NER. | `rs_embedding_core` for embedding; `tch-rs` (vendor) for inference. |
| **`src/ml_nlp/ml_pipeline.py`** | Unified MLPipeline entry point. Registers all predictors, handles model selection, runs confidence cascade. | No direct Rust equivalent. |
| **`src/ml_nlp/service.py`** | NLPService FastAPI-compatible service layer wrapping MLPipeline. | `rs_agent_runtime` for service orchestration. |
| **`src/ml_nlp/training_data.py`** | Serializes user correction events as structured training data for fine-tuning pipelines. | `rs_audit_log_core` for event logging. |
| **`src/ml_nlp/entity_extraction.py`** | spaCy + NLTK entity extraction (persons, orgs, dates, locations). Falls back to regex patterns. | `rs_ingestion_core` (symbol/entity extraction in AST parsing context). |
| **`src/ml_nlp/syntax_analyzer.py`** | Syntax and code structure analysis using tree-sitter or AST fallback. | `rs_ast_core`, `rs_ingestion_core`. |
| **`src/ml_nlp/nlp_cache.py`** | LRU cache for NLP results (entity extraction, classification) to avoid recomputing on re-visits. | `rs_tiered_cache`. |
| **`src/database/sqlite_backend.py`** | Full async SQLite backend (aiosqlite). Schema: `documents` (31 columns including ML fields, OCR text, entities JSON, review status, ML confidence). Optimized queries. | `rs_sqlite_client`. |
| **`src/database/connection_pool.py`** | Async PostgreSQL connection pool via asyncpg for server-mode deployment. | `rs_postgres_client`. |
| **`src/database/management.py`** | Unified DB layer: auto-selects SQLite or PostgreSQL. Schema migration, `ensure_table_compliance()`. | `rs_sqlite_client` + `rs_postgres_client`. |
| **`src/database/db_helper.py`** | Query helpers, batch insert utilities, typed result models. | `rs_sqlite_client`. |
| **`src/services/batch_processor.py`** | Async batch document processor with configurable concurrency (`ThreadPoolExecutor`). `DocumentTask`, `BatchStatus` state machine. Progress callbacks, cancellation support. | `rs_workflow_engine`, `rs_agent_runtime`. |
| **`src/services/search_service.py`** | Unified FTS search across `filename`, `content`, `ocr_text`, `entities` fields. SQLite FTS5 + PostgreSQL `tsvector`. Paginated `SearchResponse`. | `rs_retrieval_core` (BM25/lexical), `rs_scoring`. |
| **`src/services/health_service.py`** | System health aggregation: DB connectivity, disk space, memory, GPU status, processing queue depth. | `rs_observability_events`. |
| **`src/ui/main_window.py`** | PyQt6 `QMainWindow`. Embeds FastAPI via `qasync` event loop bridge. Manages tab layout. | `tauri` / `iced` / `egui` (ecosystem). |
| **`src/ui/dashboard.py`** | Real-time processing dashboard with live document stats, progress bars, recent activity feed. | Same as above. |
| **`src/ui/review_queue.py`** | Document review queue UI. Displays ML predictions, confidence scores. Accepts user corrections that feed back to live learners. | Same as above. |
| **`src/ui/directory.py`** | Directory browser with drag-and-drop ingestion, folder tree view. | Same as above. |
| **`src/ui/settings.py`** | Settings panel for OCR config, database backend selection, ML model tiers, processing concurrency. | Same as above. |
| **`src/ui/training.py`** | Training data review UI. Shows accumulated corrections, triggers fine-tuning exports. | Same as above. |
| **`src/web/routers/`** | 8 FastAPI routers: `documents` (CRUD), `search` (FTS), `batch` (job management), `analytics` (aggregate stats), `training` (export data), `config` (runtime config), `system` (health), `health` (liveness/readiness), `database` (direct query UI). | `axum` (ecosystem). |

---

## Phase 4: Structural Issues in MASTERd

### 4.1 Legacy `src/masterd/` Layout — Dead Code

**Finding:** The `src/masterd/` directory is a v1 layout containing the original package structure:

```
src/masterd/
├── extractors/advanced_extractor.py
├── logging/logger.py
├── settings/config.py  (imports from masterd.utils.utilities)
├── processing/processor.py  (imports masterd.* extensively)
├── database/db_management.py
├── extract/{text,pdf,code,data,ocr}/
├── main.py
├── legacy_extract/{text_spreadsheets_html.py, pdfs_images_ocr.py}
└── utils/{time_utils.py, utilities.py}
```

All imports within `src/masterd/` reference each other via `from masterd.*` (self-contained). **Zero files in the current `src/` layout** (outside `src/masterd/`) import from `src/masterd/`. Confirmed:

```bash
# grep -r "from.*masterd\." src/ --include="*.py" | grep -v "src/masterd/"
# (no output)
```

**Safe to delete:** `src/masterd/` is 100% dead code. Removing it eliminates ~15 Python files and eliminates the risk of `from masterd.` accidentally being imported at runtime.

---

### 4.2 `pyproject.toml` Entry Point Issue

**Finding:**
```toml
[project.scripts]
masterd = "src.ui.main_window:main"
```

There is **no `[tool.hatch.build.targets.wheel]`** section in `pyproject.toml`. With hatchling's auto-discovery and `src/__init__.py` present, `src` is treated as a top-level package. The entry point `src.ui.main_window:main` is therefore importable **only if `src` is installed as a package** — which it is in this case since `src/__init__.py` exists.

However, this is non-standard src-layout. The correct pattern for src-layout packaging is:
```toml
[tool.hatch.build.targets.wheel]
packages = ["src"]
```

Without this, editable installs (`pip install -e .`) may or may not resolve `src.ui.main_window` depending on whether the root is on `sys.path`. The entry point should either be:
- `"src.ui.main_window:main"` with explicit `packages = ["src"]` in hatch config, **OR**
- Restructure to `ui.main_window:main` with `packages = ["src/ui", ...]`

**Recommended fix:**
```toml
[tool.hatch.build.targets.wheel]
packages = ["src"]
```

---

### 4.3 Dependency Gaps

**Missing in MASTERd `pyproject.toml` vs. masterd-ocr:**

| Package | Used In | Risk |
|---|---|---|
| `pdfminer.six` | `extraction/pdf.py` (tier 3 fallback, explicitly named in docstring) | **HIGH** — silent extraction failure on tier 3; package is listed in masterd-ocr but not MASTERd |
| `pdf2image` | `extraction/ocr/processor.py` (PDF→image conversion for OCR) | **HIGH** — OCR on scanned PDFs will fail at runtime |
| `trafilatura` | `extraction/text.py` (HTML tier 1) | MEDIUM — gracefully skipped with warning, but HTML quality degrades to BS4-only |
| `readabilipy` | `extraction/text.py` (HTML tier 2) | MEDIUM — same as above |
| `lxml` | bs4 parser dependency for robust HTML handling | LOW — falls back to html.parser |
| `datasketch` | Not present in MASTERd at all | LOW — dedup modules are masterd-ocr only |
| `xlrd` | masterd-ocr has `xlrd>=2.0.1` for `.xls` files; MASTERd has only `openpyxl` | MEDIUM — legacy `.xls` files will fail |

**Present in MASTERd but not masterd-ocr** (MASTERd extras):
`PyQt6`, `qasync`, `fastapi`, `uvicorn`, `asyncpg`, `aiosqlite`, `spacy`, `nltk`, `scikit-learn`, `simhash`, `rake-nltk`, `transformers`, `flair`, `gensim`, `torch`, `GPUtil`, `colorama`, `rich`

---

### 4.4 Python Version

- **MASTERd:** `.python-version = 3.12`, `pyproject.toml requires-python = ">=3.12"`
- **masterd-ocr:** `requires-python = ">=3.11"`
- **masterd-galactic:** `requires-python = ">=3.11"`

If MASTERd adopts masterd-libs as pip dependencies, the version constraint gap (3.11 vs 3.12) is non-blocking. However, masterd-ocr's `requires-python = ">=3.11"` should be tightened to `">=3.12"` to match MASTERd's actual runtime and prevent CI regressions on 3.11.

---

## Phase 5: Rust Capability Mapping

### Full Capability → Rust Crate Table

| Capability | Python Location | Local Rust Crate | Ecosystem Crate (if no local) | Notes |
|---|---|---|---|---|
| PDF text extraction (PyMuPDF/pdfplumber/pdfminer tiers) | `extraction/pdf.py` | — | `lopdf`, `pdfium-render`, `pdf-extract` | No local crate; would require new `rs_pdf_extractor` |
| PDF decryption (pikepdf) | `extraction/pdf_decrypt.py` | — | `lopdf` (partial) | pikepdf wraps QPDF; full Rust alternative TBD |
| Textract subprocess fallback | `extraction/pdf.py` (ocr-lib) | — | — | Subprocess pattern; Rust can shell-exec identically |
| Document ingestion pipeline (7-step) | `galactic/extraction/ingest_controller.py` | `rs_ingestion_core` | — | Full match: doc_id, hash, index, SQL, FS, Neo4j write |
| AST/symbol parsing | `ml_nlp/syntax_analyzer.py` | `rs_ast_core`, `rs_ingestion_core` | `tree-sitter` (vendor) | `rs_ingestion_core` includes tree-sitter AST |
| Document chunking | Not in Python | `rs_chunking_core` | `text-splitter` (vendor) | Python has no chunker; Rust is ahead |
| Content type routing (prose/code/mixed) | Not in Python | `rs_content_router` | — | Python has no equivalent classifier |
| OCR (Tesseract) | `extraction/ocr/processor.py` | — | `tesseract-rs` bindings | No local crate; external binding required |
| OCR image preprocessing (OpenCV) | `extraction/ocr/preprocess.py` | — | `image` crate, `imageproc` | No local crate |
| OCR quality evaluation | `extraction/ocr/evaluate.py` | — | — | Text-density heuristics; trivially portable |
| HTML extraction (trafilatura/BS4) | `extraction/text.py` | — | `scraper` crate | No local crate |
| Chat log parsing | `extraction/chat.py` (ocr-lib) | — | `nom` parser combinator | No local crate; `nom` is ideal |
| Syntax lexing (Pygments) | `extraction/lexers.py` (ocr-lib) | — | `syntect` crate | No local crate |
| MinHash near-duplicate detection | `dedup/minhash.py` (ocr-lib) | **`rs_minhash_dedup`** | — | Direct match; PyO3 `cdylib` ready |
| Bloom filter existence check | `dedup/bloom.py` (ocr-lib) | — | `bloomfilter` crate | No local crate; trivial to add |
| Document-level deduplication | `utils/duplicate_detector.py` | **`rs_document_deduper`** | — | Direct match; PyO3 `cdylib` ready |
| Lexical/token deduplication | Not in Python | **`rs_lex_dedup`** | — | Rust ahead; Python has no equivalent |
| Chunk-level deduplication | Not in Python | **`rs_dedup_chunker`** | — | Rust ahead |
| Streaming fingerprinting/hashing | `utils/generate_key.py`, `utils/security.py` | **`rs_fingerprinting_core`**, **`rs_hashing`** | — | Both PyO3 `cdylib` ready; `rs_hashing` uses blake2 |
| Language detection (fasttext) | `nlp/language.py` (ocr-lib) | — | `whatlang-rs` (vendor) | Vendor `whatlang-rs` is local; no wrapper crate |
| PII scrubbing (scrubadub) | `nlp/scrub.py` (ocr-lib) | **`rs_evidence_sanitizer`** | — | Scaffolded; needs full entity patterns |
| Fuzzy/OCR spelling correction | `nlp/fuzzy.py` (ocr-lib) | **`rs_string_matching`** | — | `nlp/fuzzy.py` already checks for `rust_string_matching` import |
| BM25 / lexical search | `galactic/retrieval/bm25_search.py`, `services/search_service.py` | **`rs_retrieval_core`** | `tantivy` (vendor) | `rs_retrieval_core`: BM25/FTS5 ranking; `tantivy` for full-text |
| Query parsing (FTS5 / intent) | `galactic/retrieval/bm25_search.py` | **`rs_query_parser`** | — | PyO3 `cdylib` ready; handles boolean, proximity, negation |
| Dense / ANN vector search | `galactic/retrieval/dense_search.py` | **`rs_vector`**, **`rs_embedding_core`** | `usearch` (vendor) | `rs_vector`: vector storage/ops; `rs_embedding_core`: embedding clients |
| Cosine similarity | `galactic/retrieval/dense_search.py` | **`rs_cosine_similarity`** | — | PyO3 `cdylib` ready |
| RRF fusion / result scoring | `galactic/retrieval/fusion.py` | **`rs_scoring`** | — | PyO3 `cdylib` ready |
| ColBERT reranking | `galactic/retrieval/reranker.py` | **`rs_colbert`** | — | PyO3 `cdylib` + `half` (fp16) + `rayon` (parallel) |
| Embedding generation | `galactic/embeddings/dual_embed.py` | **`rs_embedding_core`** | — | HTTP client + vector primitives |
| Tiered cache (L1 Moka + L2) | `galactic/retrieval/search_cache.py` etc., `utils/lru_cache.py` | **`rs_tiered_cache`** | — | Two-tier async; replaces Python LRU + SQLite cache |
| KV store abstractions | Not in Python | `rs_kv_core`, `rs_kv_broker`, `rs_kv_control` | — | Rust ahead |
| HyDE generation | `galactic/retrieval/hyde.py` | — | — | Requires LLM; `rs_chatterbox_runtime` for prompt handling |
| Self-RAG iterative retrieval | `galactic/retrieval/self_rag.py` | — | — | `rs_agent_runtime` + `rs_workflow_engine` for orchestration |
| Query intent clustering | `galactic/retrieval/query_intent.py` | — | — | scikit-learn MiniBatchKMeans; no Rust equivalent |
| GPU embedding clustering | `galactic/embeddings/gpu_cluster.py` | — | `burn` / `tch-rs` (vendor) | ROCm-specific; `rs_fa3_kernel` for AMD kernels |
| Adaptive ML classifiers | `ml_nlp/classification_learner.py` etc. | — | — | Custom incremental learners; no Rust equivalent |
| ML pipeline / model registry | `ml_nlp/ml_pipeline.py`, `model_registry.py` | — | — | Orchestration; `rs_agent_runtime` for service layer |
| NLP entity extraction | `ml_nlp/entity_extraction.py` | `rs_ingestion_core` (partial) | — | `rs_ingestion_core` has symbol extraction; spaCy NER has no full Rust equivalent |
| SQLite backend | `database/sqlite_backend.py` | **`rs_sqlite_client`** | `rusqlite` (vendor) | Full match; `rs_sqlite_client` wraps rusqlite |
| PostgreSQL backend | `database/connection_pool.py` | **`rs_postgres_client`** | `sqlx` (vendor) | Full match |
| LanceDB vector store | `galactic/embeddings/embedding_cache.py`, `ingest_controller.py` | **`rs_lancedb_client`** | — | Full match |
| Extraction callback system | `core/callbacks.py` (ocr-lib) | `rs_observability_events` | — | Event emit/subscribe pattern |
| Third-party experiment tracking callback | `integrations/` (ocr-lib) | — | — | Dropped from new app scope |
| Structured logging | `core/logger.py` | **`rs_logly_logger`** | `tracing` | Local logger crate with `logly_mesh` |
| Async concurrency / process pool | `utils/concurrency.py` | — | `rayon`, `tokio` | Standard async Rust |
| Hardware detection | `utils/hardware.py` | — | — | `sysinfo` crate (ecosystem) |
| Interrupt / signal handling | `utils/interrupt.py` | — | — | `ctrlc` crate (ecosystem) |
| Archive offload | `galactic/extraction/archive_offload.py` | `rs_artifacts` | — | `rs_artifacts` handles artifact storage |
| Source trust management | `galactic/retrieval/source_trust.py` | `rs_policy_mesh` | — | Policy gate pattern |
| REST API server | `src/web/` (FastAPI) | — | `axum`, `actix-web` | No local Rust API crate |
| Desktop GUI | `src/ui/` (PyQt6) | — | `tauri`, `iced`, `egui` | No local UI crate |
| Workflow / agent orchestration | Not in Python | `rs_agent_runtime`, `rs_workflow_engine`, `rs_workflow_core` | — | Rust ahead; Python has no workflow engine |
| Audit log | Not in Python | `rs_audit_log_core`, `rs_audit_storage` | — | Rust ahead |
| PyO3 bridge harness | — | **`rs_pyo3_mesh`** | — | Shared PyO3 contract + linker-based auto-discovery for all `*_pyo3` crates |

---

## Phase 6: Integration Strategy Recommendation

### Options Evaluated

| Option | Description | Verdict |
|---|---|---|
| **A** | MASTERd adopts masterd-libs as pip deps | ✅ Correct direction |
| **B** | Backport lib improvements to MASTERd, keep separate | ❌ Creates permanent maintenance fork |
| **C** | Keep inline extraction, add galactic as optional dep | ⚠️ Half-measure — dedup and NLP modules are also valuable |
| **D** | Rust-forward — replace hot paths via PyO3 | ✅ Correct destination, but not a standalone choice |

---

### Recommendation: **Option A + D (Adopt libs first, then Rust-forward incrementally)**

#### Rationale

**Step 1 — Immediate (Option A):** MASTERd should adopt `masterd-ocr` as a pip dependency and delete its inline extraction copies. The 96% identity rate means the migration is essentially a namespace replace (`from src.extraction` → `from masterd_ocr.extraction`). MASTERd gains:
- The textract 4th-tier fallback
- The trafilatura/readabilipy HTML cascade
- Chat log parsing for `.txt` files
- The full NLP suite (`dedup/`, `nlp/`, callbacks, ExtractionParams) at zero incremental cost

`masterd-galactic` should become an **optional dependency** (`pip install masterd[search]`) powering a new `SearchServiceV2` that replaces the current SQLite LIKE search with BM25 + dense ANN + ColBERT reranking.

**Step 2 — Medium term (Option D):** Wire PyO3 bridges to the Rust workspace for the 8 hot-path operations that already have `cdylib` crates: `rs_minhash_dedup`, `rs_document_deduper`, `rs_string_matching`, `rs_fingerprinting_core`, `rs_cosine_similarity`, `rs_scoring`, `rs_query_parser`, `rs_retrieval_core`. The `nlp/fuzzy.py` module already checks for `rust_string_matching` — this bridge is intentionally pre-wired.

**Why not Option B alone?**  
With 24/27 shared files byte-for-byte identical, maintaining them separately guarantees divergence. The 3 files that differ all show masterd-ocr ahead. Backporting manually creates two maintenance surfaces with no benefit.

**Why not Option C alone?**  
The `dedup/` and `nlp/` modules in masterd-ocr are directly valuable to MASTERd's document processing pipeline (dedup at ingest, PII scrubbing, language detection for OCR). Treating galactic-only as the integration target undersells what masterd-ocr adds.

---

## Prioritized Action List

### P1 — Do Now (blocks correctness)

| # | Action | Files |
|---|---|---|
| P1.1 | **Add `pdfminer.six` and `pdf2image` to MASTERd `pyproject.toml`** — both are used at runtime and absent from deps | `pyproject.toml` |
| P1.2 | **Delete `src/masterd/` subtree** — confirmed 100% dead code; risk of accidental import if namespace collides with pip-installed `masterd-ocr` | `src/masterd/` |
| P1.3 | **Add `[tool.hatch.build.targets.wheel] packages = ["src"]`** to `pyproject.toml` to make the `masterd = "src.ui.main_window:main"` entry point deterministic | `pyproject.toml` |
| P1.4 | **Pin `masterd-ocr requires-python = ">=3.12"`** to match MASTERd's actual runtime and prevent 3.11 CI breakage | `masterd-ocr/pyproject.toml` |

### P2 — Short Term (capability gaps)

| # | Action | Files |
|---|---|---|
| P2.1 | **Replace inline extraction with `masterd-ocr` dependency** — change `from src.extraction.*` imports to `from masterd_ocr.extraction.*` | All extraction imports in `src/` |
| P2.2 | **Add `trafilatura`, `readabilipy`, `lxml`, `xlrd` to MASTERd deps** — improves HTML extraction and `.xls` support immediately, even before full lib migration | `pyproject.toml` |
| P2.3 | **Wire `rs_minhash_dedup` PyO3 bridge** for `duplicate_detector.py` — `rs_minhash_dedup` is already `cdylib`; `nlp/fuzzy.py` pre-wires the import as `rust_string_matching` | `utils/duplicate_detector.py` |
| P2.4 | **Wire `rs_string_matching` PyO3 bridge** for `nlp/fuzzy.py` — the import check is already present in masterd-ocr's fuzzy.py | `nlp/fuzzy.py` |
| P2.5 | **Add `masterd-galactic` as optional dep** (`[project.optional-dependencies] search = ["masterd-galactic"]`) and implement `SearchServiceV2` wrapping galactic's BM25+dense pipeline | `pyproject.toml`, `src/services/search_service.py` |

### P3 — Medium Term (architectural uplift)

| # | Action | Files |
|---|---|---|
| P3.1 | **Replace `utils/lru_cache.py` with `rs_tiered_cache` PyO3 bridge** — two-tier async cache with Moka L1 replaces Python LRU; directly improves NLP cache throughput | `ml_nlp/nlp_cache.py` |
| P3.2 | **Replace `services/search_service.py` FTS with `rs_retrieval_core` + `rs_query_parser`** — Rust BM25 will outperform SQLite FTS5 LIKE queries; `rs_query_parser` handles boolean/proximity correctly | `src/services/search_service.py` |
| P3.3 | **Add ColBERT reranking to search pipeline** via `galactic/retrieval/reranker.py` (LFM2-ColBERT-350M already deployed at port 8082 per galactic code) | New `SearchServiceV2` |
| P3.4 | **Backfill `rs_evidence_sanitizer`** Rust implementation — currently scaffolded; PII scrubbing is a compliance requirement for document pipelines | `rs_evidence_sanitizer/src/lib.rs` |
| P3.5 | **Tighten masterd-galactic's internal import paths** — current code still references `python.src.database.*` and `python.src.services.*` (REPLICATOR project paths), not a standalone package; these must be abstracted before galactic can be installed as a pip dep | `masterd-galactic/src/**/*.py` |
| P3.6 | **Create `rs_pdf_extractor` crate** wrapping `pdfium-render` or `lopdf` for Rust-native PDF text extraction — fills the largest gap in the Rust capability map | New crate in `/home/local/ai/rust/crates/` |

---

## Agnostic Patterns from Legacy MASTERd (Conservative)

Scope: `references/MASTERd Train-*`, `references/MASTERd database modules-*`, `references/MASTERd v0.1b-*`, plus top-level legacy scripts.  
Filter applied: **legal-domain terminology/routing logic excluded** for this program.

### Pattern Matrix (source → abstract pattern → Rust target)

| Legacy Source | Abstract Pattern | Classification | Rust-first Target |
|---|---|---|---|
| `MASTERd Train/watcher.py` | Stage registry pipeline (`extraction_methods`) with per-stage isolation and best-result selection | portable_with_adaptation | `masterd-pipeline`, `masterd-ingest` |
| `MASTERd Train/naming_logic.json` | External rule-table taxonomy for filename generation and categorization | portable | `masterd-ingest`, `config/naming/*.json` |
| `MASTERd database modules/modules/pdf_processing.py` | End-to-end ingest loop (hash gate → extraction/OCR fan-in → classify → suggest route/name → persist/index) | portable_with_adaptation | `masterd-pipeline`, `masterd-ingest` |
| `MASTERd database modules/modules/indexing.py`, `hash_utils.py` | Hash-first dedup + durable index map for skip-on-seen behavior | portable | `masterd-ingest` |
| `MASTERd database modules/modules/ocr_processing.py` | CPU-aware worker scheduling and bounded OCR pools | portable | `masterd-runtime-tune`, `masterd-pipeline` |
| `MASTERd database modules/modules/interrupt_handling.py` | Signal-aware cancellation path and controlled shutdown | portable_with_adaptation | `masterd-runtime-tune`, `masterd-pipeline` |
| `MASTERd database modules/modules/logger.py` | Split telemetry sinks (console + file) with run summaries | portable | `masterd-pipeline`, `masterd-runtime-tune` |
| `MASTERd v0.1b/organizer.py` | Multi-engine extraction fallback cascade + OCR merge | portable | `masterd-embed-engine`, `masterd-pipeline` |
| `MASTERd v0.1b/organizer.py` | Preflight dependency validation before run start | portable | `masterd-bootstrap` |
| `MASTERd v0.1b/naming_conventions.json` | Data-driven naming convention packs (date/entity/type templates) | portable | `masterd-ingest`, `config/naming/*.json` |
| `MASTERd v0.1b/venv/Scripts/log2design.py` | Ingest normalization pattern (column rename/drop-empty/concat batches) | portable_with_adaptation | `masterd-ingest` |
| `references/main.py` / `references/pdf_processing.py` | Deterministic summary telemetry (processed, OCR’d, errors, duration) | portable | `masterd-pipeline` |

### Legacy Anti-Patterns to Avoid Carrying Forward

| Anti-pattern | Where seen | Why not port |
|---|---|---|
| Interactive prompts in pipeline path (`input()` for decrypt/overwrite/move) | `organizer.py`, `modules/pdf_processing.py` | Breaks unattended/batch automation |
| Broad exception swallowing / continue-only handling | multiple extraction functions | Hides failure quality and retry signals |
| Non-atomic index persistence without lock coordination | index save/update paths | Risk of corruption under concurrent runs |
| Hardcoded credentials/config values in source | legacy config files | Security and portability risk |

### Gap Analysis vs Current Rust Workspace

| Capability Pattern | Current Coverage | Gap |
|---|---|---|
| Staged fault-isolated pipeline orchestration | partial (`masterd-ingest` + engine checks) | unified stage graph + policy-based fallback runner |
| Data-driven naming/routing packs | partial (config exists, no full pack executor) | compile-time validated naming rule engine |
| Hash-first skip index | partial | durable concurrent-safe index backend |
| Cancellation + safe rollback path | partial (`masterd-runtime-tune`) | pipeline-wide cancellation contract |
| Structured ingest telemetry | partial | consistent per-stage counters + failure taxonomy |
| Extraction/OCR fallback policy | partial (`masterd-embed-engine`) | centralized strategy selection + score policy |

### Conservative Rollout Backlog (High-confidence portable only)

1. Build a `masterd-pipeline` stage runner contract (extract → OCR → merge → persist) with explicit stage result enums.
2. Add hash-gate index service in `masterd-ingest` with atomic writes and concurrency-safe updates.
3. Promote naming conventions into validated config packs (`config/naming/*.json`) and wire resolver into ingest.
4. Add preflight dependency probe + startup capability report in `masterd-bootstrap`.
5. Standardize cancellation handling across ingest/runtime tune with one interrupt token contract.
6. Introduce structured run telemetry (stage counters, failure classes, duration) emitted by all ingest paths.
7. Implement extraction strategy policy module (tier ordering + scoring + bounded retries) shared by ingest and checks.
8. Add schema-normalization utility for ingestion sources (column canonicalization/drop-empty/merge) in `masterd-ingest`.
