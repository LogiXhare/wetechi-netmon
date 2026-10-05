//! Thin binary entry point; the logic is in the library so tests can run
//! it in-process.

#[tokio::main]
async fn main() {
    wetechinetmon_common::logging::init();

    let config = wetechinetmon_incident_manager::Config::from_env().unwrap_or_else(|error| {
        tracing::error!(error = %error, "invalid configuration");
        std::process::exit(1);
    });
    tracing::info!(
        worker_id = %config.worker_id,
        metrics_bind = %config.metrics_bind,
        migrate = config.migrate,
        "starting wetechinetmon-incident-manager"
    );

    match wetechinetmon_incident_manager::run(
        config,
        wetechinetmon_incident_manager::shutdown_signal(),
    )
    .await
    {
        Ok(report) => tracing::info!(
            batches = report.worker.batches,
            processed = report.worker.processed,
            dead_lettered = report.worker.dead_lettered,
            "wetechinetmon-incident-manager stopped"
        ),
        Err(error) => {
            tracing::error!(error = %error, "wetechinetmon-incident-manager could not start");
            std::process::exit(1);
        }
    }
}
