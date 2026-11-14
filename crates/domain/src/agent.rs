use std::{error::Error, fmt};

use time::OffsetDateTime;

use crate::{identity::AgentId, identity::TokenDigest};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentError {
    EmptyName,
    NameTooLong,
    InvalidName,
    EmptyVersion,
    VersionTooLong,
    InvalidVersion,
}

impl fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyName => "agent name cannot be empty",
            Self::NameTooLong => "agent name is too long",
            Self::InvalidName => "agent name contains control characters",
            Self::EmptyVersion => "agent version cannot be empty",
            Self::VersionTooLong => "agent version is too long",
            Self::InvalidVersion => "agent version contains control characters",
        };
        formatter.write_str(message)
    }
}

impl Error for AgentError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AgentName(String);

impl AgentName {
    pub fn new(value: impl AsRef<str>) -> Result<Self, AgentError> {
        let value = value.as_ref().trim();
        if value.is_empty() {
            return Err(AgentError::EmptyName);
        }
        if value.chars().count() > 128 {
            return Err(AgentError::NameTooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(AgentError::InvalidName);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agent {
    id: AgentId,
    name: AgentName,
    token_digest: Option<TokenDigest>,
    created_at: OffsetDateTime,
    last_seen_at: Option<OffsetDateTime>,
    connected: bool,
    version: Option<String>,
}

impl Agent {
    pub fn new(id: AgentId, name: AgentName, created_at: OffsetDateTime) -> Self {
        Self {
            id,
            name,
            token_digest: None,
            created_at,
            last_seen_at: None,
            connected: false,
            version: None,
        }
    }

    pub fn restore(
        id: AgentId,
        name: AgentName,
        token_digest: Option<TokenDigest>,
        created_at: OffsetDateTime,
        last_seen_at: Option<OffsetDateTime>,
        connected: bool,
        version: Option<String>,
    ) -> Result<Self, AgentError> {
        if let Some(version) = &version {
            validate_version(version)?;
        }
        Ok(Self {
            id,
            name,
            token_digest,
            created_at,
            last_seen_at,
            connected,
            version,
        })
    }

    pub fn mark_connected(
        &mut self,
        version: impl AsRef<str>,
        now: OffsetDateTime,
    ) -> Result<(), AgentError> {
        let version = version.as_ref().trim();
        validate_version(version)?;
        self.version = Some(version.to_owned());
        self.last_seen_at = Some(now);
        self.connected = true;
        Ok(())
    }

    pub fn mark_disconnected(&mut self) {
        self.connected = false;
    }

    pub fn replace_token_digest(&mut self, token_digest: Option<TokenDigest>) {
        self.token_digest = token_digest;
    }

    pub fn id(&self) -> AgentId {
        self.id
    }

    pub fn name(&self) -> &AgentName {
        &self.name
    }

    pub fn token_digest(&self) -> Option<TokenDigest> {
        self.token_digest
    }

    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub const fn last_seen_at(&self) -> Option<OffsetDateTime> {
        self.last_seen_at
    }

    pub const fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

fn validate_version(value: &str) -> Result<(), AgentError> {
    if value.is_empty() {
        return Err(AgentError::EmptyVersion);
    }
    if value.len() > 64 {
        return Err(AgentError::VersionTooLong);
    }
    if value.chars().any(char::is_control) {
        return Err(AgentError::InvalidVersion);
    }
    Ok(())
}
