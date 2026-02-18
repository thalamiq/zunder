//! Package installation worker

use super::base::{Worker, WorkerConfig};
use crate::{
    db::packages::PackageRepository,
    hooks::{
        compartment_definition::CompartmentDefinitionHook, search_parameter::SearchParameterHook,
        terminology::TerminologyHook, ResourceHook,
    },
    queue::{Job, JobQueue},
    services::{CrudService, IndexingService, PackageService},
    Result,
};
use async_trait::async_trait;
use std::sync::Arc;
use ferrum_registry_client::RegistryClient;

pub struct PackageWorker {
    job_queue: Arc<dyn JobQueue>,
    indexing_service: Arc<IndexingService>,
    registry_cache_dir: Option<std::path::PathBuf>,
    search_parameter_active_statuses: Vec<String>,
    _config: WorkerConfig,
}

impl PackageWorker {
    pub fn new(
        job_queue: Arc<dyn JobQueue>,
        indexing_service: Arc<IndexingService>,
        registry_cache_dir: Option<std::path::PathBuf>,
        search_parameter_active_statuses: Vec<String>,
        config: WorkerConfig,
    ) -> Self {
        Self {
            job_queue,
            indexing_service,
            registry_cache_dir,
            search_parameter_active_statuses,
            _config: config,
        }
    }
}

#[async_trait]
impl Worker for PackageWorker {
    fn name(&self) -> &str {
        "PackageWorker"
    }

    fn supported_job_types(&self) -> &[&str] {
        &["install_package"]
    }

    async fn start(&self) -> Result<()> {
        tracing::info!("{} starting...", self.name());
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        tracing::info!("{} stopping...", self.name());
        Ok(())
    }

