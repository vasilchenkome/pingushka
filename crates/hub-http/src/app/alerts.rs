use std::{collections::HashSet, sync::Arc};

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
};
use pingushka_application::alerts::{AlertChannel, AlertRuleRecord, ChannelKind, RuleView};
use pingushka_domain::{
    Timestamp,
    alert::{AlertChannelName, AlertMode, AlertRule, AlertScope},
    identity::{TargetGroupId, TargetId},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::app::{AppState, error::ApiError, session::AuthSession};

const DEFAULT_FAILURE_THRESHOLD: u16 = 3;
const DEFAULT_RECOVERY_THRESHOLD: u16 = 1;
const DEFAULT_COOLDOWN_SECS: u32 = 900;

#[derive(Deserialize)]
pub struct ChannelRequest {
    name: String,
    kind: String,
    config: Value,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

const fn default_enabled() -> bool {
    true
}

#[derive(Serialize)]
pub struct ChannelDto {
    id: Uuid,
    name: String,
    kind: &'static str,
    enabled: bool,
    configured: bool,
    updated_at: String,
}

#[derive(Serialize)]
pub struct ChannelTestDto {
    ok: bool,
    message: String,
}

#[derive(Deserialize)]
pub struct RuleRequest {
    #[serde(default)]
    target_id: Option<Uuid>,
    #[serde(default)]
    group_id: Option<Uuid>,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_failure_threshold")]
    failure_threshold: u16,
    #[serde(default = "default_recovery_threshold")]
    recovery_threshold: u16,
    #[serde(default = "default_cooldown")]
    cooldown_secs: u32,
    #[serde(default)]
    channel_ids: Vec<Uuid>,
}

fn default_mode() -> String {
    "all".to_owned()
}

const fn default_failure_threshold() -> u16 {
    DEFAULT_FAILURE_THRESHOLD
}

const fn default_recovery_threshold() -> u16 {
    DEFAULT_RECOVERY_THRESHOLD
}

const fn default_cooldown() -> u32 {
    DEFAULT_COOLDOWN_SECS
}

#[derive(Serialize)]
pub struct RuleDto {
    id: Uuid,
    scope: &'static str,
    target_id: Option<Uuid>,
    group_id: Option<Uuid>,
    target_name: Option<String>,
    target_kind: Option<String>,
    target_ref: Option<String>,
    group_name: Option<String>,
    enabled: bool,
    mode: &'static str,
    failure_threshold: u16,
    recovery_threshold: u16,
    cooldown_secs: u32,
    channel_ids: Vec<Uuid>,
    state: StateDto,
}

#[derive(Serialize)]
pub struct StateDto {
    status: &'static str,
    consecutive_failures: u16,
    consecutive_successes: u16,
    last_result_at: Option<String>,
    last_notified_at: Option<String>,
    opened_at: Option<String>,
}

#[derive(Deserialize)]
pub struct EventsQuery {
    limit: Option<u16>,
}

#[derive(Serialize)]
pub struct EventDto {
    id: Uuid,
    event: &'static str,
    target_name: Option<String>,
    group_name: Option<String>,
    channel_name: String,
    status: String,
    attempts: u16,
    last_error: Option<String>,
    created_at: String,
    sent_at: Option<String>,
}

#[derive(Serialize)]
pub struct RetryDto {
    retried: bool,
}

pub async fn list_channels(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
) -> Result<Json<Vec<ChannelDto>>, ApiError> {
    let channels = state
        .alerts
        .list_channels()
        .await
        .map_err(ApiError::from_port)?;
    Ok(Json(channels.iter().map(channel_dto).collect()))
}

pub async fn create_channel(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    payload: Result<Json<ChannelRequest>, JsonRejection>,
) -> Result<Json<ChannelDto>, ApiError> {
    let Json(request) = payload?;
    let name = AlertChannelName::new(&request.name)
        .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let (kind, config) = normalize_config(&request.kind, request.config)?;
    let plaintext = serde_json::to_vec(&config)
        .map_err(|_| ApiError::internal("cannot serialize channel config"))?;
    let encrypted = state
        .cipher
        .encrypt(&plaintext)
        .map_err(|_| ApiError::internal("cannot encrypt channel config"))?;
    let now = state.clock.now();
    let channel = AlertChannel {
        id: Uuid::new_v4(),
        name: name.as_str().to_owned(),
        kind,
        config_encrypted: encrypted,
        enabled: request.enabled,
        created_at: now,
        updated_at: now,
    };
    state
        .alerts
        .save_channel(&channel)
        .await
        .map_err(ApiError::from_port)?;
    Ok(Json(channel_dto(&channel)))
}

pub async fn update_channel(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
    payload: Result<Json<ChannelRequest>, JsonRejection>,
) -> Result<Json<ChannelDto>, ApiError> {
    let Json(request) = payload?;
    let name = AlertChannelName::new(&request.name)
        .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let existing = state
        .alerts
        .list_channels()
        .await
        .map_err(ApiError::from_port)?
        .into_iter()
        .find(|channel| channel.id == id)
        .ok_or_else(|| ApiError::not_found("channel not found"))?;
    let (kind, config) = normalize_config(&request.kind, request.config)?;
    let plaintext = serde_json::to_vec(&config)
        .map_err(|_| ApiError::internal("cannot serialize channel config"))?;
    let encrypted = state
        .cipher
        .encrypt(&plaintext)
        .map_err(|_| ApiError::internal("cannot encrypt channel config"))?;
    let channel = AlertChannel {
        id,
        name: name.as_str().to_owned(),
        kind,
        config_encrypted: encrypted,
        enabled: request.enabled,
        created_at: existing.created_at,
        updated_at: state.clock.now(),
    };
    state
        .alerts
        .save_channel(&channel)
        .await
        .map_err(ApiError::from_port)?;
    Ok(Json(channel_dto(&channel)))
}

pub async fn delete_channel(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if state
        .alerts
        .delete_channel(id)
        .await
        .map_err(ApiError::from_port)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("channel not found"))
    }
}

