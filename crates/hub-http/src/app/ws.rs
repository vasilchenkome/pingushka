use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    extract::{
        State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use pingushka_application::{
    error::PortError, ports::AgentControlGateway, use_cases::RecordCheckBatchError,
};
use pingushka_domain::{
    check::{
        CheckFailure, CheckFailureKind, CheckMode, CheckOutcome, CheckResult, LatencyMs, StatusCode,
    },
    identity::{AgentId, CheckResultId, TargetId},
    target::{HttpMethod, Target, TargetSpec},
};
use pingushka_protocol::{
    ClientMessage, PROTOCOL_VERSION, ProbeFailureKind, ProbeHttpMethod, ProbeSpec, ProbeTarget,
    ResultItem, ServerMessage,
};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{
    app::{AppState, error::ApiError},
    system::{from_unix_seconds, unix_seconds},
};

const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const PING_INTERVAL: Duration = Duration::from_secs(15);
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const CHANNEL_CAPACITY: usize = 32;

#[derive(Debug)]
pub enum ServerCommand {
    Message(ServerMessage),
    ReloadConfiguration,
    CheckNow(TargetId),
    Close,
}

pub struct AgentRegistry {
    channels: Mutex<HashMap<AgentId, mpsc::Sender<ServerCommand>>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
        }
    }

    fn register(
        self: &Arc<Self>,
        agent_id: AgentId,
        sender: mpsc::Sender<ServerCommand>,
    ) -> ConnectionGuard {
        self.channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(agent_id, sender.clone());
        ConnectionGuard {
            registry: self.clone(),
            agent_id,
            sender,
        }
    }

    pub fn is_connected(&self, agent_id: AgentId) -> bool {
        self.channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&agent_id)
    }

    fn send(&self, agent_id: AgentId, command: ServerCommand) -> Result<(), PortError> {
        let sender = self
            .channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&agent_id)
            .cloned()
            .ok_or(PortError::Unavailable)?;
        sender.try_send(command).map_err(|_| PortError::Unavailable)
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ConnectionGuard {
    registry: Arc<AgentRegistry>,
    agent_id: AgentId,
    sender: mpsc::Sender<ServerCommand>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let mut channels = self
            .registry
            .channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if channels
            .get(&self.agent_id)
            .is_some_and(|sender| sender.same_channel(&self.sender))
        {
            channels.remove(&self.agent_id);
        }
    }
}

#[async_trait]
impl AgentControlGateway for AgentRegistry {
    async fn send_configuration(&self, agent_id: AgentId) -> Result<(), PortError> {
        self.send(agent_id, ServerCommand::ReloadConfiguration)
    }

    async fn trigger_check(&self, agent_id: AgentId, target_id: TargetId) -> Result<(), PortError> {
        self.send(agent_id, ServerCommand::CheckNow(target_id))
    }

    async fn disconnect(&self, agent_id: AgentId) -> Result<(), PortError> {
        self.send(agent_id, ServerCommand::Close)
    }
}

