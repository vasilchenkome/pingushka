use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{ConnectInfo, FromRequestParts, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use pingushka_domain::user::User;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::app::{AppState, error::ApiError};

const COOKIE_NAME: &str = "pingushka_session";
const MAX_ATTEMPTS: usize = 5;
const ATTEMPT_WINDOW: Duration = Duration::from_secs(15 * 60);
const FAILURE_DELAY: Duration = Duration::from_millis(300);

pub struct LoginGuard {
    attempts: Mutex<HashMap<String, Vec<Instant>>>,
}

impl LoginGuard {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
        }
    }

    fn is_blocked(&self, key: &str) -> bool {
        let mut attempts = self
            .attempts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let entries = attempts.entry(key.to_owned()).or_default();
        entries.retain(|instant| now.duration_since(*instant) < ATTEMPT_WINDOW);
        entries.len() >= MAX_ATTEMPTS
    }

    fn record_failure(&self, key: &str) {
        let mut attempts = self
            .attempts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        attempts
            .entry(key.to_owned())
            .or_default()
            .push(Instant::now());
    }

    fn clear(&self, key: &str) {
        let mut attempts = self
            .attempts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        attempts.remove(key);
    }
}

impl Default for LoginGuard {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AuthSession {
    user: User,
    token: String,
}

impl AuthSession {
    pub fn user(&self) -> &User {
        &self.user
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

impl FromRequestParts<Arc<AppState>> for AuthSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = session_token(&parts.headers)
            .ok_or_else(|| ApiError::unauthorized("authentication required"))?;
        let authenticated = state
            .authenticate
            .execute(&token)
            .await
            .map_err(ApiError::from_port)?
            .ok_or_else(|| ApiError::unauthorized("session is invalid or expired"))?;
        Ok(Self {
            user: authenticated.user().clone(),
            token,
        })
    }
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';')
        .filter_map(|part| part.trim().strip_prefix(&format!("{COOKIE_NAME}=")))
        .find(|value| !value.is_empty() && value.len() <= 128)
        .map(str::to_owned)
}

fn session_cookie(state: &AppState, token: &str, max_age_secs: i64) -> HeaderValue {
    let secure = if state.config.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn cleared_cookie(state: &AppState) -> HeaderValue {
    session_cookie(state, "", 0)
}

#[derive(Deserialize)]
pub struct SetupRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct UserDto {
    id: Uuid,
    username: String,
}

impl From<&User> for UserDto {
    fn from(user: &User) -> Self {
        Self {
            id: user.id().as_uuid(),
            username: user.username().as_str().to_owned(),
        }
    }
}

#[derive(Serialize)]
pub struct AuthStateResponse {
    setup_required: bool,
}

pub async fn state(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AuthStateResponse>, ApiError> {
    let users = state.users.count().await.map_err(ApiError::from_port)?;
    Ok(Json(AuthStateResponse {
        setup_required: users == 0,
    }))
}

pub async fn setup(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<SetupRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = payload?;
    let user = state
        .setup
        .execute(&request.username, &request.password)
        .await
        .map_err(ApiError::from_setup)?;
    Ok((StatusCode::CREATED, Json(UserDto::from(&user))).into_response())
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = payload?;
    let key = login_key(&headers, address, &request.username);
    if state.login_guard.is_blocked(&key) {
        return Err(ApiError::too_many_requests(
            "too many failed login attempts, try again later",
        ));
    }
    match state
        .login
        .execute(&request.username, &request.password)
        .await
    {
        Ok(issued) => {
            state.login_guard.clear(&key);
            let max_age = (issued.session().expires_at() - issued.session().created_at())
                .whole_seconds()
                .max(0);
            let cookie = session_cookie(&state, issued.token(), max_age);
            let body = Json(json!({
                "user": UserDto::from(issued.user()),
                "expires_at": crate::system::rfc3339(issued.session().expires_at()),
            }));
            let mut response = (StatusCode::OK, body).into_response();
            response.headers_mut().insert(header::SET_COOKIE, cookie);
            Ok(response)
        }
        Err(error) => {
            state.login_guard.record_failure(&key);
            tokio::time::sleep(FAILURE_DELAY).await;
            Err(ApiError::from_login(error))
        }
    }
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    auth: AuthSession,
) -> Result<Response, ApiError> {
    state
        .logout
        .execute(auth.token())
        .await
        .map_err(ApiError::from_port)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cleared_cookie(&state));
    Ok(response)
}

pub async fn me(auth: AuthSession) -> Json<UserDto> {
    Json(UserDto::from(auth.user()))
}

fn login_key(headers: &HeaderMap, address: SocketAddr, username: &str) -> String {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| address.ip().to_string());
    format!("{ip}:{}", username.trim().to_lowercase())
}
