pub mod ollama;
mod rag;
mod session;
mod web_search;

pub use ollama::{OLLAMA_DEFAULT_MODEL, OLLAMA_DEFAULT_URL, OllamaBackend};
pub use rag::{RagContextBuilder, WebResult};
pub use session::{ChatMessage, ChatSession, Role};
pub use web_search::WebSearchBackend;

use anyhow::{Context, Result};
use candle_core::{Device, quantized::gguf_file};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_lfm2::ModelWeights;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, info};

pub use masterd_index::{
    BM25Okapi, DocumentDeduper, IndexSnapshot, IndexedDocument, LocalIndex, SearchResult,
};

// ── Compile-time embedded assets ────────────────────────────────────────────

/// LFM2.5-1.2B-Thinking GGUF — embedded at compile time (~1.3 GB in binary).
static THINKING_GGUF: &[u8] = include_bytes!("../assets/models/thinking/model.gguf");

/// LFM2.5-350M-Instruct GGUF — embedded at compile time (~375 MB in binary).
static INSTRUCT_GGUF: &[u8] = include_bytes!("../assets/models/instruct/model.gguf");

/// Tokenizer for the Thinking model.
static THINKING_TOKENIZER: &[u8] = include_bytes!("../assets/models/thinking/tokenizer.json");

/// Tokenizer for the Instruct model.
static INSTRUCT_TOKENIZER: &[u8] = include_bytes!("../assets/models/instruct/tokenizer.json");

/// MASTERd persona prompt — injected as system context on every call.
static MASTERD_PERSONA: &str = include_str!("../assets/prompts/masterd_personality.txt");

// ── User-facing mode enums ──────────────────────────────────────────────────

/// Which LFM2.5 variant to use for generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThinkMode {
    /// Heuristically choose: Thinking for complex queries, Instruct for fast chat.
    #[default]
    Auto,
    /// Always use LFM2.5-1.2B-Thinking (richer reasoning, slower).
    Thinking,
    /// Always use LFM2.5-350M-Instruct (fast, conversational).
    Instruct,
}

/// Where to retrieve context before answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Search only the local indexed document store.
    #[default]
    LocalDocuments,
    /// Search the web via the embedded SearXNG backend.
    WebSearch,
    /// Search both and merge context.
    Both,
}

// ── Streamed token types ────────────────────────────────────────────────────

/// Tokens delivered over the streaming channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum ChatToken {
    /// Reasoning token inside `<think>…</think>` (Thinking model only).
    Think(String),
    /// Normal response token.
    Response(String),
    /// Generation complete.
    Done {
        model: String,
        citations: Vec<WebResult>,
    },
}

// ── Loaded model handle ─────────────────────────────────────────────────────

struct LoadedModel {
    weights: ModelWeights,
    tokenizer: Tokenizer,
    eos_token_id: u32,
    device: Device,
}

impl LoadedModel {
    /// Load from compile-time embedded bytes — no filesystem access.
    fn from_embedded(
        gguf_bytes: &'static [u8],
        tokenizer_bytes: &'static [u8],
        label: &'static str,
    ) -> Result<Self> {
        info!(
            "loading embedded model '{label}' ({:.0} MB)",
            gguf_bytes.len() as f64 / 1_048_576.0
        );
        let device = Device::Cpu;

        // GGUF reader requires Read + Seek — Cursor provides both over &[u8].
        let mut cursor = Cursor::new(gguf_bytes);
        let content = gguf_file::Content::read(&mut cursor)
            .with_context(|| format!("parse embedded gguf for {label}"))?;
        let weights = ModelWeights::from_gguf(content, &mut cursor, &device)
            .with_context(|| format!("build model weights for {label}"))?;

        // Tokenizer from raw JSON bytes — no file path needed.
        let tokenizer = Tokenizer::from_bytes(tokenizer_bytes)
            .map_err(|e| anyhow::anyhow!("load tokenizer for {label}: {e}"))?;

        let eos_token_id = Self::guess_eos(&tokenizer);
        info!("'{label}' ready — eos_token_id={eos_token_id}");

        Ok(Self {
            weights,
            tokenizer,
            eos_token_id,
            device,
        })
    }

    fn guess_eos(tok: &Tokenizer) -> u32 {
        let vocab = tok.get_vocab(true);
        ["<|im_end|>", "</s>", "<|end|>", "<|endoftext|>"]
            .iter()
            .find_map(|t| vocab.get(*t).copied())
            .unwrap_or(2)
    }
}

