/// Rust-first event contract for MASTERd UI shells (Tauri / Iced).
///
/// This crate defines the canonical set of events emitted by the backend
/// pipeline and commands accepted from the frontend.  Both Tauri and Iced
/// frontends consume/emit these types — all are `Serialize`/`Deserialize`
/// so Tauri's invoke bridge can use them directly.
///
/// Design principles:
/// - Events are namespaced under `masterd://` for Tauri routing.
/// - All user-facing state transitions flow through `ReviewQueueEvent`.
/// - Operator control (cancel/pause/resume) flows through `OperatorCommand`.
/// - Correction loop (user accepting/rejecting results) flows through
///   `CorrectionEvent`.
/// - All events carry a monotonic `sequence` number for reliable ordering.
use serde::{Deserialize, Serialize};

// ── Shared envelope ───────────────────────────────────────────────────────────

/// Every event payload is wrapped in this envelope for reliable ordering and
/// tracing across the frontend/backend boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    /// Monotonic counter per-session.  Frontend uses this to detect lost events.
    pub sequence: u64,
    /// ISO-8601 timestamp string (UTC).
    pub timestamp: String,
    pub payload: T,
}

impl<T: Serialize> EventEnvelope<T> {
    pub fn new(sequence: u64, timestamp: impl Into<String>, payload: T) -> Self {
        Self {
            sequence,
            timestamp: timestamp.into(),
            payload,
        }
    }
}

// ── Review queue events ───────────────────────────────────────────────────────

/// Events emitted by the pipeline as documents flow through the review queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReviewQueueEvent {
    /// A new document entered the review queue.
    DocumentQueued {
        doc_id: String,
        path: String,
        content_hash: String,
        route: String,
        tags: Vec<String>,
    },
    /// A document was successfully ingested and is ready for review.
    DocumentReady {
        doc_id: String,
        canonical_name: String,
        route: String,
        ingest_duration_ms: f64,
    },
    /// A document was rejected by the dedup/policy gate.
    DocumentRejected {
        doc_id: String,
        reason: String,
        code: String,
    },
    /// A document failed processing (retryable or not).
    DocumentFailed {
        doc_id: String,
        stage: String,
        retryable: bool,
        error: String,
    },
    /// The entire queue run finished.
    QueueCompleted {
        total_discovered: u64,
        total_ingested: u64,
        total_rejected: u64,
        total_failed: u64,
        wall_ms: f64,
    },
    /// Queue run was cancelled by an operator command.
    QueueCancelled {
        reason: String,
        processed_so_far: u64,
    },
}

impl ReviewQueueEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            ReviewQueueEvent::DocumentQueued { .. } => "masterd://queue/document_queued",
            ReviewQueueEvent::DocumentReady { .. } => "masterd://queue/document_ready",
            ReviewQueueEvent::DocumentRejected { .. } => "masterd://queue/document_rejected",
            ReviewQueueEvent::DocumentFailed { .. } => "masterd://queue/document_failed",
            ReviewQueueEvent::QueueCompleted { .. } => "masterd://queue/completed",
            ReviewQueueEvent::QueueCancelled { .. } => "masterd://queue/cancelled",
        }
    }
}

// ── Correction loop events ────────────────────────────────────────────────────

/// Events emitted when a human operator corrects or validates a pipeline result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CorrectionEvent {
    /// Operator accepted the canonical name and route assigned by the pipeline.
    Accepted {
        doc_id: String,
        operator_id: String,
    },
    /// Operator overrode the canonical name.
    NameOverridden {
        doc_id: String,
        original_name: String,
        new_name: String,
        operator_id: String,
    },
    /// Operator moved the document to a different route.
    RouteOverridden {
        doc_id: String,
        original_route: String,
        new_route: String,
        operator_id: String,
    },
    /// Operator flagged the document for manual review.
    FlaggedForReview {
        doc_id: String,
        reason: String,
        operator_id: String,
    },
    /// Operator deleted the document from the review queue.
    Deleted {
        doc_id: String,
        operator_id: String,
    },
}

