use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::IntoResponse,
};
use pingushka_domain::{
    identity::TargetGroupId,
    organization::{TargetGroupName, TargetGroupWithCounts},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::{AppState, error::ApiError, session::AuthSession};

#[derive(Deserialize)]
pub struct GroupNameRequest {
    name: String,
}

#[derive(Serialize)]
pub struct GroupDto {
    id: Uuid,
    name: String,
    target_count: u32,
    rule_count: u32,
    created_at: String,
    updated_at: String,
}

impl From<&TargetGroupWithCounts> for GroupDto {
    fn from(value: &TargetGroupWithCounts) -> Self {
        Self {
            id: value.group().id().as_uuid(),
            name: value.group().name().as_str().to_owned(),
            target_count: value.target_count(),
            rule_count: value.rule_count(),
            created_at: crate::system::rfc3339(value.group().created_at()),
            updated_at: crate::system::rfc3339(value.group().updated_at()),
        }
    }
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
) -> Result<Json<Vec<GroupDto>>, ApiError> {
    let groups = state
        .list_groups
        .execute()
        .await
        .map_err(ApiError::from_target_group)?;
    Ok(Json(groups.iter().map(GroupDto::from).collect()))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    payload: Result<Json<GroupNameRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(request) = payload?;
    let name = parse_name(&request.name)?;
    let group = state
        .create_group
        .execute(name)
        .await
        .map_err(ApiError::from_target_group)?;
    Ok((
        StatusCode::CREATED,
        Json(GroupDto {
            id: group.id().as_uuid(),
            name: group.name().as_str().to_owned(),
            target_count: 0,
            rule_count: 0,
            created_at: crate::system::rfc3339(group.created_at()),
            updated_at: crate::system::rfc3339(group.updated_at()),
        }),
    ))
}

pub async fn rename(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
    payload: Result<Json<GroupNameRequest>, JsonRejection>,
) -> Result<Json<GroupDto>, ApiError> {
    let Json(request) = payload?;
    let name = parse_name(&request.name)?;
    let group = state
        .rename_group
        .execute(TargetGroupId::from_uuid(id), name)
        .await
        .map_err(ApiError::from_target_group)?;
    Ok(Json(GroupDto {
        id: group.id().as_uuid(),
        name: group.name().as_str().to_owned(),
        target_count: 0,
        rule_count: 0,
        created_at: crate::system::rfc3339(group.created_at()),
        updated_at: crate::system::rfc3339(group.updated_at()),
    }))
}

pub async fn delete_one(
    State(state): State<Arc<AppState>>,
    _auth: AuthSession,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .delete_group
        .execute(TargetGroupId::from_uuid(id))
        .await
        .map_err(ApiError::from_target_group)?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_name(value: &str) -> Result<TargetGroupName, ApiError> {
    TargetGroupName::new(value).map_err(|error| ApiError::unprocessable(error.to_string()))
}