// ── ChatEngine ──────────────────────────────────────────────────────────────

/// Runtime-tunable generation parameters (no file paths — everything is embedded).
#[derive(Debug, Clone)]
pub struct ChatEngineConfig {
    pub searxng_url: String,
    pub max_new_tokens: usize,
    pub temperature: f64,
    pub top_k: usize,
    pub repeat_penalty: f32,
    pub repeat_last_n: usize,
    /// Ollama daemon base URL — used as fallback when embedded models fail to load.
    pub ollama_url: String,
    /// Ollama model name to request (e.g. "llama3.2", "mistral", "phi3").
    pub ollama_model: String,
}

impl Default for ChatEngineConfig {
    fn default() -> Self {
        Self {
            searxng_url: "http://127.0.0.1:9265".to_string(),
            max_new_tokens: 1024,
            temperature: 0.7,
            top_k: 40,
            repeat_penalty: 1.1,
            repeat_last_n: 64,
            ollama_url: OLLAMA_DEFAULT_URL.to_string(),
            ollama_model: OLLAMA_DEFAULT_MODEL.to_string(),
        }
    }
}

/// The main chat engine. Models are lazy-loaded on first use from embedded bytes.
pub struct ChatEngine {
    config: ChatEngineConfig,
    thinking: Mutex<Option<LoadedModel>>,
    instruct: Mutex<Option<LoadedModel>>,
    rag: RagContextBuilder,
    web: WebSearchBackend,
    /// Shared local document index — attached at construction or later via `set_index`.
    pub index: Arc<RwLock<LocalIndex>>,
}

impl ChatEngine {
    pub fn new(config: ChatEngineConfig) -> Self {
        let web = WebSearchBackend::new(config.searxng_url.clone());
        let index = Arc::new(RwLock::new(LocalIndex::new(256)));
        Self {
            config,
            thinking: Mutex::new(None),
            instruct: Mutex::new(None),
            rag: RagContextBuilder::with_index(Arc::clone(&index)),
            web,
            index,
        }
    }

    /// Pre-warm both models (call at startup to avoid first-message latency).
    pub fn preload(&self) -> Result<()> {
        self.ensure_instruct()?;
        self.ensure_thinking()?;
        Ok(())
    }

