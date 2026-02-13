//! Registry client for loading FHIR packages
//!
//! This is the async-first registry client for loading and caching FHIR packages.

use crate::async_simplifier::SimplifierClient;
use crate::cache::{FileSystemCache, PackageCache};
use crate::error::{Error, Result};
use crate::models::SimplifierSearchParams;
use crate::version_resolver::select_version;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use zunder_package::FhirPackage;

/// Registry client for loading FHIR packages.
///
/// Uses async HTTP requests for registry access and offloads cache (file I/O) to
/// `tokio::task::spawn_blocking` for efficient concurrent operations.
pub struct RegistryClient<C: PackageCache> {
    cache: Arc<C>,
    simplifier: Option<SimplifierClient>,
}

impl RegistryClient<FileSystemCache> {
    /// Create a new registry client with file system cache and Simplifier support.
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self {
            cache: Arc::new(FileSystemCache::new(cache_dir)),
            simplifier: SimplifierClient::new().ok(),
        }
    }

    /// Create a registry client without remote registry access (cache-only).
    pub fn cache_only(cache_dir: Option<PathBuf>) -> Self {
        Self {
            cache: Arc::new(FileSystemCache::new(cache_dir)),
            simplifier: None,
        }
    }
}

impl<C: PackageCache + 'static> RegistryClient<C> {
    /// Create a registry client with a custom cache implementation.
    pub fn with_cache(cache: C) -> Self {
        Self {
            cache: Arc::new(cache),
            simplifier: SimplifierClient::new().ok(),
        }
    }

    /// Create a registry client with a custom cache and no remote registry access.
    pub fn with_cache_only(cache: C) -> Self {
        Self {
            cache: Arc::new(cache),
            simplifier: None,
        }
    }

    async fn cache_has_package(&self, name: &str, version: &str) -> Result<bool> {
        let cache = self.cache.clone();
        let name = name.to_string();
        let version = version.to_string();
        let name_for_log = name.clone();
        let version_for_log = version.clone();
        let has = tokio::task::spawn_blocking(move || {
            Ok::<bool, Error>(cache.has_package(&name, &version))
        })
        .await
        .map_err(|e| Error::Registry(format!("Cache task failed: {e}")))??;
        if has {
            tracing::debug!("Cache hit: {}#{}", name_for_log, version_for_log);
        } else {
            tracing::debug!("Cache miss: {}#{}", name_for_log, version_for_log);
        }
        Ok(has)
    }

    async fn cache_get_package(&self, name: &str, version: &str) -> Result<FhirPackage> {
        let cache = self.cache.clone();
        let name = name.to_string();
        let version = version.to_string();
        tokio::task::spawn_blocking(move || cache.get_package(&name, &version))
            .await
            .map_err(|e| Error::Registry(format!("Cache task failed: {e}")))?
    }

    async fn cache_store_package(&self, package: FhirPackage) -> Result<()> {
        let cache = self.cache.clone();
        let name = package.manifest.name.clone();
        let version = package.manifest.version.clone();
        tokio::task::spawn_blocking(move || cache.store_package(&package))
            .await
            .map_err(|e| Error::Registry(format!("Cache task failed: {e}")))??;
        tracing::debug!("Cache store: {}#{}", name, version);
        Ok(())
    }

    async fn cache_list_packages(&self) -> Result<Vec<(String, String)>> {
        let cache = self.cache.clone();
        tokio::task::spawn_blocking(move || Ok(cache.list_packages()))
            .await
            .map_err(|e| Error::Registry(format!("Cache task failed: {e}")))?
    }

    /// Resolve a version range to a specific version.
    ///
    /// If the package isn't present in the cache, falls back to querying Simplifier (if enabled).
    pub async fn resolve_version(&self, name: &str, version_range: Option<&str>) -> Result<String> {
        let packages = self.cache_list_packages().await?;
        let cached_versions: Vec<String> = packages
            .into_iter()
            .filter(|(pkg_name, _)| pkg_name == name)
            .map(|(_, version)| version)
            .collect();

        // Try to resolve from cached versions first
        let mut available_versions = cached_versions.clone();
        let cached_resolution = if !available_versions.is_empty() {
            select_version(&available_versions, version_range)
        } else {
            None
        };

        // If resolution succeeded, return it; otherwise fetch all available versions
        if let Some(resolved) = cached_resolution {
            return Ok(resolved);
        }
        if let Some(simplifier) = &self.simplifier {
            available_versions = simplifier.get_versions(name).await?;
        }

        if available_versions.is_empty() {
            return Err(Error::PackageNotFound {
                name: name.to_string(),
                version: version_range.unwrap_or("latest").to_string(),
            });
        }

        let resolved = select_version(&available_versions, version_range);

        resolved.ok_or_else(|| Error::PackageNotFound {
            name: name.to_string(),
            version: version_range.unwrap_or("latest").to_string(),
        })
    }

    /// Load a package with optional version resolution, downloading if needed.
    pub async fn load_package_with_version(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<FhirPackage> {
        let resolved_version = self.resolve_version(name, version).await?;

        if self.cache_has_package(name, &resolved_version).await? {
            tracing::debug!("Loading from cache: {}#{}", name, resolved_version);
            return self.cache_get_package(name, &resolved_version).await;
        }

        let simplifier = self
            .simplifier
            .as_ref()
            .ok_or_else(|| Error::PackageNotFound {
                name: name.to_string(),
                version: resolved_version.clone(),
            })?;

        let package = simplifier.download_package(name, &resolved_version).await?;
        self.cache_store_package(package.clone()).await?;
        Ok(package)
    }

    /// Load a package with all transitive dependencies.
    ///
    /// Returns a vector of packages including the requested package and all its dependencies.
    /// Handles circular dependencies by only loading each package once.
    pub async fn load_package_with_dependencies(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<Vec<FhirPackage>> {
        let mut loaded_packages = HashMap::new();
        let mut loading = HashSet::new();

        struct Frame {
            name: String,
            version: Option<String>,
            stage: Stage,
        }

        enum Stage {
            Enter,
            Exit {
                package_key: String,
                package: Box<FhirPackage>,
            },
        }

        let mut stack = vec![Frame {
            name: name.to_string(),
            version: version.map(|v| v.to_string()),
            stage: Stage::Enter,
        }];

        while let Some(frame) = stack.pop() {
            match frame.stage {
                Stage::Enter => {
                    let package = self
                        .load_package_with_version(&frame.name, frame.version.as_deref())
                        .await?;
                    let package_key =
                        format!("{}#{}", package.manifest.name, package.manifest.version);

                    if loaded_packages.contains_key(&package_key) {
                        continue;
                    }

                    if loading.contains(&package_key) {
                        return Err(Error::InvalidPackage(format!(
                            "Circular dependency detected: {}",
                            package_key
                        )));
                    }

                    loading.insert(package_key.clone());

                    // Push exit frame first, then dependencies. This ensures a depth-first walk.
                    stack.push(Frame {
                        name: frame.name,
                        version: frame.version,
                        stage: Stage::Exit {
                            package_key: package_key.clone(),
                            package: Box::new(package.clone()),
                        },
                    });

                    for (dep_name, dep_version_range) in &package.manifest.dependencies {
                        let resolved_version = self
                            .resolve_version(dep_name, Some(dep_version_range))
                            .await?;
                        let dep_key = format!("{}#{}", dep_name, resolved_version);
                        if !loaded_packages.contains_key(&dep_key) {
                            stack.push(Frame {
                                name: dep_name.clone(),
                                version: Some(resolved_version),
                                stage: Stage::Enter,
                            });
                        }
                    }
                }
                Stage::Exit {
                    package_key,
                    package,
                } => {
                    loading.remove(&package_key);
                    loaded_packages.insert(package_key, *package);
                }
            }
        }

        Ok(loaded_packages.into_values().collect())
    }

    /// Load package from cache or download from Simplifier if not cached.
    pub async fn load_or_download_package(&self, name: &str, version: &str) -> Result<FhirPackage> {
        if self.cache_has_package(name, version).await? {
            tracing::debug!("Loading from cache: {}#{}", name, version);
            return self.cache_get_package(name, version).await;
        }

        let simplifier = self
            .simplifier
            .as_ref()
            .ok_or_else(|| Error::PackageNotFound {
                name: name.to_string(),
                version: version.to_string(),
            })?;

        let package = simplifier.download_package(name, version).await?;
        self.cache_store_package(package.clone()).await?;
        Ok(package)
    }

    /// Search for packages in Simplifier registry.
    pub async fn search_packages(
        &self,
        params: &SimplifierSearchParams,
    ) -> Result<Vec<crate::models::SimplifierSearchResult>> {
        let simplifier = self
            .simplifier
            .as_ref()
            .ok_or_else(|| Error::Registry("Simplifier client not available".to_string()))?;

        simplifier.search(params).await
    }

    /// Get available versions for a package from Simplifier.
    pub async fn get_package_versions(&self, package_name: &str) -> Result<Vec<String>> {
        let simplifier = self
            .simplifier
            .as_ref()
            .ok_or_else(|| Error::Registry("Simplifier client not available".to_string()))?;

        simplifier.get_versions(package_name).await
    }
}
