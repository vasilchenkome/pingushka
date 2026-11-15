use std::{
    collections::HashMap,
    fmt,
    path::Path,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use pingushka_protocol::{ClientMessage, PROTOCOL_VERSION, ProbeTarget, ServerMessage};
use tokio::{
    net::TcpStream,
    sync::{Mutex, Notify, mpsc, watch},
    task::JoinHandle,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{buffer::ResultBuffer, checks::Checker, config::AgentConfig};

const MAX_BATCH: usize = 100;
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BACKOFF_SECS: f64 = 60.0;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct TargetTask {
    trigger: mpsc::Sender<()>,
    handle: JoinHandle<()>,
}

struct Shared {
    buffer: Mutex<ResultBuffer>,
    notify: Notify,
    checker: Checker,
    tasks: StdMutex<HashMap<Uuid, TargetTask>>,
    shutdown: watch::Receiver<bool>,
}

impl Shared {
    fn apply_targets(self: &Arc<Self>, targets: &[ProbeTarget]) {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (_, task) in tasks.drain() {
            task.handle.abort();
        }
        for target in targets {
            let shared = Arc::clone(self);
            let target = target.clone();
            let target_id = target.target_id;
            let mut shutdown = self.shutdown.clone();
            let (trigger, mut triggered) = mpsc::channel::<()>(4);
            let interval = Duration::from_secs(u64::from(target.interval_secs.max(5)));
            let handle = tokio::spawn(async move {
                loop {
                    let item = shared.checker.run(&target).await;
                    {
                        let mut buffer = shared.buffer.lock().await;
                        buffer.push(item);
                    }
                    shared.notify.notify_one();
                    tokio::select! {
                        _ = tokio::time::sleep(interval) => {}
                        Some(()) = triggered.recv() => {}
                        changed = shutdown.changed() => {
                            if changed.is_err() || *shutdown.borrow() {
                                break;
                            }
                        }
                    }
                }
            });
            tasks.insert(target_id, TargetTask { trigger, handle });
        }
    }

    async fn trigger(&self, target_id: Uuid) {
        let trigger = {
            let tasks = self
                .tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            tasks.get(&target_id).map(|task| task.trigger.clone())
        };
        if let Some(trigger) = trigger {
            let _ = trigger.send(()).await;
        }
    }

    fn abort_all(&self) {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (_, task) in tasks.drain() {
            task.handle.abort();
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SessionError {
    Config(String),
    Socket(String),
    Protocol(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "configuration error: {error}"),
            Self::Socket(error) => write!(formatter, "websocket error: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
        }
    }
}

impl std::error::Error for SessionError {}

pub async fn run(config: AgentConfig, buffer_path: &Path) {
    let buffer = ResultBuffer::new(Some(buffer_path.to_path_buf()), config.buffer.capacity);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shared = Arc::new(Shared {
        buffer: Mutex::new(buffer),
        notify: Notify::new(),
        checker: Checker::new(config.limits.max_concurrent_checks),
        tasks: StdMutex::new(HashMap::new()),
        shutdown: shutdown_rx,
    });

    let mut supervisor = tokio::spawn(supervise(config, Arc::clone(&shared)));
    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("shutdown requested"),
        _ = &mut supervisor => warn!("agent supervisor stopped unexpectedly"),
    }
    let _ = shutdown_tx.send(true);

    let _ = tokio::time::timeout(Duration::from_secs(10), supervisor).await;
    shared.abort_all();
    let buffer = shared.buffer.lock().await;
    info!(pending = buffer.len(), "agent stopped");
}

async fn supervise(config: AgentConfig, shared: Arc<Shared>) -> Result<(), SessionError> {
    let mut backoff = 0_u32;
    loop {
        if *shared.shutdown.borrow() {
            return Ok(());
        }
        match session(&config, Arc::clone(&shared)).await {
            Ok(true) => {
                backoff = 0;
                info!("hub connection closed");
            }
            Ok(false) => {
                warn!("hub connection closed before welcome");
            }
            Err(error) => {
                warn!(%error, "hub connection failed");
            }
        }
        if *shared.shutdown.borrow() {
            return Ok(());
        }
        if !wait_backoff(&shared.shutdown, backoff).await {
            return Ok(());
        }
        backoff = backoff.saturating_add(1).min(6);
    }
}

async fn session(config: &AgentConfig, shared: Arc<Shared>) -> Result<bool, SessionError> {
    let endpoint = config
        .websocket_endpoint()
        .map_err(|error| SessionError::Config(error.to_string()))?;
    let (mut socket, _response) = connect_async(&endpoint)
        .await
        .map_err(|error| SessionError::Socket(error.to_string()))?;

    let hello = ClientMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
        agent_id: config.hub.agent_id,
        token: config.hub.credential.clone(),
        agent_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    send_json(&mut socket, &hello).await?;

    let welcome = tokio::time::timeout(HELLO_TIMEOUT, next_message(&mut socket)).await;
    let welcome = match welcome {
        Ok(Some(Ok(message))) => message,
        Ok(Some(Err(error))) => return Err(SessionError::Socket(error)),
        Ok(None) => return Err(SessionError::Protocol("hub closed the socket".into())),
        Err(_) => return Err(SessionError::Protocol("welcome timed out".into())),
    };

    let targets = match welcome {
        ServerMessage::Welcome { targets, .. } => targets,
        ServerMessage::Configured { targets, .. } => targets,
        other => {
            return Err(SessionError::Protocol(format!(
                "expected welcome, got {other:?}"
            )));
        }
    };
    shared.apply_targets(&targets);
    info!(targets = targets.len(), "hub session established");

    let mut inflight = false;
    let mut shutdown = shared.shutdown.clone();
    let shutting_down = *shutdown.borrow();
    loop {
        tokio::select! {
            incoming = next_message(&mut socket) => {
                match incoming {
                    Some(Ok(message)) => {
                        match message {
                            ServerMessage::Configured { targets, config_revision } => {
                                shared.apply_targets(&targets);
                                info!(targets = targets.len(), config_revision, "configuration applied");
                            }
                            ServerMessage::Ack { up_to_result_id } => {
                                let acknowledged = {
                                    let mut buffer = shared.buffer.lock().await;
                                    buffer.ack(up_to_result_id)
                                };
                                inflight = false;
                                if acknowledged == 0 {
                                    warn!(%up_to_result_id, "acknowledgement did not match buffered result");
                                }
                                shared.notify.notify_one();
                            }
                            ServerMessage::CheckNow { target_id } => {
                                shared.trigger(target_id).await;
                            }
                            ServerMessage::Ping { .. } => {
                                let pong = ClientMessage::Pong { sent_at_unix: time::OffsetDateTime::now_utc().unix_timestamp() };
                                send_json(&mut socket, &pong).await?;
                            }
                            ServerMessage::Welcome { .. } => {}
                        }
                    }
                    Some(Err(error)) => return Err(SessionError::Socket(error)),
                    None => {
                        if shutting_down {
                            break;
                        }
                        return Err(SessionError::Socket("hub closed the socket".into()));
                    }
                }
            }
            _ = shared.notify.notified() => {
                if !inflight {
                    flush(&mut socket, &shared, &mut inflight).await?;
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    drain(&mut socket, &shared).await;
                    break;
                }
            }
        }
    }
    Ok(true)
}

async fn flush(
    socket: &mut Socket,
    shared: &Shared,
    inflight: &mut bool,
) -> Result<(), SessionError> {
    let (batch, dropped) = {
        let buffer = shared.buffer.lock().await;
        (buffer.batch(MAX_BATCH), buffer.dropped())
    };
    if batch.is_empty() {
        return Ok(());
    }
    let message = ClientMessage::ResultsBatch {
        results: batch,
        dropped_results_count: dropped,
    };
    send_json(socket, &message).await?;
    *inflight = true;
    Ok(())
}

async fn drain(socket: &mut Socket, shared: &Shared) {
    let deadline = tokio::time::Instant::now() + DRAIN_TIMEOUT;
    let mut inflight = false;
    while !shared.buffer.lock().await.is_empty() {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        let batch = {
            let buffer = shared.buffer.lock().await;
            buffer.batch(MAX_BATCH)
        };
        if batch.is_empty() {
            break;
        }
        let message = ClientMessage::ResultsBatch {
            results: batch,
            dropped_results_count: 0,
        };
        if send_json(socket, &message).await.is_err() {
            break;
        }
        inflight = true;
        let wait = tokio::time::timeout_at(deadline, next_message(socket)).await;
        match wait {
            Ok(Some(Ok(ServerMessage::Ack { up_to_result_id }))) => {
                shared.buffer.lock().await.ack(up_to_result_id);
                inflight = false;
            }
            Ok(Some(Ok(ServerMessage::Ping { .. }))) => {
                let pong = ClientMessage::Pong {
                    sent_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
                };
                let _ = send_json(socket, &pong).await;
            }
            _ => break,
        }
    }
    if inflight {
        warn!("shutdown drain ended with unacknowledged results");
    }
}

async fn next_message(socket: &mut Socket) -> Option<Result<ServerMessage, String>> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return Some(
                    serde_json::from_str(text.as_str()).map_err(|error| error.to_string()),
                );
            }
            Some(Ok(Message::Close(_))) | None => return None,
            Some(Ok(Message::Ping(payload))) => {
                let _ = socket.send(Message::Pong(payload)).await;
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => return Some(Err(error.to_string())),
        }
    }
}

