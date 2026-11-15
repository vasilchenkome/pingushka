use std::{
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use pingushka_protocol::{
    ProbeFailureKind, ProbeHttpMethod, ProbeMode, ProbeSpec, ProbeTarget, ResultItem,
};
use reqwest::{Client, Method, redirect::Policy};
use surge_ping::{
    Client as IcmpClient, Config as IcmpConfig, PingIdentifier, PingSequence, SurgeError,
};
use tokio::{net::TcpStream, sync::Semaphore};
use uuid::Uuid;

const ICMP_FALLBACK_PORT: u16 = 443;

pub struct Checker {
    http: Client,
    icmp: Option<IcmpClient>,
    http_slots: Semaphore,
    network_slots: Semaphore,
}

impl Checker {
    pub fn new(max_concurrent_checks: usize) -> Self {
        let http = Client::builder()
            .redirect(Policy::limited(10))
            .user_agent(concat!("pingushka-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| Client::new());
        let icmp = match IcmpClient::new(&IcmpConfig::default()) {
            Ok(client) => Some(client),
            Err(error) => {
                tracing::warn!(%error, "ICMP socket unavailable; probes will use TCP fallback");
                None
            }
        };
        Self::build(max_concurrent_checks, http, icmp)
    }

    fn build(max_concurrent_checks: usize, http: Client, icmp: Option<IcmpClient>) -> Self {
        let half = (max_concurrent_checks / 2).max(1);
        Self {
            http,
            icmp,
            http_slots: Semaphore::new(half),
            network_slots: Semaphore::new(half),
        }
    }

    pub async fn run(&self, target: &ProbeTarget) -> ResultItem {
        let timeout = Duration::from_millis(u64::from(target.timeout_ms.max(1)));
        match &target.spec {
            ProbeSpec::Http {
                url,
                method,
                expected_status,
            } => {
                let _permit = self.http_slots.acquire().await;
                self.http_probe(target.target_id, url, *method, *expected_status, timeout)
                    .await
            }
            ProbeSpec::Tcp { host, port } => {
                let _permit = self.network_slots.acquire().await;
                self.tcp_probe(target.target_id, host, *port, timeout).await
            }
            ProbeSpec::Icmp { host } => {
                let _permit = self.network_slots.acquire().await;
                self.icmp_probe(target.target_id, host, timeout).await
            }
        }
    }

    async fn http_probe(
        &self,
        target_id: Uuid,
        url: &str,
        method: ProbeHttpMethod,
        expected_status: Option<u16>,
        timeout: Duration,
    ) -> ResultItem {
        let method = match method {
            ProbeHttpMethod::Get => Method::GET,
            ProbeHttpMethod::Head => Method::HEAD,
        };
        let started = Instant::now();
        match self.http.request(method, url).timeout(timeout).send().await {
            Ok(response) => {
                let latency = elapsed_ms(started);
                let status = response.status().as_u16();
                let expected = expected_status.unwrap_or(200);
                let ok = match expected_status {
                    Some(expected) => status == expected,
                    None => response.status().is_success(),
                };
                let failure = if ok {
                    None
                } else {
                    Some((
                        ProbeFailureKind::HttpStatus,
                        format!("unexpected status {status}, expected {expected}"),
                    ))
                };
                result(
                    target_id,
                    ok,
                    latency,
                    Some(status),
                    ProbeMode::Http,
                    failure,
                )
            }
            Err(error) => result(
                target_id,
                false,
                elapsed_ms(started),
                None,
                ProbeMode::Http,
                Some((
                    classify_http_error(&error),
                    format!("http request failed: {error}"),
                )),
            ),
        }
    }

    async fn tcp_probe(
        &self,
        target_id: Uuid,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> ResultItem {
        let started = Instant::now();
        match tokio::time::timeout(timeout, TcpStream::connect((host, port))).await {
            Ok(Ok(_stream)) => result(
                target_id,
                true,
                elapsed_ms(started),
                None,
                ProbeMode::Tcp,
                None,
            ),
            Ok(Err(error)) => result(
                target_id,
                false,
                elapsed_ms(started),
                None,
                ProbeMode::Tcp,
                Some((
                    classify_io_error(&error),
                    format!("tcp connect failed: {error}"),
                )),
            ),
            Err(_) => result(
                target_id,
                false,
                elapsed_ms(started),
                None,
                ProbeMode::Tcp,
                Some((
                    ProbeFailureKind::Timeout,
                    format!("tcp connect timed out after {:?}", timeout),
                )),
            ),
        }
    }

    async fn icmp_probe(&self, target_id: Uuid, host: &str, timeout: Duration) -> ResultItem {
        let Some(client) = &self.icmp else {
            return self
                .tcp_fallback(target_id, host, timeout, "icmp socket is unavailable")
                .await;
        };
        let address = match resolve(host).await {
            Ok(address) => address,
            Err(message) => {
                return result(
                    target_id,
                    false,
                    0,
                    None,
                    ProbeMode::Icmp,
                    Some((ProbeFailureKind::DnsFailure, message)),
                );
            }
        };
        let mut pinger = client
            .pinger(address, PingIdentifier(rand::random::<u16>()))
            .await;
        pinger.timeout(timeout);
        let started = Instant::now();
        match pinger.ping(PingSequence(1), &[0_u8; 32]).await {
            Ok((_packet, rtt)) => result(
                target_id,
                true,
                u32::try_from(rtt.as_millis()).unwrap_or(u32::MAX),
                None,
                ProbeMode::Icmp,
                None,
            ),
            Err(SurgeError::Timeout { .. }) => result(
                target_id,
                false,
                elapsed_ms(started),
                None,
                ProbeMode::Icmp,
                Some((
                    ProbeFailureKind::Timeout,
                    format!("icmp echo timed out after {:?}", timeout),
                )),
            ),
            Err(error) if is_permission_denied(&error.to_string()) => {
                self.tcp_fallback(target_id, host, timeout, &error.to_string())
                    .await
            }
            Err(error) => result(
                target_id,
                false,
                elapsed_ms(started),
                None,
                ProbeMode::Icmp,
                Some((
                    ProbeFailureKind::Network,
                    format!("icmp echo failed: {error}"),
                )),
            ),
        }
    }

    async fn tcp_fallback(
        &self,
        target_id: Uuid,
        host: &str,
        timeout: Duration,
        reason: &str,
    ) -> ResultItem {
        let started = Instant::now();
        match tokio::time::timeout(timeout, TcpStream::connect((host, ICMP_FALLBACK_PORT))).await {
            Ok(Ok(_stream)) => result(
                target_id,
                true,
                elapsed_ms(started),
                None,
                ProbeMode::TcpFallback,
                None,
            ),
            Ok(Err(error)) => result(
                target_id,
                false,
                elapsed_ms(started),
                None,
                ProbeMode::TcpFallback,
                Some((
                    ProbeFailureKind::Unsupported,
                    format!(
                        "icmp unavailable ({reason}); tcp fallback to {host}:{ICMP_FALLBACK_PORT} failed: {error}"
                    ),
                )),
            ),
            Err(_) => result(
                target_id,
                false,
                elapsed_ms(started),
                None,
                ProbeMode::TcpFallback,
                Some((
                    ProbeFailureKind::Timeout,
                    format!(
                        "icmp unavailable ({reason}); tcp fallback to {host}:{ICMP_FALLBACK_PORT} timed out"
                    ),
                )),
            ),
        }
    }
}

fn result(
    target_id: Uuid,
    ok: bool,
    latency_ms: u32,
    status_code: Option<u16>,
    mode: ProbeMode,
    failure: Option<(ProbeFailureKind, String)>,
) -> ResultItem {
    ResultItem {
        result_id: Uuid::new_v4(),
        target_id,
        checked_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
        ok,
        latency_ms,
        status_code,
        mode,
        error_kind: failure.as_ref().map(|(kind, _)| *kind),
        error_message: failure.map(|(_, message)| truncate(&message, 512)),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn elapsed_ms(started: Instant) -> u32 {
    u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX)
}

async fn resolve(host: &str) -> Result<IpAddr, String> {
    if let Ok(address) = host.parse::<IpAddr>() {
        return Ok(address);
    }
    let mut addresses = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|error| format!("cannot resolve {host}: {error}"))?;
    addresses
        .next()
        .map(|address: SocketAddr| address.ip())
        .ok_or_else(|| format!("no addresses for {host}"))
}

fn classify_http_error(error: &reqwest::Error) -> ProbeFailureKind {
    if error.is_timeout() {
        return ProbeFailureKind::Timeout;
    }
    let message = error.to_string().to_lowercase();
    if message.contains("dns") || message.contains("lookup address") {
        return ProbeFailureKind::DnsFailure;
    }
    if error.is_connect() {
        if message.contains("refused") {
            return ProbeFailureKind::ConnectionRefused;
        }
        return ProbeFailureKind::Network;
    }
    ProbeFailureKind::Network
}

fn classify_io_error(error: &std::io::Error) -> ProbeFailureKind {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => ProbeFailureKind::ConnectionRefused,
        std::io::ErrorKind::TimedOut => ProbeFailureKind::Timeout,
        _ => ProbeFailureKind::Network,
    }
}

fn is_permission_denied(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("permission")
        || message.contains("operation not permitted")
        || message.contains("access is denied")
        || message.contains("requires root")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn http_server() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let port = listener.local_addr().expect("address").port();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = [0_u8; 1024];
                let mut received = Vec::new();
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(read) => {
                            received.extend_from_slice(&buffer[..read]);
                            if received.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
                let _ = stream.shutdown().await;
            }
        });
        (port, handle)
    }

    fn target(spec: ProbeSpec, timeout_ms: u32) -> ProbeTarget {
        ProbeTarget {
            target_id: Uuid::new_v4(),
            name: "test".to_owned(),
            spec,
            interval_secs: 30,
            timeout_ms,
        }
    }

    #[tokio::test]
    async fn http_probe_accepts_success_and_rejects_unexpected_status() {
        let (port, server) = http_server().await;
        let checker = Checker::new(4);
        let url = format!("http://127.0.0.1:{port}/");
        let ok = checker
            .run(&target(
                ProbeSpec::Http {
                    url: url.clone(),
                    method: ProbeHttpMethod::Get,
                    expected_status: None,
                },
                1_000,
            ))
            .await;
        assert!(ok.ok, "unexpected failure: {:?}", ok.error_message);
        assert_eq!(ok.status_code, Some(200));
        assert_eq!(ok.mode, ProbeMode::Http);
        let mismatch = checker
            .run(&target(
                ProbeSpec::Http {
                    url,
                    method: ProbeHttpMethod::Get,
                    expected_status: Some(204),
                },
                1_000,
            ))
            .await;
        assert!(!mismatch.ok);
        assert_eq!(mismatch.error_kind, Some(ProbeFailureKind::HttpStatus));
        server.abort();
    }

    #[tokio::test]
    async fn tcp_probe_detects_open_and_closed_ports() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        let checker = Checker::new(4);
        let open = checker
            .run(&target(
                ProbeSpec::Tcp {
                    host: "127.0.0.1".to_owned(),
                    port,
                },
                500,
            ))
            .await;
        assert!(open.ok);
        drop(listener);
        let closed = checker
            .run(&target(
                ProbeSpec::Tcp {
                    host: "127.0.0.1".to_owned(),
                    port,
                },
                500,
            ))
            .await;
        assert!(!closed.ok);
        assert_eq!(closed.mode, ProbeMode::Tcp);
    }

    #[tokio::test]
    async fn icmp_without_socket_uses_explicit_tcp_fallback() {
        let checker = Checker::build(4, Client::new(), None);
        let item = checker
            .run(&target(
                ProbeSpec::Icmp {
                    host: "127.0.0.1".to_owned(),
                },
                300,
            ))
            .await;
        assert_eq!(item.mode, ProbeMode::TcpFallback);
        assert!(
            item.error_message
                .unwrap_or_default()
                .contains("icmp unavailable")
        );
    }
}
