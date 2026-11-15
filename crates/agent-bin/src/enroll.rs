use std::{fmt, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::config::{
    AgentConfig, BufferSettings, ConfigError, HubSettings, LimitSettings, normalize_hub_base,
};

#[derive(Clone, Debug)]
pub struct EnrollmentOutcome {
    pub agent_id: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnrollError {
    EmptyToken,
    InvalidUrl,
    Request(String),
    Rejected(u16),
    Decode(String),
    Config(ConfigError),
}

impl fmt::Display for EnrollError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyToken => formatter.write_str("enrollment token is empty"),
            Self::InvalidUrl => formatter.write_str("hub URL is invalid"),
            Self::Request(error) => write!(formatter, "enrollment request failed: {error}"),
            Self::Rejected(status) => {
                write!(formatter, "hub rejected enrollment with status {status}")
            }
            Self::Decode(error) => write!(formatter, "cannot decode enrollment response: {error}"),
            Self::Config(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for EnrollError {}

#[derive(Serialize)]
struct EnrollRequest<'a> {
    enrollment_token: &'a str,
}

#[derive(Deserialize)]
struct EnrollResponse {
    agent_id: Uuid,
    credential: String,
}

pub async fn enroll(
    hub_url: &str,
    enrollment_token: &str,
    config_path: &Path,
    buffer_path: Option<&Path>,
) -> Result<EnrollmentOutcome, EnrollError> {
    if enrollment_token.trim().is_empty() {
        return Err(EnrollError::EmptyToken);
    }
    let ws_base = normalize_hub_base(hub_url).map_err(|error| match error {
        ConfigError::InvalidUrl | ConfigError::UnsupportedScheme | ConfigError::MissingHost => {
            EnrollError::InvalidUrl
        }
        other => EnrollError::Config(other),
    })?;
    let endpoint = format!(
        "{}/api/agent/enroll",
        http_base(&ws_base).ok_or(EnrollError::InvalidUrl)?
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("pingushka-agent/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| EnrollError::Request(error.to_string()))?;
    let response = client
        .post(&endpoint)
        .json(&EnrollRequest { enrollment_token })
        .send()
        .await
        .map_err(|error| EnrollError::Request(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(EnrollError::Rejected(status.as_u16()));
    }
    let enrolled: EnrollResponse = response
        .json()
        .await
        .map_err(|error| EnrollError::Decode(error.to_string()))?;
    let config = AgentConfig {
        hub: HubSettings {
            url: ws_base,
            agent_id: enrolled.agent_id,
            credential: enrolled.credential,
        },
        buffer: BufferSettings {
            path: buffer_path.map(Path::to_path_buf),
            ..BufferSettings::default()
        },
        limits: LimitSettings::default(),
    };
    config.validate().map_err(EnrollError::Config)?;
    config.save(config_path).map_err(EnrollError::Config)?;
    Ok(EnrollmentOutcome {
        agent_id: enrolled.agent_id,
    })
}

fn http_base(ws_base: &str) -> Option<String> {
    let mut url = Url::parse(ws_base).ok()?;
    match url.scheme() {
        "ws" => {
            url.set_scheme("http").ok()?;
        }
        "wss" => {
            url.set_scheme("https").ok()?;
        }
        _ => return None,
    }
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string().trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_base_maps_websocket_schemes() {
        assert_eq!(
            http_base("ws://127.0.0.1:8080").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(
            http_base("wss://monitor.example.com").as_deref(),
            Some("https://monitor.example.com")
        );
    }

    #[tokio::test]
    async fn empty_token_is_rejected_before_network() {
        let path = std::env::temp_dir().join(format!("pingushka-{}.toml", Uuid::new_v4()));
        let result = enroll("http://127.0.0.1:1", " ", &path, None).await;
        assert_eq!(result.err(), Some(EnrollError::EmptyToken));
    }
}