pub async fn test_channel(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<Json<ChannelTestDto>, ApiError> {
    let channel = state
        .alerts
        .list_channels()
        .await
        .map_err(ApiError::from_port)?
        .into_iter()
        .find(|channel| channel.id == id)
        .ok_or_else(|| ApiError::not_found("channel not found"))?;
    let plaintext = state
        .cipher
        .decrypt(&channel.config_encrypted)
        .map_err(|_| ApiError::internal("cannot decrypt channel config"))?;
    let config: Value = serde_json::from_slice(&plaintext)
        .map_err(|_| ApiError::internal("stored channel config is invalid"))?;
    let payload = json!({
        "event": "down",
        "target": {
            "id": Uuid::nil(),
            "name": "pingushka test",
            "kind": "manual",
            "ref": "test notification",
        },
        "failures": 1,
        "since": state.clock.now().format(&Rfc3339).unwrap_or_default(),
        "agents": [],
    });
    match state.notifier.send(channel.kind, &config, &payload).await {
        Ok(()) => Ok(Json(ChannelTestDto {
            ok: true,
            message: "test notification delivered".to_owned(),
        })),
        Err(error) => Ok(Json(ChannelTestDto {
            ok: false,
            message: error,
        })),
    }
}

pub async fn list_rules(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
) -> Result<Json<Vec<RuleDto>>, ApiError> {
    let views = state
        .alerts
        .rule_views()
        .await
        .map_err(ApiError::from_port)?;
    Ok(Json(views.iter().map(rule_dto).collect()))
}

pub async fn upsert_rule(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    payload: Result<Json<RuleRequest>, JsonRejection>,
) -> Result<Json<RuleDto>, ApiError> {
    let Json(request) = payload?;
    let scope = match (request.target_id, request.group_id) {
        (Some(target_id), None) => AlertScope::Target(TargetId::from_uuid(target_id)),
        (None, Some(group_id)) => AlertScope::Group(TargetGroupId::from_uuid(group_id)),
        _ => {
            return Err(ApiError::unprocessable(
                "exactly one of target_id or group_id is required",
            ));
        }
    };
    match scope {
        AlertScope::Target(target_id) => {
            let targets = state.targets.list().await.map_err(ApiError::from_port)?;
            if !targets.iter().any(|target| target.id() == target_id) {
                return Err(ApiError::not_found("target not found"));
            }
        }
        AlertScope::Group(group_id) => {
            let groups = state
                .groups
                .list_with_counts()
                .await
                .map_err(ApiError::from_port)?;
            if !groups.iter().any(|group| group.group().id() == group_id) {
                return Err(ApiError::not_found("group not found"));
            }
        }
    }
    let mode = AlertMode::try_from(request.mode.as_str())
        .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let now = state.clock.now();
    let views = state
        .alerts
        .rule_views()
        .await
        .map_err(ApiError::from_port)?;
    let existing = views.iter().find(|view| view.rule.scope() == scope);
    let mut rule = if let Some(view) = existing {
        view.rule.clone()
    } else {
        AlertRule::new(
            scope,
            mode,
            request.failure_threshold,
            request.recovery_threshold,
            request.cooldown_secs,
            now,
        )
        .map_err(|error| ApiError::unprocessable(error.to_string()))?
    };
    rule.update(
        mode,
        request.failure_threshold,
        request.recovery_threshold,
        request.cooldown_secs,
        request.enabled,
        now,
    )
    .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let available: HashSet<Uuid> = state
        .alerts
        .list_channels()
        .await
        .map_err(ApiError::from_port)?
        .into_iter()
        .map(|channel| channel.id)
        .collect();
    if let Some(unknown) = request
        .channel_ids
        .iter()
        .find(|channel_id| !available.contains(channel_id))
    {
        return Err(ApiError::unprocessable(format!(
            "unknown channel: {unknown}"
        )));
    }
    let id = state
        .alerts
        .save_rule(
            &AlertRuleRecord {
                id: existing.map_or_else(Uuid::new_v4, |view| view.id),
                rule: rule.clone(),
            },
            &request.channel_ids,
        )
        .await
        .map_err(ApiError::from_port)?;
    let view = state
        .alerts
        .rule_views()
        .await
        .map_err(ApiError::from_port)?
        .into_iter()
        .find(|view| view.id == id)
        .ok_or_else(|| ApiError::internal("saved rule not found"))?;
    Ok(Json(rule_dto(&view)))
}

pub async fn delete_rule(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(target_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if state
        .alerts
        .delete_rule_for_target(TargetId::from_uuid(target_id))
        .await
        .map_err(ApiError::from_port)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("rule not found"))
    }
}

