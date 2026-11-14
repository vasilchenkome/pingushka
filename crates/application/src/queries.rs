use std::{error::Error, fmt};

use pingushka_domain::{
    Timestamp,
    identity::{AgentId, TargetId},
    target::Target,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryError {
    InvalidTimeRange,
    InvalidLimit,
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidTimeRange => "query start must be before query end",
            Self::InvalidLimit => "history limit must be between 1 and 1000",
        };
        formatter.write_str(message)
    }
}

impl Error for QueryError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryFilter {
    target_id: TargetId,
    agent_id: Option<AgentId>,
    from: Timestamp,
    to: Timestamp,
    limit: u16,
}

impl HistoryFilter {
    pub fn new(
        target_id: TargetId,
        agent_id: Option<AgentId>,
        from: Timestamp,
        to: Timestamp,
        limit: u16,
    ) -> Result<Self, QueryError> {
        if from >= to {
            return Err(QueryError::InvalidTimeRange);
        }
        if !(1..=1000).contains(&limit) {
            return Err(QueryError::InvalidLimit);
        }
        Ok(Self {
            target_id,
            agent_id,
            from,
            to,
            limit,
        })
    }

    pub const fn target_id(self) -> TargetId {
        self.target_id
    }

    pub const fn agent_id(self) -> Option<AgentId> {
        self.agent_id
    }

    pub const fn from(self) -> Timestamp {
        self.from
    }

    pub const fn to(self) -> Timestamp {
        self.to
    }

    pub const fn limit(self) -> u16 {
        self.limit
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SummaryFilter {
    target_id: Option<TargetId>,
    agent_id: Option<AgentId>,
    from: Timestamp,
    to: Timestamp,
}

impl SummaryFilter {
    pub fn new(
        target_id: Option<TargetId>,
        agent_id: Option<AgentId>,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Self, QueryError> {
        if from >= to {
            return Err(QueryError::InvalidTimeRange);
        }
        Ok(Self {
            target_id,
            agent_id,
            from,
            to,
        })
    }

    pub const fn target_id(self) -> Option<TargetId> {
        self.target_id
    }

    pub const fn agent_id(self) -> Option<AgentId> {
        self.agent_id
    }

    pub const fn from(self) -> Timestamp {
        self.from
    }

    pub const fn to(self) -> Timestamp {
        self.to
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CheckSummary {
    total_checks: u64,
    successful_checks: u64,
    average_latency_ms: Option<f64>,
    p95_latency_ms: Option<f64>,
}

impl CheckSummary {
    pub const fn new(
        total_checks: u64,
        successful_checks: u64,
        average_latency_ms: Option<f64>,
        p95_latency_ms: Option<f64>,
    ) -> Self {
        Self {
            total_checks,
            successful_checks,
            average_latency_ms,
            p95_latency_ms,
        }
    }

    pub const fn total_checks(self) -> u64 {
        self.total_checks
    }

    pub const fn successful_checks(self) -> u64 {
        self.successful_checks
    }

    pub const fn average_latency_ms(self) -> Option<f64> {
        self.average_latency_ms
    }

    pub const fn p95_latency_ms(self) -> Option<f64> {
        self.p95_latency_ms
    }

    pub fn uptime_percent(self) -> Option<f64> {
        if self.total_checks == 0 {
            return None;
        }
        Some(self.successful_checks as f64 * 100.0 / self.total_checks as f64)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetWithAgents {
    target: Target,
    agent_ids: Vec<AgentId>,
}

impl TargetWithAgents {
    pub fn new(target: Target, agent_ids: Vec<AgentId>) -> Self {
        Self { target, agent_ids }
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn agent_ids(&self) -> &[AgentId] {
        &self.agent_ids
    }

    pub fn into_parts(self) -> (Target, Vec<AgentId>) {
        (self.target, self.agent_ids)
    }
}
