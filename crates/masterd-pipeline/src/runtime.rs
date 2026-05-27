use std::str::FromStr;

use masterd_core::{CancellationSource, CancellationToken};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum IngestStage {
    HotCache,
    Dedup,
    CanonicalWrite,
    Snapshot,
    ColbertRerank,
    LexicalAnalyze,
    MultimodalEmbed,
    FalkorMirror,
}

impl IngestStage {
    pub const DEFAULT_ORDER: [Self; 8] = [
        Self::HotCache,
        Self::Dedup,
        Self::CanonicalWrite,
        Self::Snapshot,
        Self::ColbertRerank,
        Self::LexicalAnalyze,
        Self::MultimodalEmbed,
        Self::FalkorMirror,
    ];
}

impl FromStr for IngestStage {
    type Err = IngestStageConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hot_cache" => Ok(Self::HotCache),
            "dedup" => Ok(Self::Dedup),
            "canonical_write" => Ok(Self::CanonicalWrite),
            "snapshot" => Ok(Self::Snapshot),
            "colbert_rerank" => Ok(Self::ColbertRerank),
            "lexical_analyze" => Ok(Self::LexicalAnalyze),
            "multimodal_embed" => Ok(Self::MultimodalEmbed),
            "falkor_mirror" => Ok(Self::FalkorMirror),
            other => Err(IngestStageConfigError::UnknownStage(other.to_string())),
        }
    }
}