pub async fn delete_rule_by_id(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if state
        .alerts
        .delete_rule(id)
        .await
        .map_err(ApiError::from_port)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("rule not found"))
    }
}

pub async fn list_events(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<EventDto>>, ApiError> {
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let events = state
        .alerts
        .list_events(limit)
        .await
        .map_err(ApiError::from_port)?;
    Ok(Json(
        events
            .iter()
            .map(|event| EventDto {
                id: event.id,
                event: event.event.as_str(),
                target_name: event.target_name.clone(),
                group_name: event.group_name.clone(),
                channel_name: event.channel_name.clone(),
                status: event.status.clone(),
                attempts: event.attempts,
                last_error: event.last_error.clone(),
                created_at: timestamp(&event.created_at),
                sent_at: event.sent_at.map(|value| timestamp(&value)),
            })
            .collect(),
    ))
}

pub async fn retry_event(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<Json<RetryDto>, ApiError> {
    let retried = state
        .alerts
        .retry_event(id)
        .await
        .map_err(ApiError::from_port)?;
    if !retried {
        return Err(ApiError::not_found("failed event not found"));
    }
    Ok(Json(RetryDto { retried }))
}

fn channel_dto(channel: &AlertChannel) -> ChannelDto {
    ChannelDto {
        id: channel.id,
        name: channel.name.clone(),
        kind: channel.kind.as_str(),
        enabled: channel.enabled,
        configured: !channel.config_encrypted.is_empty(),
        updated_at: timestamp(&channel.updated_at),
    }
}

fn rule_dto(view: &RuleView) -> RuleDto {
    RuleDto {
        id: view.id,
        scope: view.rule.scope().kind(),
        target_id: view.rule.target_id().map(|id| id.as_uuid()),
        group_id: view.rule.group_id().map(|id| id.as_uuid()),
        target_name: view.target_name.clone(),
        target_kind: view.target_kind.clone(),
        target_ref: view.target_ref.clone(),
        group_name: view.group_name.clone(),
        enabled: view.rule.is_enabled(),
        mode: view.rule.mode().as_str(),
        failure_threshold: view.rule.failure_threshold(),
        recovery_threshold: view.rule.recovery_threshold(),
        cooldown_secs: view.rule.cooldown_secs(),
        channel_ids: view.channels.clone(),
        state: StateDto {
            status: view.state.status.as_str(),
            consecutive_failures: view.state.failures,
            consecutive_successes: view.state.successes,
            last_result_at: view.state.last_result_at.map(|value| timestamp(&value)),
            last_notified_at: view.state.last_notified_at.map(|value| timestamp(&value)),
            opened_at: view.state.opened_at.map(|value| timestamp(&value)),
        },
    }
}

fn timestamp(value: &Timestamp) -> String {
    value.format(&Rfc3339).unwrap_or_default()
}

fn normalize_config(kind: &str, config: Value) -> Result<(ChannelKind, Value), ApiError> {
    let channel_kind = ChannelKind::try_from(kind)
        .map_err(|_| ApiError::unprocessable("kind must be telegram, webhook or email"))?;
    let object = config
        .as_object()
        .ok_or_else(|| ApiError::unprocessable("config must be an object"))?;
    let normalized = match channel_kind {
        ChannelKind::Telegram => {
            let bot_token = required(object.get("bot_token"), "bot_token")?;
            let chat_id = required(object.get("chat_id"), "chat_id")?;
            json!({ "bot_token": bot_token, "chat_id": chat_id })
        }
        ChannelKind::Webhook => {
            let url = required(object.get("url"), "url")?;
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(ApiError::unprocessable("url must be http or https"));
            }
            let secret = object
                .get("secret")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            json!({ "url": url, "secret": secret })
        }
        ChannelKind::Email => {
            let host = required(object.get("host"), "host")?;
            let from = required(object.get("from"), "from")?;
            let to = required(object.get("to"), "to")?;
            let port = object.get("port").and_then(Value::as_u64).unwrap_or(587);
            let port = u16::try_from(port)
                .map_err(|_| ApiError::unprocessable("port must be a valid u16"))?;
            let tls = object
                .get("tls")
                .and_then(Value::as_str)
                .unwrap_or("starttls");
            if !matches!(tls, "starttls" | "implicit" | "none") {
                return Err(ApiError::unprocessable(
                    "tls must be starttls, implicit or none",
                ));
            }
            let username = object.get("username").and_then(Value::as_str).unwrap_or("");
            let password = object.get("password").and_then(Value::as_str).unwrap_or("");
            json!({
                "host": host,
                "port": port,
                "from": from,
                "to": to,
                "tls": tls,
                "username": username,
                "password": password,
            })
        }
    };
    Ok((channel_kind, normalized))
}

fn required<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str, ApiError> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::unprocessable(format!("{field} is required")))
}