impl CorrectionEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            CorrectionEvent::Accepted { .. } => "masterd://correction/accepted",
            CorrectionEvent::NameOverridden { .. } => "masterd://correction/name_overridden",
            CorrectionEvent::RouteOverridden { .. } => "masterd://correction/route_overridden",
            CorrectionEvent::FlaggedForReview { .. } => "masterd://correction/flagged",
            CorrectionEvent::Deleted { .. } => "masterd://correction/deleted",
        }
    }

    pub fn doc_id(&self) -> &str {
        match self {
            CorrectionEvent::Accepted { doc_id, .. }
            | CorrectionEvent::NameOverridden { doc_id, .. }
            | CorrectionEvent::RouteOverridden { doc_id, .. }
            | CorrectionEvent::FlaggedForReview { doc_id, .. }
            | CorrectionEvent::Deleted { doc_id, .. } => doc_id.as_str(),
        }
    }
}

// ── Operator commands ─────────────────────────────────────────────────────────

/// Commands sent from the UI to the backend pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperatorCommand {
    /// Cancel the current pipeline run.
    Cancel { reason: String },
    /// Pause the pipeline (drain in-flight, stop accepting new work).
    Pause,
    /// Resume a paused pipeline.
    Resume,
    /// Request a full status report from the backend.
    StatusRequest,
    /// Request that a specific document be re-processed.
    RetryDocument { doc_id: String },
    /// Open a specific document path for preview.
    OpenPreview { path: String },
}

// ── Status report (backend → frontend) ───────────────────────────────────────

/// Periodic status snapshot sent from the backend to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatusReport {
    pub state: PipelineState,
    pub queue_depth: u64,
    pub ingested: u64,
    pub rejected: u64,
    pub failed: u64,
    pub active_stage: Option<String>,
    pub current_doc: Option<String>,
    pub wall_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineState {
    Idle,
    Running,
    Paused,
    Cancelled,
    Completed,
    Error,
}

impl PipelineStatusReport {
    pub fn idle() -> Self {
        Self {
            state: PipelineState::Idle,
            queue_depth: 0,
            ingested: 0,
            rejected: 0,
            failed: 0,
            active_stage: None,
            current_doc: None,
            wall_ms: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_are_namespaced() {
        let ev = ReviewQueueEvent::DocumentQueued {
            doc_id: "d1".to_string(),
            path: "/tmp/a.pdf".to_string(),
            content_hash: "abcd".to_string(),
            route: "documents/pdf".to_string(),
            tags: vec![],
        };
        assert!(ev.event_name().starts_with("masterd://"));
    }

    #[test]
    fn correction_event_doc_id() {
        let ev = CorrectionEvent::Accepted {
            doc_id: "doc42".to_string(),
            operator_id: "admin".to_string(),
        };
        assert_eq!(ev.doc_id(), "doc42");
    }

    #[test]
    fn operator_command_roundtrip() {
        let cmd = OperatorCommand::Cancel {
            reason: "user requested".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let decoded: OperatorCommand = serde_json::from_str(&json).unwrap();
        if let OperatorCommand::Cancel { reason } = decoded {
            assert_eq!(reason, "user requested");
        } else {
            panic!("unexpected variant");
        }
    }

    #[test]
    fn pipeline_status_report_idle() {
        let report = PipelineStatusReport::idle();
        assert_eq!(report.state, PipelineState::Idle);
        assert_eq!(report.queue_depth, 0);
    }

    #[test]
    fn event_envelope_sequence() {
        let env1 = EventEnvelope::new(1, "2026-01-01T00:00:00Z", PipelineState::Running);
        let env2 = EventEnvelope::new(2, "2026-01-01T00:00:01Z", PipelineState::Completed);
        assert!(env2.sequence > env1.sequence);
    }
}
