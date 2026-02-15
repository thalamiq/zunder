//! Package cache trait and implementations

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use ferrum_package::FhirPackage;

/// Trait for FHIR package cache implementations.
///
/// Implement this trait to create custom cache backends (e.g., database, Redis, S3).
pub trait PackageCache: Send + Sync {
    /// Check if a package is cached
    fn has_package(&self, name: &str, version: &str) -> bool;

    /// Load a package from cache
    fn get_package(&self, name: &str, version: &str) -> Result<FhirPackage>;

    /// Store a package in cache
    fn store_package(&self, package: &FhirPackage) -> Result<()>;

    /// List all cached packages as (name, version) tuples
    fn list_packages(&self) -> Vec<(String, String)>;
}

/// File system-based package cache following FHIR package specification.
///
/// Stores packages in `~/.fhir/packages` by default.
pub struct FileSystemCache {
    cache_root: PathBuf,
}

impl FileSystemCache {
    /// Create a new file system cache
    pub fn new(cache_root: Option<PathBuf>) -> Self {
        let root = cache_root.unwrap_or_else(Self::default_cache_location);
        Self { cache_root: root }
    }

    /// Get default cache location: ~/.fhir/packages
    fn default_cache_location() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".fhir")
            .join("packages")
    }

    /// Get package directory path
    fn get_package_directory(&self, name: &str, version: &str) -> PathBuf {
        self.cache_root.join(format!("{}#{}", name, version))
    }

    /// Get cache root directory
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }
}

impl PackageCache for FileSystemCache {
    fn has_package(&self, name: &str, version: &str) -> bool {
        let package_dir = self.get_package_directory(name, version);
        let package_json = package_dir.join("package").join("package.json");
        package_json.exists()
    }

    fn get_package(&self, name: &str, version: &str) -> Result<FhirPackage> {
        let package_dir = self.get_package_directory(name, version);
        let package_path = package_dir.join("package");

        if !package_path.exists() {
            return Err(Error::PackageNotFound {
                name: name.to_string(),
                version: version.to_string(),
            });
        }

        FhirPackage::from_directory(&package_path).map_err(Into::into)
    }

    fn list_packages(&self) -> Vec<(String, String)> {
        let mut packages = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&self.cache_root) {
            for entry in entries.flatten() {
                let dir_name = entry.file_name();
                if let Some(name_str) = dir_name.to_str() {
                    // Parse format: "name#version"
                    if let Some((name, version)) = name_str.split_once('#') {
                        if self.has_package(name, version) {
                            packages.push((name.to_string(), version.to_string()));
                        }
                    }
                }
            }
        }

        packages
    }

    fn store_package(&self, package: &FhirPackage) -> Result<()> {
        use std::fs;

        let name = &package.manifest.name;
        let version = &package.manifest.version;
        let package_dir = self.get_package_directory(name, version);
        let package_path = package_dir.join("package");

        // Remove any existing incomplete cache before writing
        if package_path.exists() {
            fs::remove_dir_all(&package_path)?;
        }

        // Create directory structure
        fs::create_dir_all(&package_path)?;

        // Write manifest
        let manifest_path = package_path.join("package.json");
        let manifest_json = serde_json::to_string_pretty(&package.manifest)?;
        fs::write(manifest_path, manifest_json)?;

        // Write index if present
        if let Some(index) = &package.index {
            let index_path = package_path.join(".index.json");
            let index_json = serde_json::to_string_pretty(index)?;
            fs::write(index_path, index_json)?;
        }

        // Write resources
        for (i, resource) in package.resources.iter().enumerate() {
            let resource_type = resource
                .get("resourceType")
                .and_then(|v| v.as_str())
                .unwrap_or("Resource");
            let default_id = format!("resource-{}", i);
            let id = resource
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(&default_id);
            // Sanitize id to remove path separators that would break fs::write
            let safe_id: String = id.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
            let filename = format!("{}-{}.json", resource_type, safe_id);
            let resource_path = package_path.join(&filename);
            let resource_json = serde_json::to_string_pretty(resource)?;
            fs::write(resource_path, resource_json)?;
        }

        // Write examples
        if !package.examples.is_empty() {
            let examples_dir = package_path.join("examples");
            fs::create_dir_all(&examples_dir)?;

            for (i, example) in package.examples.iter().enumerate() {
                let resource_type = example
                    .get("resourceType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Example");
                let default_id = format!("example-{}", i);
                let id = example
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&default_id);
                let filename = format!("{}-{}.json", resource_type, id);
                let example_path = examples_dir.join(filename);
                let example_json = serde_json::to_string_pretty(example)?;
                fs::write(example_path, example_json)?;
            }
        }

        Ok(())
    }
}
