use std::{error::Error, fmt};

use time::OffsetDateTime;

use crate::{
    Timestamp,
    identity::{AgentId, CheckResultId, TargetId},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckError {
    InvalidStatusCode,
    InvalidCheckMode,
    InvalidFailureKind,
    EmptyFailureMessage,
    FailureMessageTooLong,
    InconsistentOutcome,
}

impl fmt::Display for CheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidStatusCode => "HTTP status code must be between 100 and 599",
            Self::InvalidCheckMode => "check mode is not supported",
            Self::InvalidFailureKind => "check failure kind is not supported",
            Self::EmptyFailureMessage => "check failure message cannot be empty",
            Self::FailureMessageTooLong => "check failure message is too long",
            Self::InconsistentOutcome => "successful checks cannot contain a failure",
        };
        formatter.write_str(message)
    }
}

impl Error for CheckError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatusCode(u16);

impl StatusCode {
    pub fn new(value: u16) -> Result<Self, CheckError> {
        if !(100..=599).contains(&value) {
            return Err(CheckError::InvalidStatusCode);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LatencyMs(u32);

impl LatencyMs {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckMode {
    Http,
    Tcp,
    Icmp,
    TcpFallback,
}

impl CheckMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Tcp => "tcp",
            Self::Icmp => "icmp",
            Self::TcpFallback => "tcp_fallback",
        }
    }
}

impl TryFrom<&str> for CheckMode {
    type Error = CheckError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "http" => Ok(Self::Http),
            "tcp" => Ok(Self::Tcp),
            "icmp" => Ok(Self::Icmp),
            "tcp_fallback" => Ok(Self::TcpFallback),
            _ => Err(CheckError::InvalidCheckMode),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckFailureKind {
    Timeout,
    Connection,
    Dns,
    InvalidResponse,
    PermissionDenied,
    Other,
}

impl CheckFailureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connection => "connection",
            Self::Dns => "dns",
            Self::InvalidResponse => "invalid_response",
            Self::PermissionDenied => "permission_denied",
            Self::Other => "other",
        }
    }
}

impl TryFrom<&str> for CheckFailureKind {
    type Error = CheckError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "timeout" => Ok(Self::Timeout),
            "connection" => Ok(Self::Connection),
            "dns" => Ok(Self::Dns),
            "invalid_response" => Ok(Self::InvalidResponse),
            "permission_denied" => Ok(Self::PermissionDenied),
            "other" => Ok(Self::Other),
            _ => Err(CheckError::InvalidFailureKind),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckFailure {
    kind: CheckFailureKind,
    message: String,
}

impl CheckFailure {
    pub fn new(kind: CheckFailureKind, message: impl AsRef<str>) -> Result<Self, CheckError> {
        let message = message.as_ref().trim();
        if message.is_empty() {
            return Err(CheckError::EmptyFailureMessage);
        }
        if message.chars().count() > 512 {
            return Err(CheckError::FailureMessageTooLong);
        }
        Ok(Self {
            kind,
            message: message.to_owned(),
        })
    }

    pub const fn kind(&self) -> CheckFailureKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckOutcome {
    successful: bool,
    latency: LatencyMs,
    status_code: Option<StatusCode>,
    mode: CheckMode,
    failure: Option<CheckFailure>,
}

impl CheckOutcome {
    pub fn new(
        successful: bool,
        latency: LatencyMs,
        status_code: Option<StatusCode>,
        mode: CheckMode,
        failure: Option<CheckFailure>,
    ) -> Result<Self, CheckError> {
        if successful && failure.is_some() {
            return Err(CheckError::InconsistentOutcome);
        }
        Ok(Self {
            successful,
            latency,
            status_code,
            mode,
            failure,
        })
    }

    pub const fn is_successful(&self) -> bool {
        self.successful
    }

    pub const fn latency(&self) -> LatencyMs {
        self.latency
    }

    pub const fn status_code(&self) -> Option<StatusCode> {
        self.status_code
    }

    pub const fn mode(&self) -> CheckMode {
        self.mode
    }

    pub fn failure(&self) -> Option<&CheckFailure> {
        self.failure.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckResult {
    id: CheckResultId,
    target_id: TargetId,
    agent_id: AgentId,
    checked_at: Timestamp,
    outcome: CheckOutcome,
}

impl CheckResult {
    pub fn new(
        id: CheckResultId,
        target_id: TargetId,
        agent_id: AgentId,
        checked_at: OffsetDateTime,
        outcome: CheckOutcome,
    ) -> Self {
        Self {
            id,
            target_id,
            agent_id,
            checked_at,
            outcome,
        }
    }

    pub fn id(&self) -> CheckResultId {
        self.id
    }

    pub fn target_id(&self) -> TargetId {
        self.target_id
    }

    pub fn agent_id(&self) -> AgentId {
        self.agent_id
    }

    pub const fn checked_at(&self) -> Timestamp {
        self.checked_at
    }

    pub const fn is_successful(&self) -> bool {
        self.outcome.is_successful()
    }

    pub const fn latency(&self) -> LatencyMs {
        self.outcome.latency()
    }

    pub const fn status_code(&self) -> Option<StatusCode> {
        self.outcome.status_code()
    }

    pub const fn mode(&self) -> CheckMode {
        self.outcome.mode()
    }

    pub fn failure(&self) -> Option<&CheckFailure> {
        self.outcome.failure()
    }
}
