//! Server startup and initialization
//!
//! This module handles server initialization tasks including:
//! - Internal server package (ferrum.fhir.server) - loaded directly from filesystem
//! - Public FHIR packages (core, extensions, terminology) - loaded from registry with caching

use crate::{
    config::{Config, ResourceTypeFilter},
    db::{packages::PackageRepository, PostgresResourceStore},
    hooks::{search_parameter::SearchParameterHook, terminology::TerminologyHook, ResourceHook},
    queue::{JobQueue, PostgresJobQueue},
    services::{CrudService, PackageService},
    Result,
};
use sqlx::PgPool;
use std::sync::Arc;
use ferrum_registry_client::RegistryClient;

/// Package descriptor with installation configuration
#[derive(Debug, Clone)]
pub struct PackageDescriptor {
    pub name: String,
    pub version: String,
    pub install_examples: bool,
    pub filter: ResourceTypeFilter,
    pub package_category: PackageCategory,
}

/// Category of package for logging and tracking
#[derive(Debug, Clone, Copy)]
pub enum PackageCategory {
    Core,
    Extensions,
    Terminology,
    Custom,
}

impl PackageCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Extensions => "extensions",
            Self::Terminology => "terminology",
            Self::Custom => "custom",
        }
    }
}

/// Install all FHIR packages required by the server
pub async fn install_all_packages(config: &Config, db_pool: &PgPool) -> Result<()> {
    let package_repo = PackageRepository::new(db_pool.clone());

    // 1. Load public packages from registry FIRST (provides StructureDefinitions for FHIRPath)
    //    This ensures the FHIR context has all necessary type definitions before
    //    indexing the internal package's OperationDefinitions
    install_public_packages(&package_repo, db_pool, config).await?;

    // 2. Load internal packages from fhir_packages/ directory SECOND
    //    Now the FHIR context has StructureDefinitions, so indexing will work
    if config.fhir.install_internal_packages {
        install_internal_packages(&package_repo, db_pool, config).await?;
    } else {
        tracing::info!(
            "Internal package installation disabled (fhir.install_internal_packages=false)"
        );
    }

    Ok(())
}

/// Install all internal packages from fhir_packages/ directory
///
/// These packages contain custom OperationDefinitions and other resources and are loaded directly
/// from the filesystem without using the registry client.
///
/// **IMPORTANT**: Must be called AFTER public packages are installed!
/// The indexing service needs StructureDefinitions (uri, canonical, etc.)
/// from the core FHIR package to successfully index OperationDefinitions.
async fn install_internal_packages(
    package_repo: &PackageRepository,
    db_pool: &PgPool,
    config: &Config,
) -> Result<()> {
    // Resolve internal package directory:
    // 1) explicit config override
    // 2) ./fhir_packages relative to current working directory (container-friendly)
    // 3) compile-time crate directory (development-friendly)
    let packages_dir = if let Some(dir) = config.fhir.internal_packages_dir.as_deref() {
        std::path::PathBuf::from(dir)
    } else {
        let cwd_packages_dir = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("fhir_packages");

        if cwd_packages_dir.exists() {
            cwd_packages_dir
        } else {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fhir_packages")
        }
    };

    if !packages_dir.exists() {
        tracing::info!(
            "Internal packages directory not found at {:?}, skipping internal packages",
            packages_dir
        );
        return Ok(());
    }

    tracing::info!("Scanning for internal packages in: {:?}", packages_dir);

    // Read directory and find all package directories
    let entries = std::fs::read_dir(&packages_dir).map_err(|e| {
        crate::Error::Internal(format!("Failed to read fhir_packages directory: {}", e))
    })?;

    let mut packages_to_install = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| {
            crate::Error::Internal(format!("Failed to read directory entry: {}", e))
        })?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Check if directory contains a package/ subdirectory
        let package_path = path.join("package");
        if !package_path.exists() {
            tracing::debug!("Skipping {:?} - no package/ subdirectory", path);
            continue;
        }

        // Extract package name and version from directory name (format: name#version)
        let dir_name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            crate::Error::Internal(format!("Invalid package directory name: {:?}", path))
        })?;

        packages_to_install.push((dir_name.to_string(), package_path));
    }

    if packages_to_install.is_empty() {
        tracing::info!("No internal packages found in fhir_packages/");
        return Ok(());
    }

    tracing::info!(
        "Found {} internal package(s) to install",
        packages_to_install.len()
    );

    // Install each package
    let package_service = create_package_service(package_repo, db_pool, config)?;
    let no_filter = ResourceTypeFilter {
        include_resource_types: None,
        exclude_resource_types: None,
    };

    for (pkg_name, package_path) in packages_to_install {
        tracing::info!("Loading internal package from filesystem: {}", pkg_name);

        // Load package directly from filesystem
        let package = ferrum_registry_client::FhirPackage::from_directory(&package_path)
            .map_err(|e| {
                crate::Error::Internal(format!(
                    "Failed to load internal package {}: {}",
                    pkg_name, e
                ))
            })?;

        // Install using package service (no examples, no filtering for internal packages).
        // PackageService is responsible for detecting whether the exact name#version
        // is already installed and skipping accordingly.
        let outcome = package_service
            .install_package(&package, false, &no_filter)
            .await?;

        if outcome.is_failure() {
            let error_msg = outcome
                .error_message
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(crate::Error::Internal(format!(
                "Failed to install internal package {}: {}",
                pkg_name, error_msg
            )));
        }

        if outcome.already_loaded {
            tracing::info!("Internal package {} already loaded, skipping", pkg_name);
        } else {
            tracing::info!(
                "Internal package {} installed successfully: {} resources",
                pkg_name,
                outcome.stored_resources
            );
        }
    }

    Ok(())
}

