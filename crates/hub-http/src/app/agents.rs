use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::IntoResponse,
};
use pingushka_application::ports::AgentControlGateway;
use pingushka_domain::{agent::Agent, identity::AgentId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::{AppState, error::ApiError, session::AuthSession};

#[derive(Serialize)]
pub struct AgentDto {
    id: Uuid,
    name: String,
    connected: bool,
    enrolled: bool,
    last_seen_at: Option<String>,
    version: Option<String>,
}

impl AgentDto {
    fn new(agent: &Agent, connected: bool) -> Self {
        Self {
            id: agent.id().as_uuid(),
            name: agent.name().as_str().to_owned(),
            connected,
            enrolled: agent.token_digest().is_some(),
            last_seen_at: agent.last_seen_at().map(crate::system::rfc3339),
            version: agent.version().map(str::to_owned),
        }
    }
}

#[derive(Serialize)]
pub struct ProvisionResponse {
    agent: AgentDto,
    enrollment_token: String,
    expires_at: String,
    install_command: String,
}

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    name: String,
}

#[derive(Deserialize)]
pub struct EnrollRequest {
    enrollment_token: String,
}

#[derive(Serialize)]
pub struct EnrollResponse {
    agent_id: Uuid,
    credential: String,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
) -> Result<Json<Vec<AgentDto>>, ApiError> {
    let agents = state.agents.list().await.map_err(ApiError::from_port)?;
    Ok(Json(
        agents
            .iter()
            .map(|agent| AgentDto::new(agent, state.registry.is_connected(agent.id())))
            .collect(),
    ))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    payload: Result<Json<CreateAgentRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(request) = payload?;
    let provision = state
        .register_agent
        .execute(&request.name)
        .await
        .map_err(ApiError::from_register_agent)?;
    Ok((
        StatusCode::CREATED,
        Json(provision_response(&state, provision)),
    ))
}

pub async fn rotate(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<Json<ProvisionResponse>, ApiError> {
    let provision = state
        .rotate_agent
        .execute(AgentId::from_uuid(id))
        .await
        .map_err(ApiError::from_rotate)?;
    Ok(Json(provision_response(&state, provision)))
}

pub async fn delete_one(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let agent_id = AgentId::from_uuid(id);
    let existing = state
        .agents
        .find(agent_id)
        .await
        .map_err(ApiError::from_port)?
        .ok_or_else(|| ApiError::not_found("agent was not found"))?;
    let _ = state.registry.disconnect(existing.id()).await;
    if !state
        .agents
        .delete(agent_id)
        .await
        .map_err(ApiError::from_port)?
    {
        return Err(ApiError::not_found("agent was not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn enroll(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<EnrollRequest>, JsonRejection>,
) -> Result<Json<EnrollResponse>, ApiError> {
    let Json(request) = payload?;
    let enrolled = state
        .enroll_agent
        .execute(&request.enrollment_token)
        .await
        .map_err(ApiError::from_enroll)?;
    Ok(Json(EnrollResponse {
        agent_id: enrolled.agent_id().as_uuid(),
        credential: enrolled.credential().to_owned(),
    }))
}

fn provision_response(
    state: &AppState,
    provision: pingushka_application::use_cases::NewAgentCredentials,
) -> ProvisionResponse {
    let install_command = state.config.install_command(provision.enrollment_token());
    ProvisionResponse {
        agent: AgentDto::new(provision.agent(), false),
        enrollment_token: provision.enrollment_token().to_owned(),
        expires_at: crate::system::rfc3339(provision.expires_at()),
        install_command,
    }
}
