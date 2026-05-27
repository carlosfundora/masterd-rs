use anyhow::Result;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::rag::WebResult;

/// Calls the local SearXNG instance and returns structured results.
pub struct WebSearchBackend {
    base_url: String,
    client: reqwest::Client,
}

impl WebSearchBackend {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("build reqwest client"),
        }
    }

    pub async fn search(&self, query: &str, num_results: usize) -> Result<Vec<WebResult>> {
        let url = format!("{}/search", self.base_url);
        debug!(query, num_results, "searxng search");

        let resp = self
            .client
            .get(&url)
            .query(&[
                ("q", query),
                ("format", "json"),
                ("pageno", "1"),
            ])
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                warn!("SearXNG unreachable: {e}");
                return Ok(vec![]);
            }
        };

        if !resp.status().is_success() {
            warn!("SearXNG returned {}", resp.status());
            return Ok(vec![]);
        }

        let raw: SearxResponse = resp.json().await?;
        let results = raw
            .results
            .into_iter()
            .take(num_results)
            .map(|r| WebResult {
                title:   r.title.unwrap_or_default(),
                url:     r.url,
                snippet: r.content.unwrap_or_default(),
            })
            .collect();

        Ok(results)
    }
}

// ── SearXNG JSON contract ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearxResponse {
    #[serde(default)]
    results: Vec<SearxResult>,
}

#[derive(Deserialize)]
struct SearxResult {
    url: String,
    title: Option<String>,
    content: Option<String>,
}
