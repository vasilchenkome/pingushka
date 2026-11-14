use pingushka_domain::{
    Timestamp,
    check::{
        CheckFailure, CheckFailureKind, CheckMode, CheckOutcome, CheckResult, LatencyMs, StatusCode,
    },
    identity::{AgentId, CheckResultId, TargetId},
};

fn checked_at() -> Timestamp {
    Timestamp::from_unix_timestamp(1_700_000_000).expect("valid test timestamp")
}

#[test]
fn successful_result_keeps_observation_values() {
    let outcome = CheckOutcome::new(
        true,
        LatencyMs::new(42),
        Some(StatusCode::new(204).unwrap()),
        CheckMode::Http,
        None,
    )
    .expect("consistent outcome");
    let result = CheckResult::new(
        CheckResultId::new(),
        TargetId::new(),
        AgentId::new(),
        checked_at(),
        outcome,
    );

    assert!(result.is_successful());
    assert_eq!(result.latency().get(), 42);
    assert_eq!(result.status_code().unwrap().get(), 204);
    assert_eq!(result.mode(), CheckMode::Http);
}

#[test]
fn successful_outcome_cannot_contain_failure_details() {
    let failure =
        CheckFailure::new(CheckFailureKind::Timeout, "probe timed out").expect("valid failure");
    let outcome = CheckOutcome::new(
        true,
        LatencyMs::new(100),
        None,
        CheckMode::Tcp,
        Some(failure),
    );

    assert!(outcome.is_err());
}

#[test]
fn failed_result_can_record_failure_without_http_status() {
    let failure = CheckFailure::new(CheckFailureKind::Connection, "connection refused")
        .expect("valid failure");
    let outcome = CheckOutcome::new(
        false,
        LatencyMs::new(3),
        None,
        CheckMode::Tcp,
        Some(failure),
    )
    .expect("consistent outcome");
    let result = CheckResult::new(
        CheckResultId::new(),
        TargetId::new(),
        AgentId::new(),
        checked_at(),
        outcome,
    );

    assert!(!result.is_successful());
    assert_eq!(result.failure().unwrap().message(), "connection refused");
}
