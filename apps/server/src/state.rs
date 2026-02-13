//! Shared application state

use crate::{
    config::Config,
    conformance::cached_core_fhir_context,
    db::{
        admin::AdminRepository, packages::PackageRepository, search::engine::SearchEngine,
        PostgresResourceStore, RuntimeConfigRepository,
    },
    hooks::{
        compartment_definition::CompartmentDefinitionHook, search_parameter::SearchParameterHook,
        terminology::TerminologyHook, ResourceHook,
    },
    queue::{InlineJobQueue, JobQueue, PostgresJobQueue},
    runtime_config::RuntimeConfigCache,
    services::{
        AdminService, ConditionalReferenceResolver, CrudService, MetadataService, MetricsService,
        OperationExecutor, OperationRegistry, PackageService, RuntimeConfigService, SearchService,
        SystemService, TerminologyService,
    },
    Result,
};
use sqlx::PgPool;
use std::sync::Arc;
use zunder_context::FhirContext;
use zunder_fhirpath::Engine as FhirPathEngine;

#[derive(Debug, Clone, Copy)]
pub enum JobQueueKind {
    /// Persist jobs in Postgres and rely on background workers.
    Postgres,
    /// Execute supported jobs immediately in-process (useful for tests).
    Inline,
}

#[derive(Debug, Clone)]
pub struct AppStateOptions {
    pub run_migrations: bool,
    pub install_packages: bool,
    pub load_operation_definitions: bool,
    pub job_queue: JobQueueKind,
}

impl Default for AppStateOptions {
    fn default() -> Self {
        Self {
            run_migrations: true,
            install_packages: true,
            load_operation_definitions: true,
            job_queue: JobQueueKind::Postgres,
        }
    }
}

/// Shared application state passed to all handlers
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub auth: Arc<crate::auth::AuthManager>,
    pub admin_auth: Arc<crate::admin_auth::AdminAuthManager>,
    pub db_pool: PgPool,
    pub job_queue: Arc<dyn JobQueue>,
    pub resource_hooks: Vec<Arc<dyn ResourceHook>>,
    pub fhir_context: Arc<dyn FhirContext>,
    pub fhirpath_engine: Arc<FhirPathEngine>,
    pub indexing_service: Arc<crate::services::IndexingService>,
    pub search_engine: Arc<SearchEngine>,
    pub crud_service: Arc<crate::services::CrudService>,
    pub audit_service: Arc<crate::services::AuditService>,
    pub batch_service: Arc<crate::services::BatchService>,
    pub transaction_service: Arc<crate::services::TransactionService>,
    pub history_service: Arc<crate::services::HistoryService>,
    pub search_service: Arc<SearchService>,
    pub conditional_service: Arc<crate::services::conditional::ConditionalService>,
    pub conditional_reference_resolver: Arc<ConditionalReferenceResolver>,
    pub system_service: Arc<SystemService>,
    pub metadata_service: Arc<MetadataService>,
    pub package_service: Arc<PackageService>,
    pub admin_service: Arc<AdminService>,
    pub metrics_service: Arc<MetricsService>,
    pub operation_registry: Arc<OperationRegistry>,
    pub operation_executor: Arc<OperationExecutor>,
    pub runtime_config_cache: Arc<RuntimeConfigCache>,
    pub runtime_config_service: Arc<RuntimeConfigService>,
}

impl AppState {
    /// Initialize the application state
    pub async fn new(config: Config) -> Result<Self> {
        Self::new_with_options(config, AppStateOptions::default()).await
    }

    /// Create a new ResourceResolver instance for a request
    ///
    /// Creates a per-request resolver with its own LRU cache. This allows
    /// FHIRPath expressions to resolve references while maintaining proper
    /// cache scoping and preventing data leakage across requests.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL of this server (e.g., "http://localhost:8080")
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let base_url = format!("http://{}:{}", state.config.server.host, state.config.server.port);
    /// let resolver = state.create_resolver(Some(base_url));
    ///
    /// // Pre-warm cache for resource
    /// resolver.prewarm_cache_for_resource(&resource).await?;
    ///
    /// // Create FHIRPath engine with resolver
    /// let engine = FhirPathEngine::new(state.fhir_context.clone(), Some(Arc::new(resolver)));
    /// ```
    pub fn create_resolver(&self, base_url: Option<String>) -> crate::db::FhirResourceResolver {
        crate::db::FhirResourceResolver::new(
            self.db_pool.clone(),
            self.search_engine.clone(),
            base_url,
            self.config.fhir.fhirpath.resolve_cache_size,
            self.config.fhir.fhirpath.enable_external_http,
            self.config.fhir.fhirpath.http_timeout_seconds,
        )
    }