    /// Which models are currently loaded.
    pub fn loaded_models(&self) -> Vec<&'static str> {
        let mut v = vec![];
        if self.thinking.lock().unwrap().is_some() {
            v.push("lfm2.5-thinking-1.2b");
        }
        if self.instruct.lock().unwrap().is_some() {
            v.push("lfm2.5-instruct-350m");
        }
        v
    }

    /// Index a single document for local retrieval.
    /// Thread-safe: takes a write lock on the shared index.
    pub async fn index_document(&self, doc: IndexedDocument) {
        let mut idx = self.index.write().await;
        idx.insert(doc);
    }

    /// Index multiple documents in a batch.
    pub async fn index_documents(&self, docs: Vec<IndexedDocument>) {
        let mut idx = self.index.write().await;
        for doc in docs {
            idx.insert(doc);
        }
    }

    /// Snapshot the current index for persistence.
    pub async fn snapshot_index(&self) -> IndexSnapshot {
        let idx = self.index.read().await;
        IndexSnapshot::from_index(&idx)
    }

    /// Restore the index from a snapshot (e.g., loaded from SQLite at startup).
    pub async fn restore_index(&self, snapshot: IndexSnapshot) {
        let restored = snapshot.into_index();
        let mut idx = self.index.write().await;
        *idx = restored;
    }

    /// Number of documents currently in the local index.
    pub async fn index_doc_count(&self) -> usize {
        self.index.read().await.len()
    }

    fn ensure_thinking(&self) -> Result<()> {
        let mut g = self.thinking.lock().unwrap();
        if g.is_none() {
            *g = Some(LoadedModel::from_embedded(
                THINKING_GGUF,
                THINKING_TOKENIZER,
                "lfm2.5-thinking",
            )?);
        }
        Ok(())
    }

    fn ensure_instruct(&self) -> Result<()> {
        let mut g = self.instruct.lock().unwrap();
        if g.is_none() {
            *g = Some(LoadedModel::from_embedded(
                INSTRUCT_GGUF,
                INSTRUCT_TOKENIZER,
                "lfm2.5-instruct",
            )?);
        }
        Ok(())
    }

    fn pick_model(&self, mode: ThinkMode, query: &str) -> ThinkMode {
        match mode {
            ThinkMode::Auto => {
                let complex = [
                    "explain",
                    "analyze",
                    "compare",
                    "why",
                    "how does",
                    "summarize",
                    "describe",
                    "difference",
                    "step by step",
                    "reason",
                ];
                let q = query.to_lowercase();
                if query.split_whitespace().count() > 12 || complex.iter().any(|kw| q.contains(kw))
                {
                    ThinkMode::Thinking
                } else {
                    ThinkMode::Instruct
                }
            }
            other => other,
        }
    }

    /// Send a user message and stream tokens to `tx`.
    ///
    /// Execution order:
    /// 1. Try embedded LFM2.5 model (compiled into the binary).
    /// 2. If loading the embedded model fails, transparently fall back to
    ///    a locally-running Ollama daemon if one is reachable.
    pub async fn chat(
        self: Arc<Self>,
        session: &mut ChatSession,
        query: String,
        think_mode: ThinkMode,
        search_mode: SearchMode,
        tx: mpsc::Sender<ChatToken>,
    ) -> Result<()> {
        let resolved = self.pick_model(think_mode, &query);

        let (context_block, citations) = self.rag.build(&query, search_mode, &self.web).await?;

        // Persona is baked in — always injected, never configurable at runtime.
        let system_prompt = format!("{}\n\n{}", MASTERD_PERSONA, context_block);
        session.push(Role::User, query.clone());
        let prompt = session.to_chatml(&system_prompt);

        debug!(model = ?resolved, prompt_chars = prompt.len(), "dispatching generation");

        let engine = self.clone();
        let tx2 = tx.clone();
        let cfg = self.config.clone();
        // Clone values needed by the Ollama fallback path (moved into closure otherwise).
        let citations_fb = citations.clone();
        let system_prompt_fb = system_prompt.clone();
        let query_fb = query.clone();

        // ── Attempt 1: embedded GGUF model ──────────────────────────────────
        let embedded_result = tokio::task::spawn_blocking(move || match resolved {
            ThinkMode::Thinking => {
                engine.ensure_thinking()?;
                let mut g = engine.thinking.lock().unwrap();
                generate(
                    g.as_mut().unwrap(),
                    &prompt,
                    &cfg,
                    ThinkMode::Thinking,
                    tx2,
                    citations,
                )
            }
            _ => {
                engine.ensure_instruct()?;
                let mut g = engine.instruct.lock().unwrap();
                generate(
                    g.as_mut().unwrap(),
                    &prompt,
                    &cfg,
                    ThinkMode::Instruct,
                    tx2,
                    citations,
                )
            }
        })
        .await
        .context("generation thread panicked");

        match embedded_result {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "embedded model failed — trying Ollama fallback");
            }
            Err(e) => {
                tracing::warn!(error = %e, "embedded model panicked — trying Ollama fallback");
            }
        }

        // ── Attempt 2: Ollama fallback ───────────────────────────────────────
        let ollama = OllamaBackend::new(
            self.config.ollama_url.clone(),
            self.config.ollama_model.clone(),
        )?;

        if !ollama.is_available().await {
            anyhow::bail!(
                "embedded engine failed and Ollama is not reachable at {}. \
                 Install Ollama (https://ollama.com) or place model assets in assets/models/.",
                self.config.ollama_url
            );
        }

        ollama
            .chat_stream(
                &system_prompt_fb,
                &query_fb,
                self.config.max_new_tokens,
                citations_fb,
                tx,
            )
            .await
    }
}

// ── Generation loop ─────────────────────────────────────────────────────────

