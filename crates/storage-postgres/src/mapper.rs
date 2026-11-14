use serde::{Deserialize, Serialize};
use sqlx::{FromRow, types::Json};
use time::OffsetDateTime;
use uuid::Uuid;

use pingushka_application::error::PortError;
use pingushka_domain::{
    agent::{Agent, AgentName},
    check::{
        CheckFailure, CheckFailureKind, CheckMode, CheckOutcome, CheckResult, LatencyMs, StatusCode,
    },
    identity::{AgentId, CheckResultId, TargetId, TokenDigest, UserId},
    target::{
        CheckIntervalSecs, HttpMethod, HttpUrl, ProbeHost, Target, TargetConfig, TargetName,
        TargetSpec, TimeoutMs,
    },
    user::{PasswordHash, Session, User, Username},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum StoredTargetSpec {
    Http {
        url: String,
        method: StoredHttpMethod,
        expected_status: Option<u16>,
    },
    Tcp {
        host: String,
        port: u16,
    },
    Icmp {
        host: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum StoredHttpMethod {
    Get,
    Head,
}

impl StoredTargetSpec {
    pub(crate) fn from_domain(spec: &TargetSpec) -> Self {
        match spec {
            TargetSpec::Http {
                url,
                method,
                expected_status,
            } => Self::Http {
                url: url.as_str().to_owned(),
                method: match method {
                    HttpMethod::Get => StoredHttpMethod::Get,
                    HttpMethod::Head => StoredHttpMethod::Head,
                },
                expected_status: expected_status.map(StatusCode::get),
            },
            TargetSpec::Tcp { host, port } => Self::Tcp {
                host: host.as_str().to_owned(),
                port: port.get(),
            },
            TargetSpec::Icmp { host } => Self::Icmp {
                host: host.as_str().to_owned(),
            },
        }
    }

    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Http { .. } => "http",
            Self::Tcp { .. } => "tcp",
            Self::Icmp { .. } => "icmp",
        }
    }

    pub(crate) fn into_domain(self) -> Result<TargetSpec, PortError> {
        match self {
            Self::Http {
                url,
                method,
                expected_status,
            } => {
                let url = HttpUrl::parse(url).map_err(|_| PortError::InvalidData)?;
                let expected_status = expected_status
                    .map(StatusCode::new)
                    .transpose()
                    .map_err(|_| PortError::InvalidData)?;
                let method = match method {
                    StoredHttpMethod::Get => HttpMethod::Get,
                    StoredHttpMethod::Head => HttpMethod::Head,
                };
                Ok(TargetSpec::Http {
                    url,
                    method,
                    expected_status,
                })
            }
            Self::Tcp { host, port } => {
                let host = ProbeHost::parse(host).map_err(|_| PortError::InvalidData)?;
                TargetSpec::tcp(host, port).map_err(|_| PortError::InvalidData)
            }
            Self::Icmp { host } => {
                let host = ProbeHost::parse(host).map_err(|_| PortError::InvalidData)?;
                Ok(TargetSpec::icmp(host))
            }
        }
    }
}

#[derive(FromRow)]
pub(crate) struct TargetRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) spec_json: Json<StoredTargetSpec>,
    pub(crate) interval_secs: i32,
    pub(crate) timeout_ms: i32,
    pub(crate) enabled: bool,
    pub(crate) created_at: OffsetDateTime,
    pub(crate) updated_at: OffsetDateTime,
}

impl TryFrom<TargetRow> for Target {
    type Error = PortError;

    fn try_from(row: TargetRow) -> Result<Self, Self::Error> {
        if row.spec_json.0.kind() != row.kind {
            return Err(PortError::InvalidData);
        }
        let id = TargetId::from_uuid(row.id);
        let name = TargetName::new(row.name).map_err(|_| PortError::InvalidData)?;
        let spec = row.spec_json.0.into_domain()?;
        let interval = u32::try_from(row.interval_secs)
            .ok()
            .and_then(|value| CheckIntervalSecs::new(value).ok())
            .ok_or(PortError::InvalidData)?;
        let timeout = u32::try_from(row.timeout_ms)
            .ok()
            .and_then(|value| TimeoutMs::new(value).ok())
            .ok_or(PortError::InvalidData)?;
        let config =
            TargetConfig::new(name, spec, interval, timeout).map_err(|_| PortError::InvalidData)?;
        Target::restore(id, config, row.enabled, row.created_at, row.updated_at)
            .map_err(|_| PortError::InvalidData)
    }
}

