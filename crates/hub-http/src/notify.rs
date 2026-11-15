use std::time::Duration;

use async_trait::async_trait;
use hmac::{Hmac, KeyInit, Mac};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::Mailbox,
    transport::smtp::authentication::Credentials,
};
use pingushka_application::alerts::{ChannelKind, NotificationSender};
use serde_json::Value;
use sha2::Sha256;

pub struct Notifier {
    client: reqwest::Client,
    telegram_api_base: String,
}

impl Notifier {
    pub fn new(telegram_api_base: impl Into<String>) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            telegram_api_base: telegram_api_base.into().trim_end_matches('/').to_owned(),
        })
    }

    async fn telegram(&self, config: &Value, payload: &Value) -> Result<(), String> {
        let token = required(config, "bot_token")?;
        let chat_id = required(config, "chat_id")?;
        let text = message_text(payload);
        let response = self
            .client
            .post(format!(
                "{}/bot{}/sendMessage",
                self.telegram_api_base, token
            ))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "disable_web_page_preview": true,
            }))
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("telegram returned {}", response.status()))
        }
    }

    async fn webhook(&self, config: &Value, payload: &Value) -> Result<(), String> {
        let url = required(config, "url")?;
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err("webhook url must be http or https".to_owned());
        }
        let body = serde_json::to_vec(payload).map_err(|error| error.to_string())?;
        let mut request = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .header("user-agent", "pingushka");
        if let Some(secret) = config.get("secret").and_then(Value::as_str)
            && !secret.is_empty()
        {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .map_err(|_| "invalid webhook secret".to_owned())?;
            mac.update(&body);
            let signature = hex_encode(&mac.finalize().into_bytes());
            request = request.header("x-pingushka-signature", format!("sha256={signature}"));
        }
        let response = request
            .body(body)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("webhook returned {}", response.status()))
        }
    }

    async fn email(&self, config: &Value, payload: &Value) -> Result<(), String> {
        let host = required(config, "host")?;
        let from: Mailbox = required(config, "from")?
            .parse()
            .map_err(|_| "invalid email from address".to_owned())?;
        let to: Mailbox = required(config, "to")?
            .parse()
            .map_err(|_| "invalid email to address".to_owned())?;
        let port = config.get("port").and_then(Value::as_u64).unwrap_or(587);
        let port = u16::try_from(port).map_err(|_| "invalid email port".to_owned())?;
        let tls = config
            .get("tls")
            .and_then(Value::as_str)
            .unwrap_or("starttls");
        let transport = match tls {
            "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host).port(port),
            "implicit" => AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                .map_err(|error| error.to_string())?
                .port(port),
            _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                .map_err(|error| error.to_string())?
                .port(port),
        };
        let username = config.get("username").and_then(Value::as_str).unwrap_or("");
        let password = config.get("password").and_then(Value::as_str).unwrap_or("");
        let transport = if username.is_empty() {
            transport.build()
        } else {
            transport
                .credentials(Credentials::new(username.to_owned(), password.to_owned()))
                .build()
        };
        let subject = message_subject(payload);
        let email = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .body(message_text(payload))
            .map_err(|error| error.to_string())?;
        transport
            .send(email)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[async_trait]
impl NotificationSender for Notifier {
    async fn send(&self, kind: ChannelKind, config: &Value, payload: &Value) -> Result<(), String> {
        match kind {
            ChannelKind::Telegram => self.telegram(config, payload).await,
            ChannelKind::Webhook => self.webhook(config, payload).await,
            ChannelKind::Email => self.email(config, payload).await,
        }
    }
}

fn required<'a>(config: &'a Value, key: &str) -> Result<&'a str, String> {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

fn message_subject(payload: &Value) -> String {
    let event = payload
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or("down");
    let name = payload
        .get("target")
        .and_then(|target| target.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("target");
    if event == "recovered" {
        format!("[pingushka] RECOVERED {name}")
    } else {
        format!("[pingushka] DOWN {name}")
    }
}

fn message_text(payload: &Value) -> String {
    let event = payload
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or("down");
    let target = payload.get("target").cloned().unwrap_or(Value::Null);
    let name = target
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("target");
    let kind = target
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("check");
    let reference = target.get("ref").and_then(Value::as_str).unwrap_or("");
    let failures = payload.get("failures").and_then(Value::as_u64).unwrap_or(0);
    let since = payload.get("since").and_then(Value::as_str).unwrap_or("");
    if event == "recovered" {
        format!("pingushka: {name} ({kind} {reference}) is back UP.\nDown since: {since}")
    } else {
        format!(
            "pingushka: {name} ({kind} {reference}) is DOWN after {failures} consecutive failed checks.\nSince: {since}"
        )
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[(byte >> 4) as usize]));
        encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    encoded
}