    pub async fn new_with_options(config: Config, options: AppStateOptions) -> Result<Self> {
        tracing::info!("Initializing application state...");

        let config_arc = Arc::new(config);

        // Create database connection pool
        let db_pool = create_db_pool(config_arc.as_ref()).await?;

        // Run migrations
        if options.run_migrations {
            tracing::info!("Running database migrations...");
            sqlx::migrate!("./migrations")
                .run(&db_pool)
                .await
                .map_err(|e| crate::Error::Internal(format!("Migration failed: {}", e)))?;
        }

        // Install FHIR packages into database synchronously at startup
        if options.install_packages {
            tracing::info!("Installing FHIR packages...");
            crate::startup::install_all_packages(config_arc.as_ref(), &db_pool).await?;
        }

        // Ensure core FHIR package is cached (download if needed)
        tracing::info!("Ensuring core FHIR package is available...");
        crate::conformance::ensure_core_package_cached(&config_arc.fhir.version).await?;

        // Create DB-backed FHIR context (no fallback - fail fast if resources not in DB)
        let fhir_context = cached_core_fhir_context(&config_arc.fhir.version)?;

        // Create FHIRPath engine using the already-loaded context
        let fhirpath_engine = Arc::new(FhirPathEngine::new(fhir_context.clone(), None));

        // Initialize indexing service
        let indexing_service = Arc::new(crate::services::IndexingService::new(
            db_pool.clone(),
            &config_arc.fhir.version,
            config_arc.database.indexing_batch_size,
            config_arc.database.indexing_bulk_threshold,
            config_arc.fhir.search.enable_text,
            config_arc.fhir.search.enable_content,
        )?);

        // Runtime configuration cache (static defaults come from config.yaml + env).
        let runtime_config_cache = Arc::new(RuntimeConfigCache::new(config_arc.clone()));
        let runtime_config_repo = RuntimeConfigRepository::new(db_pool.clone());
        let runtime_config_service = Arc::new(RuntimeConfigService::new(
            runtime_config_repo,
            runtime_config_cache.clone(),
        ));
        runtime_config_service.initialize_cache().await?;
        if matches!(options.job_queue, JobQueueKind::Postgres) {
            spawn_runtime_config_listener(db_pool.clone(), runtime_config_service.clone());
        }

        // Create job queue (may run jobs inline for tests).
        let job_queue: Arc<dyn JobQueue> = match options.job_queue {
            JobQueueKind::Postgres => Arc::new(PostgresJobQueue::new(
                db_pool.clone(),
                config_arc.workers.poll_interval_seconds,
            )),
            JobQueueKind::Inline => Arc::new(InlineJobQueue::new(
                db_pool.clone(),
                indexing_service.clone(),
            )),
        };

        let store = PostgresResourceStore::new(db_pool.clone());
        let search_engine = Arc::new(SearchEngine::new_with_runtime_config(
            db_pool.clone(),
            config_arc.fhir.search.clone(),
            runtime_config_cache.clone(),
        ));

        // Initialize resource hooks
        let resource_hooks: Vec<Arc<dyn ResourceHook>> = vec![
            Arc::new(SearchParameterHook::new(
                db_pool.clone(),
                indexing_service.clone(),
                search_engine.clone(),
                config_arc
                    .fhir
                    .search
                    .search_parameter_active_statuses
                    .clone(),
            )),
            Arc::new(TerminologyHook::new(db_pool.clone())),
            Arc::new(CompartmentDefinitionHook::new(db_pool.clone())),
        ];
        let mut crud_service_inner = CrudService::with_hooks_and_indexing_and_runtime_config(
            store.clone(),
            resource_hooks.clone(),
            job_queue.clone(),
            indexing_service.clone(),
            config_arc.fhir.allow_update_create,
            config_arc.fhir.hard_delete,
            runtime_config_cache.clone(),
        );
        crud_service_inner.set_referential_integrity_mode(
            config_arc.fhir.referential_integrity.mode.clone(),
        );
        let crud_service = Arc::new(crud_service_inner);

        let conditional_service = Arc::new(crate::services::conditional::ConditionalService::new(
            search_engine.clone(),
        ));
        let conditional_reference_resolver =
            Arc::new(ConditionalReferenceResolver::new(search_engine.clone()));
        let audit_service = Arc::new(crate::services::AuditService::new(
            runtime_config_cache.clone(),
            config_arc.fhir.version.clone(),
            config_arc.logging.service_name.clone(),
            db_pool.clone(),
        ));
        let mut batch_service_inner = crate::services::BatchService::new_with_runtime_config(
            store.clone(),
            resource_hooks.clone(),
            job_queue.clone(),
            search_engine.clone(),
            config_arc.fhir.allow_update_create,
            config_arc.fhir.hard_delete,
            runtime_config_cache.clone(),
        );
        batch_service_inner.set_referential_integrity_mode(
            config_arc.fhir.referential_integrity.mode.clone(),
        );
        let batch_service = Arc::new(batch_service_inner);
        let mut transaction_service_inner =
            crate::services::TransactionService::new_with_runtime_config(
                store.clone(),
                resource_hooks.clone(),
                indexing_service.clone(),
                fhir_context.clone(),
                search_engine.clone(),
                config_arc.fhir.allow_update_create,
                config_arc.fhir.hard_delete,
                runtime_config_cache.clone(),
            );
        transaction_service_inner.set_referential_integrity_mode(
            config_arc.fhir.referential_integrity.mode.clone(),
        );
        let transaction_service = Arc::new(transaction_service_inner);
        let history_service = Arc::new(crate::services::HistoryService::new_with_runtime_config(
            store.clone(),
            resource_hooks.clone(),
            job_queue.clone(),
            config_arc.fhir.allow_update_create,
            config_arc.fhir.hard_delete,
            runtime_config_cache.clone(),
        ));

        // Create search service with summary filtering
        let summary_filter = Arc::new(crate::services::SummaryFilter::new(fhir_context.clone()));
        let search_service = Arc::new(SearchService::with_summary_filter(
            search_engine.clone(),
            summary_filter,
            runtime_config_cache.clone(),
        ));
        let system_service = Arc::new(SystemService::new(
            search_engine.clone(),
            crud_service.clone(),
            config_arc
                .fhir
                .capability_statement
                .supported_resources
                .clone(),
        ));

        // Create metadata service
        let auth = Arc::new(
            crate::auth::AuthManager::new(config_arc.clone()).map_err(|e| {
                crate::Error::Internal(format!("Failed to initialize auth manager: {e:?}"))
            })?,
        );
        let admin_auth = Arc::new(crate::admin_auth::AdminAuthManager::new(config_arc.clone()));
        let metadata_repo = crate::db::MetadataRepository::new(db_pool.clone());
        let metadata_service = Arc::new(MetadataService::new(config_arc.clone(), metadata_repo));

        let package_service = Arc::new(PackageService::new_admin(PackageRepository::new(
            db_pool.clone(),
        )));
        let admin_service = Arc::new(AdminService::new(AdminRepository::new(db_pool.clone())));

        let metrics_repo = crate::db::MetricsRepository::new(db_pool.clone());
        let metrics_service = Arc::new(MetricsService::new(metrics_repo));

        let terminology_repo = crate::db::TerminologyRepository::new(db_pool.clone());
        let terminology_service = Arc::new(TerminologyService::new(terminology_repo));

        // Create operation services
        let operation_registry = Arc::new(OperationRegistry::new(Arc::new(store.clone())));
        let operation_executor = Arc::new(OperationExecutor::with_services(
            package_service.clone(),
            indexing_service.clone(),
            terminology_service,
            job_queue.clone(),
            search_engine.clone(),
            store.clone(),
        ));

        // Load operation definitions from database (after packages are installed)
        if options.load_operation_definitions {
            tracing::info!("Loading FHIR operation definitions...");
            operation_registry.load_definitions().await?;
        }

        tracing::info!("Application state initialized successfully");

        Ok(Self {
            config: config_arc,
            auth,
            admin_auth,
            db_pool,
            job_queue,
            resource_hooks,
            fhir_context,
            fhirpath_engine,
            indexing_service,
            search_engine,
            crud_service,
            audit_service,
            batch_service,
            transaction_service,
            history_service,
            search_service,
            conditional_service,
            conditional_reference_resolver,
            system_service,
            metadata_service,
            package_service,
            admin_service,
            metrics_service,
            operation_registry,
            operation_executor,
            runtime_config_cache,
            runtime_config_service,
        })
    }
}