pub async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> Result<Response, ApiError> {
    Ok(ws
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| handle_socket(socket, state)))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sink, mut stream) = socket.split();
    let first = match tokio::time::timeout(HELLO_TIMEOUT, stream.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => text,
        _ => {
            close(&mut sink, 4408, "hello timeout").await;
            return;
        }
    };
    let Ok(ClientMessage::Hello {
        protocol_version,
        agent_id,
        token,
        agent_version,
    }) = serde_json::from_str::<ClientMessage>(first.as_str())
    else {
        close(&mut sink, 4400, "expected hello").await;
        return;
    };
    if protocol_version != PROTOCOL_VERSION {
        close(&mut sink, 4400, "unsupported protocol version").await;
        return;
    }
    let agent_id = AgentId::from_uuid(agent_id);
    let mut agent = match state.agents.find(agent_id).await {
        Ok(Some(agent)) => agent,
        Ok(None) => {
            close(&mut sink, 4401, "unknown agent").await;
            return;
        }
        Err(error) => {
            warn!(%error, "agent lookup failed");
            close(&mut sink, 1011, "storage unavailable").await;
            return;
        }
    };
    let expected = agent.token_digest();
    if expected.is_none() || expected != Some(state.digester.digest(&token)) {
        close(&mut sink, 4401, "invalid credential").await;
        return;
    }
    let now = state.clock.now();
    if agent.mark_connected(&agent_version, now).is_err() {
        close(&mut sink, 4400, "invalid agent version").await;
        return;
    }
    if let Err(error) = state
        .agents
        .mark_connected(agent_id, &agent_version, now)
        .await
    {
        warn!(%error, "failed to persist agent connection");
        close(&mut sink, 1011, "storage unavailable").await;
        return;
    }
    let (sender, mut receiver) = mpsc::channel(CHANNEL_CAPACITY);
    let guard = state.registry.register(agent_id, sender);
    info!(agent = %agent.name().as_str(), "agent connected");

    let targets = load_targets(&state, agent_id).await.unwrap_or_default();
    let welcome = ServerMessage::Welcome {
        server_time_unix: unix_seconds(state.clock.now()),
        targets,
    };
    if !send(&mut sink, &welcome).await {
        drop(guard);
        mark_disconnected(&state, agent_id).await;
        return;
    }

    let mut last_activity = tokio::time::Instant::now();
    let mut ticker = tokio::time::interval(PING_INTERVAL);
    ticker.tick().await;

    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        last_activity = tokio::time::Instant::now();
                        let Ok(message) = serde_json::from_str::<ClientMessage>(text.as_str()) else {
                            close(&mut sink, 4400, "malformed message").await;
                            break;
                        };
                        match message {
                            ClientMessage::ResultsBatch { results, dropped_results_count } => {
                                if dropped_results_count > 0 {
                                    warn!(agent_id = %agent_id.as_uuid(), dropped_results_count, "agent dropped results");
                                }
                                let Some(batch) = map_batch(&results, agent_id) else {
                                    close(&mut sink, 4400, "invalid result payload").await;
                                    break;
                                };
                                let Some(last_result_id) = results.last().map(|item| item.result_id) else {
                                    continue;
                                };
                                match state.record_batch.execute(batch).await {
                                    Ok(_) => {
                                        let ack = ServerMessage::Ack { up_to_result_id: last_result_id };
                                        if !send(&mut sink, &ack).await {
                                            break;
                                        }
                                    }
                                    Err(RecordCheckBatchError::InvalidBatchSize) => {
                                        close(&mut sink, 4400, "invalid batch size").await;
                                        break;
                                    }
                                    Err(RecordCheckBatchError::Repository(error)) => {
                                        warn!(%error, "failed to persist results");
                                        close(&mut sink, 1011, "storage unavailable").await;
                                        break;
                                    }
                                }
                            }
                            ClientMessage::Pong { .. } => {}
                            ClientMessage::Hello { .. } => {
                                close(&mut sink, 4400, "duplicate hello").await;
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        last_activity = tokio::time::Instant::now();
                        if sink.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(error)) => {
                        warn!(%error, "agent socket error");
                        break;
                    }
                    _ => {}
                }
            }
            command = receiver.recv() => {
                match command {
                    Some(ServerCommand::Message(message)) => {
                        if !send(&mut sink, &message).await {
                            break;
                        }
                    }
                    Some(ServerCommand::ReloadConfiguration) => {
                        let targets = load_targets(&state, agent_id).await.unwrap_or_default();
                        let revision = (state.clock.now().unix_timestamp_nanos() / 1_000_000) as i64;
                        let configured = ServerMessage::Configured { config_revision: revision, targets };
                        if !send(&mut sink, &configured).await {
                            break;
                        }
                    }
                    Some(ServerCommand::CheckNow(target_id)) => {
                        let message = ServerMessage::CheckNow { target_id: target_id.as_uuid() };
                        if !send(&mut sink, &message).await {
                            break;
                        }
                    }
                    Some(ServerCommand::Close) => {
                        close(&mut sink, 1000, "credential rotated").await;
                        break;
                    }
                    None => break,
                }
            }
            _ = ticker.tick() => {
                if last_activity.elapsed() > IDLE_TIMEOUT {
                    close(&mut sink, 4408, "idle timeout").await;
                    break;
                }
                let ping = ServerMessage::Ping { sent_at_unix: unix_seconds(state.clock.now()) };
                if !send(&mut sink, &ping).await {
                    break;
                }
            }
        }
    }

    drop(guard);
    mark_disconnected(&state, agent_id).await;
    info!(agent = %agent.name().as_str(), "agent disconnected");
}

