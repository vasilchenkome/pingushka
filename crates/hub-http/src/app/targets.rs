use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::IntoResponse,
};
use pingushka_application::queries::{HistoryFilter, SummaryFilter};
use pingushka_application::use_cases::{GroupChange, TagChange};
use pingushka_domain::{
    check::{CheckResult, StatusCode as HttpStatus},
    identity::{AgentId, TargetGroupId, TargetId},
    organization::{TargetOrganization, TargetTagSet},
    target::{
        CheckIntervalSecs, HttpMethod, HttpUrl, ProbeHost, Target, TargetConfig, TargetName,
        TargetSpec, TimeoutMs,
    },
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::{AppState, error::ApiError, session::AuthSession};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpecRequest {
    Http {
        url: String,
        #[serde(default)]
        method: Option<String>,
        #[serde(default)]
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

#[derive(Deserialize)]
pub struct CreateTargetRequest {
    name: String,
    spec: SpecRequest,
    interval_secs: u32,
    timeout_ms: u32,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    agent_ids: Vec<Uuid>,
    #[serde(default)]
    group_id: Option<Uuid>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateTargetRequest {
    name: Option<String>,
    spec: Option<SpecRequest>,
    interval_secs: Option<u32>,
    timeout_ms: Option<u32>,
    enabled: Option<bool>,
    agent_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    group_id: Option<Option<Uuid>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

const fn default_enabled() -> bool {
    true
}

#[derive(Serialize)]
pub struct TargetDto {
    id: Uuid,
    name: String,
    spec: SpecDto,
    interval_secs: u32,
    timeout_ms: u32,
    enabled: bool,
    agent_ids: Vec<Uuid>,
    group: Option<TargetGroupRefDto>,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
pub struct TargetGroupRefDto {
    id: Uuid,
    name: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpecDto {
    Http {
        url: String,
        method: &'static str,
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

#[derive(Deserialize)]
pub struct HistoryQuery {
    from: Option<i64>,
    to: Option<i64>,
    agent_id: Option<Uuid>,
    limit: Option<u16>,
}

#[derive(Serialize)]
pub struct ResultDto {
    result_id: Uuid,
    target_id: Uuid,
    agent_id: Uuid,
    checked_at: String,
    checked_at_unix: i64,
    ok: bool,
    latency_ms: u32,
    status_code: Option<u16>,
    mode: &'static str,
    error_kind: Option<&'static str>,
    error_message: Option<String>,
}

#[derive(Deserialize)]
pub struct SummaryQuery {
    period: Option<String>,
    target_id: Option<Uuid>,
    agent_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct SummaryDto {
    total_checks: u64,
    successful_checks: u64,
    uptime_percent: Option<f64>,
    average_latency_ms: Option<f64>,
    p95_latency_ms: Option<f64>,
    from: String,
    to: String,
}

#[derive(Serialize)]
pub struct TriggerResponse {
    triggered: usize,
}

#[derive(Deserialize)]
pub struct BulkUpdateRequest {
    target_ids: Vec<Uuid>,
    #[serde(default)]
    group: BulkGroupRequest,
    #[serde(default)]
    tags: BulkTagRequest,
}

#[derive(Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum BulkGroupRequest {
    #[default]
    Keep,
    Clear,
    Set {
        group_id: Uuid,
    },
    Create {
        name: String,
    },
}

#[derive(Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum BulkTagRequest {
    #[default]
    Keep,
    Add {
        values: Vec<String>,
    },
    Remove {
        values: Vec<String>,
    },
    Replace {
        values: Vec<String>,
    },
}

#[derive(Serialize)]
pub struct BulkUpdateResponse {
    updated: usize,
    group_id: Option<Uuid>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
) -> Result<Json<Vec<TargetDto>>, ApiError> {
    let targets = state
        .targets
        .list_with_agents()
        .await
        .map_err(ApiError::from_port)?;
    let groups = state
        .groups
        .list_with_counts()
        .await
        .map_err(ApiError::from_port)?;
    let names: std::collections::HashMap<Uuid, String> = groups
        .iter()
        .map(|group| {
            (
                group.group().id().as_uuid(),
                group.group().name().as_str().to_owned(),
            )
        })
        .collect();
    Ok(Json(
        targets
            .iter()
            .map(|entry| target_dto(entry.target(), entry.agent_ids(), &names))
            .collect(),
    ))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    payload: Result<Json<CreateTargetRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(request) = payload?;
    let config = config_from(
        request.name,
        request.spec,
        request.interval_secs,
        request.timeout_ms,
    )?;
    let organization = organization_from(request.group_id, request.tags)?;
    ensure_group(&state, organization.group_id()).await?;
    let agent_ids: Vec<AgentId> = request
        .agent_ids
        .into_iter()
        .map(AgentId::from_uuid)
        .collect();
    let target = state
        .create_target
        .execute(config, organization, request.enabled, agent_ids.clone())
        .await
        .map_err(ApiError::from_target)?;
    let names = group_names(&state, target.group_id()).await?;
    Ok((
        StatusCode::CREATED,
        Json(target_dto(&target, &unique(agent_ids), &names)),
    ))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
    payload: Result<Json<UpdateTargetRequest>, JsonRejection>,
) -> Result<Json<TargetDto>, ApiError> {
    let Json(request) = payload?;
    let target_id = TargetId::from_uuid(id);
    let current = state
        .targets
        .find(target_id)
        .await
        .map_err(ApiError::from_port)?
        .ok_or_else(|| ApiError::not_found("target was not found"))?;
    let name = request
        .name
        .unwrap_or_else(|| current.name().as_str().to_owned());
    let spec = request.spec.unwrap_or_else(|| spec_request(current.spec()));
    let interval_secs = request.interval_secs.unwrap_or(current.interval().get());
    let timeout_ms = request.timeout_ms.unwrap_or(current.timeout().get());
    let enabled = request.enabled.unwrap_or(current.is_enabled());
    let group_id = match request.group_id {
        Some(value) => value.map(TargetGroupId::from_uuid),
        None => current.group_id(),
    };
    let tags = match request.tags {
        Some(values) => TargetTagSet::parse(values)
            .map_err(|error| ApiError::unprocessable(error.to_string()))?,
        None => current.tag_set().clone(),
    };
    let organization = TargetOrganization::new(group_id, tags);
    ensure_group(&state, organization.group_id()).await?;
    let agent_ids = match request.agent_ids {
        Some(agent_ids) => agent_ids.into_iter().map(AgentId::from_uuid).collect(),
        None => state
            .targets
            .agent_ids(target_id)
            .await
            .map_err(ApiError::from_port)?,
    };
    let config = config_from(name, spec, interval_secs, timeout_ms)?;
    let target = state
        .update_target
        .execute(target_id, config, organization, enabled, agent_ids.clone())
        .await
        .map_err(ApiError::from_target)?;
    let names = group_names(&state, target.group_id()).await?;
    Ok(Json(target_dto(&target, &unique(agent_ids), &names)))
}

pub async fn bulk_update(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    payload: Result<Json<BulkUpdateRequest>, JsonRejection>,
) -> Result<Json<BulkUpdateResponse>, ApiError> {
    let Json(request) = payload?;
    let target_ids: Vec<TargetId> = request
        .target_ids
        .into_iter()
        .map(TargetId::from_uuid)
        .collect();
    let group_change = match request.group {
        BulkGroupRequest::Keep => GroupChange::Keep,
        BulkGroupRequest::Clear => GroupChange::Clear,
        BulkGroupRequest::Set { group_id } => {
            let group_id = TargetGroupId::from_uuid(group_id);
            if state
                .groups
                .find(group_id)
                .await
                .map_err(ApiError::from_port)?
                .is_none()
            {
                return Err(ApiError::not_found("target group not found"));
            }
            GroupChange::Set(group_id)
        }
        BulkGroupRequest::Create { name } => GroupChange::CreateAndSet(
            pingushka_domain::organization::TargetGroupName::new(&name)
                .map_err(|error| ApiError::unprocessable(error.to_string()))?,
        ),
    };
    let tag_values = |values: Vec<String>| {
        TargetTagSet::parse(values).map_err(|error| ApiError::unprocessable(error.to_string()))
    };
    let tag_change = match request.tags {
        BulkTagRequest::Keep => TagChange::Keep,
        BulkTagRequest::Add { values } => TagChange::Add(tag_values(values)?),
        BulkTagRequest::Remove { values } => TagChange::Remove(tag_values(values)?),
        BulkTagRequest::Replace { values } => TagChange::Replace(tag_values(values)?),
    };
    let result = state
        .bulk_update
        .execute(&target_ids, group_change, tag_change)
        .await
        .map_err(ApiError::from_target)?;
    Ok(Json(BulkUpdateResponse {
        updated: result.updated,
        group_id: result.group_id.map(|id| id.as_uuid()),
    }))
}

pub async fn delete_one(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .delete_target
        .execute(TargetId::from_uuid(id))
        .await
        .map_err(ApiError::from_target)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn check_now(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<TriggerResponse>), ApiError> {
    let triggered = state
        .manual_check
        .execute(TargetId::from_uuid(id))
        .await
        .map_err(ApiError::from_target)?;
    Ok((StatusCode::ACCEPTED, Json(TriggerResponse { triggered })))
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
    query: Result<Query<HistoryQuery>, QueryRejection>,
) -> Result<Json<Vec<ResultDto>>, ApiError> {
    let Query(query) = query?;
    let now = state.clock.now();
    let to = match query.to {
        Some(seconds) => crate::system::from_unix_seconds(seconds)
            .ok_or_else(|| ApiError::bad_request("invalid `to` timestamp"))?,
        None => now,
    };
    let from = match query.from {
        Some(seconds) => crate::system::from_unix_seconds(seconds)
            .ok_or_else(|| ApiError::bad_request("invalid `from` timestamp"))?,
        None => to - Duration::from_secs(24 * 3_600),
    };
    let filter = HistoryFilter::new(
        TargetId::from_uuid(id),
        query.agent_id.map(AgentId::from_uuid),
        from,
        to,
        query.limit.unwrap_or(200),
    )
    .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let results = state
        .results
        .history(filter)
        .await
        .map_err(ApiError::from_port)?;
    Ok(Json(results.iter().map(result_dto).collect()))
}

pub async fn summary(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    query: Result<Query<SummaryQuery>, QueryRejection>,
) -> Result<Json<SummaryDto>, ApiError> {
    let Query(query) = query?;
    let window = match query.period.as_deref().unwrap_or("24h") {
        "24h" => Duration::from_secs(24 * 3_600),
        "7d" => Duration::from_secs(7 * 86_400),
        "30d" => Duration::from_secs(30 * 86_400),
        _ => {
            return Err(ApiError::unprocessable(
                "period must be one of 24h, 7d, 30d",
            ));
        }
    };
    let to = state.clock.now();
    let from = to - window;
    let filter = SummaryFilter::new(
        query.target_id.map(TargetId::from_uuid),
        query.agent_id.map(AgentId::from_uuid),
        from,
        to,
    )
    .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let summary = state
        .results
        .summarize(filter)
        .await
        .map_err(ApiError::from_port)?;
    Ok(Json(SummaryDto {
        total_checks: summary.total_checks(),
        successful_checks: summary.successful_checks(),
        uptime_percent: summary.uptime_percent(),
        average_latency_ms: summary.average_latency_ms(),
        p95_latency_ms: summary.p95_latency_ms(),
        from: crate::system::rfc3339(from),
        to: crate::system::rfc3339(to),
    }))
}

fn config_from(
    name: String,
    spec: SpecRequest,
    interval_secs: u32,
    timeout_ms: u32,
) -> Result<TargetConfig, ApiError> {
    let name =
        TargetName::new(&name).map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let spec = match spec {
        SpecRequest::Http {
            url,
            method,
            expected_status,
        } => {
            let url =
                HttpUrl::parse(&url).map_err(|error| ApiError::unprocessable(error.to_string()))?;
            let method = match method.as_deref().map(str::to_ascii_uppercase).as_deref() {
                None | Some("GET") => HttpMethod::Get,
                Some("HEAD") => HttpMethod::Head,
                Some(_) => {
                    return Err(ApiError::unprocessable("http method must be GET or HEAD"));
                }
            };
            let expected_status = match expected_status {
                Some(code) => Some(
                    HttpStatus::new(code)
                        .map_err(|error| ApiError::unprocessable(error.to_string()))?,
                ),
                None => None,
            };
            TargetSpec::Http {
                url,
                method,
                expected_status,
            }
        }
        SpecRequest::Tcp { host, port } => {
            let host = ProbeHost::parse(&host)
                .map_err(|error| ApiError::unprocessable(error.to_string()))?;
            TargetSpec::tcp(host, port)
                .map_err(|error| ApiError::unprocessable(error.to_string()))?
        }
        SpecRequest::Icmp { host } => {
            let host = ProbeHost::parse(&host)
                .map_err(|error| ApiError::unprocessable(error.to_string()))?;
            TargetSpec::icmp(host)
        }
    };
    let interval = CheckIntervalSecs::new(interval_secs)
        .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let timeout =
        TimeoutMs::new(timeout_ms).map_err(|error| ApiError::unprocessable(error.to_string()))?;
    TargetConfig::new(name, spec, interval, timeout)
        .map_err(|error| ApiError::unprocessable(error.to_string()))
}

fn spec_request(spec: &TargetSpec) -> SpecRequest {
    match spec {
        TargetSpec::Http {
            url,
            method,
            expected_status,
        } => SpecRequest::Http {
            url: url.as_str().to_owned(),
            method: Some(
                match method {
                    HttpMethod::Get => "GET",
                    HttpMethod::Head => "HEAD",
                }
                .to_owned(),
            ),
            expected_status: expected_status.map(HttpStatus::get),
        },
        TargetSpec::Tcp { host, port } => SpecRequest::Tcp {
            host: host.as_str().to_owned(),
            port: port.get(),
        },
        TargetSpec::Icmp { host } => SpecRequest::Icmp {
            host: host.as_str().to_owned(),
        },
    }
}

fn spec_dto(spec: &TargetSpec) -> SpecDto {
    match spec {
        TargetSpec::Http {
            url,
            method,
            expected_status,
        } => SpecDto::Http {
            url: url.as_str().to_owned(),
            method: match method {
                HttpMethod::Get => "GET",
                HttpMethod::Head => "HEAD",
            },
            expected_status: expected_status.map(HttpStatus::get),
        },
        TargetSpec::Tcp { host, port } => SpecDto::Tcp {
            host: host.as_str().to_owned(),
            port: port.get(),
        },
        TargetSpec::Icmp { host } => SpecDto::Icmp {
            host: host.as_str().to_owned(),
        },
    }
}

fn target_dto(
    target: &Target,
    agent_ids: &[AgentId],
    groups: &std::collections::HashMap<Uuid, String>,
) -> TargetDto {
    let group = target.group_id().and_then(|group_id| {
        groups
            .get(&group_id.as_uuid())
            .map(|name| TargetGroupRefDto {
                id: group_id.as_uuid(),
                name: name.clone(),
            })
    });
    TargetDto {
        id: target.id().as_uuid(),
        name: target.name().as_str().to_owned(),
        spec: spec_dto(target.spec()),
        interval_secs: target.interval().get(),
        timeout_ms: target.timeout().get(),
        enabled: target.is_enabled(),
        agent_ids: agent_ids.iter().map(|id| id.as_uuid()).collect(),
        group,
        tags: target
            .tags()
            .iter()
            .map(|tag| tag.as_str().to_owned())
            .collect(),
        created_at: crate::system::rfc3339(target.created_at()),
        updated_at: crate::system::rfc3339(target.updated_at()),
    }
}

fn organization_from(
    group_id: Option<Uuid>,
    tags: Vec<String>,
) -> Result<TargetOrganization, ApiError> {
    let tags =
        TargetTagSet::parse(tags).map_err(|error| ApiError::unprocessable(error.to_string()))?;
    Ok(TargetOrganization::new(
        group_id.map(TargetGroupId::from_uuid),
        tags,
    ))
}

async fn ensure_group(state: &AppState, group_id: Option<TargetGroupId>) -> Result<(), ApiError> {
    if let Some(group_id) = group_id
        && state
            .groups
            .find(group_id)
            .await
            .map_err(ApiError::from_port)?
            .is_none()
    {
        return Err(ApiError::not_found("target group not found"));
    }
    Ok(())
}

async fn group_names(
    state: &AppState,
    group_id: Option<TargetGroupId>,
) -> Result<std::collections::HashMap<Uuid, String>, ApiError> {
    let mut names = std::collections::HashMap::new();
    if let Some(group_id) = group_id
        && let Some(group) = state
            .groups
            .find(group_id)
            .await
            .map_err(ApiError::from_port)?
    {
        names.insert(group.id().as_uuid(), group.name().as_str().to_owned());
    }
    Ok(names)
}

fn result_dto(result: &CheckResult) -> ResultDto {
    ResultDto {
        result_id: result.id().as_uuid(),
        target_id: result.target_id().as_uuid(),
        agent_id: result.agent_id().as_uuid(),
        checked_at: crate::system::rfc3339(result.checked_at()),
        checked_at_unix: crate::system::unix_seconds(result.checked_at()),
        ok: result.is_successful(),
        latency_ms: result.latency().get(),
        status_code: result.status_code().map(HttpStatus::get),
        mode: result.mode().as_str(),
        error_kind: result.failure().map(|failure| failure.kind().as_str()),
        error_message: result.failure().map(|failure| failure.message().to_owned()),
    }
}

fn unique(agent_ids: Vec<AgentId>) -> Vec<AgentId> {
    let mut seen = HashSet::new();
    agent_ids
        .into_iter()
        .filter(|id| seen.insert(*id))
        .collect()
}