/// Install public FHIR packages from registry
///
/// Includes:
/// - Core FHIR package (hl7.fhir.r4.core, etc.)
/// - Companion packages (extensions, terminology)
/// - User-configured packages from config
async fn install_public_packages(
    package_repo: &PackageRepository,
    db_pool: &PgPool,
    config: &Config,
) -> Result<()> {
    // Create registry client (uses ~/.fhir/packages cache by default)
    let registry = Arc::new(RegistryClient::new(None));

    // Get default packages (core, extensions, terminology)
    let mut package_descriptors =
        get_default_packages(&config.fhir.version, &config.fhir.default_packages)?;

    // Add custom packages from config
    for pkg_config in &config.fhir.packages {
        package_descriptors.push(PackageDescriptor {
            name: pkg_config.name.clone(),
            version: pkg_config.version.clone(),
            install_examples: pkg_config.install_examples,
            filter: pkg_config.filter.clone(),
            package_category: PackageCategory::Custom,
        });
    }

    let default_count = package_descriptors
        .iter()
        .filter(|p| {
            matches!(
                p.package_category,
                PackageCategory::Core | PackageCategory::Extensions | PackageCategory::Terminology
            )
        })
        .count();
    let custom_count = package_descriptors
        .iter()
        .filter(|p| matches!(p.package_category, PackageCategory::Custom))
        .count();

    tracing::info!(
        "Loading {} FHIR packages from registry ({} default, {} custom)...",
        package_descriptors.len(),
        default_count,
        custom_count
    );

    // Load all packages in parallel
    let mut load_tasks = Vec::new();
    for descriptor in package_descriptors {
        let registry_clone = registry.clone();
        let package_repo_clone = package_repo.clone();
        let name = descriptor.name.clone();
        let version = descriptor.version.clone();

        let task = tokio::spawn(async move {
            // Check if any version already cached in database
            if let Ok(Some((existing_id, existing_version))) = package_repo_clone
                .get_existing_package_id_by_name(&name)
                .await
            {
                tracing::info!(
                    "Package {} already exists in database (id: {}, version: {}), skipping",
                    name,
                    existing_id,
                    existing_version
                );
                return Ok::<_, crate::Error>((Vec::new(), descriptor));
            }

            tracing::info!("Loading package from registry: {}#{}", name, version);

            let packages = registry_clone
                .load_package_with_dependencies(&name, Some(&version))
                .await
                .map_err(|e| crate::Error::FhirContext(e.to_string()))?;

            tracing::info!(
                "Loaded {}#{} with {} total packages",
                name,
                version,
                packages.len()
            );
            Ok((packages, descriptor))
        });

        load_tasks.push(task);
    }

    // Wait for all loads to complete and collect with descriptors
    let mut packages_with_descriptors = Vec::new();
    for task in load_tasks {
        let (packages, descriptor) = task
            .await
            .map_err(|e| crate::Error::Internal(format!("Task join error: {}", e)))??;

        // Attach descriptor to each package in the dependency tree
        for package in packages {
            packages_with_descriptors.push((
                package,
                descriptor.install_examples,
                descriptor.filter.clone(),
                descriptor.package_category,
            ));
        }
    }

    // Deduplicate packages (strip descriptors for dedup, then reattach)
    let packages_only: Vec<_> = packages_with_descriptors
        .iter()
        .map(|(pkg, _, _, _)| pkg.clone())
        .collect();
    let unique_packages = deduplicate_packages(packages_only, package_repo).await?;

    // Reattach descriptors to unique packages
    let mut unique_with_descriptors = Vec::new();
    for unique_pkg in unique_packages {
        // Find the descriptor for this package
        if let Some((_, install_examples, filter, category)) = packages_with_descriptors
            .iter()
            .find(|(pkg, _, _, _)| pkg.manifest.name == unique_pkg.manifest.name)
        {
            unique_with_descriptors.push((
                unique_pkg,
                *install_examples,
                filter.clone(),
                *category,
            ));
        }
    }

    // Install packages in batch
    if !unique_with_descriptors.is_empty() {
        tracing::info!(
            "Installing {} unique packages into database...",
            unique_with_descriptors.len()
        );
        install_packages_batch(package_repo, db_pool, &unique_with_descriptors, config).await?;
    } else {
        tracing::info!("All public packages already installed");
    }

    tracing::info!("All FHIR packages checked/installed successfully");
    Ok(())
}