async fn mark_disconnected(state: &AppState, agent_id: AgentId) {
    if let Err(error) = state.agents.mark_disconnected(agent_id).await {
        warn!(%error, "failed to persist agent disconnection");
    }
}

async fn load_targets(state: &AppState, agent_id: AgentId) -> Result<Vec<ProbeTarget>, PortError> {
    let targets = state.targets.list_for_agent(agent_id).await?;
    Ok(targets.iter().map(probe_target).collect())
}

fn probe_target(target: &Target) -> ProbeTarget {
    ProbeTarget {
        target_id: target.id().as_uuid(),
        name: target.name().as_str().to_owned(),
        spec: match target.spec() {
            TargetSpec::Http {
                url,
                method,
                expected_status,
            } => ProbeSpec::Http {
                url: url.as_str().to_owned(),
                method: match method {
                    HttpMethod::Get => ProbeHttpMethod::Get,
                    HttpMethod::Head => ProbeHttpMethod::Head,
                },
                expected_status: expected_status.map(StatusCode::get),
            },
            TargetSpec::Tcp { host, port } => ProbeSpec::Tcp {
                host: host.as_str().to_owned(),
                port: port.get(),
            },
            TargetSpec::Icmp { host } => ProbeSpec::Icmp {
                host: host.as_str().to_owned(),
            },
        },
        interval_secs: target.interval().get(),
        timeout_ms: target.timeout().get(),
    }
}

fn map_batch(items: &[ResultItem], agent_id: AgentId) -> Option<Vec<CheckResult>> {
    if items.is_empty() || items.len() > 100 {
        return None;
    }
    items.iter().map(|item| to_domain(item, agent_id)).collect()
}

fn to_domain(item: &ResultItem, agent_id: AgentId) -> Option<CheckResult> {
    let checked_at = from_unix_seconds(item.checked_at_unix)?;
    let mode = CheckMode::try_from(item.mode.as_str()).ok()?;
    let status_code = item.status_code.map(StatusCode::new).transpose().ok()?;
    let failure = match item.error_kind {
        Some(kind) => {
            let message = item
                .error_message
                .clone()
                .unwrap_or_else(|| kind.as_str().to_owned());
            Some(CheckFailure::new(map_failure(kind), truncate(&message, 512)).ok()?)
        }
        None => None,
    };
    let outcome = CheckOutcome::new(
        item.ok,
        LatencyMs::new(item.latency_ms),
        status_code,
        mode,
        failure,
    )
    .ok()?;
    Some(CheckResult::new(
        CheckResultId::from_uuid(item.result_id),
        TargetId::from_uuid(item.target_id),
        agent_id,
        checked_at,
        outcome,
    ))
}

fn map_failure(kind: ProbeFailureKind) -> CheckFailureKind {
    match kind {
        ProbeFailureKind::Timeout => CheckFailureKind::Timeout,
        ProbeFailureKind::ConnectionRefused => CheckFailureKind::Connection,
        ProbeFailureKind::DnsFailure => CheckFailureKind::Dns,
        ProbeFailureKind::TlsFailure
        | ProbeFailureKind::HttpStatus
        | ProbeFailureKind::Network
        | ProbeFailureKind::Unsupported => CheckFailureKind::Other,
    }
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

async fn send(sink: &mut SplitSink<WebSocket, Message>, message: &ServerMessage) -> bool {
    let Ok(text) = serde_json::to_string(message) else {
        return false;
    };
    sink.send(Message::Text(text.into())).await.is_ok()
}

async fn close(sink: &mut SplitSink<WebSocket, Message>, code: u16, reason: &'static str) {
    let frame = CloseFrame {
        code,
        reason: reason.into(),
    };
    let _ = sink.send(Message::Close(Some(frame))).await;
}
