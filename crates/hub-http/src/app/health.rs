use std::sync::Arc;

use axum::{Json, extract::State};
use serde::Serialize;

use crate::app::{AppState, error::ApiError};

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    version: &'static str,
    uptime_seconds: u64,
    database: &'static str,
    protocol_version: u32,
}

pub async fn health(State(state): State<Arc<AppState>>) -> Result<Json<HealthResponse>, ApiError> {
    state.health.check().await.map_err(ApiError::from_port)?;
    Ok(Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        database: "ok",
        protocol_version: pingushka_protocol::PROTOCOL_VERSION,
    }))
}