/// Get default packages for a FHIR version with configuration
fn get_default_packages(
    fhir_version: &str,
    default_pkg_config: &crate::config::DefaultPackagesConfig,
) -> Result<Vec<PackageDescriptor>> {
    let mut packages = Vec::new();

    // Define version-specific packages
    let (core_name, core_version, ext_name, ext_version, term_name, term_version) =
        match fhir_version {
            "R4" => (
                "hl7.fhir.r4.core",
                "4.0.1",
                "hl7.fhir.uv.extensions.r4",
                "latest",
                "hl7.terminology.r4",
                "latest",
            ),
            "R4B" => (
                "hl7.fhir.r4b.core",
                "4.3.0",
                "hl7.fhir.uv.extensions.r4",
                "latest",
                "hl7.terminology.r4",
                "latest",
            ),
            "R5" => (
                "hl7.fhir.r5.core",
                "5.0.0",
                "hl7.fhir.uv.extensions.r5",
                "latest",
                "hl7.terminology.r5",
                "latest",
            ),
            other => {
                return Err(crate::Error::Internal(format!(
                    "Unsupported FHIR version: {}",
                    other
                )));
            }
        };

    // Add core package if enabled
    if default_pkg_config.core.install {
        packages.push(PackageDescriptor {
            name: core_name.to_string(),
            version: core_version.to_string(),
            install_examples: default_pkg_config.core.install_examples,
            filter: default_pkg_config.core.filter.clone(),
            package_category: PackageCategory::Core,
        });
    }

    // Add extensions package if enabled
    if default_pkg_config.extensions.install {
        packages.push(PackageDescriptor {
            name: ext_name.to_string(),
            version: ext_version.to_string(),
            install_examples: default_pkg_config.extensions.install_examples,
            filter: default_pkg_config.extensions.filter.clone(),
            package_category: PackageCategory::Extensions,
        });
    }

    // Add terminology package if enabled
    if default_pkg_config.terminology.install {
        packages.push(PackageDescriptor {
            name: term_name.to_string(),
            version: term_version.to_string(),
            install_examples: default_pkg_config.terminology.install_examples,
            filter: default_pkg_config.terminology.filter.clone(),
            package_category: PackageCategory::Terminology,
        });
    }

    Ok(packages)
}

/// Deduplicate packages and filter out already-installed ones
async fn deduplicate_packages(
    packages: Vec<ferrum_registry_client::FhirPackage>,
    package_repo: &PackageRepository,
) -> Result<Vec<ferrum_registry_client::FhirPackage>> {
    // Deduplicate by name#version
    let mut seen = std::collections::HashSet::new();
    let mut unique_packages = Vec::new();
    for pkg in packages {
        let key = format!("{}#{}", pkg.manifest.name, pkg.manifest.version);
        if seen.insert(key.clone()) {
            unique_packages.push(pkg);
        } else {
            tracing::debug!("Skipping duplicate package: {}", key);
        }
    }

    // Filter out already-installed packages (any version)
    let mut packages_to_install = Vec::new();
    for pkg in unique_packages {
        if let Some((existing_id, existing_version)) = package_repo
            .get_existing_package_id_by_name(&pkg.manifest.name)
            .await?
        {
            tracing::debug!(
                "Package {} already exists (id: {}, version: {}), skipping",
                pkg.manifest.name,
                existing_id,
                existing_version
            );
        } else {
            packages_to_install.push(pkg);
        }
    }

    Ok(packages_to_install)
}

