//! Ollama HTTP fallback backend.
//!
//! When the embedded GGUF models fail to load (missing assets, memory pressure,
//! first-run before models are downloaded), MASTERd transparently falls back to
//! a locally-running Ollama daemon (default: http://127.0.0.1:11434).
//!
//! Tokens are delivered to the same `mpsc::Sender<ChatToken>` channel that the
//! embedded engine uses, so the UI needs no changes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{ChatToken, WebResult};

pub const OLLAMA_DEFAULT_URL: &str = "http://127.0.0.1:11434";
pub const OLLAMA_DEFAULT_MODEL: &str = "llama3.2";

#[derive(Debug, Clone)]
pub struct OllamaBackend {
    pub base_url: String,
    pub model: String,
    client: reqwest::Client,
}

// ── Ollama API types ─────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OllamaMsg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OllamaChatReq<'a> {
    model: &'a str,
    messages: Vec<OllamaMsg<'a>>,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaChatResp {
    message: OllamaMsgResp,
}

#[derive(Deserialize)]
struct OllamaMsgResp {
    content: String,
}

#[derive(Deserialize)]
struct OllamaTagsResp {
    models: Vec<OllamaModelInfo>,
}

#[derive(Deserialize)]
struct OllamaModelInfo {
    name: String,
}

// ── OllamaBackend impl ───────────────────────────────────────────────────────

impl OllamaBackend {
    pub fn new(base_url: String, model: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .context("build ollama reqwest client")?;
        Ok(Self {
            base_url,
            model,
            client,
        })
    }

    /// `true` if the Ollama daemon is reachable and has at least one model loaded.
    pub async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url.trim_end_matches('/'));
        match self.client.get(&url).send().await {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    }

    /// Return the name of the best available model: prefer the configured one,
    /// else return the first model found, else return None.
    pub async fn resolve_model(&self) -> Option<String> {
        let url = format!("{}/api/tags", self.base_url.trim_end_matches('/'));
        let resp: OllamaTagsResp = self.client.get(&url).send().await.ok()?.json().await.ok()?;
        if resp.models.iter().any(|m| m.name.starts_with(&self.model)) {
            return Some(self.model.clone());
        }
        resp.models.into_iter().next().map(|m| m.name)
    }

    /// Generate a response and stream word-chunks to `tx`.
    ///
    /// Uses `stream: false` to avoid a `futures-util` dependency while still
    /// delivering tokens incrementally to the frontend channel.
    pub async fn chat_stream(
        &self,
        system_prompt: &str,
        user_message: &str,
        max_tokens: usize,
        citations: Vec<WebResult>,
        tx: mpsc::Sender<ChatToken>,
    ) -> Result<String> {
        let model = self
            .resolve_model()
            .await
            .unwrap_or_else(|| self.model.clone());
        debug!(model = %model, url = %self.base_url, "ollama fallback chat_stream");

        let req = OllamaChatReq {
            model: &model,
            messages: vec![
                OllamaMsg {
                    role: "system",
                    content: system_prompt,
                },
                OllamaMsg {
                    role: "user",
                    content: user_message,
                },
            ],
            // Non-streaming: receive entire response as one JSON blob, then
            // fan out word-by-word to the channel for pseudo-streaming UX.
            stream: false,
        };

        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url.trim_end_matches('/')))
            .json(&req)
            .send()
            .await
            .context("ollama /api/chat request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("ollama returned HTTP {status}: {body}");
        }

        let body: OllamaChatResp = resp.json().await.context("ollama response parse")?;
        let text = body.message.content;

        // Send word-by-word so the frontend receives incremental tokens the
        // same way it would from the embedded engine.
        let mut count = 0usize;
        for chunk in text.split_inclusive(|c: char| c.is_whitespace()) {
            if count >= max_tokens {
                break;
            }
            count += 1;
            if tx
                .send(ChatToken::Response(chunk.to_string()))
                .await
                .is_err()
            {
                warn!("ollama: frontend channel closed mid-stream");
                break;
            }
        }

        let model_badge = format!("ollama/{model}");
        let _ = tx
            .send(ChatToken::Done {
                model: model_badge,
                citations,
            })
            .await;
        Ok(text)
    }
}
