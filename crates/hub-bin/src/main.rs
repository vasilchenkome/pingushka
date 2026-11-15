#![forbid(unsafe_code)]

mod config;

use std::{error::Error, net::SocketAddr, sync::Arc, time::Duration};

use config::Config;
use pingushka_application::alerts::{DispatchNotifications, EvaluateAlerts};
use pingushka_hub_http::{
    app::{AppState, HubConfig, HubPorts, build_router},
    notify::Notifier,
    security::{AlertSecretCipher, Argon2PasswordHasher, OsTokenGenerator, Sha256TokenDigester},
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
    let cipher = Arc::new(
        AlertSecretCipher::from_configured_key(&config.alert_secret_key)
            .map_err(|_| config::ConfigError::Invalid("ALERT_SECRET_KEY"))?,
    );
    let notifier = Arc::new(
        Notifier::new(&config.telegram_api_base)
            .map_err(|error| std::io::Error::other(error.to_string()))?,
    );
    let state = AppState::new(
        HubPorts {
            users: store.clone(),
            sessions: store.clone(),
            agents: store.clone(),
            enrollments: store.clone(),
            targets: store.clone(),
            groups: store.clone(),
            results: store.clone(),
            alerts: store.clone(),
            cipher: cipher.clone(),
            notifier: notifier.clone(),
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
            static_dir: config.static_dir.clone(),
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
    let alerts = state.alerts.clone();
    let prune_interval = config.prune_interval;
    let retention = config.retention;
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
                    let before = time::OffsetDateTime::now_utc()
                        - time::Duration::seconds(i64::try_from(retention.as_secs()).unwrap_or(30 * 86_400));
                    match alerts.prune_events(before, 500).await {
                        Ok(removed) if removed > 0 => info!(removed, "alert journal pruned"),
                        Ok(_) => {}
                        Err(error) => error!(%error, "alert journal pruning failed"),
                    }
                }
            }
        }
    });

    let evaluator = Arc::new(EvaluateAlerts::new(
        state.alerts.clone(),
        state.clock.clone(),
    ));
    let dispatcher = Arc::new(DispatchNotifications::new(
        state.alerts.clone(),
        state.cipher.clone(),
        state.notifier.clone(),
    ));
    let mut evaluate_shutdown = shutdown_tx.subscribe();
    let evaluate_interval = config.alert_evaluate_interval;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(evaluate_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = evaluate_shutdown.changed() => {
                    if changed.is_err() || *evaluate_shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match evaluator.execute().await {
                        Ok(report) => {
                            if report.events > 0 {
                                info!(rules = report.rules, events = report.events, "alert evaluation produced events");
                            }
                        }
                        Err(error) => error!(%error, "alert evaluation failed"),
                    }
                }
            }
        }
    });

    let mut dispatch_shutdown = shutdown_tx.subscribe();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = dispatch_shutdown.changed() => {
                    if changed.is_err() || *dispatch_shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match dispatcher.execute().await {
                        Ok(sent) if sent > 0 => info!(sent, "alert notifications delivered"),
                        Ok(_) => {}
                        Err(error) => error!(%error, "alert dispatch failed"),
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
