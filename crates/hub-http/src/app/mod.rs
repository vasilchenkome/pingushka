pub mod agents;
pub mod error;
pub mod health;
pub mod session;
pub mod targets;
pub mod ws;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, patch, post},
};
use pingushka_application::{
    ports::{
        AgentControlGateway, AgentEnrollmentRepository, AgentRepository, CheckResultRepository,
        Clock, HealthCheck, PasswordHasher, SessionRepository, TargetRepository, TokenDigester,
        TokenGenerator, UserRepository,
    },
    use_cases::{
        AuthenticateSession, CreateTarget, DeleteTarget, EnrollAgent, Login, Logout, PruneOldData,
        RecordCheckBatch, RegisterAgent, RotateAgentToken, SetupAdmin, TriggerManualCheck,
        UpdateTarget,
    },
};
use tower_http::timeout::TimeoutLayer;

use crate::app::{error::ApiError, ws::AgentRegistry};

pub struct HubConfig {
    pub public_url: String,
    pub install_script_url: String,
    pub cookie_secure: bool,
    pub session_ttl: Duration,
    pub enrollment_ttl: Duration,
    pub retention: Duration,
    pub manual_check_cooldown: Duration,
    pub body_limit_bytes: usize,
    pub request_timeout: Duration,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            public_url: "http://localhost:8080".to_owned(),
            install_script_url:
                "https://raw.githubusercontent.com/pingushka/pingushka/main/install.sh".to_owned(),
            cookie_secure: false,
            session_ttl: crate::system::days(7),
            enrollment_ttl: Duration::from_secs(30 * 60),
            retention: crate::system::days(30),
            manual_check_cooldown: Duration::from_secs(10),
            body_limit_bytes: 64 * 1024,
            request_timeout: Duration::from_secs(10),
        }
    }
}

impl HubConfig {
    pub fn install_command(&self, enrollment_token: &str) -> String {
        format!(
            "curl -fsSL {} | sudo sh -s -- --hub {} --enrollment-token {}",
            self.install_script_url, self.public_url, enrollment_token
        )
    }
}

pub struct HubPorts {
    pub users: Arc<dyn UserRepository>,
    pub sessions: Arc<dyn SessionRepository>,
    pub agents: Arc<dyn AgentRepository>,
    pub enrollments: Arc<dyn AgentEnrollmentRepository>,
    pub targets: Arc<dyn TargetRepository>,
    pub results: Arc<dyn CheckResultRepository>,
    pub health: Arc<dyn HealthCheck>,
    pub clock: Arc<dyn Clock>,
    pub hasher: Arc<dyn PasswordHasher>,
    pub digester: Arc<dyn TokenDigester>,
    pub generator: Arc<dyn TokenGenerator>,
}

pub struct AppState {
    pub setup: Arc<SetupAdmin>,
    pub login: Arc<Login>,
    pub authenticate: Arc<AuthenticateSession>,
    pub logout: Arc<Logout>,
    pub register_agent: Arc<RegisterAgent>,
    pub rotate_agent: Arc<RotateAgentToken>,
    pub enroll_agent: Arc<EnrollAgent>,
    pub create_target: Arc<CreateTarget>,
    pub update_target: Arc<UpdateTarget>,
    pub delete_target: Arc<DeleteTarget>,
    pub manual_check: Arc<TriggerManualCheck>,
    pub record_batch: Arc<RecordCheckBatch>,
    pub prune: Arc<PruneOldData>,
    pub users: Arc<dyn UserRepository>,
    pub agents: Arc<dyn AgentRepository>,
    pub targets: Arc<dyn TargetRepository>,
    pub results: Arc<dyn CheckResultRepository>,
    pub health: Arc<dyn HealthCheck>,
    pub clock: Arc<dyn Clock>,
    pub digester: Arc<dyn TokenDigester>,
    pub registry: Arc<AgentRegistry>,
    pub config: HubConfig,
    pub started_at: Instant,
    pub login_guard: session::LoginGuard,
}