#[derive(Debug, Error)]
pub enum IngestStageConfigError {
    #[error("unknown ingest stage `{0}`")]
    UnknownStage(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageInputEnvelope<T> {
    pub stage: IngestStage,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageOutputEnvelope<T> {
    pub stage: IngestStage,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageFailure {
    pub stage: IngestStage,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageCancellation {
    pub stage: IngestStage,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageResult<T> {
    Success(T),
    RetryableFailure(StageFailure),
    NonRetryableFailure(StageFailure),
    Cancelled(StageCancellation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageTransitionState {
    Success,
    RetryableFailure,
    NonRetryableFailure,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageTransition {
    pub stage: IngestStage,
    pub state: StageTransitionState,
}

#[derive(Debug, Clone)]
pub struct IngestStageOrder {
    stages: Vec<IngestStage>,
}

impl IngestStageOrder {
    pub fn default_order() -> Self {
        Self {
            stages: IngestStage::DEFAULT_ORDER.to_vec(),
        }
    }

    pub fn from_config<I>(configured: I) -> Self
    where
        I: IntoIterator<Item = IngestStage>,
    {
        let mut stages = Vec::new();
        for stage in configured {
            if !stages.contains(&stage) {
                stages.push(stage);
            }
        }
        for stage in IngestStage::DEFAULT_ORDER {
            if !stages.contains(&stage) {
                stages.push(stage);
            }
        }
        Self { stages }
    }

    pub fn stages(&self) -> &[IngestStage] {
        &self.stages
    }
}

impl Default for IngestStageOrder {
    fn default() -> Self {
        Self::default_order()
    }
}

pub trait IngestStageExecutor {
    type Context;

    fn execute_stage(
        &self,
        input: StageInputEnvelope<&mut Self::Context>,
    ) -> StageResult<StageOutputEnvelope<()>>;
}

pub trait StageRollbackHook<C> {
    fn rollback_stage(&mut self, stage: IngestStage, context: &mut C) -> Result<(), StageFailure>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopStageRollbackHook;

impl<C> StageRollbackHook<C> for NoopStageRollbackHook {
    fn rollback_stage(
        &mut self,
        _stage: IngestStage,
        _context: &mut C,
    ) -> Result<(), StageFailure> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeExecutionOutcome<C> {
    pub transitions: Vec<StageTransition>,
    pub result: StageResult<C>,
}

#[derive(Debug, Clone, Default)]
pub struct IngestStageRuntime {
    order: IngestStageOrder,
}

impl IngestStageRuntime {
    pub fn new(order: IngestStageOrder) -> Self {
        Self { order }
    }

    pub fn order(&self) -> &IngestStageOrder {
        &self.order
    }

    pub fn execute<E>(
        &self,
        executor: &E,
        context: E::Context,
    ) -> RuntimeExecutionOutcome<E::Context>
    where
        E: IngestStageExecutor,
    {
        let cancellation = CancellationSource::new();
        let token = cancellation.token();
        self.execute_with_control(executor, context, &token, &mut NoopStageRollbackHook)
    }

    pub fn execute_with_control<E, R>(
        &self,
        executor: &E,
        mut context: E::Context,
        cancellation: &CancellationToken,
        rollback: &mut R,
    ) -> RuntimeExecutionOutcome<E::Context>
    where
        E: IngestStageExecutor,
        R: StageRollbackHook<E::Context>,
    {
        let mut transitions = Vec::new();
        let mut completed_stages = Vec::new();
        for &stage in self.order.stages() {
            if cancellation.is_cancelled() {
                transitions.push(StageTransition {
                    stage,
                    state: StageTransitionState::Cancelled,
                });
                if let Some(err) = rollback_stages(
                    rollback,
                    &mut context,
                    std::iter::once(stage).chain(completed_stages.iter().rev().copied()),
                ) {
                    transitions.push(StageTransition {
                        stage: err.stage,
                        state: StageTransitionState::NonRetryableFailure,
                    });
                    return RuntimeExecutionOutcome {
                        transitions,
                        result: StageResult::NonRetryableFailure(err),
                    };
                }
                return RuntimeExecutionOutcome {
                    transitions,
                    result: StageResult::Cancelled(StageCancellation {
                        stage,
                        reason: cancellation
                            .reason()
                            .unwrap_or_else(|| "cancellation requested".to_string()),
                    }),
                };
            }
            let input = StageInputEnvelope {
                stage,
                payload: &mut context,
            };
            match executor.execute_stage(input) {
                StageResult::Success(_) => {
                    transitions.push(StageTransition {
                        stage,
                        state: StageTransitionState::Success,
                    });
                    completed_stages.push(stage);
                }
                StageResult::RetryableFailure(err) => {
                    transitions.push(StageTransition {
                        stage,
                        state: StageTransitionState::RetryableFailure,
                    });
                    if let Some(rollback_err) = rollback_stages(
                        rollback,
                        &mut context,
                        std::iter::once(stage).chain(completed_stages.iter().rev().copied()),
                    ) {
                        transitions.push(StageTransition {
                            stage: rollback_err.stage,
                            state: StageTransitionState::NonRetryableFailure,
                        });
                        return RuntimeExecutionOutcome {
                            transitions,
                            result: StageResult::NonRetryableFailure(rollback_err),
                        };
                    }
                    return RuntimeExecutionOutcome {
                        transitions,
                        result: StageResult::RetryableFailure(err),
                    };
                }
                StageResult::NonRetryableFailure(err) => {
                    transitions.push(StageTransition {
                        stage,
                        state: StageTransitionState::NonRetryableFailure,
                    });
                    if let Some(rollback_err) = rollback_stages(
                        rollback,
                        &mut context,
                        std::iter::once(stage).chain(completed_stages.iter().rev().copied()),
                    ) {
                        transitions.push(StageTransition {
                            stage: rollback_err.stage,
                            state: StageTransitionState::NonRetryableFailure,
                        });
                        return RuntimeExecutionOutcome {
                            transitions,
                            result: StageResult::NonRetryableFailure(rollback_err),
                        };
                    }
                    return RuntimeExecutionOutcome {
                        transitions,
                        result: StageResult::NonRetryableFailure(err),
                    };
                }
                StageResult::Cancelled(cancelled) => {
                    transitions.push(StageTransition {
                        stage,
                        state: StageTransitionState::Cancelled,
                    });
                    if let Some(rollback_err) = rollback_stages(
                        rollback,
                        &mut context,
                        std::iter::once(cancelled.stage)
                            .chain(completed_stages.iter().rev().copied()),
                    ) {
                        transitions.push(StageTransition {
                            stage: rollback_err.stage,
                            state: StageTransitionState::NonRetryableFailure,
                        });
                        return RuntimeExecutionOutcome {
                            transitions,
                            result: StageResult::NonRetryableFailure(rollback_err),
                        };
                    }
                    return RuntimeExecutionOutcome {
                        transitions,
                        result: StageResult::Cancelled(cancelled),
                    };
                }
            }
        }

        RuntimeExecutionOutcome {
            transitions,
            result: StageResult::Success(context),
        }
    }
}

fn rollback_stages<C, I, R>(rollback: &mut R, context: &mut C, stages: I) -> Option<StageFailure>
where
    I: IntoIterator<Item = IngestStage>,
    R: StageRollbackHook<C>,
{
    for stage in stages {
        if let Err(err) = rollback.rollback_stage(stage, context) {
            return Some(err);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use masterd_core::CancellationSource;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Debug, Default)]
    struct FakeContext {
        visited: Vec<IngestStage>,
    }

    struct FakeExecutor {
        fail: Option<(IngestStage, StageResult<StageOutputEnvelope<()>>)>,
        calls: RefCell<Vec<IngestStage>>,
    }

    #[derive(Clone, Default)]
    struct RecordingRollback {
        calls: Rc<RefCell<Vec<IngestStage>>>,
    }

    impl RecordingRollback {
        fn take(&self) -> Vec<IngestStage> {
            self.calls.borrow().clone()
        }
    }

    impl StageRollbackHook<FakeContext> for RecordingRollback {
        fn rollback_stage(
            &mut self,
            stage: IngestStage,
            _context: &mut FakeContext,
        ) -> Result<(), StageFailure> {
            self.calls.borrow_mut().push(stage);
            Ok(())
        }
    }

    impl FakeExecutor {
        fn new(fail: Option<(IngestStage, StageResult<StageOutputEnvelope<()>>)>) -> Self {
            Self {
                fail,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl IngestStageExecutor for FakeExecutor {
        type Context = FakeContext;

        fn execute_stage(
            &self,
            input: StageInputEnvelope<&mut Self::Context>,
        ) -> StageResult<StageOutputEnvelope<()>> {
            self.calls.borrow_mut().push(input.stage);
            input.payload.visited.push(input.stage);
            if let Some((fail_stage, ref fail_result)) = self.fail {
                if fail_stage == input.stage {
                    return fail_result.clone();
                }
            }
            StageResult::Success(StageOutputEnvelope {
                stage: input.stage,
                payload: (),
            })
        }
    }

    #[test]
    fn stage_order_is_stable_and_deterministic() {
        let order = IngestStageOrder::from_config([
            IngestStage::LexicalAnalyze,
            IngestStage::HotCache,
            IngestStage::LexicalAnalyze,
        ]);

        assert_eq!(
            order.stages(),
            &[
                IngestStage::LexicalAnalyze,
                IngestStage::HotCache,
                IngestStage::Dedup,
                IngestStage::CanonicalWrite,
                IngestStage::Snapshot,
                IngestStage::ColbertRerank,
                IngestStage::MultimodalEmbed,
                IngestStage::FalkorMirror,
            ]
        );
    }

    #[test]
    fn runtime_returns_retryable_transition() {
        let runtime = IngestStageRuntime::new(IngestStageOrder::from_config([
            IngestStage::HotCache,
            IngestStage::Dedup,
            IngestStage::Snapshot,
        ]));

        let executor = FakeExecutor::new(Some((
            IngestStage::Dedup,
            StageResult::RetryableFailure(StageFailure {
                stage: IngestStage::Dedup,
                message: "temporary lock".to_string(),
            }),
        )));

        let out = runtime.execute(&executor, FakeContext::default());
        assert_eq!(
            out.transitions,
            vec![
                StageTransition {
                    stage: IngestStage::HotCache,
                    state: StageTransitionState::Success,
                },
                StageTransition {
                    stage: IngestStage::Dedup,
                    state: StageTransitionState::RetryableFailure,
                },
            ]
        );
        assert!(matches!(out.result, StageResult::RetryableFailure(_)));
    }

    #[test]
    fn runtime_returns_cancelled_transition() {
        let runtime = IngestStageRuntime::new(IngestStageOrder::from_config([
            IngestStage::HotCache,
            IngestStage::Dedup,
            IngestStage::CanonicalWrite,
        ]));

        let executor = FakeExecutor::new(Some((
            IngestStage::Dedup,
            StageResult::Cancelled(StageCancellation {
                stage: IngestStage::Dedup,
                reason: "duplicate".to_string(),
            }),
        )));

        let out = runtime.execute(&executor, FakeContext::default());
        assert_eq!(
            out.transitions,
            vec![
                StageTransition {
                    stage: IngestStage::HotCache,
                    state: StageTransitionState::Success,
                },
                StageTransition {
                    stage: IngestStage::Dedup,
                    state: StageTransitionState::Cancelled,
                },
            ]
        );
        assert!(matches!(out.result, StageResult::Cancelled(_)));
    }

    #[test]
    fn runtime_cooperatively_cancels_before_stage_execution() {
        let runtime = IngestStageRuntime::new(IngestStageOrder::default());
        let executor = FakeExecutor::new(None);
        let mut rollback = RecordingRollback::default();
        let cancel = CancellationSource::new();
        cancel.cancel("operator_requested");

        let out = runtime.execute_with_control(
            &executor,
            FakeContext::default(),
            &cancel.token(),
            &mut rollback,
        );
        assert!(matches!(
            out.result,
            StageResult::Cancelled(StageCancellation { reason, .. }) if reason == "operator_requested"
        ));
        assert_eq!(out.transitions[0].state, StageTransitionState::Cancelled);
        assert_eq!(executor.calls.borrow().len(), 0);
        assert_eq!(rollback.take(), vec![IngestStage::HotCache]);
    }

    #[test]
    fn runtime_rolls_back_completed_stages_after_cancelled_stage() {
        let runtime = IngestStageRuntime::new(IngestStageOrder::from_config([
            IngestStage::HotCache,
            IngestStage::Dedup,
            IngestStage::CanonicalWrite,
        ]));
        let executor = FakeExecutor::new(Some((
            IngestStage::Dedup,
            StageResult::Cancelled(StageCancellation {
                stage: IngestStage::Dedup,
                reason: "explicit_cancel".to_string(),
            }),
        )));
        let mut rollback = RecordingRollback::default();
        let cancel = CancellationSource::new();

        let out = runtime.execute_with_control(
            &executor,
            FakeContext::default(),
            &cancel.token(),
            &mut rollback,
        );
        assert!(matches!(out.result, StageResult::Cancelled(_)));
        assert_eq!(
            rollback.take(),
            vec![IngestStage::Dedup, IngestStage::HotCache]
        );
    }
}
