/// Shared telemetry schema and failure taxonomy for MASTERd pipeline, ingest, and bootstrap.
///
/// Design: all types are `Serialize`/`Deserialize` for structured log emission, zero
/// dynamic allocations in the hot path for stage counters, and machine-actionable
/// failure classes that upstream observability can act on directly.
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

// ── Failure taxonomy ──────────────────────────────────────────────────────────

/// High-level failure class.  Each class maps to a distinct operator action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    /// Transient I/O error (network, disk).  Operator action: retry with back-off.
    TransientIo,
    /// Corrupt or unreadable input.  Operator action: quarantine file, continue.
    CorruptInput,
    /// Upstream dependency (DB, cache) is unreachable.  Operator action: circuit-break.
    DependencyUnavailable,
    /// Resource exhaustion (OOM, quota).  Operator action: shed load / scale.
    ResourceExhausted,
    /// Policy rejection (dedup gate, score threshold).  Not an error; expected path.
    PolicyRejected,
    /// Operator-requested cancellation.  Operator action: none (intentional stop).
    Cancelled,
    /// Internal logic invariant violated.  Operator action: page on-call.
    InternalError,
}

impl FailureClass {
    /// Returns `true` when the class represents an expected/normal rejection rather
    /// than a true failure.
    pub fn is_expected(&self) -> bool {
        matches!(self, FailureClass::PolicyRejected | FailureClass::Cancelled)
    }

    /// Returns `true` when automatic retry is safe for this class.
    pub fn is_retryable(&self) -> bool {
        matches!(self, FailureClass::TransientIo | FailureClass::DependencyUnavailable)
    }
}

/// Structured failure record emitted when a stage does not complete successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageFailureEvent {
    pub stage: String,
    pub class: FailureClass,
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub duration_ms: f64,
}

impl StageFailureEvent {
    pub fn new(
        stage: impl Into<String>,
        class: FailureClass,
        code: impl Into<String>,
        message: impl Into<String>,
        duration: Duration,
    ) -> Self {
        Self {
            retryable: class.is_retryable(),
            stage: stage.into(),
            class,
            code: code.into(),
            message: message.into(),
            duration_ms: duration.as_secs_f64() * 1000.0,
        }
    }
}

// ── Stage counter set ─────────────────────────────────────────────────────────

/// Per-pipeline-run counters.  Cheap to clone and accumulate across workers.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StageCounters {
    pub discovered: u64,
    pub ingested: u64,
    pub deduped: u64,
    pub skipped_policy: u64,
    pub failed_transient: u64,
    pub failed_corrupt: u64,
    pub failed_dependency: u64,
    pub failed_resource: u64,
    pub failed_internal: u64,
    pub cancelled: u64,
}

impl StageCounters {
    pub fn total_errors(&self) -> u64 {
        self.failed_transient
            + self.failed_corrupt
            + self.failed_dependency
            + self.failed_resource
            + self.failed_internal
    }

    pub fn total_processed(&self) -> u64 {
        self.ingested + self.deduped + self.skipped_policy
    }

    pub fn record_failure(&mut self, class: FailureClass) {
        match class {
            FailureClass::TransientIo => self.failed_transient += 1,
            FailureClass::CorruptInput => self.failed_corrupt += 1,
            FailureClass::DependencyUnavailable => self.failed_dependency += 1,
            FailureClass::ResourceExhausted => self.failed_resource += 1,
            FailureClass::InternalError => self.failed_internal += 1,
            FailureClass::PolicyRejected => self.skipped_policy += 1,
            FailureClass::Cancelled => self.cancelled += 1,
        }
    }

    /// Merge another counter set into this one (for aggregating across workers).
    pub fn merge(&mut self, other: &StageCounters) {
        self.discovered += other.discovered;
        self.ingested += other.ingested;
        self.deduped += other.deduped;
        self.skipped_policy += other.skipped_policy;
        self.failed_transient += other.failed_transient;
        self.failed_corrupt += other.failed_corrupt;
        self.failed_dependency += other.failed_dependency;
        self.failed_resource += other.failed_resource;
        self.failed_internal += other.failed_internal;
        self.cancelled += other.cancelled;
    }
}

// ── Stage duration tracker ────────────────────────────────────────────────────

/// Per-stage timing record accumulated over a pipeline run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StageDuration {
    pub stage: String,
    pub invocations: u64,
    pub total_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

