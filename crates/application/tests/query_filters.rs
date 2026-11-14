use pingushka_application::queries::{HistoryFilter, QueryError};
use pingushka_domain::{
    Timestamp,
    identity::{AgentId, TargetId},
};

fn timestamp(seconds: i64) -> Timestamp {
    Timestamp::from_unix_timestamp(seconds).expect("valid timestamp")
}

#[test]
fn history_filter_requires_a_valid_range_and_bounded_limit() {
    let from = timestamp(1_700_000_000);
    let to = timestamp(1_700_000_001);
    assert!(HistoryFilter::new(TargetId::new(), None, from, to, 100).is_ok());
    assert_eq!(
        HistoryFilter::new(TargetId::new(), None, from, from, 100).unwrap_err(),
        QueryError::InvalidTimeRange
    );
    assert_eq!(
        HistoryFilter::new(TargetId::new(), None, from, to, 0).unwrap_err(),
        QueryError::InvalidLimit
    );
    assert!(HistoryFilter::new(TargetId::new(), Some(AgentId::new()), from, to, 1000,).is_ok());
    assert_eq!(
        HistoryFilter::new(TargetId::new(), None, from, to, 1001).unwrap_err(),
        QueryError::InvalidLimit
    );
}
