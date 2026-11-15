use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentConfig {
    pub hub: HubSettings,
    #[serde(default)]
    pub buffer: BufferSettings,
    #[serde(default)]
    pub limits: LimitSettings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HubSettings {
    pub url: String,
    pub agent_id: Uuid,
    pub credential: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BufferSettings {
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default = "default_capacity")]
    pub capacity: usize,
}

impl Default for BufferSettings {
    fn default() -> Self {
        Self {
            path: None,
            capacity: default_capacity(),
        }
    }
}

const fn default_capacity() -> usize {
    10_000
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LimitSettings {
    #[serde(default = "default_concurrency")]
    pub max_concurrent_checks: usize,
}

impl Default for LimitSettings {
    fn default() -> Self {
        Self {
            max_concurrent_checks: default_concurrency(),
        }
    }
}

const fn default_concurrency() -> usize {
    32
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    Read(String),
    Parse(String),
    Write(String),
    InvalidUrl,
    UnsupportedScheme,
    MissingHost,
    EmptyCredential,
    InvalidCapacity,
    InvalidConcurrency,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "cannot read configuration: {error}"),
            Self::Parse(error) => write!(formatter, "cannot parse configuration: {error}"),
            Self::Write(error) => write!(formatter, "cannot write configuration: {error}"),
            Self::InvalidUrl => formatter.write_str("hub URL is invalid"),
            Self::UnsupportedScheme => {
                formatter.write_str("hub URL must use ws, wss, http or https")
            }
            Self::MissingHost => formatter.write_str("hub URL must include a host"),
            Self::EmptyCredential => formatter.write_str("agent credential is missing"),
            Self::InvalidCapacity => {
                formatter.write_str("buffer capacity must be between 100 and 100000")
            }
            Self::InvalidConcurrency => {
                formatter.write_str("max_concurrent_checks must be between 1 and 256")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl AgentConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = fs::read_to_string(path).map_err(|error| ConfigError::Read(error.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(path) {
                if metadata.permissions().mode() & 0o077 != 0 {
                    tracing::warn!(
                        path = %path.display(),
                        "configuration file is readable by other users; expected mode 0600"
                    );
                }
            }
        }
        let config: Self =
            toml::from_str(&raw).map_err(|error| ConfigError::Parse(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| ConfigError::Write(error.to_string()))?;
        }
        let encoded =
            toml::to_string_pretty(self).map_err(|error| ConfigError::Write(error.to_string()))?;
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, encoded).map_err(|error| ConfigError::Write(error.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
                .map_err(|error| ConfigError::Write(error.to_string()))?;
        }
        #[cfg(windows)]
        {
            if path.exists() {
                fs::remove_file(path).map_err(|error| ConfigError::Write(error.to_string()))?;
            }
        }
        fs::rename(&temporary, path).map_err(|error| ConfigError::Write(error.to_string()))
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let url = Url::parse(&self.hub.url).map_err(|_| ConfigError::InvalidUrl)?;
        if !matches!(url.scheme(), "ws" | "wss" | "http" | "https") {
            return Err(ConfigError::UnsupportedScheme);
        }
        if url.host().is_none() {
            return Err(ConfigError::MissingHost);
        }
        if self.hub.credential.trim().is_empty() {
            return Err(ConfigError::EmptyCredential);
        }
        if self.hub.agent_id.is_nil() {
            return Err(ConfigError::EmptyCredential);
        }
        if !(100..=100_000).contains(&self.buffer.capacity) {
            return Err(ConfigError::InvalidCapacity);
        }
        if !(1..=256).contains(&self.limits.max_concurrent_checks) {
            return Err(ConfigError::InvalidConcurrency);
        }
        Ok(())
    }

    pub fn websocket_endpoint(&self) -> Result<String, ConfigError> {
        let mut url = Url::parse(&self.hub.url).map_err(|_| ConfigError::InvalidUrl)?;
        match url.scheme() {
            "ws" | "wss" => {}
            "http" => {
                url.set_scheme("ws").map_err(|_| ConfigError::InvalidUrl)?;
            }
            "https" => {
                url.set_scheme("wss").map_err(|_| ConfigError::InvalidUrl)?;
            }
            _ => return Err(ConfigError::UnsupportedScheme),
        }
        url.set_path("/api/agent/ws");
        url.set_query(None);
        url.set_fragment(None);
        Ok(url.to_string())
    }

    pub fn buffer_path(&self, config_path: &Path) -> PathBuf {
        self.buffer
            .path
            .clone()
            .unwrap_or_else(|| default_buffer_path(config_path))
    }
}

pub fn default_config_path() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/etc/pingushka/agent.toml")
    }
    #[cfg(not(unix))]
    {
        PathBuf::from("agent.toml")
    }
}

fn default_buffer_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|parent| parent.join("buffer.jsonl"))
        .unwrap_or_else(|| PathBuf::from("buffer.jsonl"))
}

pub fn normalize_hub_base(input: &str) -> Result<String, ConfigError> {
    let mut url = Url::parse(input).map_err(|_| ConfigError::InvalidUrl)?;
    let scheme = match url.scheme() {
        "ws" => "ws",
        "wss" => "wss",
        "http" => "ws",
        "https" => "wss",
        _ => return Err(ConfigError::UnsupportedScheme),
    };
    if url.host().is_none() {
        return Err(ConfigError::MissingHost);
    }
    url.set_scheme(scheme)
        .map_err(|_| ConfigError::InvalidUrl)?;
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AgentConfig {
        AgentConfig {
            hub: HubSettings {
                url: "ws://127.0.0.1:8080".to_owned(),
                agent_id: Uuid::new_v4(),
                credential: "a".repeat(64),
            },
            buffer: BufferSettings::default(),
            limits: LimitSettings::default(),
        }
    }

    #[test]
    fn config_round_trips_through_toml() {
        let path = std::env::temp_dir().join(format!("pingushka-config-{}.toml", Uuid::new_v4()));
        let config = sample();
        config.save(&path).expect("save config");
        let loaded = AgentConfig::load(&path).expect("load config");
        assert_eq!(loaded.hub.url, config.hub.url);
        assert_eq!(loaded.hub.agent_id, config.hub.agent_id);
        assert_eq!(loaded.buffer.capacity, 10_000);
        assert_eq!(loaded.limits.max_concurrent_checks, 32);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn hub_base_normalisation_accepts_http_and_ws() {
        assert_eq!(
            normalize_hub_base("http://example.com:8080/").unwrap(),
            "ws://example.com:8080"
        );
        assert_eq!(
            normalize_hub_base("https://example.com").unwrap(),
            "wss://example.com"
        );
        assert!(normalize_hub_base("ftp://example.com").is_err());
    }

    #[test]
    fn websocket_endpoint_is_fixed_to_agent_path() {
        let config = sample();
        assert_eq!(
            config.websocket_endpoint().unwrap(),
            "ws://127.0.0.1:8080/api/agent/ws"
        );
    }

    #[test]
    fn empty_credential_is_rejected() {
        let mut config = sample();
        config.hub.credential.clear();
        assert_eq!(config.validate(), Err(ConfigError::EmptyCredential));
    }
}
