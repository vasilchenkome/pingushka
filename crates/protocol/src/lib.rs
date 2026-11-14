#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        protocol_version: u32,
        agent_id: Uuid,
        token: String,
        agent_version: String,
    },
    ResultsBatch {
        results: Vec<ResultItem>,
        #[serde(default)]
        dropped_results_count: u32,
    },
    Pong {
        #[serde(default)]
        sent_at_unix: i64,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        server_time_unix: i64,
        targets: Vec<ProbeTarget>,
    },
    Configured {
        config_revision: i64,
        targets: Vec<ProbeTarget>,
    },
    Ack {
        up_to_result_id: Uuid,
    },
    CheckNow {
        target_id: Uuid,
    },
    Ping {
        sent_at_unix: i64,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProbeTarget {
    pub target_id: Uuid,
    pub name: String,
    pub spec: ProbeSpec,
    pub interval_secs: u32,
    pub timeout_ms: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProbeSpec {
    Http {
        url: String,
        method: ProbeHttpMethod,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeHttpMethod {
    Get,
    Head,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResultItem {
    pub result_id: Uuid,
    pub target_id: Uuid,
    pub checked_at_unix: i64,
    pub ok: bool,
    pub latency_ms: u32,
    pub status_code: Option<u16>,
    pub mode: ProbeMode,
    pub error_kind: Option<ProbeFailureKind>,
    pub error_message: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeMode {
    Http,
    Tcp,
    Icmp,
    TcpFallback,
}

impl ProbeMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Tcp => "tcp",
            Self::Icmp => "icmp",
            Self::TcpFallback => "tcp_fallback",
        }
    }
}

impl TryFrom<&str> for ProbeMode {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "http" => Ok(Self::Http),
            "tcp" => Ok(Self::Tcp),
            "icmp" => Ok(Self::Icmp),
            "tcp_fallback" => Ok(Self::TcpFallback),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeFailureKind {
    Timeout,
    ConnectionRefused,
    DnsFailure,
    TlsFailure,
    HttpStatus,
    Network,
    Unsupported,
}

impl ProbeFailureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::ConnectionRefused => "connection_refused",
            Self::DnsFailure => "dns_failure",
            Self::TlsFailure => "tls_failure",
            Self::HttpStatus => "http_status",
            Self::Network => "network",
            Self::Unsupported => "unsupported",
        }
    }
}

impl TryFrom<&str> for ProbeFailureKind {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "timeout" => Ok(Self::Timeout),
            "connection_refused" => Ok(Self::ConnectionRefused),
            "dns_failure" => Ok(Self::DnsFailure),
            "tls_failure" => Ok(Self::TlsFailure),
            "http_status" => Ok(Self::HttpStatus),
            "network" => Ok(Self::Network),
            "unsupported" => Ok(Self::Unsupported),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_messages_round_trip() {
        let message = ClientMessage::ResultsBatch {
            results: vec![ResultItem {
                result_id: Uuid::new_v4(),
                target_id: Uuid::new_v4(),
                checked_at_unix: 1_700_000_000,
                ok: true,
                latency_ms: 42,
                status_code: Some(200),
                mode: ProbeMode::Http,
                error_kind: None,
                error_message: None,
            }],
            dropped_results_count: 3,
        };
        let encoded = serde_json::to_string(&message).expect("serialize");
        let decoded: ClientMessage = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, message);
    }

    #[test]
    fn server_messages_round_trip() {
        let message = ServerMessage::Configured {
            config_revision: 12,
            targets: vec![ProbeTarget {
                target_id: Uuid::new_v4(),
                name: "website".to_owned(),
                spec: ProbeSpec::Tcp {
                    host: "example.com".to_owned(),
                    port: 443,
                },
                interval_secs: 30,
                timeout_ms: 5_000,
            }],
        };
        let encoded = serde_json::to_string(&message).expect("serialize");
        let decoded: ServerMessage = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, message);
    }
}
