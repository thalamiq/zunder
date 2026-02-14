//! Lightweight state for background workers
//!
//! Workers don't need the full package-backed FHIR context loaded into memory.
//! This module provides a minimal state that workers need to operate.

use crate::{
    config::Config,
    queue::{JobQueue, PostgresJobQueue},
    Result,
};
use sqlx::PgPool;
use std::sync::Arc;
use ferrum_fhirpath::Engine as FhirPathEngine;

/// Lightweight state for background workers
///
/// Unlike AppState, this avoids loading packages from the registry.
/// It builds a DB-backed FHIR context (used by FHIRPath) that fetches conformance
/// resources on-demand from Postgres.
#[derive(Clone)]
pub struct WorkerState {
    pub config: Arc<Config>,
    pub db_pool: PgPool,
    pub job_queue: Arc<dyn JobQueue>,
    pub fhir_context: Arc<dyn ferrum_context::FhirContext>,
    pub fhirpath_engine: Arc<FhirPathEngine>,
    pub indexing_service: Arc<crate::services::IndexingService>,
}

impl WorkerState {
    /// Initialize lightweight worker state.
    ///
    /// This avoids loading FHIR packages from the registry, and instead builds a
    /// DB-backed FHIR context (for StructureDefinition lookup) plus a FHIRPath engine.
    pub async fn new(config: Config) -> Result<Self> {
        tracing::info!("Initializing worker state...");

        // Create database connection pool
        let db_pool = create_db_pool(&config).await?;

        // Run migrations (idempotent)
        tracing::info!("Running database migrations...");
        sqlx::migrate!("./migrations")
            .run(&db_pool)
            .await
            .map_err(|e| match e {
                sqlx::migrate::MigrateError::Execute(db_err) => crate::Error::Database(db_err),
                other => crate::Error::Internal(format!("Migration failed: {}", other)),
            })?;

        // Load core FHIR package into memory (needed by IndexingService)
        tracing::info!("Loading core FHIR package...");
        crate::conformance::load_core_fhir_context(&config.fhir.version).await?;

        // Create job queue
        let job_queue: Arc<dyn JobQueue> = Arc::new(PostgresJobQueue::new(
            db_pool.clone(),
            config.workers.poll_interval_seconds,
        ));

        // Build DB-backed FHIR context + FHIRPath engine (no registry package loading).
        let fhir_context = crate::conformance::db_backed_fhir_context(db_pool.clone())?;
        let fhirpath_engine = Arc::new(FhirPathEngine::new(fhir_context.clone(), None));

        // Create a shared indexing service (reused across worker jobs).
        let indexing_service = Arc::new(crate::services::IndexingService::new(
            db_pool.clone(),
            &config.fhir.version,
            config.database.indexing_batch_size,
            config.database.indexing_bulk_threshold,
            config.fhir.search.enable_text,
            config.fhir.search.enable_content,
        )?);

        tracing::info!("Worker state initialized successfully (no FHIR packages loaded)");

        Ok(Self {
            config: Arc::new(config),
            db_pool,
            job_queue,
            fhir_context,
            fhirpath_engine,
            indexing_service,
        })
    }
}

async fn create_db_pool(config: &Config) -> Result<PgPool> {
    tracing::info!("Creating worker database connection pool...");

    let statement_timeout = config.database.statement_timeout_seconds;
    let lock_timeout = config.database.lock_timeout_seconds;

    let pool = sqlx::postgres::PgPoolOptions::new()
        .min_connections(config.database.worker_pool_min_size)
        .max_connections(config.database.worker_pool_max_size)
        .acquire_timeout(std::time::Duration::from_secs(
            config.database.worker_pool_timeout_seconds,
        ))
        .after_connect(move |conn, _meta| {
            Box::pin(async move {
                // Set statement timeout (max query execution time)
                sqlx::query(&format!("SET statement_timeout = '{}s'", statement_timeout))
                    .execute(&mut *conn)
                    .await?;

                // Set lock timeout (max lock wait time - fail fast)
                sqlx::query(&format!("SET lock_timeout = '{}s'", lock_timeout))
                    .execute(&mut *conn)
                    .await?;

                Ok(())
            })
        })
        .connect(&config.database.url)
        .await
        .map_err(crate::Error::Database)?;

    tracing::info!(
        "Worker database pool created (min: {}, max: {})",
        config.database.worker_pool_min_size,
        config.database.worker_pool_max_size
    );

    Ok(pool)
}