/// Install a batch of packages into the database
///
/// Packages are installed in parallel with controlled concurrency to speed up installation
/// while avoiding overwhelming the database connection pool. After deduplication, packages
/// are independent and can be safely installed concurrently.
async fn install_packages_batch(
    package_repo: &PackageRepository,
    db_pool: &PgPool,
    packages: &[(
        ferrum_registry_client::FhirPackage,
        bool,
        ResourceTypeFilter,
        PackageCategory,
    )],
    config: &Config,
) -> Result<()> {
    // Use semaphore to limit concurrent installations (avoids overwhelming DB pool)
    // Limit to 4 concurrent installations - balances speed with connection pool usage
    let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
    let mut install_tasks = Vec::new();

    for (package, install_examples, filter, category) in packages {
        let package = package.clone();
        let install_examples = *install_examples;
        let filter = filter.clone();
        let category = *category;
        let package_repo_clone = package_repo.clone();
        let db_pool_clone = db_pool.clone();
        let config_clone = config.clone();
        let semaphore_clone = semaphore.clone();

        let name = package.manifest.name.clone();
        let version = package.manifest.version.clone();

        let task = tokio::spawn(async move {
            // Acquire semaphore permit (limits concurrent installations)
            let _permit = semaphore_clone.acquire().await.map_err(|e| {
                crate::Error::Internal(format!("Failed to acquire semaphore: {}", e))
            })?;

            // Create package service for this installation
            let package_service =
                create_package_service(&package_repo_clone, &db_pool_clone, &config_clone)?;

            let start = std::time::Instant::now();
            let filter_status = if filter.is_active() { "active" } else { "none" };
            tracing::info!(
                "Installing {} package: {}#{} (examples: {}, filter: {})",
                category.as_str(),
                name,
                version,
                install_examples,
                filter_status
            );

            let outcome = package_service
                .install_package(&package, install_examples, &filter)
                .await?;

            let duration = start.elapsed();

            if outcome.is_failure() {
                let error_msg = outcome
                    .error_message
                    .unwrap_or_else(|| "Unknown error".to_string());
                tracing::error!(
                    "Failed to install package {}#{} after {:?}: {}",
                    name,
                    version,
                    duration,
                    error_msg
                );
                Err(crate::Error::Internal(format!(
                    "Failed to install package {}#{}: {}",
                    name, version, error_msg
                )))
            } else if outcome.is_partial() {
                tracing::warn!(
                    "Package {}#{} partially installed in {:?}: {} succeeded, {} failed",
                    name,
                    version,
                    duration,
                    outcome.stored_resources,
                    outcome.failed_resources
                );
                Ok(())
            } else {
                tracing::info!(
                    "Package {}#{} installed in {:?}: {} resources",
                    name,
                    version,
                    duration,
                    outcome.stored_resources
                );
                Ok(())
            }
        });

        install_tasks.push(task);
    }

    // Wait for all installations to complete and collect any errors
    for task in install_tasks {
        task.await
            .map_err(|e| crate::Error::Internal(format!("Task join error: {}", e)))??;
    }

    Ok(())
}

/// Create package service with hooks for conformance resource processing
fn create_package_service(
    package_repo: &PackageRepository,
    db_pool: &PgPool,
    config: &Config,
) -> Result<PackageService> {
    let job_queue: Arc<dyn JobQueue> = Arc::new(PostgresJobQueue::new(
        db_pool.clone(),
        config.workers.poll_interval_seconds,
    ));

    let search_engine = std::sync::Arc::new(crate::db::search::engine::SearchEngine::new(
        db_pool.clone(),
        config.fhir.search.clone(),
    ));

    // Create hooks (SearchParameter processing, etc.)
    let hooks = create_package_hooks(
        db_pool,
        &config.fhir.version,
        config.fhir.search.enable_text,
        config.fhir.search.enable_content,
        config.fhir.search.search_parameter_active_statuses.clone(),
        search_engine.clone(),
    )?;

    // Create services
    let store = PostgresResourceStore::new(db_pool.clone());
    let crud = CrudService::with_hooks(store.clone(), hooks.clone());
    let batch = crate::services::BatchService::new(
        store,
        hooks,
        job_queue,
        search_engine,
        config.fhir.allow_update_create,
        config.fhir.hard_delete,
    );

    Ok(PackageService::new(package_repo.clone(), crud, batch))
}

/// Create hooks for package installation
///
/// These hooks are triggered when conformance resources (like SearchParameter, CompartmentDefinition)
/// are installed from packages.
fn create_package_hooks(
    db_pool: &PgPool,
    fhir_version: &str,
    enable_text_search: bool,
    enable_content_search: bool,
    search_parameter_active_statuses: Vec<String>,
    search_engine: std::sync::Arc<crate::db::search::engine::SearchEngine>,
) -> Result<Vec<Arc<dyn ResourceHook>>> {
    let indexing_service = Arc::new(crate::services::IndexingService::new(
        db_pool.clone(),
        fhir_version,
        50,  // Default batch size
        200, // Default bulk threshold
        enable_text_search,
        enable_content_search,
    )?);

    Ok(vec![
        Arc::new(SearchParameterHook::new(
            db_pool.clone(),
            indexing_service,
            search_engine,
            search_parameter_active_statuses,
        )),
        Arc::new(TerminologyHook::new(db_pool.clone())),
        Arc::new(
            crate::hooks::compartment_definition::CompartmentDefinitionHook::new(db_pool.clone()),
        ),
    ])
}
