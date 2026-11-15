use std::{error::Error, fmt};

use time::OffsetDateTime;

use crate::identity::TargetId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertMode {
    All,
    Any,
}

impl AlertMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
        }
    }
}

impl TryFrom<&str> for AlertMode {
    type Error = AlertError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "all" => Ok(Self::All),
            "any" => Ok(Self::Any),
            _ => Err(AlertError::InvalidMode),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertStatus {
    Ok,
    Pending,
    Firing,
}

impl AlertStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Pending => "pending",
            Self::Firing => "firing",
        }
    }
}

impl TryFrom<&str> for AlertStatus {
    type Error = AlertError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "ok" => Ok(Self::Ok),
            "pending" => Ok(Self::Pending),
            "firing" => Ok(Self::Firing),
            _ => Err(AlertError::InvalidStatus),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertEvent {
    Down,
    Recovered,
}

impl AlertEvent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Recovered => "recovered",
        }
    }
}

impl TryFrom<&str> for AlertEvent {
    type Error = AlertError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "down" => Ok(Self::Down),
            "recovered" => Ok(Self::Recovered),
            _ => Err(AlertError::InvalidEvent),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AlertError {
    EmptyName,
    NameTooLong,
    InvalidName,
    InvalidMode,
    InvalidStatus,
    InvalidEvent,
    InvalidFailureThreshold,
    InvalidRecoveryThreshold,
    InvalidCooldown,
    UpdatedBeforeCreated,
}

impl fmt::Display for AlertError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyName => "alert channel name cannot be empty",
            Self::NameTooLong => "alert channel name is too long",
            Self::InvalidName => "alert channel name contains control characters",
            Self::InvalidMode => "alert mode must be all or any",
            Self::InvalidStatus => "alert status is not supported",
            Self::InvalidEvent => "alert event is not supported",
            Self::InvalidFailureThreshold => "failure threshold must be between 1 and 100",
            Self::InvalidRecoveryThreshold => "recovery threshold must be between 1 and 100",
            Self::InvalidCooldown => "cooldown must be between 0 and 86400 seconds",
            Self::UpdatedBeforeCreated => "alert rule update time cannot be before creation time",
        };
        formatter.write_str(message)
    }
}

impl Error for AlertError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AlertChannelName(String);

impl AlertChannelName {
    pub fn new(value: impl AsRef<str>) -> Result<Self, AlertError> {
        let value = value.as_ref().trim();
        if value.is_empty() {
            return Err(AlertError::EmptyName);
        }
        if value.chars().count() > 64 {
            return Err(AlertError::NameTooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(AlertError::InvalidName);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlertRule {
    target_id: TargetId,
    enabled: bool,
    mode: AlertMode,
    failure_threshold: u16,
    recovery_threshold: u16,
    cooldown_secs: u32,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl AlertRule {
    pub fn new(
        target_id: TargetId,
        mode: AlertMode,
        failure_threshold: u16,
        recovery_threshold: u16,
        cooldown_secs: u32,
        now: OffsetDateTime,
    ) -> Result<Self, AlertError> {
        validate_failure_threshold(failure_threshold)?;
        validate_recovery_threshold(recovery_threshold)?;
        validate_cooldown(cooldown_secs)?;
        Ok(Self {
            target_id,
            enabled: true,
            mode,
            failure_threshold,
            recovery_threshold,
            cooldown_secs,
            created_at: now,
            updated_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        target_id: TargetId,
        enabled: bool,
        mode: AlertMode,
        failure_threshold: u16,
        recovery_threshold: u16,
        cooldown_secs: u32,
        created_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Result<Self, AlertError> {
        if updated_at < created_at {
            return Err(AlertError::UpdatedBeforeCreated);
        }
        validate_failure_threshold(failure_threshold)?;
        validate_recovery_threshold(recovery_threshold)?;
        validate_cooldown(cooldown_secs)?;
        Ok(Self {
            target_id,
            enabled,
            mode,
            failure_threshold,
            recovery_threshold,
            cooldown_secs,
            created_at,
            updated_at,
        })
    }

    pub fn update(
        &mut self,
        mode: AlertMode,
        failure_threshold: u16,
        recovery_threshold: u16,
        cooldown_secs: u32,
        enabled: bool,
        now: OffsetDateTime,
    ) -> Result<(), AlertError> {
        validate_failure_threshold(failure_threshold)?;
        validate_recovery_threshold(recovery_threshold)?;
        validate_cooldown(cooldown_secs)?;
        if now < self.created_at {
            return Err(AlertError::UpdatedBeforeCreated);
        }
        self.mode = mode;
        self.failure_threshold = failure_threshold;
        self.recovery_threshold = recovery_threshold;
        self.cooldown_secs = cooldown_secs;
        self.enabled = enabled;
        self.updated_at = now;
        Ok(())
    }

    pub fn target_id(&self) -> TargetId {
        self.target_id
    }

    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub const fn mode(&self) -> AlertMode {
        self.mode
    }

    pub const fn failure_threshold(&self) -> u16 {
        self.failure_threshold
    }

    pub const fn recovery_threshold(&self) -> u16 {
        self.recovery_threshold
    }

    pub const fn cooldown_secs(&self) -> u32 {
        self.cooldown_secs
    }

    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub const fn updated_at(&self) -> OffsetDateTime {
        self.updated_at
    }
}

fn validate_failure_threshold(value: u16) -> Result<(), AlertError> {
    if !(1..=100).contains(&value) {
        return Err(AlertError::InvalidFailureThreshold);
    }
    Ok(())
}

fn validate_recovery_threshold(value: u16) -> Result<(), AlertError> {
    if !(1..=100).contains(&value) {
        return Err(AlertError::InvalidRecoveryThreshold);
    }
    Ok(())
}

fn validate_cooldown(value: u32) -> Result<(), AlertError> {
    if value > 86_400 {
        return Err(AlertError::InvalidCooldown);
    }
    Ok(())
}
