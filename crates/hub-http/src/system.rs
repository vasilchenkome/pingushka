use std::time::Duration;

use pingushka_application::ports::Clock;
use pingushka_domain::Timestamp;
use time::OffsetDateTime;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        OffsetDateTime::now_utc()
    }
}

pub fn unix_seconds(timestamp: Timestamp) -> i64 {
    timestamp.unix_timestamp()
}

pub fn from_unix_seconds(seconds: i64) -> Option<Timestamp> {
    OffsetDateTime::from_unix_timestamp(seconds).ok()
}

pub fn rfc3339(timestamp: Timestamp) -> String {
    timestamp
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub fn hours(hours: u64) -> Duration {
    Duration::from_secs(hours.saturating_mul(3_600))
}

pub fn days(days: u64) -> Duration {
    Duration::from_secs(days.saturating_mul(86_400))
}