#[derive(FromRow)]
pub(crate) struct AgentRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) token_hash: Option<Vec<u8>>,
    pub(crate) created_at: OffsetDateTime,
    pub(crate) last_seen_at: Option<OffsetDateTime>,
    pub(crate) connected: bool,
    pub(crate) version: Option<String>,
}

impl TryFrom<AgentRow> for Agent {
    type Error = PortError;

    fn try_from(row: AgentRow) -> Result<Self, Self::Error> {
        let token_digest = row
            .token_hash
            .map(|bytes| digest_from_bytes(&bytes))
            .transpose()?;
        Agent::restore(
            AgentId::from_uuid(row.id),
            AgentName::new(row.name).map_err(|_| PortError::InvalidData)?,
            token_digest,
            row.created_at,
            row.last_seen_at,
            row.connected,
            row.version,
        )
        .map_err(|_| PortError::InvalidData)
    }
}

#[derive(FromRow)]
pub(crate) struct UserRow {
    pub(crate) id: Uuid,
    pub(crate) username: String,
    pub(crate) password_hash: String,
    pub(crate) created_at: OffsetDateTime,
}

impl TryFrom<UserRow> for User {
    type Error = PortError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        Ok(User::new(
            UserId::from_uuid(row.id),
            Username::new(row.username).map_err(|_| PortError::InvalidData)?,
            PasswordHash::new(row.password_hash).map_err(|_| PortError::InvalidData)?,
            row.created_at,
        ))
    }
}

#[derive(FromRow)]
pub(crate) struct SessionRow {
    pub(crate) token_hash: Vec<u8>,
    pub(crate) user_id: Uuid,
    pub(crate) created_at: OffsetDateTime,
    pub(crate) expires_at: OffsetDateTime,
}

impl TryFrom<SessionRow> for Session {
    type Error = PortError;

    fn try_from(row: SessionRow) -> Result<Self, Self::Error> {
        Session::new(
            UserId::from_uuid(row.user_id),
            digest_from_bytes(&row.token_hash)?,
            row.created_at,
            row.expires_at,
        )
        .map_err(|_| PortError::InvalidData)
    }
}

#[derive(FromRow)]
pub(crate) struct CheckResultRow {
    pub(crate) result_id: Uuid,
    pub(crate) target_id: Uuid,
    pub(crate) agent_id: Uuid,
    pub(crate) checked_at: OffsetDateTime,
    pub(crate) ok: bool,
    pub(crate) latency_ms: i32,
    pub(crate) status_code: Option<i16>,
    pub(crate) check_mode: String,
    pub(crate) error_kind: Option<String>,
    pub(crate) error_message: Option<String>,
}

impl TryFrom<CheckResultRow> for CheckResult {
    type Error = PortError;

    fn try_from(row: CheckResultRow) -> Result<Self, Self::Error> {
        let latency = u32::try_from(row.latency_ms).map_err(|_| PortError::InvalidData)?;
        let status_code = row
            .status_code
            .map(|value| u16::try_from(value).map_err(|_| PortError::InvalidData))
            .transpose()?
            .map(StatusCode::new)
            .transpose()
            .map_err(|_| PortError::InvalidData)?;
        let mode =
            CheckMode::try_from(row.check_mode.as_str()).map_err(|_| PortError::InvalidData)?;
        let failure = match (row.error_kind, row.error_message) {
            (Some(kind), Some(message)) => Some(
                CheckFailure::new(
                    CheckFailureKind::try_from(kind.as_str())
                        .map_err(|_| PortError::InvalidData)?,
                    message,
                )
                .map_err(|_| PortError::InvalidData)?,
            ),
            (None, None) => None,
            _ => return Err(PortError::InvalidData),
        };
        let outcome =
            CheckOutcome::new(row.ok, LatencyMs::new(latency), status_code, mode, failure)
                .map_err(|_| PortError::InvalidData)?;
        Ok(CheckResult::new(
            CheckResultId::from_uuid(row.result_id),
            TargetId::from_uuid(row.target_id),
            AgentId::from_uuid(row.agent_id),
            row.checked_at,
            outcome,
        ))
    }
}

pub(crate) fn digest_from_bytes(bytes: &[u8]) -> Result<TokenDigest, PortError> {
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| PortError::InvalidData)?;
    Ok(TokenDigest::new(bytes))
}

pub(crate) fn target_kind_from_spec(spec: &TargetSpec) -> &'static str {
    StoredTargetSpec::from_domain(spec).kind()
}