impl AppState {
    pub fn new(ports: HubPorts, config: HubConfig) -> Arc<Self> {
        let registry = Arc::new(AgentRegistry::new());
        let control: Arc<dyn AgentControlGateway> = registry.clone();
        let setup = Arc::new(SetupAdmin::new(
            ports.users.clone(),
            ports.hasher.clone(),
            ports.clock.clone(),
        ));
        let login = Arc::new(Login::new(
            ports.users.clone(),
            ports.sessions.clone(),
            ports.hasher.clone(),
            ports.digester.clone(),
            ports.generator.clone(),
            ports.clock.clone(),
            config.session_ttl,
        ));
        let authenticate = Arc::new(AuthenticateSession::new(
            ports.sessions.clone(),
            ports.users.clone(),
            ports.digester.clone(),
            ports.clock.clone(),
        ));
        let logout = Arc::new(Logout::new(ports.sessions.clone(), ports.digester.clone()));
        let register_agent = Arc::new(RegisterAgent::new(
            ports.agents.clone(),
            ports.enrollments.clone(),
            ports.generator.clone(),
            ports.digester.clone(),
            ports.clock.clone(),
            config.enrollment_ttl,
        ));
        let rotate_agent = Arc::new(RotateAgentToken::new(
            ports.agents.clone(),
            ports.enrollments.clone(),
            control.clone(),
            ports.generator.clone(),
            ports.digester.clone(),
            ports.clock.clone(),
            config.enrollment_ttl,
        ));
        let enroll_agent = Arc::new(EnrollAgent::new(
            ports.agents.clone(),
            ports.enrollments.clone(),
            ports.generator.clone(),
            ports.digester.clone(),
            ports.clock.clone(),
        ));
        let create_target = Arc::new(CreateTarget::new(
            ports.targets.clone(),
            control.clone(),
            ports.clock.clone(),
        ));
        let update_target = Arc::new(UpdateTarget::new(
            ports.targets.clone(),
            control.clone(),
            ports.clock.clone(),
        ));
        let delete_target = Arc::new(DeleteTarget::new(ports.targets.clone(), control.clone()));
        let manual_check = Arc::new(TriggerManualCheck::new(
            ports.targets.clone(),
            control,
            ports.clock.clone(),
            config.manual_check_cooldown,
        ));
        let record_batch = Arc::new(RecordCheckBatch::new(ports.results.clone()));
        let prune = Arc::new(PruneOldData::new(
            ports.sessions.clone(),
            ports.results.clone(),
            ports.enrollments,
            ports.clock.clone(),
            config.retention,
            500,
        ));
        Arc::new(Self {
            setup,
            login,
            authenticate,
            logout,
            register_agent,
            rotate_agent,
            enroll_agent,
            create_target,
            update_target,
            delete_target,
            manual_check,
            record_batch,
            prune,
            users: ports.users,
            agents: ports.agents,
            targets: ports.targets,
            results: ports.results,
            health: ports.health,
            clock: ports.clock,
            digester: ports.digester,
            registry,
            config,
            started_at: Instant::now(),
            login_guard: session::LoginGuard::new(),
        })
    }
}

async fn not_found() -> ApiError {
    ApiError::not_found("route not found")
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let body_limit = state.config.body_limit_bytes;
    let request_timeout = state.config.request_timeout;
    Router::new()
        .route("/api/health", get(health::health))
        .route("/api/auth/state", get(session::state))
        .route("/api/setup", post(session::setup))
        .route("/api/auth/login", post(session::login))
        .route("/api/auth/logout", post(session::logout))
        .route("/api/auth/me", get(session::me))
        .route("/api/agents", get(agents::list).post(agents::create))
        .route("/api/agents/{id}", delete(agents::delete_one))
        .route("/api/agents/{id}/token", post(agents::rotate))
        .route("/api/agent/enroll", post(agents::enroll))
        .route("/api/agent/ws", get(ws::upgrade))
        .route("/api/targets", get(targets::list).post(targets::create))
        .route(
            "/api/targets/{id}",
            patch(targets::update).delete(targets::delete_one),
        )
        .route("/api/targets/{id}/check", post(targets::check_now))
        .route("/api/targets/{id}/results", get(targets::history))
        .route("/api/summary", get(targets::summary))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        .with_state(state)
}
