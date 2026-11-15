use std::{env, error::Error, fmt, time::Duration};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub public_url: String,
    pub install_script_url: String,
    pub cookie_secure: bool,
    pub session_ttl: Duration,
    pub enrollment_ttl: Duration,
    pub retention: Duration,
    pub manual_check_cooldown: Duration,
    pub prune_interval: Duration,
    pub alert_evaluate_interval: Duration,
    pub alert_secret_key: String,
    pub telegram_api_base: String,
    pub static_dir: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug)]
pub enum ConfigError {
    Missing(&'static str),
    Invalid(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(name) => write!(formatter, "environment variable {name} is required"),
            Self::Invalid(name) => write!(formatter, "environment variable {name} is invalid"),
        }
    }
}

impl Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
        let public_url = env::var("PUBLIC_URL").unwrap_or_else(|_| format!("http://{bind_addr}"));
        let install_script_url = env::var("INSTALL_SCRIPT_URL").unwrap_or_else(|_| {
            "https://raw.githubusercontent.com/vasilchenkome/pingushka/main/install.sh".to_owned()
        });
        let cookie_secure = match env::var("COOKIE_SECURE").as_deref() {
            Ok("1" | "true" | "TRUE" | "yes") => true,
            Ok("0" | "false" | "FALSE" | "no") | Err(_) => false,
            Ok(_) => return Err(ConfigError::Invalid("COOKIE_SECURE")),
        };
        Ok(Self {
            database_url,
            bind_addr,
            public_url,
            install_script_url,
            cookie_secure,
            session_ttl: hours(parse_env("SESSION_TTL_HOURS", 168)?),
            enrollment_ttl: minutes(parse_env("ENROLLMENT_TTL_MINUTES", 30)?),
            retention: days(parse_env("RETENTION_DAYS", 30)?),
            manual_check_cooldown: Duration::from_secs(parse_env(
                "MANUAL_CHECK_COOLDOWN_SECS",
                10,
            )?),
            prune_interval: minutes(parse_env("PRUNE_INTERVAL_MINUTES", 60)?),
            alert_evaluate_interval: Duration::from_secs(parse_env(
                "ALERT_EVALUATE_INTERVAL_SECS",
                30,
            )?),
            alert_secret_key: env::var("ALERT_SECRET_KEY")
                .map_err(|_| ConfigError::Missing("ALERT_SECRET_KEY"))?,
            telegram_api_base: env::var("TELEGRAM_API_BASE")
                .unwrap_or_else(|_| "https://api.telegram.org".to_owned()),
            static_dir: env::var("STATIC_DIR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(std::path::PathBuf::from),
        })
    }
}

fn parse_env(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    match env::var(name) {
        Ok(value) => value.parse().map_err(|_| ConfigError::Invalid(name)),
        Err(_) => Ok(default),
    }
}

fn hours(value: u64) -> Duration {
    Duration::from_secs(value.saturating_mul(3_600))
}

fn minutes(value: u64) -> Duration {
    Duration::from_secs(value.saturating_mul(60))
}

fn days(value: u64) -> Duration {
    Duration::from_secs(value.saturating_mul(86_400))
}
