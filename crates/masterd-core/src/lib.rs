use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Capability {
    PdfExtraction,
    Ocr,
    DesktopUi,
    Search,
    VectorStore,
    KvCache,
    GraphStore,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectFoundation {
    pub name: String,
    pub capabilities: Vec<Capability>,
}

impl ProjectFoundation {
    pub fn rust_first() -> Self {
        Self {
            name: "MASTERd".to_string(),
            capabilities: vec![
                Capability::PdfExtraction,
                Capability::Ocr,
                Capability::DesktopUi,
                Capability::Search,
                Capability::VectorStore,
                Capability::KvCache,
                Capability::GraphStore,
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CancellationState {
    pub cancelled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<String>>>,
}

impl CancellationToken {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn reason(&self) -> Option<String> {
        self.reason.lock().ok().and_then(|reason| reason.clone())
    }

    pub fn snapshot(&self) -> CancellationState {
        CancellationState {
            cancelled: self.is_cancelled(),
            reason: self.reason(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CancellationSource {
    cancelled: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<String>>>,
}

impl CancellationSource {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            reason: Arc::new(Mutex::new(None)),
        }
    }

    pub fn token(&self) -> CancellationToken {
        CancellationToken {
            cancelled: Arc::clone(&self.cancelled),
            reason: Arc::clone(&self.reason),
        }
    }

    pub fn cancel(&self, reason: impl Into<String>) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.reason.lock() {
            if slot.is_none() {
                *slot = Some(reason.into());
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl Default for CancellationSource {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_source_propagates_reason_to_token() {
        let source = CancellationSource::new();
        let token = source.token();
        assert!(!token.is_cancelled());
        assert_eq!(token.reason(), None);

        source.cancel("shutdown_requested");
        assert!(token.is_cancelled());
        assert_eq!(token.reason().as_deref(), Some("shutdown_requested"));
    }

    #[test]
    fn first_cancellation_reason_wins() {
        let source = CancellationSource::new();
        let token = source.token();

        source.cancel("first");
        source.cancel("second");

        assert_eq!(token.reason().as_deref(), Some("first"));
    }

    #[test]
    fn project_foundation_has_all_capabilities() {
        let f = ProjectFoundation::rust_first();
        assert!(f.capabilities.contains(&Capability::PdfExtraction));
        assert!(f.capabilities.contains(&Capability::Search));
        assert!(f.capabilities.contains(&Capability::VectorStore));
    }
}
