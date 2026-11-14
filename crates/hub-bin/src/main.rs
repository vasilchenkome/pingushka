#![forbid(unsafe_code)]

mod config;

use std::{error::Error, net::SocketAddr, sync::Arc, time::Duration};

use config::Config;
use pingushka_hub_http::{
    app::{AppState, HubConfig, HubPorts, build_router},
    security::{Argon2PasswordHasher, OsTokenGenerator, Sha256TokenDigester},
    system::SystemClock,
};
use pingushka_storage_postgres::PgStore;
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();
    let config = Config::from_env()?;
    let store = Arc::new(PgStore::connect(&config.database_url).await?);
    store.migrate().await?;
    let state = AppState::new(
        HubPorts {
            users: store.clone(),
            sessions: store.clone(),
            agents: store.clone(),
            enrollments: store.clone(),
            targets: store.clone(),
            results: store.clone(),
            health: store.clone(),
            clock: Arc::new(SystemClock),
            hasher: Arc::new(Argon2PasswordHasher),
            digester: Arc::new(Sha256TokenDigester),
            generator: Arc::new(OsTokenGenerator),
        },
        HubConfig {
            public_url: config.public_url.clone(),
            install_script_url: config.install_script_url.clone(),
            cookie_secure: config.cookie_secure,
            session_ttl: config.session_ttl,
            enrollment_ttl: config.enrollment_ttl,
            retention: config.retention,
            manual_check_cooldown: config.manual_check_cooldown,
            ..HubConfig::default()
        },
    );
    let app = build_router(state.clone());
    let listener = TcpListener::bind(&config.bind_addr).await?;
    info!(
        address = %listener.local_addr()?,
        public_url = %config.public_url,
        "pingushka hub listening"
    );

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let pruner = state.prune.clone();
    let prune_interval = config.prune_interval;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(prune_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await;
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match pruner.execute().await {
                        Ok(report) => info!(
                            sessions = report.sessions,
                            results = report.results,
                            enrollments = report.enrollments,
                            "retention pruning finished"
                        ),
                        Err(error) => error!(%error, "retention pruning failed"),
                    }
                }
            }
        }
    });

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = tokio::signal::ctrl_c().await;
        info!("shutdown signal received");
        let _ = shutdown_tx.send(true);
    })
    .await?;

    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();
}
