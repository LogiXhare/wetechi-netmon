//! Thin binary entry point; the logic is in the library so tests can run
//! it in-process.

use wetechinetmon_api::server::{bind, Bound};
use wetechinetmon_api::{config::Config, router, AppState};

#[tokio::main]
async fn main() {
    wetechinetmon_common::logging::init();

    let config = Config::from_env().unwrap_or_else(|error| {
        tracing::error!(error = %error, "invalid configuration");
        std::process::exit(1);
    });
    let (pool, transport) = wetechinetmon_incident_postgres::connect::connect(
        &config.database_url,
        config.database_tls.as_ref(),
        config.pool,
    )
    .unwrap_or_else(|error| {
        tracing::error!(error = %error, "the incident database cannot be used");
        std::process::exit(1);
    });
    let listener = bind(config.bind, config.tls.as_ref())
        .await
        .unwrap_or_else(|error| {
            tracing::error!(error = %error, "the API cannot listen");
            std::process::exit(1);
        });
    tracing::info!(
        bind = %config.bind,
        tls = config.tls.is_some(),
        database_transport = ?transport,
        "starting wetechinetmon-api"
    );

    let app = router(AppState { pool });
    let served = match listener {
        Bound::Plain(tcp) => {
            axum::serve(tcp, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
        }
        Bound::Tls(tls) => {
            axum::serve(tls, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
        }
    };
    if let Err(error) = served {
        tracing::error!(error = %error, "the API stopped with an error");
        std::process::exit(1);
    }
    tracing::info!("wetechinetmon-api stopped");
}

/// Ctrl+C, or SIGTERM on Unix (what systemd and containers send).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
