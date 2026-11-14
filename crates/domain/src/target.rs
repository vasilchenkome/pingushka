use std::{error::Error, fmt, num::NonZeroU16};

use time::OffsetDateTime;
use url::{Host, Url};

use crate::{check::StatusCode, identity::TargetId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetError {
    EmptyName,
    NameTooLong,
    InvalidName,
    InvalidUrl,
    UnsupportedUrlScheme,
    UrlCredentialsNotAllowed,
    MissingUrlHost,
    InvalidHost,
    InvalidPort,
    InvalidInterval,
    InvalidTimeout,
    TimeoutNotBelowInterval,
    UpdatedBeforeCreated,
}

impl fmt::Display for TargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyName => "target name cannot be empty",
            Self::NameTooLong => "target name is too long",
            Self::InvalidName => "target name contains control characters",
            Self::InvalidUrl => "target URL is invalid",
            Self::UnsupportedUrlScheme => "target URL must use HTTP or HTTPS",
            Self::UrlCredentialsNotAllowed => "target URL cannot contain credentials",
            Self::MissingUrlHost => "target URL must include a host",
            Self::InvalidHost => "target host is invalid",
            Self::InvalidPort => "TCP port must be between 1 and 65535",
            Self::InvalidInterval => "check interval must be between 5 and 3600 seconds",
            Self::InvalidTimeout => "timeout must be between 100 and 30000 milliseconds",
            Self::TimeoutNotBelowInterval => "timeout must be shorter than the check interval",
            Self::UpdatedBeforeCreated => "target update time cannot be before creation time",
        };
        formatter.write_str(message)
    }
}

impl Error for TargetError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TargetName(String);

impl TargetName {
    pub fn new(value: impl AsRef<str>) -> Result<Self, TargetError> {
        let value = value.as_ref().trim();
        if value.is_empty() {
            return Err(TargetError::EmptyName);
        }
        if value.chars().count() > 128 {
            return Err(TargetError::NameTooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(TargetError::InvalidName);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HttpUrl(String);

impl HttpUrl {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, TargetError> {
        let url = Url::parse(value.as_ref()).map_err(|_| TargetError::InvalidUrl)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(TargetError::UnsupportedUrlScheme);
        }
        if url.host().is_none() {
            return Err(TargetError::MissingUrlHost);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(TargetError::UrlCredentialsNotAllowed);
        }
        Ok(Self(url.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    Get,
    Head,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProbeHost(String);

impl ProbeHost {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, TargetError> {
        let value = value.as_ref().trim();
        let host = Host::parse(value).map_err(|_| TargetError::InvalidHost)?;
        Ok(Self(host.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetSpec {
    Http {
        url: HttpUrl,
        method: HttpMethod,
        expected_status: Option<StatusCode>,
    },
    Tcp {
        host: ProbeHost,
        port: NonZeroU16,
    },
    Icmp {
        host: ProbeHost,
    },
}

impl TargetSpec {
    pub fn http(url: HttpUrl) -> Self {
        Self::Http {
            url,
            method: HttpMethod::Get,
            expected_status: None,
        }
    }

    pub fn tcp(host: ProbeHost, port: u16) -> Result<Self, TargetError> {
        let port = NonZeroU16::new(port).ok_or(TargetError::InvalidPort)?;
        Ok(Self::Tcp { host, port })
    }

    pub fn icmp(host: ProbeHost) -> Self {
        Self::Icmp { host }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckIntervalSecs(u32);

impl CheckIntervalSecs {
    pub fn new(value: u32) -> Result<Self, TargetError> {
        if !(5..=3600).contains(&value) {
            return Err(TargetError::InvalidInterval);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeoutMs(u32);

impl TimeoutMs {
    pub fn new(value: u32) -> Result<Self, TargetError> {
        if !(100..=30_000).contains(&value) {
            return Err(TargetError::InvalidTimeout);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetConfig {
    name: TargetName,
    spec: TargetSpec,
    interval: CheckIntervalSecs,
    timeout: TimeoutMs,
}

impl TargetConfig {
    pub fn new(
        name: TargetName,
        spec: TargetSpec,
        interval: CheckIntervalSecs,
        timeout: TimeoutMs,
    ) -> Result<Self, TargetError> {
        if u64::from(timeout.get()) >= u64::from(interval.get()) * 1000 {
            return Err(TargetError::TimeoutNotBelowInterval);
        }
        Ok(Self {
            name,
            spec,
            interval,
            timeout,
        })
    }

    pub fn name(&self) -> &TargetName {
        &self.name
    }

    pub fn spec(&self) -> &TargetSpec {
        &self.spec
    }

    pub const fn interval(&self) -> CheckIntervalSecs {
        self.interval
    }

    pub const fn timeout(&self) -> TimeoutMs {
        self.timeout
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    id: TargetId,
    name: TargetName,
    spec: TargetSpec,
    interval: CheckIntervalSecs,
    timeout: TimeoutMs,
    enabled: bool,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl Target {
    pub fn new(id: TargetId, config: TargetConfig, now: OffsetDateTime) -> Self {
        Self {
            id,
            name: config.name,
            spec: config.spec,
            interval: config.interval,
            timeout: config.timeout,
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn restore(
        id: TargetId,
        config: TargetConfig,
        enabled: bool,
        created_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Result<Self, TargetError> {
        if updated_at < created_at {
            return Err(TargetError::UpdatedBeforeCreated);
        }
        let mut target = Self::new(id, config, created_at);
        target.enabled = enabled;
        target.updated_at = updated_at;
        Ok(target)
    }

    pub fn id(&self) -> TargetId {
        self.id
    }

    pub fn name(&self) -> &TargetName {
        &self.name
    }

    pub fn spec(&self) -> &TargetSpec {
        &self.spec
    }

    pub fn interval(&self) -> CheckIntervalSecs {
        self.interval
    }

    pub fn timeout(&self) -> TimeoutMs {
        self.timeout
    }

    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub const fn updated_at(&self) -> OffsetDateTime {
        self.updated_at
    }

    pub fn set_enabled(&mut self, enabled: bool, now: OffsetDateTime) {
        self.enabled = enabled;
        self.updated_at = now;
    }

    pub fn update(
        &mut self,
        config: TargetConfig,
        enabled: bool,
        now: OffsetDateTime,
    ) -> Result<(), TargetError> {
        if now < self.created_at {
            return Err(TargetError::UpdatedBeforeCreated);
        }
        self.name = config.name;
        self.spec = config.spec;
        self.interval = config.interval;
        self.timeout = config.timeout;
        self.enabled = enabled;
        self.updated_at = now;
        Ok(())
    }
}
