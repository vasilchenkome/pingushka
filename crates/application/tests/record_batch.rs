use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use pingushka_application::{
    error::PortError,
    ports::CheckResultRepository,
    use_cases::{RecordCheckBatch, RecordCheckBatchError},
};
use pingushka_domain::{
    Timestamp,
    check::{CheckMode, CheckOutcome, CheckResult, LatencyMs},
    identity::{AgentId, CheckResultId, TargetId},
};

#[derive(Default)]
struct RecordingRepository {
    calls: AtomicUsize,
}

#[async_trait]
impl CheckResultRepository for RecordingRepository {
    async fn record_batch(&self, _results: &[CheckResult]) -> Result<usize, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(1)
    }

    async fn history(
        &self,
        _filter: pingushka_application::queries::HistoryFilter,
    ) -> Result<Vec<CheckResult>, PortError> {
        Ok(Vec::new())
    }

    async fn summarize(
        &self,
        _filter: pingushka_application::queries::SummaryFilter,
    ) -> Result<pingushka_application::queries::CheckSummary, PortError> {
        Ok(Default::default())
    }

    async fn prune_before(&self, _before: Timestamp, _limit: u16) -> Result<u16, PortError> {
        Ok(0)
    }
}

fn result() -> CheckResult {
    let outcome = CheckOutcome::new(true, LatencyMs::new(12), None, CheckMode::Http, None)
        .expect("valid outcome");
    CheckResult::new(
        CheckResultId::new(),
        TargetId::new(),
        AgentId::new(),
        Timestamp::from_unix_timestamp(1_700_000_000).expect("valid timestamp"),
        outcome,
    )
}

#[tokio::test]
async fn empty_batch_is_rejected_without_calling_repository() {
    let repository = Arc::new(RecordingRepository::default());
    let use_case = RecordCheckBatch::new(repository.clone());

    let outcome = use_case.execute(Vec::new()).await;

    assert!(matches!(
        outcome,
        Err(RecordCheckBatchError::InvalidBatchSize)
    ));
    assert_eq!(repository.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn oversized_batch_is_rejected_without_calling_repository() {
    let repository = Arc::new(RecordingRepository::default());
    let use_case = RecordCheckBatch::new(repository.clone());

    let outcome = use_case.execute(vec![result(); 101]).await;

    assert!(matches!(
        outcome,
        Err(RecordCheckBatchError::InvalidBatchSize)
    ));
    assert_eq!(repository.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn valid_batch_is_recorded_once() {
    let repository = Arc::new(RecordingRepository::default());
    let use_case = RecordCheckBatch::new(repository.clone());

    let recorded = use_case.execute(vec![result(), result()]).await;

    assert_eq!(recorded.unwrap(), 1);
    assert_eq!(repository.calls.load(Ordering::SeqCst), 1);
}
