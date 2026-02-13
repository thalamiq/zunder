//! FHIR Server - Web Server Entry Point
//!
//! This binary starts the HTTP server that handles FHIR API requests.
//! When `workers.embedded` is true (the default), background workers run
//! in-process alongside the server. For separate worker scaling, set
//! `workers.embedded: false` and use the `fhir-worker` binary.

use anyhow::Context;
use zunder::{api::create_router, config::Config, logging, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration first to get logging settings
    let config = Config::load().context("Failed to load configuration")?;

    // Validate configuration
    config
        .validate()
        .map_err(|e| anyhow::anyhow!("Invalid configuration: {e}"))?;

    // Initialize logging based on configuration
    let _telemetry_guard =
        logging::init_logging(&config.logging).context("Failed to initialize logging/telemetry")?;

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        environment = config.logging.deployment_environment,
        "Starting FHIR Server"
    );

    let addr = config
        .socket_addr()
        .context("Failed to determine socket address")?;

    tracing::info!(
        fhir_version = config.fhir.version,
        listen_addr = %addr,
        "Configuration loaded"
    );

    // Spawn embedded workers if configured
    let worker_handles = if config.workers.enabled && config.workers.embedded {
        Some(spawn_embedded_workers(&config).await?)
    } else {
        if !config.workers.embedded {
            tracing::info!("Embedded workers disabled — use the separate fhir-worker binary");
        }
        None
    };

    // Initialize application state (includes FHIR packages for server)
    let state = AppState::new(config)
        .await
        .context("Failed to initialize application state")?;

    // Create router
    let app = create_router(state);

    // Start server
    tracing::info!("FHIR Server listening on http://{}", addr);
    tracing::info!("Health check: http://{}/health", addr);
    tracing::info!("API endpoint: http://{}/fhir", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind TCP listener on {addr}"))?;

    // Run server with graceful shutdown
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!(error = %e, "Server terminated unexpectedly");
    }

    // Shut down embedded workers
    if let Some(handles) = worker_handles {
        tracing::info!("Shutting down embedded workers...");
        let _ = handles.shutdown_tx.send(true);
        for handle in handles.join_handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!("Embedded worker ended with error: {}", e),
                Err(e) => tracing::error!("Embedded worker task join error: {}", e),
            }
        }
        tracing::info!("Embedded workers stopped");
    }

    tracing::info!("Server shutdown complete");

    // Explicitly shutdown telemetry (also happens via Drop on _telemetry_guard)
    logging::shutdown_telemetry();

    Ok(())
}

struct EmbeddedWorkerHandles {
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    join_handles: Vec<tokio::task::JoinHandle<zunder::Result<()>>>,
}

async fn spawn_embedded_workers(config: &Config) -> anyhow::Result<EmbeddedWorkerHandles> {
    use zunder::workers::{
        create_workers, spawn_workers_with_config, WorkerConfig, WorkerRunnerConfig, WorkerState,
    };

    tracing::info!("Initializing embedded workers...");

    let worker_state = WorkerState::new(config.clone())
        .await
        .context("Failed to initialize embedded worker state")?;

    let worker_config = WorkerConfig {
        max_concurrent_jobs: config.workers.max_concurrent_jobs,
        poll_interval_seconds: config.workers.poll_interval_seconds,
    };

    let workers = create_workers(&worker_state, worker_config)
        .context("Failed to create embedded workers")?;

    tracing::info!(worker_count = workers.len(), "Spawning embedded workers");
    for worker in &workers {
        tracing::info!(
            worker_name = worker.name(),
            supported_jobs = ?worker.supported_job_types(),
            "Embedded worker registered"
        );
    }

    let runner_config = WorkerRunnerConfig::from_config(&config.workers);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let join_handles = spawn_workers_with_config(
        workers,
        worker_state.job_queue.clone(),
        runner_config,
        Some(shutdown_rx),
    );

    tracing::info!("Embedded workers started");

    Ok(EmbeddedWorkerHandles {
        shutdown_tx,
        join_handles,
    })
}

/// Wait for shutdown signal (SIGTERM or SIGINT)
/// Docker sends SIGTERM, while Ctrl+C sends SIGINT
#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm =
        signal(SignalKind::terminate()).expect("Failed to install SIGTERM signal handler");
    let sigint = tokio::signal::ctrl_c();

    tokio::select! {
        _ = sigint => {
            tracing::info!("SIGINT received, starting graceful shutdown...");
        }
        _ = sigterm.recv() => {
            tracing::info!("SIGTERM received, starting graceful shutdown...");
        }
    }
}

/// Wait for shutdown signal (SIGINT only on non-Unix platforms)
#[cfg(not(unix))]
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
    tracing::info!("Shutdown signal received, starting graceful shutdown...");
}
