use std::{error::Error, fmt, sync::Arc, time::Duration};

use crate::{
    error::PortError,
    ports::{AgentEnrollmentRepository, CheckResultRepository, Clock, SessionRepository},
};

const PRUNE_ROUND_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PruneReport {
    pub sessions: u64,
    pub results: u64,
    pub enrollments: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PruneOldDataError {
    Repository(PortError),
}

impl fmt::Display for PruneOldDataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for PruneOldDataError {}

pub struct PruneOldData {
    sessions: Arc<dyn SessionRepository>,
    results: Arc<dyn CheckResultRepository>,
    enrollments: Arc<dyn AgentEnrollmentRepository>,
    clock: Arc<dyn Clock>,
    retention: Duration,
    batch_size: u16,
}

impl PruneOldData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sessions: Arc<dyn SessionRepository>,
        results: Arc<dyn CheckResultRepository>,
        enrollments: Arc<dyn AgentEnrollmentRepository>,
        clock: Arc<dyn Clock>,
        retention: Duration,
        batch_size: u16,
    ) -> Self {
        Self {
            sessions,
            results,
            enrollments,
            clock,
            retention,
            batch_size: batch_size.max(1),
        }
    }

    pub async fn execute(&self) -> Result<PruneReport, PruneOldDataError> {
        let now = self.clock.now();
        let results_before = now - self.retention;
        let mut report = PruneReport::default();
        for _ in 0..PRUNE_ROUND_LIMIT {
            let removed = self
                .sessions
                .prune_expired(now, self.batch_size)
                .await
                .map_err(PruneOldDataError::Repository)?;
            report.sessions += u64::from(removed);
            if removed < self.batch_size {
                break;
            }
        }
        for _ in 0..PRUNE_ROUND_LIMIT {
            let removed = self
                .results
                .prune_before(results_before, self.batch_size)
                .await
                .map_err(PruneOldDataError::Repository)?;
            report.results += u64::from(removed);
            if removed < self.batch_size {
                break;
            }
        }
        for _ in 0..PRUNE_ROUND_LIMIT {
            let removed = self
                .enrollments
                .prune_expired(now, self.batch_size)
                .await
                .map_err(PruneOldDataError::Repository)?;
            report.enrollments += u64::from(removed);
            if removed < self.batch_size {
                break;
            }
        }
        Ok(report)
    }
}
