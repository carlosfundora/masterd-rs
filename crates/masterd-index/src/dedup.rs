//! Web result deduplication — ported from rs_document_deduper (pyo3 stripped).
//!
//! Deduplicates search results by:
//! 1. Exact URL (after normalization)
//! 2. Domain + normalized title (catches reposts of same article)
//! 3. Long titles across all domains (near-duplicate headlines)
//! 4. Content hash (SHA-256 of first 512 bytes of snippet)

use std::collections::HashSet;

use sha2::{Digest, Sha256};
use url::Url;

/// Stateful deduplicator for a single search session.
pub struct DocumentDeduper {
    seen_urls: HashSet<String>,
    seen_domain_titles: HashSet<(String, String)>,
    seen_long_titles: HashSet<String>,
    seen_content_hashes: HashSet<String>,
}

impl Default for DocumentDeduper {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentDeduper {
    pub fn new() -> Self {
        Self {
            seen_urls: HashSet::new(),
            seen_domain_titles: HashSet::new(),
            seen_long_titles: HashSet::new(),
            seen_content_hashes: HashSet::new(),
        }
    }

    /// Normalize a URL: lowercase domain, strip www., strip default ports,
    /// strip trailing slash on root path.
    pub fn normalize_url(url_str: &str) -> String {
        if url_str.is_empty() {
            return String::new();
        }
        let Ok(parsed) = Url::parse(url_str) else {
            return url_str.to_lowercase();
        };

        let mut host = parsed.host_str().unwrap_or("").to_lowercase();
        if let Some(port) = parsed.port() {
            if !((port == 80 && parsed.scheme() == "http")
                || (port == 443 && parsed.scheme() == "https"))
            {
                host = format!("{host}:{port}");
            }
        }
        if host.starts_with("www.") {
            host = host[4..].to_string();
        }

        let path = parsed.path().trim_end_matches('/');
        let query = parsed.query().map(|q| format!("?{q}")).unwrap_or_default();
        format!("{}/{}{}", host, path, query)
    }

    /// Normalize a title for comparison: lowercase, strip common boilerplate
    /// suffixes (e.g., "- GitHub", "| Wikipedia"), collapse whitespace.
    pub fn normalize_title(title: &str) -> String {
        // Strip boilerplate suffixes like "- GitHub", "| Wikipedia", "• Medium"
        static BOILERPLATE_SUFFIXES: &[&str] = &[
            " - github",
            " | github",
            " - wikipedia",
            " | wikipedia",
            " - medium",
            " | medium",
            " - stack overflow",
            " | stack overflow",
            " - reddit",
            " | reddit",
            " - youtube",
            " | youtube",
            " - hacker news",
            " | hacker news",
            " - linkedin",
            " | linkedin",
            " - twitter",
            " | twitter",
            " - x",
            "• hacker news",
            "• github",
        ];

        let mut t = title.to_lowercase();

        // Strip "Show HN:", "Ask HN:", "Launch HN:" prefixes
        for prefix in &["show hn: ", "ask hn: ", "launch hn: "] {
            if t.starts_with(prefix) {
                t = t[prefix.len()..].to_string();
                break;
            }
        }

        for suffix in BOILERPLATE_SUFFIXES {
            if t.ends_with(suffix) {
                t.truncate(t.len() - suffix.len());
            }
        }

        // Keep only alphanumeric + spaces
        t.chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn content_hash(snippet: &str) -> String {
        let sample = &snippet[..snippet.len().min(512)];
        let mut hasher = Sha256::new();
        hasher.update(sample.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn extract_domain(url_str: &str) -> String {
        Url::parse(url_str)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_lowercase()))
            .unwrap_or_default()
    }

    /// Returns `true` if this result is a duplicate (should be filtered out).
    pub fn is_duplicate(&mut self, url: &str, title: &str, snippet: &str) -> bool {
        let normalized_url = Self::normalize_url(url);
        let normalized_title = Self::normalize_title(title);

        // 1. Exact URL duplicate
        if !self.seen_urls.insert(normalized_url.clone()) {
            return true;
        }

        // 2. Domain + title duplicate
        let domain = Self::extract_domain(url);
        if !domain.is_empty() && !normalized_title.is_empty() {
            if !self.seen_domain_titles.insert((domain, normalized_title.clone())) {
                return true;
            }
        }

        // 3. Long title match (same headline, different domain) — only for titles ≥5 words
        let word_count = normalized_title.split_whitespace().count();
        if word_count >= 5 && !self.seen_long_titles.insert(normalized_title) {
            return true;
        }

        // 4. Content hash (first 512 chars of snippet)
        if !snippet.is_empty() {
            let hash = Self::content_hash(snippet);
            if !self.seen_content_hashes.insert(hash) {
                return true;
            }
        }

        false
    }

    /// Filter a list of (url, title, snippet) tuples, returning only unique results.
    pub fn filter<T, F>(&mut self, items: Vec<T>, extract: F) -> Vec<T>
    where
        F: Fn(&T) -> (&str, &str, &str),
    {
        items
            .into_iter()
            .filter(|item| {
                let (url, title, snippet) = extract(item);
                !self.is_duplicate(url, title, snippet)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_by_url() {
        let mut d = DocumentDeduper::new();
        assert!(!d.is_duplicate("https://example.com/article", "Title", "snippet"));
        assert!(d.is_duplicate("https://www.example.com/article", "Title 2", "different"));
    }

    #[test]
    fn deduplicates_by_title() {
        let mut d = DocumentDeduper::new();
        assert!(!d.is_duplicate("https://a.com/1", "My Great Article About Rust Programming", "s1"));
        // Same title, different URL → duplicate
        assert!(d.is_duplicate("https://b.com/2", "My Great Article About Rust Programming - GitHub", "s2"));
    }

    #[test]
    fn strips_boilerplate() {
        assert_eq!(
            DocumentDeduper::normalize_title("How to use Rust - GitHub"),
            "how to use rust"
        );
    }
}
