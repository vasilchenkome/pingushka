use std::{env, time::Duration};

use futures_util::{SinkExt, StreamExt};
use pingushka_protocol::{
    ClientMessage, PROTOCOL_VERSION, ProbeFailureKind, ProbeMode, ResultItem, ServerMessage,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = env::var("HUB_WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:8080/api/agent/ws".into());
    let agent_id: Uuid = env::var("AGENT_ID")?.parse()?;
    let token = env::var("AGENT_TOKEN")?;
    let target_id: Uuid = env::var("TARGET_ID")?.parse()?;
    let linger: u64 = env::var("LINGER_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);

    let (mut socket, _) = connect_async(&url).await?;
    let hello = ClientMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
        agent_id,
        token,
        agent_version: "sim-0.1.0".to_owned(),
    };
    socket
        .send(Message::Text(serde_json::to_string(&hello)?.into()))
        .await?;
    println!("hello sent");

    let results = ClientMessage::ResultsBatch {
        results: vec![
            ResultItem {
                result_id: Uuid::new_v4(),
                target_id,
                checked_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
                ok: true,
                latency_ms: 42,
                status_code: Some(200),
                mode: ProbeMode::Http,
                error_kind: None,
                error_message: None,
            },
            ResultItem {
                result_id: Uuid::new_v4(),
                target_id,
                checked_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
                ok: false,
                latency_ms: 5_000,
                status_code: None,
                mode: ProbeMode::Http,
                error_kind: Some(ProbeFailureKind::Timeout),
                error_message: Some("simulated timeout".to_owned()),
            },
        ],
        dropped_results_count: 0,
    };

    let mut sent_results = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(linger);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            println!("linger finished");
            break;
        }
        match tokio::time::timeout(remaining, socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let message: ServerMessage = serde_json::from_str(text.as_str())?;
                match &message {
                    ServerMessage::Welcome { targets, .. } => {
                        println!("welcome with {} targets", targets.len());
                        if !sent_results {
                            sent_results = true;
                            socket
                                .send(Message::Text(serde_json::to_string(&results)?.into()))
                                .await?;
                            println!("results batch sent");
                        }
                    }
                    ServerMessage::Ack { up_to_result_id } => {
                        println!("ack {up_to_result_id}");
                    }
                    ServerMessage::Ping { .. } => {
                        socket
                            .send(Message::Text(
                                serde_json::to_string(&ClientMessage::Pong { sent_at_unix: 0 })?
                                    .into(),
                            ))
                            .await?;
                        println!("ping answered");
                    }
                    ServerMessage::Configured { targets, .. } => {
                        println!("configured with {} targets", targets.len());
                    }
                    ServerMessage::CheckNow { target_id } => {
                        println!("check_now {target_id}");
                    }
                }
            }
            Ok(Some(Ok(Message::Close(frame)))) => {
                println!("closed by hub: {frame:?}");
                break;
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(error))) => {
                println!("socket error: {error}");
                break;
            }
            Ok(None) | Err(_) => break,
        }
    }
    Ok(())
}