fn generate(
    model: &mut LoadedModel,
    prompt: &str,
    config: &ChatEngineConfig,
    mode: ThinkMode,
    tx: mpsc::Sender<ChatToken>,
    citations: Vec<WebResult>,
) -> Result<()> {
    let ids: Vec<u32> = model
        .tokenizer
        .encode(prompt, true)
        .map_err(|e| anyhow::anyhow!("encode: {e}"))?
        .get_ids()
        .to_vec();

    let input = candle_core::Tensor::new(ids.as_slice(), &model.device)?.unsqueeze(0)?;
    let mut logits_proc = LogitsProcessor::from_sampling(
        42,
        Sampling::TopK {
            k: config.top_k,
            temperature: config.temperature,
        },
    );

    let mut all_tokens = ids.clone();
    let mut in_think = false;
    let mut think_buf = String::new();

    // Process full prompt in one forward pass.
    let logits = model.weights.forward(&input, 0)?;
    let logits = logits.squeeze(0)?.squeeze(0)?;
    let logits = repeat_penalty(
        &logits,
        &all_tokens,
        config.repeat_penalty,
        config.repeat_last_n,
    )?;
    let mut next = logits_proc.sample(&logits)?;
    all_tokens.push(next);

    for _ in 0..config.max_new_tokens {
        if next == model.eos_token_id {
            break;
        }
        let step_input = candle_core::Tensor::new(&[next], &model.device)?.unsqueeze(0)?;
        let logits = model.weights.forward(&step_input, all_tokens.len() - 1)?;
        let logits = logits.squeeze(0)?.squeeze(0)?;
        let logits = repeat_penalty(
            &logits,
            &all_tokens,
            config.repeat_penalty,
            config.repeat_last_n,
        )?;
        next = logits_proc.sample(&logits)?;
        all_tokens.push(next);

        if let Some(text) = decode_one(&model.tokenizer, next) {
            if mode == ThinkMode::Thinking {
                think_buf.push_str(&text);
                if !in_think && think_buf.contains("<think>") {
                    in_think = true;
                }
                if in_think {
                    let _ = tx.blocking_send(ChatToken::Think(text));
                    if think_buf.contains("</think>") {
                        in_think = false;
                        think_buf.clear();
                    }
                } else {
                    let _ = tx.blocking_send(ChatToken::Response(text));
                }
            } else {
                let _ = tx.blocking_send(ChatToken::Response(text));
            }
        }
    }

    let badge = match mode {
        ThinkMode::Thinking => "THINKING 1.2B",
        _ => "INSTRUCT 350M",
    };
    let _ = tx.blocking_send(ChatToken::Done {
        model: badge.to_string(),
        citations,
    });
    Ok(())
}

fn decode_one(tok: &Tokenizer, id: u32) -> Option<String> {
    tok.decode(&[id], true).ok().filter(|s| !s.is_empty())
}

fn repeat_penalty(
    logits: &candle_core::Tensor,
    tokens: &[u32],
    penalty: f32,
    last_n: usize,
) -> Result<candle_core::Tensor> {
    let window = if tokens.len() > last_n {
        &tokens[tokens.len() - last_n..]
    } else {
        tokens
    };
    let mut v: Vec<f32> = logits.to_vec1()?;
    for &t in window {
        let i = t as usize;
        if i < v.len() {
            if v[i] >= 0.0 {
                v[i] /= penalty;
            } else {
                v[i] *= penalty;
            }
        }
    }
    Ok(candle_core::Tensor::new(v.as_slice(), logits.device())?)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_is_embedded_and_non_empty() {
        assert!(!MASTERD_PERSONA.is_empty());
        assert!(MASTERD_PERSONA.contains("[IDENTITY]"));
    }

    #[test]
    fn tokenizer_bytes_are_embedded() {
        assert!(
            THINKING_TOKENIZER.len() > 1000,
            "thinking tokenizer suspiciously small"
        );
        assert!(
            INSTRUCT_TOKENIZER.len() > 1000,
            "instruct tokenizer suspiciously small"
        );
    }

    #[test]
    fn gguf_bytes_are_embedded() {
        // Just check magic bytes — we don't want to parse 1.6 GB in CI.
        assert_eq!(&THINKING_GGUF[..4], b"GGUF", "thinking GGUF magic wrong");
        assert_eq!(&INSTRUCT_GGUF[..4], b"GGUF", "instruct GGUF magic wrong");
    }

    #[test]
    fn config_default_has_correct_searxng_url() {
        let cfg = ChatEngineConfig::default();
        assert_eq!(cfg.searxng_url, "http://127.0.0.1:9265");
    }

    #[test]
    fn think_mode_auto_routes_complex_query_to_thinking() {
        let engine = ChatEngine::new(ChatEngineConfig::default());
        let resolved = engine.pick_model(
            ThinkMode::Auto,
            "explain how rotary embeddings work step by step",
        );
        assert_eq!(resolved, ThinkMode::Thinking);
    }

    #[test]
    fn think_mode_auto_routes_simple_query_to_instruct() {
        let engine = ChatEngine::new(ChatEngineConfig::default());
        let resolved = engine.pick_model(ThinkMode::Auto, "hello");
        assert_eq!(resolved, ThinkMode::Instruct);
    }
}