fn spawn_runtime_config_listener(db_pool: PgPool, service: Arc<RuntimeConfigService>) {
    tokio::spawn(async move {
        loop {
            if db_pool.is_closed() {
                break;
            }

            let mut listener = match sqlx::postgres::PgListener::connect_with(&db_pool).await {
                Ok(l) => l,
                Err(e) => {
                    if db_pool.is_closed() {
                        break;
                    }
                    tracing::warn!("Runtime config listener connect failed: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
            };

            if let Err(e) = listener.listen("runtime_config_changed").await {
                tracing::warn!("Runtime config listener subscribe failed: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }

            tracing::info!("Runtime config listener started (channel 'runtime_config_changed')");

            loop {
                match listener.recv().await {
                    Ok(notification) => {
                        let key = notification.payload().to_string();
                        if let Err(e) = service.invalidate_cache_entry(&key).await {
                            tracing::warn!(
                                "Failed to invalidate runtime config cache for '{}': {}",
                                key,
                                e
                            );
                        }
                    }
                    Err(e) => {
                        if db_pool.is_closed() {
                            break;
                        }
                        tracing::warn!("Runtime config listener error: {}", e);
                        break;
                    }
                }
            }
        }
    });
}

async fn create_db_pool(config: &Config) -> Result<PgPool> {
    tracing::info!("Creating database connection pool...");

    let statement_timeout = config.database.statement_timeout_seconds;
    let lock_timeout = config.database.lock_timeout_seconds;

    let pool = sqlx::postgres::PgPoolOptions::new()
        .min_connections(config.database.pool_min_size)
        .max_connections(config.database.pool_max_size)
        .acquire_timeout(std::time::Duration::from_secs(
            config.database.pool_timeout_seconds,
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
        "Database pool created (min: {}, max: {})",
        config.database.pool_min_size,
        config.database.pool_max_size
    );

    Ok(pool)
}