impl StageDuration {
    pub fn new(stage: impl Into<String>) -> Self {
        Self {
            stage: stage.into(),
            ..Default::default()
        }
    }

    pub fn record(&mut self, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        self.invocations += 1;
        self.total_ms += ms;
        if self.invocations == 1 {
            self.min_ms = ms;
            self.max_ms = ms;
        } else {
            if ms < self.min_ms {
                self.min_ms = ms;
            }
            if ms > self.max_ms {
                self.max_ms = ms;
            }
        }
    }

    pub fn mean_ms(&self) -> f64 {
        if self.invocations == 0 {
            0.0
        } else {
            self.total_ms / self.invocations as f64
        }
    }
}

// ── Scoped stage timer (RAII) ─────────────────────────────────────────────────

/// Start timing a stage.  Call `.finish()` to obtain the elapsed `Duration`.
pub struct StageTimer {
    start: Instant,
}

impl StageTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn finish(self) -> Duration {
        self.start.elapsed()
    }
}

// ── Pipeline telemetry report ─────────────────────────────────────────────────

/// Full telemetry summary for a completed (or interrupted) pipeline run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PipelineTelemetryReport {
    pub run_id: String,
    pub counters: StageCounters,
    pub stage_durations: Vec<StageDuration>,
    pub failures: Vec<StageFailureEvent>,
    pub total_wall_ms: f64,
}

impl PipelineTelemetryReport {
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            ..Default::default()
        }
    }

    pub fn add_failure(&mut self, event: StageFailureEvent) {
        self.counters.record_failure(event.class);
        self.failures.push(event);
    }

    pub fn upsert_stage_duration(&mut self, stage: &str, duration: Duration) {
        if let Some(slot) = self.stage_durations.iter_mut().find(|s| s.stage == stage) {
            slot.record(duration);
        } else {
            let mut slot = StageDuration::new(stage);
            slot.record(duration);
            self.stage_durations.push(slot);
        }
    }

    pub fn has_hard_failures(&self) -> bool {
        self.counters.total_errors() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_class_retryability() {
        assert!(FailureClass::TransientIo.is_retryable());
        assert!(FailureClass::DependencyUnavailable.is_retryable());
        assert!(!FailureClass::CorruptInput.is_retryable());
        assert!(!FailureClass::InternalError.is_retryable());
    }

    #[test]
    fn failure_class_expected() {
        assert!(FailureClass::PolicyRejected.is_expected());
        assert!(FailureClass::Cancelled.is_expected());
        assert!(!FailureClass::TransientIo.is_expected());
    }

    #[test]
    fn stage_counters_merge() {
        let mut a = StageCounters::default();
        a.ingested = 5;
        a.failed_transient = 1;

        let mut b = StageCounters::default();
        b.ingested = 3;
        b.deduped = 2;

        a.merge(&b);
        assert_eq!(a.ingested, 8);
        assert_eq!(a.deduped, 2);
        assert_eq!(a.failed_transient, 1);
    }

    #[test]
    fn stage_duration_mean() {
        let mut d = StageDuration::new("hot_cache");
        d.record(Duration::from_millis(10));
        d.record(Duration::from_millis(20));
        d.record(Duration::from_millis(30));
        assert!((d.mean_ms() - 20.0).abs() < 0.01);
        assert!((d.min_ms - 10.0).abs() < 0.01);
        assert!((d.max_ms - 30.0).abs() < 0.01);
    }

    #[test]
    fn telemetry_report_has_hard_failures() {
        let mut report = PipelineTelemetryReport::new("test-run-1");
        assert!(!report.has_hard_failures());

        report.add_failure(StageFailureEvent::new(
            "hot_cache",
            FailureClass::TransientIo,
            "io_error",
            "disk full",
            Duration::from_millis(5),
        ));
        assert!(report.has_hard_failures());
        assert_eq!(report.counters.failed_transient, 1);
    }

    #[test]
    fn telemetry_report_policy_rejection_not_hard_failure() {
        let mut report = PipelineTelemetryReport::new("test-run-2");
        report.add_failure(StageFailureEvent::new(
            "dedup",
            FailureClass::PolicyRejected,
            "exact_hash_duplicate",
            "already seen",
            Duration::from_millis(1),
        ));
        assert!(!report.has_hard_failures());
        assert_eq!(report.counters.skipped_policy, 1);
    }
}
