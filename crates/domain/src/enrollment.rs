use std::{error::Error, fmt};

use time::OffsetDateTime;

use crate::{
    Timestamp,
    identity::{AgentId, TokenDigest},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnrollmentError {
    ExpiryNotAfterCreation,
    AlreadyConsumed,
    Expired,
}

impl fmt::Display for EnrollmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ExpiryNotAfterCreation => "enrollment expiry must be after creation",
            Self::AlreadyConsumed => "enrollment token was already used",
            Self::Expired => "enrollment token has expired",
        };
        formatter.write_str(message)
    }
}

impl Error for EnrollmentError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentEnrollment {
    agent_id: AgentId,
    token_digest: TokenDigest,
    created_at: Timestamp,
    expires_at: Timestamp,
    consumed_at: Option<Timestamp>,
}

impl AgentEnrollment {
    pub fn new(
        agent_id: AgentId,
        token_digest: TokenDigest,
        created_at: Timestamp,
        expires_at: Timestamp,
    ) -> Result<Self, EnrollmentError> {
        if expires_at <= created_at {
            return Err(EnrollmentError::ExpiryNotAfterCreation);
        }
        Ok(Self {
            agent_id,
            token_digest,
            created_at,
            expires_at,
            consumed_at: None,
        })
    }

    pub fn restore(
        agent_id: AgentId,
        token_digest: TokenDigest,
        created_at: OffsetDateTime,
        expires_at: OffsetDateTime,
        consumed_at: Option<OffsetDateTime>,
    ) -> Result<Self, EnrollmentError> {
        if expires_at <= created_at {
            return Err(EnrollmentError::ExpiryNotAfterCreation);
        }
        Ok(Self {
            agent_id,
            token_digest,
            created_at,
            expires_at,
            consumed_at,
        })
    }

    pub fn consume(&mut self, now: Timestamp) -> Result<(), EnrollmentError> {
        if self.consumed_at.is_some() {
            return Err(EnrollmentError::AlreadyConsumed);
        }
        if now >= self.expires_at {
            return Err(EnrollmentError::Expired);
        }
        self.consumed_at = Some(now);
        Ok(())
    }

    pub fn agent_id(&self) -> AgentId {
        self.agent_id
    }

    pub fn token_digest(&self) -> TokenDigest {
        self.token_digest
    }

    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub const fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    pub const fn consumed_at(&self) -> Option<Timestamp> {
        self.consumed_at
    }
}
