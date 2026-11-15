use std::{error::Error, fmt};

use async_trait::async_trait;
use pingushka_domain::{
    Timestamp,
    agent::Agent,
    check::CheckResult,
    enrollment::AgentEnrollment,
    identity::{AgentId, TargetGroupId, TargetId, TokenDigest, UserId},
    organization::{TargetGroup, TargetGroupName, TargetGroupWithCounts, TargetOrganization},
    target::Target,
    user::{PasswordHash, Session, User, Username},
};

use crate::{
    error::PortError,
    queries::{CheckSummary, HistoryFilter, SummaryFilter, TargetWithAgents},
};

#[async_trait]
pub trait HealthCheck: Send + Sync {
    async fn check(&self) -> Result<(), PortError>;
}

#[async_trait]
pub trait CheckResultRepository: Send + Sync {
    async fn record_batch(&self, results: &[CheckResult]) -> Result<usize, PortError>;
    async fn history(&self, filter: HistoryFilter) -> Result<Vec<CheckResult>, PortError>;
    async fn summarize(&self, filter: SummaryFilter) -> Result<CheckSummary, PortError>;
    async fn prune_before(&self, before: Timestamp, limit: u16) -> Result<u16, PortError>;
}

#[async_trait]
pub trait AgentRepository: Send + Sync {
    async fn save(&self, agent: &Agent) -> Result<(), PortError>;
    async fn find(&self, id: AgentId) -> Result<Option<Agent>, PortError>;
    async fn find_by_token_digest(&self, digest: TokenDigest) -> Result<Option<Agent>, PortError>;
    async fn list(&self) -> Result<Vec<Agent>, PortError>;
    async fn delete(&self, id: AgentId) -> Result<bool, PortError>;
    async fn mark_connected(
        &self,
        id: AgentId,
        version: &str,
        now: Timestamp,
    ) -> Result<(), PortError>;
    async fn mark_disconnected(&self, id: AgentId) -> Result<(), PortError>;
}

#[async_trait]
pub trait AgentEnrollmentRepository: Send + Sync {
    async fn save(&self, enrollment: &AgentEnrollment) -> Result<(), PortError>;
    async fn consume(
        &self,
        digest: TokenDigest,
        now: Timestamp,
    ) -> Result<Option<AgentId>, PortError>;
    async fn prune_expired(&self, now: Timestamp, limit: u16) -> Result<u16, PortError>;
}

#[async_trait]
pub trait TargetRepository: Send + Sync {
    async fn save(&self, target: &Target) -> Result<(), PortError>;
    async fn find(&self, id: TargetId) -> Result<Option<Target>, PortError>;
    async fn list(&self) -> Result<Vec<Target>, PortError>;
    async fn list_with_agents(&self) -> Result<Vec<TargetWithAgents>, PortError>;
    async fn list_for_agent(&self, agent_id: AgentId) -> Result<Vec<Target>, PortError>;
    async fn agent_ids(&self, target_id: TargetId) -> Result<Vec<AgentId>, PortError>;
    async fn assign_agents(
        &self,
        target_id: TargetId,
        agent_ids: &[AgentId],
    ) -> Result<(), PortError>;
    async fn find_many(&self, ids: &[TargetId]) -> Result<Vec<Target>, PortError>;
    async fn save_organizations(
        &self,
        updates: &[(TargetId, TargetOrganization)],
    ) -> Result<(), PortError>;
    async fn delete(&self, id: TargetId) -> Result<bool, PortError>;
}

#[async_trait]
pub trait TargetGroupRepository: Send + Sync {
    async fn save(&self, group: &TargetGroup) -> Result<(), PortError>;
    async fn find(&self, id: TargetGroupId) -> Result<Option<TargetGroup>, PortError>;
    async fn find_by_name(&self, name: &TargetGroupName) -> Result<Option<TargetGroup>, PortError>;
    async fn list_with_counts(&self) -> Result<Vec<TargetGroupWithCounts>, PortError>;
    async fn delete(&self, id: TargetGroupId) -> Result<DeleteGroupOutcome, PortError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteGroupOutcome {
    Deleted,
    NotFound,
    HasRules,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn save(&self, user: &User) -> Result<(), PortError>;
    async fn save_if_first(&self, user: &User) -> Result<bool, PortError>;
    async fn find(&self, id: UserId) -> Result<Option<User>, PortError>;
    async fn find_by_username(&self, username: &Username) -> Result<Option<User>, PortError>;
    async fn count(&self) -> Result<u64, PortError>;
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn save(&self, session: &Session) -> Result<(), PortError>;
    async fn find_active_by_digest(
        &self,
        digest: TokenDigest,
        now: Timestamp,
    ) -> Result<Option<Session>, PortError>;
    async fn revoke(&self, digest: TokenDigest) -> Result<bool, PortError>;
    async fn prune_expired(&self, now: Timestamp, limit: u16) -> Result<u16, PortError>;
}

#[async_trait]
pub trait AgentControlGateway: Send + Sync {
    async fn send_configuration(&self, agent_id: AgentId) -> Result<(), PortError>;
    async fn trigger_check(&self, agent_id: AgentId, target_id: TargetId) -> Result<(), PortError>;
    async fn disconnect(&self, agent_id: AgentId) -> Result<(), PortError>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityError {
    HashingFailed,
    InvalidPasswordHash,
    RandomnessUnavailable,
}

impl fmt::Display for SecurityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::HashingFailed => "password hashing failed",
            Self::InvalidPasswordHash => "stored password hash is invalid",
            Self::RandomnessUnavailable => "secure randomness is unavailable",
        };
        formatter.write_str(message)
    }
}

impl Error for SecurityError {}

pub trait PasswordHasher: Send + Sync {
    fn hash_password(&self, password: &str) -> Result<PasswordHash, SecurityError>;
    fn verify_password(&self, password: &str, hash: &PasswordHash) -> Result<bool, SecurityError>;
}

pub trait TokenGenerator: Send + Sync {
    fn generate(&self) -> Result<String, SecurityError>;
}

pub trait TokenDigester: Send + Sync {
    fn digest(&self, token: &str) -> TokenDigest;
}