async fn send_json(socket: &mut Socket, message: &ClientMessage) -> Result<(), SessionError> {
    let text = serde_json::to_string(message)
        .map_err(|error| SessionError::Protocol(error.to_string()))?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|error| SessionError::Socket(error.to_string()))
}

async fn wait_backoff(shutdown: &watch::Receiver<bool>, attempt: u32) -> bool {
    let ceiling = (2_f64.powi(i32::try_from(attempt).unwrap_or(6)) * 1.0).min(MAX_BACKOFF_SECS);
    let jitter = rand::random::<f64>() * ceiling;
    let delay = Duration::from_secs_f64(jitter.max(0.2));
    info!(delay_ms = delay.as_millis() as u64, "reconnecting to hub");
    let mut shutdown = shutdown.clone();
    tokio::select! {
        _ = tokio::time::sleep(delay) => true,
        changed = shutdown.changed() => !(changed.is_err() || *shutdown.borrow()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_ceiling_grows_and_caps() {
        let mut previous = 0.0;
        for attempt in 0..8 {
            let ceiling = (2_f64.powi(attempt) * 1.0).min(MAX_BACKOFF_SECS);
            assert!(ceiling >= previous);
            previous = ceiling;
        }
        assert_eq!(previous, MAX_BACKOFF_SECS);
    }
}