    async fn process_job(&self, job: Job) -> Result<()> {
        tracing::info!("{} processing job: {}", self.name(), job.id);

        // Parse job parameters
        let params = &job.parameters;
        let package_name = params
            .get("package_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::Error::Validation("Missing package_name".to_string()))?;

        let package_version = params
            .get("package_version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let include_dependencies = params
            .get("include_dependencies")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let include_examples = params
            .get("include_examples")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Parse resource type filter
        let include_resource_types = params
            .get("include_resource_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            });

        let exclude_resource_types = params
            .get("exclude_resource_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            });

        let filter = crate::config::ResourceTypeFilter {
            include_resource_types,
            exclude_resource_types,
        };

        // Validate filter
        if let Err(e) = filter.validate() {
            return Err(crate::Error::Validation(format!(
                "Invalid resource type filter: {}",
                e
            )));
        }

        tracing::info!(
            "Installing package: {}#{:?} (dependencies: {}, examples: {}, filter: {})",
            package_name,
            package_version,
            include_dependencies,
            include_examples,
            if filter.is_active() { "active" } else { "none" }
        );

        // Load package from registry
        let registry = RegistryClient::new(self.registry_cache_dir.clone());
        let packages = if include_dependencies {
            registry
                .load_package_with_dependencies(package_name, package_version.as_deref())
                .await
                .map_err(|e| crate::Error::FhirContext(e.to_string()))?
        } else {
            vec![registry
                .load_package_with_version(package_name, package_version.as_deref())
                .await
                .map_err(|e| crate::Error::FhirContext(e.to_string()))?]
        };

        // Deduplicate packages
        let mut seen = std::collections::HashSet::new();
        let packages: Vec<_> = packages
            .into_iter()
            .filter(|p| seen.insert(format!("{}#{}", p.manifest.name, p.manifest.version)))
            .collect();

        tracing::info!("Loaded {} package(s) for {}", packages.len(), package_name);

        // Update job progress
        self.job_queue
            .update_progress(
                job.id,
                0,
                Some(packages.len() as i32),
                Some(serde_json::json!({
                    "loaded_packages": packages.len(),
                    "status": "installing"
                })),
            )
            .await?;

        let repo = PackageRepository::new(self.indexing_service.pool().clone());
        let store = crate::db::PostgresResourceStore::new(self.indexing_service.pool().clone());

        // Create search config from indexing service settings
        let search_config = crate::config::FhirSearchConfig {
            enable_text: self.indexing_service.enable_text_search(),
            enable_content: self.indexing_service.enable_content_search(),
            ..Default::default()
        };

        let search_engine = std::sync::Arc::new(crate::db::search::engine::SearchEngine::new(
            self.indexing_service.pool().clone(),
            search_config,
        ));

        // Create all hooks for package installation:
        // - SearchParameterHook: Updates search parameter definitions
        // - CompartmentDefinitionHook: Updates compartment membership rules
        let hooks: Vec<Arc<dyn ResourceHook>> = vec![
            Arc::new(SearchParameterHook::new(
                self.indexing_service.pool().clone(),
                self.indexing_service.clone(),
                search_engine.clone(),
                self.search_parameter_active_statuses.clone(),
            )),
            Arc::new(TerminologyHook::new(self.indexing_service.pool().clone())),
            Arc::new(CompartmentDefinitionHook::new(
                self.indexing_service.pool().clone(),
            )),
        ];

        let crud = CrudService::with_hooks(store.clone(), hooks.clone());
        let batch = crate::services::BatchService::new(
            store,
            hooks,
            self.job_queue.clone(),
            search_engine,
            true,
            false,
        );
        let service = PackageService::new(repo, crud, batch);

        // Install packages
        let mut installed = 0usize;
        let mut already_loaded = 0usize;
        let mut failed = 0usize;
        let mut errors = Vec::new();

        for (idx, pkg) in packages.iter().enumerate() {
            let pkg_name = pkg.manifest.name.clone();
            let pkg_version = pkg.manifest.version.clone();

            // Update progress
            self.job_queue
                .update_progress(
                    job.id,
                    idx as i32,
                    Some(packages.len() as i32),
                    Some(serde_json::json!({
                        "current_package": format!("{}#{}", pkg_name, pkg_version),
                        "installed": installed,
                        "already_loaded": already_loaded,
                        "failed": failed
                    })),
                )
                .await?;

            match service
                .install_package(pkg, include_examples, &filter)
                .await
            {
                Ok(outcome) => {
                    if outcome.already_loaded {
                        already_loaded += 1;
                        tracing::info!(
                            package_name = %outcome.name,
                            package_version = %outcome.version,
                            "Package already loaded"
                        );
                    } else {
                        installed += 1;
                        tracing::info!(
                            package_name = %outcome.name,
                            package_version = %outcome.version,
                            status = %outcome.status,
                            attempted = %outcome.attempted_resources,
                            stored = %outcome.stored_resources,
                            linked = %outcome.linked_resources,
                            failed = %outcome.failed_resources,
                            "Package installation completed"
                        );

                        // Log detailed error breakdown if available
                        if let Some(error_summary) = &outcome.error_summary {
                            tracing::warn!(
                                package_name = %outcome.name,
                                package_version = %outcome.version,
                                total_failures = %error_summary.total_failures,
                                by_category = ?error_summary.by_category,
                                by_resource_type = ?error_summary.by_resource_type,
                                sample_count = %error_summary.sample_failures.len(),
                                "Package installation had failures"
                            );

                            // Log first few sample failures for debugging
                            for (i, failure) in
                                error_summary.sample_failures.iter().take(3).enumerate()
                            {
                                tracing::debug!(
                                    package_name = %outcome.name,
                                    sample_num = %i,
                                    resource_type = ?failure.resource_type,
                                    resource_id = ?failure.resource_id,
                                    category = ?failure.category,
                                    error = %failure.error_message,
                                    "Sample failure"
                                );
                            }
                        }
                    }

                    if outcome.status == "failed" || outcome.status == "partial" {
                        if let Some(err) = &outcome.error_message {
                            errors.push(format!("{}#{}: {}", pkg_name, pkg_version, err));
                        }
                        if outcome.status == "failed" {
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    failed += 1;
                    let err_msg = format!("{}#{}: {}", pkg_name, pkg_version, e);
                    errors.push(err_msg.clone());
                    tracing::error!("Package install failed: {}", err_msg);
                }
            }

            // Check for cancellation
            if self.job_queue.is_cancelled(job.id).await? {
                tracing::warn!("Package installation job {} was cancelled", job.id);
                return Err(crate::Error::Internal("Job cancelled".to_string()));
            }
        }

        // Complete job
        let final_status = if failed == packages.len() {
            "all_failed"
        } else if failed > 0 {
            "partial_success"
        } else if already_loaded == packages.len() {
            "already_loaded"
        } else {
            "success"
        };

        let results = serde_json::json!({
            "installed": installed,
            "already_loaded": already_loaded,
            "failed": failed,
            "total": packages.len(),
            "status": final_status,
            "errors": errors
        });

        self.job_queue.complete_job(job.id, Some(results)).await?;

        tracing::info!(
            "{} completed job {}: {} installed, {} already loaded, {} failed",
            self.name(),
            job.id,
            installed,
            already_loaded,
            failed
        );

        Ok(())
    }
}
