//! FHIR Server - Background Worker Entry Point
//!
//! This binary starts background workers that process jobs from the queue.
//! Workers handle indexing, terminology processing, and other async tasks.

use anyhow::Context;
use zunder::{
    config::Config,
    logging,
    workers::{
        create_workers, spawn_workers_with_config, WorkerConfig, WorkerRunnerConfig, WorkerState,
    },
};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration first to get logging settings
    let config = Config::load().context("Failed to load configuration")?;

    // Validate configuration
    config
        .validate()
        .map_err(|e| anyhow::anyhow!("Invalid configuration: {e}"))?;

    // Initialize logging based on configuration (supports file logging, JSON format, etc.)
    let _telemetry_guard =
        logging::init_logging(&config.logging).context("Failed to initialize logging/telemetry")?;

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        environment = config.logging.deployment_environment,
        "Starting FHIR Background Workers"
    );

    if !config.workers.enabled {
        tracing::warn!("Workers are disabled in configuration");
        return Ok(());
    }

    if config.workers.embedded {
        tracing::warn!(
            "workers.embedded is true — workers are running inside the fhir-server process. \
             Set workers.embedded=false to use this separate worker binary instead."
        );
    }

    tracing::info!(
        max_concurrent = config.workers.max_concurrent_jobs,
        poll_interval_seconds = config.workers.poll_interval_seconds,
        "Worker configuration loaded"
    );

    // Initialize lightweight worker state (NO FHIR packages loaded - fast!)
    // Retry on DB connectivity errors so workers don't exit on transient startup issues.
    let state = init_worker_state_with_retry(&config).await?;

    // Create worker configuration
    let worker_config = WorkerConfig {
        max_concurrent_jobs: config.workers.max_concurrent_jobs,
        poll_interval_seconds: config.workers.poll_interval_seconds,
    };

    // Create workers
    let workers = create_workers(&state, worker_config).context("Failed to create workers")?;

    tracing::info!(worker_count = workers.len(), "Created workers");
    for worker in &workers {
        tracing::info!(
            worker_name = worker.name(),
            supported_jobs = ?worker.supported_job_types(),
            "Worker registered"
        );
    }

    // Spawn all workers to run in background
    tracing::info!("Starting workers...");
    let runner_config = WorkerRunnerConfig::from_config(&config.workers);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let handles = spawn_workers_with_config(
        workers,
        state.job_queue.clone(),
        runner_config,
        Some(shutdown_rx),
    );

    tracing::info!("All workers started and listening for jobs");
    tracing::info!("Workers running. Press Ctrl+C to stop.");

    // Wait for shutdown signal (SIGTERM or SIGINT)
    shutdown_signal().await;
    let _ = shutdown_tx.send(true);
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!("Worker task ended with error: {}", e),
            Err(e) => tracing::error!("Worker task join error: {}", e),
        }
    }

    tracing::info!("Worker shutdown complete");

    // Explicitly shutdown telemetry (also happens via Drop on _telemetry_guard)
    logging::shutdown_telemetry();

    Ok(())
}

async fn init_worker_state_with_retry(config: &Config) -> anyhow::Result<WorkerState> {
    let initial = Duration::from_secs(config.workers.reconnect_initial_seconds.max(1));
    let max = Duration::from_secs(config.workers.reconnect_max_seconds.max(1));
    let jitter_ratio = config.workers.reconnect_jitter_ratio;

    let mut retry_delay = initial;
    loop {
        match WorkerState::new(config.clone()).await {
            Ok(state) => return Ok(state),
            Err(zunder::Error::Database(e)) => {
                tracing::error!(
                    "Failed to initialize worker state (db unavailable): {} (retrying in {:?})",
                    e,
                    retry_delay
                );
                sleep(jittered_duration(retry_delay, jitter_ratio)).await;
                retry_delay = (retry_delay * 2).min(max);
            }
            Err(e) => return Err(anyhow::anyhow!(e)).context("Failed to initialize worker state"),
        }
    }
}

fn jittered_duration(base: Duration, jitter_ratio: f64) -> Duration {
    if base.is_zero() || jitter_ratio <= 0.0 {
        return base;
    }

    let bytes = *Uuid::new_v4().as_bytes();
    let value = u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
    let unit = (value as f64) / (u64::MAX as f64); // [0,1]
    let signed = unit * 2.0 - 1.0; // [-1,1]
    let factor = (1.0 + signed * jitter_ratio).max(0.0);
    base.mul_f64(factor)
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
            tracing::info!("SIGINT received, stopping workers...");
        }
        _ = sigterm.recv() => {
            tracing::info!("SIGTERM received, stopping workers...");
        }
    }
}

/// Wait for shutdown signal (SIGINT only on non-Unix platforms)
#[cfg(not(unix))]
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
    tracing::info!("Shutdown signal received, stopping workers...");
}
