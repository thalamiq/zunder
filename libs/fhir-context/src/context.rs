use crate::error::{Error, Result};
use crate::loader::PackageLoader;
use crate::version::{extract_version_algorithm, select_from_version_index, VersionKey};
use async_trait::async_trait;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::future::Future;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zunder_models::{ElementTypeInfo, StructureDefinition};
use zunder_package::FhirPackage;
use tokio::runtime::Handle;

#[async_trait]
pub trait ConformanceResourceProvider: Send + Sync {
    /// Returns resources for a canonical URL (potentially multiple versions).
    ///
    /// For database-backed providers this typically returns the "active" set
    /// (e.g., current rows), while package-backed providers often return all
    /// known versions.
    async fn list_by_canonical(&self, canonical_url: &str) -> Result<Vec<Arc<Value>>>;

    /// Fetch a specific resource by canonical URL and business version.
    ///
    /// Default implementation falls back to `list_by_canonical` + in-memory selection.
    async fn get_by_canonical_and_version(
        &self,
        canonical_url: &str,
        version: &str,
    ) -> Result<Option<Arc<Value>>> {
        let resources = self.list_by_canonical(canonical_url).await?;
        let mut versions: BTreeMap<VersionKey, Arc<Value>> = BTreeMap::new();
        for resource in resources {
            let Some(url) = resource.get("url").and_then(|v| v.as_str()) else {
                continue;
            };
            if url != canonical_url {
                continue;
            }

            let version_str = resource
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0");

            let algorithm = extract_version_algorithm(resource.as_ref());
            versions.insert(VersionKey::new(version_str, algorithm), resource);
        }

        Ok(select_from_version_index(&versions, Some(version)).cloned())
    }
}

pub struct FallbackConformanceProvider {
    primary: Arc<dyn ConformanceResourceProvider>,
    fallback: Arc<dyn ConformanceResourceProvider>,
}

impl FallbackConformanceProvider {
    pub fn new(
        primary: Arc<dyn ConformanceResourceProvider>,
        fallback: Arc<dyn ConformanceResourceProvider>,
    ) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait]
impl ConformanceResourceProvider for FallbackConformanceProvider {
    async fn list_by_canonical(&self, canonical_url: &str) -> Result<Vec<Arc<Value>>> {
        match self.primary.list_by_canonical(canonical_url).await {
            Ok(primary) if !primary.is_empty() => Ok(primary),
            Ok(_) => self.fallback.list_by_canonical(canonical_url).await,
            Err(primary_err) => match self.fallback.list_by_canonical(canonical_url).await {
                Ok(v) => Ok(v),
                Err(_) => Err(primary_err),
            },
        }
    }

    async fn get_by_canonical_and_version(
        &self,
        canonical_url: &str,
        version: &str,
    ) -> Result<Option<Arc<Value>>> {
        match self
            .primary
            .get_by_canonical_and_version(canonical_url, version)
            .await
        {
            Ok(Some(resource)) => Ok(Some(resource)),
            Ok(None) => {
                self.fallback
                    .get_by_canonical_and_version(canonical_url, version)
                    .await
            }
            Err(primary_err) => match self
                .fallback
                .get_by_canonical_and_version(canonical_url, version)
                .await
            {
                Ok(v) => Ok(v),
                Err(_) => Err(primary_err),
            },
        }
    }
}

#[derive(Clone)]
pub struct FlexibleFhirContext(Arc<FlexibleFhirContextInner>);

#[derive(Clone, Eq, PartialEq, Hash)]
struct CanonicalVersionKey {
    canonical: String,
    version: String,
}

struct FlexibleFhirContextInner {
    provider: Arc<dyn ConformanceResourceProvider>,
    canonical_cache: Mutex<LruCache<String, CanonicalCacheEntry>>,
    version_cache: Mutex<LruCache<CanonicalVersionKey, VersionCacheEntry>>,
    ttl_millis: AtomicU64,
    handle: Handle,
}

struct CanonicalCacheEntry {
    loaded_at: Instant,
    versions: BTreeMap<VersionKey, Arc<Value>>,
}

struct VersionCacheEntry {
    loaded_at: Instant,
    resource: Option<Arc<Value>>,
}

impl FlexibleFhirContext {
    pub fn new(provider: Arc<dyn ConformanceResourceProvider>) -> Result<Self> {
        let handle = Handle::try_current().map_err(|_| Error::AsyncRuntimeUnavailable)?;
        Ok(Self::with_handle(handle, provider))
    }

    pub fn with_handle(handle: Handle, provider: Arc<dyn ConformanceResourceProvider>) -> Self {
        let capacity = NonZeroUsize::new(4096).unwrap();
        let canonical_cache = Mutex::new(LruCache::new(capacity));
        let version_cache = Mutex::new(LruCache::new(capacity));
        Self(Arc::new(FlexibleFhirContextInner {
            provider,
            canonical_cache,
            version_cache,
            ttl_millis: AtomicU64::new(60_000),
            handle,
        }))
    }

    pub fn with_cache_capacity(self, capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity.max(1)).unwrap();
        {
            let mut cache = self
                .0
                .canonical_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cache = LruCache::new(capacity);
        }
        {
            let mut cache = self
                .0
                .version_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cache = LruCache::new(capacity);
        }
        self
    }

    pub fn with_ttl(self, ttl: Option<Duration>) -> Self {
        let millis = ttl.map(|d| d.as_millis() as u64).unwrap_or(0);
        self.0.ttl_millis.store(millis, AtomicOrdering::Relaxed);
        self
    }

    pub fn invalidate(&self, canonical_url: &str) {
        let mut canonical_cache = self
            .0
            .canonical_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        canonical_cache.pop(canonical_url);

        let mut version_cache = self
            .0
            .version_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys_to_remove: Vec<CanonicalVersionKey> = version_cache
            .iter()
            .filter(|(k, _)| k.canonical == canonical_url)
            .map(|(k, _)| k.clone())
            .collect();
        for k in keys_to_remove {
            version_cache.pop(&k);
        }
    }

    pub fn clear_cache(&self) {
        let mut canonical_cache = self
            .0
            .canonical_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        canonical_cache.clear();
        drop(canonical_cache);

        let mut version_cache = self
            .0
            .version_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        version_cache.clear();
    }

    fn block_on<T>(&self, fut: impl Future<Output = T> + Send + 'static) -> T
    where
        T: Send + 'static,
    {
        let handle = self.0.handle.clone();

        // When called from within a Tokio runtime, `Handle::block_on` is not allowed.
        if Handle::try_current().is_ok() {
            // Prefer `block_in_place` on multithreaded runtimes to avoid starving the executor.
            if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
                return tokio::task::block_in_place(|| handle.block_on(fut));
            }

            // Current-thread runtimes can't use `block_in_place`: hop to a plain thread.
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = tx.send(handle.block_on(fut));
            });
            return rx.recv().expect("context async task thread died");
        }

        handle.block_on(fut)
    }

    async fn get_resource_by_url_async(
        inner: Arc<FlexibleFhirContextInner>,
        canonical_url: String,
        version: Option<String>,
    ) -> Result<Option<Arc<Value>>> {
        let ttl_millis = inner.ttl_millis.load(AtomicOrdering::Relaxed);
        let ttl = (ttl_millis != 0).then(|| Duration::from_millis(ttl_millis));

        // Exact-version lookup path.
        if let Some(ref version) = version {
            // 1) Try canonical cache first (fast if that canonical was resolved recently).
            {
                let mut cache = inner
                    .canonical_cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if let Some(entry) = cache.get(&canonical_url) {
                    if ttl.is_some_and(|ttl| entry.loaded_at.elapsed() > ttl) {
                        // Expired: fall through.
                    } else if let Some(hit) =
                        select_from_version_index(&entry.versions, Some(version)).cloned()
                    {
                        return Ok(Some(hit));
                    }
                }
            }

            // 2) Try exact-version cache.
            let key = CanonicalVersionKey {
                canonical: canonical_url.clone(),
                version: version.clone(),
            };
            {
                let mut cache = inner
                    .version_cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if let Some(entry) = cache.get(&key) {
                    if ttl.is_some_and(|ttl| entry.loaded_at.elapsed() > ttl) {
                        // Expired: fall through.
                    } else {
                        return Ok(entry.resource.clone());
                    }
                }
            }

            // 3) Fetch exact version from provider (may include non-current/historical rows).
            let fetched = inner
                .provider
                .get_by_canonical_and_version(&canonical_url, version)
                .await?;

            // 4) Store (including negative cache entries).
            {
                let mut cache = inner
                    .version_cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                cache.put(
                    key,
                    VersionCacheEntry {
                        loaded_at: Instant::now(),
                        resource: fetched.clone(),
                    },
                );
            }

            return Ok(fetched);
        }

        // Latest lookup path: always derived from the canonical cache.
        {
            let mut cache = inner
                .canonical_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = cache.get(&canonical_url) {
                if ttl.is_some_and(|ttl| entry.loaded_at.elapsed() > ttl) {
                    // Expired: fall through to reload.
                } else {
                    return Ok(select_from_version_index(&entry.versions, None).cloned());
                }
            }
        }

        // Load from provider (no lock held while awaiting).
        let resources = inner.provider.list_by_canonical(&canonical_url).await?;

        let mut versions: BTreeMap<VersionKey, Arc<Value>> = BTreeMap::new();
        for resource in resources {
            let Some(url) = resource.get("url").and_then(|v| v.as_str()) else {
                continue;
            };
            if url != canonical_url {
                continue;
            }

            let version_str = resource
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0");

            let algorithm = extract_version_algorithm(resource.as_ref());
            versions.insert(VersionKey::new(version_str, algorithm), resource);
        }

        let selected = select_from_version_index(&versions, None).cloned();

        {
            let mut cache = inner
                .canonical_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            cache.put(
                canonical_url,
                CanonicalCacheEntry {
                    loaded_at: Instant::now(),
                    versions,
                },
            );
        }

        Ok(selected)
    }
}

pub trait FhirContext: Send + Sync {
    fn get_resource_by_url(
        &self,
        canonical_url: &str,
        version: Option<&str>,
    ) -> Result<Option<Arc<Value>>>;

    /// Get the latest resource (highest version) for a canonical URL
    fn get_latest_resource_by_url(&self, canonical_url: &str) -> Result<Option<Arc<Value>>> {
        self.get_resource_by_url(canonical_url, None)
    }

    /// Get a StructureDefinition by canonical URL
    fn get_structure_definition(
        &self,
        canonical_url: &str,
    ) -> Result<Option<Arc<StructureDefinition>>> {
        if let Some(resource) = self.get_latest_resource_by_url(canonical_url)? {
            let sd: StructureDefinition = serde_json::from_value(Arc::unwrap_or_clone(resource))?;
            Ok(Some(Arc::new(sd)))
        } else {
            Ok(None)
        }
    }

    /// Get a StructureDefinition by type name (e.g., "Patient")
    fn get_core_structure_definition_by_type(
        &self,
        type_name: &str,
    ) -> Result<Option<Arc<StructureDefinition>>> {
        let canonical_url = format!("http://hl7.org/fhir/StructureDefinition/{}", type_name);
        self.get_structure_definition(&canonical_url)
    }

    /// Get a StructureDefinition from a resource (checks meta.profile or resourceType)
    fn get_structure_definition_from_resource(
        &self,
        resource: &Value,
    ) -> Result<Option<Arc<StructureDefinition>>> {
        // Try meta.profile first
        if let Some(profiles) = resource
            .get("meta")
            .and_then(|m| m.get("profile"))
            .and_then(|p| p.as_array())
        {
            if let Some(profile_url) = profiles.first().and_then(|v| v.as_str()) {
                if let Some(sd) = self.get_structure_definition(profile_url)? {
                    return Ok(Some(sd));
                }
            }
        }

        // Fallback to resourceType
        if let Some(resource_type) = resource.get("resourceType").and_then(|v| v.as_str()) {
            return self.get_core_structure_definition_by_type(resource_type);
        }

        Ok(None)
    }

    /// Resolve profile URLs for validation based on explicit profiles, meta.profile, and resourceType
    ///
    /// Returns a list of canonical URLs to validate against in priority order:
    /// 1. If explicit_profiles is provided, use those
    /// 2. Otherwise, if meta.profile is present, use those + base as fallback
    /// 3. Otherwise, use base profile for the resourceType
    fn resolve_validation_profiles(
        &self,
        resource: &Value,
        explicit_profiles: Option<&[String]>,
    ) -> Vec<String> {
        let mut profiles = Vec::new();

        // 1. Explicit profiles take highest priority
        if let Some(explicit) = explicit_profiles {
            profiles.extend(explicit.iter().cloned());
            return profiles;
        }

        // 2. Try meta.profile
        if let Some(meta_profiles) = resource
            .get("meta")
            .and_then(|m| m.get("profile"))
            .and_then(|p| p.as_array())
        {
            for profile_value in meta_profiles {
                if let Some(profile_url) = profile_value.as_str() {
                    profiles.push(profile_url.to_string());
                }
            }
        }

        // 3. Always include base profile as fallback
        if let Some(resource_type) = resource.get("resourceType").and_then(|v| v.as_str()) {
            let base_url = format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type);
            // Only add base if not already in the list
            if !profiles.contains(&base_url) {
                profiles.push(base_url);
            }
        }

        profiles
    }

    /// Get element type information for a path segment
    ///
    /// Given a base type and a field name, returns the type information for that field.
    /// Handles choice types by returning all possible types.
    fn get_element_type(
        &self,
        base_type: &str,
        field_name: &str,
    ) -> Result<Option<ElementTypeInfo>> {
        let ensure_base_prefix = |name: &str| {
            let expected_prefix = format!("{}.", base_type);
            if name.starts_with(&expected_prefix) || name == base_type {
                name.to_string()
            } else {
                format!("{}.{}", base_type, name)
            }
        };

        let sd = self
            .get_core_structure_definition_by_type(base_type)?
            .ok_or_else(|| Error::StructureDefinitionNotFound(base_type.to_string()))?;

        // Get snapshot elements
        let snapshot = sd
            .snapshot
            .as_ref()
            .ok_or_else(|| Error::InvalidStructureDefinition("Missing snapshot".to_string()))?;

        // Build expected path
        let element_path = if field_name.contains("[x]") {
            ensure_base_prefix(field_name)
        } else {
            // Try exact match first
            let exact_path = ensure_base_prefix(field_name);
            if let Some(elem) = snapshot.get_element(&exact_path) {
                return Ok(elem.to_type_info());
            }

            // Try choice element
            let choice_path = ensure_base_prefix(&format!("{}[x]", field_name));
            if let Some(elem) = snapshot.get_element(&choice_path) {
                return Ok(elem.to_type_info());
            }

            // If still not found, return None
            return Ok(None);
        };

        // Find exact match for choice path
        if let Some(elem) = snapshot.get_element(&element_path) {
            return Ok(elem.to_type_info());
        }

        Ok(None)
    }

    /// Get choice type expansions for a choice element path
    ///
    /// Returns the list of possible types for a choice element (e.g., ["Quantity", "String", "CodeableConcept"])
    fn get_choice_expansions(
        &self,
        base_type: &str,
        field_name: &str,
    ) -> Result<Option<Vec<String>>> {
        let element_info = self.get_element_type(base_type, field_name)?;

        if let Some(info) = element_info {
            if info.is_choice {
                return Ok(Some(info.type_codes));
            }
        }

        Ok(None)
    }

    /// Resolve a navigation path (e.g., "name.given" starting from "Patient")
    ///
    /// Returns the type information for the final element in the path.
    fn resolve_path_type(&self, base_type: &str, path: &str) -> Result<Option<ElementTypeInfo>> {
        let segments: Vec<&str> = path.split('.').collect();
        let mut current_type = base_type.to_string();
        for (i, segment) in segments.iter().enumerate() {
            // Handle choice types
            let field_name = if segment.ends_with("[x]") {
                segment.to_string()
            } else {
                // Check if this is a choice variant (e.g., "valueQuantity")
                if let Some(base_name) = self.find_choice_base(&current_type, segment) {
                    // This is a choice variant, use the base choice path
                    format!("{}[x]", base_name)
                } else {
                    segment.to_string()
                }
            };

            let element_info = self.get_element_type(&current_type, &field_name)?;

            if let Some(info) = element_info {
                if i == segments.len() - 1 {
                    // Last segment - return its type info
                    return Ok(Some(info));
                } else {
                    // Continue navigation with the result type
                    if let Some(next_type) = info.type_codes.first() {
                        current_type = normalize_type_code(next_type);
                    } else {
                        return Ok(None);
                    }
                }
            } else {
                return Ok(None);
            }
        }

        Ok(None)
    }

    /// Find the base choice element name for a choice variant
    ///
    /// Example: "valueQuantity" -> "value"
    fn find_choice_base(&self, base_type: &str, field_name: &str) -> Option<String> {
        let sd = self
            .get_core_structure_definition_by_type(base_type)
            .ok()??;
        let snapshot = sd.snapshot.as_ref()?;

        for element in &snapshot.element {
            if element.is_choice_type() {
                // Extract the last part of the path (e.g., "value[x]" from "Observation.value[x]")
                let last_part = element.path.rsplit('.').next()?;
                if last_part.ends_with("[x]") {
                    let base_name = last_part.trim_end_matches("[x]");
                    // Check if field_name starts with this base name
                    if field_name.starts_with(base_name) && field_name.len() > base_name.len() {
                        // Return the full path prefix (e.g., "Observation.value")
                        let prefix = element.path.trim_end_matches("[x]");
                        return Some(prefix.to_string());
                    }
                }
            }
        }

        None
    }
}

/// Normalize type code (remove namespace prefixes)
fn normalize_type_code(code: &str) -> String {
    if code.starts_with("http://hl7.org/fhirpath/System.") {
        return code
            .replace("http://hl7.org/fhirpath/System.", "")
            .to_lowercase();
    }
    if code.starts_with("http://hl7.org/fhir/StructureDefinition/") {
        return code.replace("http://hl7.org/fhir/StructureDefinition/", "");
    }
    code.to_string()
}

/// Lightweight view of a loaded package for debugging/introspection
#[derive(Debug, Clone, Serialize)]
pub struct PackageIntrospection {
    pub name: String,
    pub version: String,
    pub canonical: Option<String>,
    pub dependencies: Option<serde_json::Map<String, Value>>,
    pub resource_ids: Vec<String>,
    pub canonical_urls: Vec<String>,
    pub resource_counts_by_type: HashMap<String, usize>,
}

/// Lock file structure for pinning exact package versions
///
/// This ensures reproducible builds by storing the exact versions of all packages
/// (including transitive dependencies) that were loaded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageLock {
    /// Version of the lock file format
    pub lock_version: String,
    /// Main package that was requested
    pub root_package: LockedPackage,
    /// All packages including transitive dependencies
    pub packages: Vec<LockedPackage>,
    /// Timestamp when lock file was created (ISO 8601)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// A single locked package entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedPackage {
    /// Package name (e.g., "hl7.fhir.r5.core")
    pub name: String,
    /// Exact version that was loaded (e.g., "5.0.0")
    pub version: String,
    /// Canonical URL if available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
}

impl PackageLock {
    /// Create a new lock file from loaded packages
    pub fn from_packages(root_name: &str, root_version: &str, packages: &[FhirPackage]) -> Self {
        let packages: Vec<LockedPackage> = packages
            .iter()
            .map(|pkg| LockedPackage {
                name: pkg.manifest.name.clone(),
                version: pkg.manifest.version.clone(),
                canonical: pkg.manifest.canonical.clone(),
            })
            .collect();

        let root_package = LockedPackage {
            name: root_name.to_string(),
            version: root_version.to_string(),
            canonical: None,
        };

        Self {
            lock_version: "1.0".to_string(),
            root_package,
            packages,
            created_at: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        }
    }

    /// Save lock file to disk
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| Error::LockFileError(format!("Failed to serialize lock file: {}", e)))?;
        fs::write(path.as_ref(), json)
            .map_err(|e| Error::LockFileError(format!("Failed to write lock file: {}", e)))?;
        Ok(())
    }

    /// Load lock file from disk
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| Error::LockFileError(format!("Failed to read lock file: {}", e)))?;
        serde_json::from_str(&content)
            .map_err(|e| Error::LockFileError(format!("Failed to parse lock file: {}", e)))
    }

    /// Get the exact version for a package name
    pub fn get_version(&self, package_name: &str) -> Option<&str> {
        self.packages
            .iter()
            .find(|p| p.name == package_name)
            .map(|p| p.version.as_str())
    }

    /// Validate that loaded packages match the lock file
    pub fn validate_packages(&self, packages: &[FhirPackage]) -> Result<()> {
        for pkg in packages {
            if let Some(expected_version) = self.get_version(&pkg.manifest.name) {
                if pkg.manifest.version != expected_version {
                    return Err(Error::PackageVersionMismatch {
                        name: pkg.manifest.name.clone(),
                        expected: expected_version.to_string(),
                        actual: pkg.manifest.version.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// Default implementation using pinned packages (exact versions)
pub struct DefaultFhirContext {
    _packages: Vec<Arc<FhirPackage>>,
    resources_by_canonical: HashMap<String, BTreeMap<VersionKey, Arc<Value>>>,
    structure_definition_cache: Mutex<LruCache<String, Arc<StructureDefinition>>>,
}

impl DefaultFhirContext {
    /// Create a new context with a loaded package
    pub fn new(package: FhirPackage) -> Self {
        Self::from_packages(vec![package])
    }

    /// Create a context from already loaded packages (no client required)
    pub fn from_packages(packages: Vec<FhirPackage>) -> Self {
        let packages: Vec<Arc<FhirPackage>> = packages.into_iter().map(Arc::new).collect();
        let mut resources_by_canonical: HashMap<String, BTreeMap<VersionKey, Arc<Value>>> =
            HashMap::new();

        for package in &packages {
            // Index all resources (conformance + examples)
            for resource in package.resources.iter().chain(package.examples.iter()) {
                if let Some(canonical_url) = resource.get("url").and_then(|v: &Value| v.as_str()) {
                    let version = resource
                        .get("version")
                        .and_then(|v: &Value| v.as_str())
                        .unwrap_or(&package.manifest.version);

                    let algorithm = extract_version_algorithm(resource);

                    resources_by_canonical
                        .entry(canonical_url.to_string())
                        .or_default()
                        .insert(
                            VersionKey::new(version, algorithm),
                            Arc::new(resource.clone()),
                        );
                }
            }
        }

        Self {
            _packages: packages,
            resources_by_canonical,
            structure_definition_cache: Mutex::new(LruCache::new(NonZeroUsize::new(4096).unwrap())),
        }
    }

    /// Create a context from already loaded, shared packages.
    ///
    /// This avoids cloning large `FhirPackage` instances when the caller needs
    /// to reuse the same package set elsewhere (e.g., DB installation, diagnostics).
    pub fn from_arc_packages(packages: Vec<Arc<FhirPackage>>) -> Self {
        let mut resources_by_canonical: HashMap<String, BTreeMap<VersionKey, Arc<Value>>> =
            HashMap::new();

        for package in &packages {
            // Index all resources (conformance + examples)
            for resource in package.resources.iter().chain(package.examples.iter()) {
                if let Some(canonical_url) = resource.get("url").and_then(|v: &Value| v.as_str()) {
                    let version = resource
                        .get("version")
                        .and_then(|v: &Value| v.as_str())
                        .unwrap_or(&package.manifest.version);

                    let algorithm = extract_version_algorithm(resource);

                    resources_by_canonical
                        .entry(canonical_url.to_string())
                        .or_default()
                        .insert(
                            VersionKey::new(version, algorithm),
                            Arc::new(resource.clone()),
                        );
                }
            }
        }

        Self {
            _packages: packages,
            resources_by_canonical,
            structure_definition_cache: Mutex::new(LruCache::new(NonZeroUsize::new(4096).unwrap())),
        }
    }

    /// Expose loaded packages and indexed resources for diagnostics
    pub fn package_introspection(&self) -> Vec<PackageIntrospection> {
        self._packages
            .iter()
            .map(|pkg| {
                // Collect resource IDs
                let mut resource_ids: Vec<String> = pkg
                    .resources
                    .iter()
                    .chain(pkg.examples.iter())
                    .filter_map(|r: &Value| {
                        r.get("id")
                            .and_then(|v: &Value| v.as_str())
                            .map(String::from)
                    })
                    .collect();
                resource_ids.sort();
                resource_ids.dedup();

                // Collect canonical URLs
                let mut canonical_urls: Vec<String> = pkg
                    .resources
                    .iter()
                    .chain(pkg.examples.iter())
                    .filter_map(|r: &Value| {
                        r.get("url")
                            .and_then(|v: &Value| v.as_str())
                            .map(String::from)
                    })
                    .collect();
                canonical_urls.sort();
                canonical_urls.dedup();

                // Count resources by type
                let mut resource_counts_by_type: HashMap<String, usize> = HashMap::new();
                for resource in pkg.resources.iter().chain(pkg.examples.iter()) {
                    if let Some(resource_type) = resource
                        .get("resourceType")
                        .and_then(|v: &Value| v.as_str())
                    {
                        *resource_counts_by_type
                            .entry(resource_type.to_string())
                            .or_insert(0) += 1;
                    }
                }

                // Convert dependencies HashMap to serde_json::Map
                let dependencies = if pkg.manifest.dependencies.is_empty() {
                    None
                } else {
                    let mut map = serde_json::Map::new();
                    for (k, v) in &pkg.manifest.dependencies {
                        map.insert(k.clone(), serde_json::Value::String(v.clone()));
                    }
                    Some(map)
                };

                PackageIntrospection {
                    name: pkg.manifest.name.clone(),
                    version: pkg.manifest.version.clone(),
                    canonical: pkg.manifest.canonical.clone(),
                    dependencies,
                    resource_ids,
                    canonical_urls,
                    resource_counts_by_type,
                }
            })
            .collect()
    }

    /// Return the latest version of all StructureDefinitions known to this context.
    pub fn all_structure_definitions(&self) -> Vec<Arc<Value>> {
        self.resources_by_canonical
            .keys()
            .filter_map(|canonical| self.get_from_index(canonical, None))
            .filter(|resource| {
                resource.get("resourceType").and_then(|v| v.as_str()) == Some("StructureDefinition")
            })
            .collect()
    }

    /// Create from async registry client and package name/version
    ///
    /// Loads the specified package with all transitive dependencies.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use zunder_context::DefaultFhirContext;
    /// use std::sync::Arc;
    /// use zunder_context::PackageLoader;
    /// use zunder_registry_client::RegistryClient;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let loader: Arc<dyn PackageLoader> = Arc::new(RegistryClient::new(None));
    /// let context = DefaultFhirContext::from_package_async(loader, "hl7.fhir.r5.core", "5.0.0").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_package_async(
        loader: Arc<dyn PackageLoader>,
        package_name: &str,
        package_version: &str,
    ) -> Result<Self> {
        let packages = loader
            .load_package_with_dependencies(package_name, Some(package_version))
            .await?;
        Ok(Self::from_packages(packages))
    }

    /// Create from async registry client and package name with optional version
    ///
    /// If version is None, resolves to the latest version.
    /// Loads the package with all transitive dependencies.
    pub async fn from_package_name_async(
        loader: Arc<dyn PackageLoader>,
        package_name: &str,
        version: Option<&str>,
    ) -> Result<Self> {
        let packages = loader
            .load_package_with_dependencies(package_name, version)
            .await?;
        Ok(Self::from_packages(packages))
    }

    /// Create from FHIR version (R4, R4B, or R5) using an async package loader
    ///
    /// Maps the FHIR version to the appropriate core package and loads it with all dependencies.
    /// If `loader` is `None`, the default loader will be used (requires `registry-loader` feature).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use zunder_context::DefaultFhirContext;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Default loader
    /// let context = DefaultFhirContext::from_fhir_version_async(None, "R5").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_fhir_version_async(
        loader: Option<Arc<dyn PackageLoader>>,
        fhir_version: &str,
    ) -> Result<Self> {
        let loader = match loader {
            Some(loader) => loader,
            None => crate::loader::default_package_loader()?,
        };

        let (package_name, package_version) = match fhir_version {
            "R4" => ("hl7.fhir.r4.core", "4.0.1"),
            "R4B" => ("hl7.fhir.r4b.core", "4.3.0"),
            "R5" => ("hl7.fhir.r5.core", "5.0.0"),
            _ => {
                return Err(Error::InvalidFhirVersion(format!(
                    "Unsupported FHIR version: {}. Supported versions: R4, R4B, R5",
                    fhir_version
                )));
            }
        };

        Self::from_package_async(loader, package_name, package_version).await
    }

    /// Create from lock file using an async package loader
    ///
    /// Loads packages using exact versions specified in the lock file.
    /// This ensures reproducible builds by pinning all package versions.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use zunder_context::{DefaultFhirContext, PackageLock};
    /// use std::sync::Arc;
    /// use zunder_context::PackageLoader;
    /// use zunder_registry_client::RegistryClient;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let loader: Arc<dyn PackageLoader> = Arc::new(RegistryClient::new(None));
    /// let lock = PackageLock::load("fhir.lock")?;
    /// let context = DefaultFhirContext::from_lock_file_async(loader, &lock).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_lock_file_async(
        loader: Arc<dyn PackageLoader>,
        lock: &PackageLock,
    ) -> Result<Self> {
        let mut packages = Vec::new();

        // Load all packages from lock file using exact versions
        for locked_pkg in &lock.packages {
            let pkg = loader
                .load_or_download_package(&locked_pkg.name, &locked_pkg.version)
                .await?;
            packages.push(pkg);
        }

        // Validate that all loaded packages match the lock file
        lock.validate_packages(&packages)?;

        Ok(Self::from_packages(packages))
    }

    /// Create from lock file path using async registry client
    ///
    /// Convenience method that loads the lock file and creates a context.
    pub async fn from_lock_file_path_async<P: AsRef<Path>>(
        loader: Arc<dyn PackageLoader>,
        lock_path: P,
    ) -> Result<Self> {
        let lock = PackageLock::load(lock_path)?;
        Self::from_lock_file_async(loader, &lock).await
    }

    /// Generate a lock file from the currently loaded packages
    ///
    /// This creates a lock file that pins the exact versions of all packages
    /// currently loaded in this context.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use zunder_context::DefaultFhirContext;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let context = DefaultFhirContext::from_fhir_version_async(None, "R5").await?;
    /// let lock = context.generate_lock_file("hl7.fhir.r5.core", "5.0.0");
    /// lock.save("fhir.lock")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate_lock_file(&self, root_name: &str, root_version: &str) -> PackageLock {
        let packages: Vec<FhirPackage> = self
            ._packages
            .iter()
            .map(|arc_pkg| {
                // Clone the package data
                FhirPackage::new(
                    arc_pkg.manifest.clone(),
                    arc_pkg.resources.clone(),
                    arc_pkg.examples.clone(),
                )
            })
            .collect();
        PackageLock::from_packages(root_name, root_version, &packages)
    }

    /// Insert an additional resource into this context's canonical index.
    ///
    /// The resource must have a `url` field. An optional `version` field is used
    /// for version-specific lookups; when absent the resource is indexed as "0".
    pub fn add_resource(&mut self, resource: Value) {
        let Some(canonical_url) = resource.get("url").and_then(|v| v.as_str()).map(String::from)
        else {
            return;
        };
        let version_str = resource
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();
        let algorithm = extract_version_algorithm(&resource);
        self.resources_by_canonical
            .entry(canonical_url.clone())
            .or_default()
            .insert(VersionKey::new(&version_str, algorithm), Arc::new(resource));
        // Invalidate the SD cache for this canonical URL so the new resource is picked up.
        let mut cache = self
            .structure_definition_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cache.pop(&canonical_url);
    }

    fn get_from_index(&self, canonical_url: &str, version: Option<&str>) -> Option<Arc<Value>> {
        self.get_from_index_ref(canonical_url, version)
            .map(Arc::clone)
    }

    fn get_from_index_ref(
        &self,
        canonical_url: &str,
        version: Option<&str>,
    ) -> Option<&Arc<Value>> {
        let versions = self.resources_by_canonical.get(canonical_url)?;
        select_from_version_index(versions, version)
    }
}

impl FhirContext for DefaultFhirContext {
    fn get_resource_by_url(
        &self,
        canonical_url: &str,
        version: Option<&str>,
    ) -> Result<Option<Arc<Value>>> {
        Ok(self.get_from_index(canonical_url, version))
    }

    fn get_structure_definition(
        &self,
        canonical_url: &str,
    ) -> Result<Option<Arc<StructureDefinition>>> {
        {
            let mut cache = self
                .structure_definition_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(hit) = cache.get(canonical_url) {
                return Ok(Some(hit.clone()));
            }
        }

        let Some(resource) = self.get_from_index(canonical_url, None) else {
            return Ok(None);
        };
        let sd: StructureDefinition = serde_json::from_value(Arc::unwrap_or_clone(resource))?;
        let sd = Arc::new(sd);

        {
            let mut cache = self
                .structure_definition_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            cache.put(canonical_url.to_string(), sd.clone());
        }

        Ok(Some(sd))
    }

    fn get_core_structure_definition_by_type(
        &self,
        type_name: &str,
    ) -> Result<Option<Arc<StructureDefinition>>> {
        let canonical_url = format!("http://hl7.org/fhir/StructureDefinition/{}", type_name);
        self.get_structure_definition(&canonical_url)
    }
}

#[async_trait]
impl ConformanceResourceProvider for DefaultFhirContext {
    async fn list_by_canonical(&self, canonical_url: &str) -> Result<Vec<Arc<Value>>> {
        let Some(versions) = self.resources_by_canonical.get(canonical_url) else {
            return Ok(vec![]);
        };

        Ok(versions.values().cloned().collect())
    }

    async fn get_by_canonical_and_version(
        &self,
        canonical_url: &str,
        version: &str,
    ) -> Result<Option<Arc<Value>>> {
        Ok(self.get_from_index(canonical_url, Some(version)))
    }
}

impl FhirContext for FlexibleFhirContext {
    fn get_resource_by_url(
        &self,
        canonical_url: &str,
        version: Option<&str>,
    ) -> Result<Option<Arc<Value>>> {
        let inner = self.0.clone();
        let canonical_url = canonical_url.to_string();
        let version = version.map(|v| v.to_string());

        self.block_on(Self::get_resource_by_url_async(
            inner,
            canonical_url,
            version,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zunder_package::PackageManifest;

    /// Create a mock StructureDefinition for testing
    fn create_mock_patient_sd() -> Value {
        json!({
            "resourceType": "StructureDefinition",
            "id": "Patient",
            "url": "http://hl7.org/fhir/StructureDefinition/Patient",
            "name": "Patient",
            "status": "active",
            "kind": "resource",
            "abstract": false,
            "type": "Patient",
            "snapshot": {
                "element": [
                    {
                        "id": "Patient",
                        "path": "Patient",
                        "min": 0,
                        "max": "*"
                    },
                    {
                        "id": "Patient.id",
                        "path": "Patient.id",
                        "type": [{"code": "id"}],
                        "min": 0,
                        "max": "1"
                    },
                    {
                        "id": "Patient.name",
                        "path": "Patient.name",
                        "type": [{"code": "HumanName"}],
                        "min": 0,
                        "max": "*"
                    },
                    {
                        "id": "Patient.name.given",
                        "path": "Patient.name.given",
                        "type": [{"code": "string"}],
                        "min": 0,
                        "max": "*"
                    },
                    {
                        "id": "Patient.name.family",
                        "path": "Patient.name.family",
                        "type": [{"code": "string"}],
                        "min": 0,
                        "max": "1"
                    },
                    {
                        "id": "Patient.birthDate",
                        "path": "Patient.birthDate",
                        "type": [{"code": "date"}],
                        "min": 0,
                        "max": "1"
                    }
                ]
            }
        })
    }

    /// Create a mock StructureDefinition with choice type
    fn create_mock_observation_sd() -> Value {
        json!({
            "resourceType": "StructureDefinition",
            "id": "Observation",
            "url": "http://hl7.org/fhir/StructureDefinition/Observation",
            "name": "Observation",
            "status": "active",
            "kind": "resource",
            "abstract": false,
            "type": "Observation",
            "snapshot": {
                "element": [
                    {
                        "id": "Observation",
                        "path": "Observation",
                        "min": 0,
                        "max": "*"
                    },
                    {
                        "id": "Observation.id",
                        "path": "Observation.id",
                        "type": [{"code": "id"}],
                        "min": 0,
                        "max": "1"
                    },
                    {
                        "id": "Observation.value[x]",
                        "path": "Observation.value[x]",
                        "type": [
                            {"code": "Quantity"},
                            {"code": "string"},
                            {"code": "CodeableConcept"}
                        ],
                        "min": 0,
                        "max": "1"
                    },
                    {
                        "id": "Observation.valueQuantity",
                        "path": "Observation.valueQuantity",
                        "type": [{"code": "Quantity"}],
                        "min": 0,
                        "max": "1"
                    },
                    {
                        "id": "Observation.status",
                        "path": "Observation.status",
                        "type": [{"code": "code"}],
                        "min": 1,
                        "max": "1"
                    }
                ]
            }
        })
    }

    #[test]
    fn flexible_context_caches_by_canonical() {
        struct MockProvider {
            calls: AtomicUsize,
            data: HashMap<String, Vec<Arc<Value>>>,
        }

        #[async_trait]
        impl ConformanceResourceProvider for MockProvider {
            async fn list_by_canonical(&self, canonical_url: &str) -> Result<Vec<Arc<Value>>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(self
                    .data
                    .get(canonical_url)
                    .cloned()
                    .unwrap_or_else(Vec::new))
            }
        }

        let canonical = "http://example.org/fhir/StructureDefinition/Foo";
        let resources = vec![
            Arc::new(json!({
                "resourceType": "StructureDefinition",
                "url": canonical,
                "version": "1.0.0-alpha"
            })),
            Arc::new(json!({
                "resourceType": "StructureDefinition",
                "url": canonical,
                "version": "1.0.0"
            })),
        ];

        let provider = Arc::new(MockProvider {
            calls: AtomicUsize::new(0),
            data: HashMap::from([(canonical.to_string(), resources)]),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx =
            FlexibleFhirContext::with_handle(rt.handle().clone(), provider.clone()).with_ttl(None);

        let first = ctx.get_latest_resource_by_url(canonical).unwrap().unwrap();
        assert_eq!(first.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
        let second = ctx.get_latest_resource_by_url(canonical).unwrap().unwrap();
        assert_eq!(
            second.get("version").and_then(|v| v.as_str()),
            Some("1.0.0")
        );

        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn flexible_context_caches_by_canonical_and_version() {
        struct MockProvider {
            list_calls: AtomicUsize,
            get_calls: AtomicUsize,
            value: Arc<Value>,
        }

        #[async_trait]
        impl ConformanceResourceProvider for MockProvider {
            async fn list_by_canonical(&self, _canonical_url: &str) -> Result<Vec<Arc<Value>>> {
                self.list_calls.fetch_add(1, Ordering::SeqCst);
                Ok(vec![])
            }

            async fn get_by_canonical_and_version(
                &self,
                _canonical_url: &str,
                _version: &str,
            ) -> Result<Option<Arc<Value>>> {
                self.get_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(self.value.clone()))
            }
        }

        let canonical = "http://example.org/fhir/StructureDefinition/Bar";
        let v = "9.9.9";

        let provider = Arc::new(MockProvider {
            list_calls: AtomicUsize::new(0),
            get_calls: AtomicUsize::new(0),
            value: Arc::new(json!({
                "resourceType": "StructureDefinition",
                "url": canonical,
                "version": v
            })),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx =
            FlexibleFhirContext::with_handle(rt.handle().clone(), provider.clone()).with_ttl(None);

        let first = ctx
            .get_resource_by_url(canonical, Some(v))
            .unwrap()
            .unwrap();
        assert_eq!(first.get("version").and_then(|x| x.as_str()), Some(v));
        let second = ctx
            .get_resource_by_url(canonical, Some(v))
            .unwrap()
            .unwrap();
        assert_eq!(second.get("version").and_then(|x| x.as_str()), Some(v));

        assert_eq!(provider.list_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.get_calls.load(Ordering::SeqCst), 1);
    }

    /// Create a mock HumanName StructureDefinition
    fn create_mock_humanname_sd() -> Value {
        json!({
            "resourceType": "StructureDefinition",
            "id": "HumanName",
            "url": "http://hl7.org/fhir/StructureDefinition/HumanName",
            "name": "HumanName",
            "status": "active",
            "kind": "complex-type",
            "abstract": false,
            "type": "HumanName",
            "snapshot": {
                "element": [
                    {
                        "id": "HumanName",
                        "path": "HumanName",
                        "min": 0,
                        "max": "*"
                    },
                    {
                        "id": "HumanName.given",
                        "path": "HumanName.given",
                        "type": [{"code": "string"}],
                        "min": 0,
                        "max": "*"
                    },
                    {
                        "id": "HumanName.family",
                        "path": "HumanName.family",
                        "type": [{"code": "string"}],
                        "min": 0,
                        "max": "1"
                    }
                ]
            }
        })
    }

    /// Create a mock package with test StructureDefinitions
    fn create_mock_package() -> FhirPackage {
        let resources = vec![
            create_mock_patient_sd(),
            create_mock_observation_sd(),
            create_mock_humanname_sd(),
        ];

        let manifest = PackageManifest {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            canonical: None,
            url: None,
            homepage: None,
            title: None,
            description: String::new(),
            fhir_versions: vec![],
            dependencies: HashMap::new(),
            keywords: vec![],
            author: "test".to_string(),
            maintainers: vec![],
            package_type: None,
            jurisdiction: None,
            license: None,
            extra: serde_json::Map::new(),
        };

        FhirPackage::new(manifest, resources, vec![])
    }

    #[test]
    fn test_get_structure_definition_by_url() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test getting Patient SD by URL
        let sd = FhirContext::get_structure_definition(
            &context,
            "http://hl7.org/fhir/StructureDefinition/Patient",
        )
        .unwrap();
        assert!(sd.is_some());
        assert_eq!(sd.unwrap().id.as_deref(), Some("Patient"));

        // Test getting non-existent SD
        let sd = FhirContext::get_structure_definition(
            &context,
            "http://hl7.org/fhir/StructureDefinition/NonExistent",
        )
        .unwrap();
        assert!(sd.is_none());
    }

    fn make_sd(version: &str) -> Value {
        json!({
            "resourceType": "StructureDefinition",
            "id": "Patient",
            "url": "http://hl7.org/fhir/StructureDefinition/Patient",
            "name": "Patient",
            "status": "active",
            "kind": "resource",
            "abstract": false,
            "version": version,
            "type": "Patient",
            "snapshot": { "element": [] }
        })
    }

    fn make_pkg(name: &str, version: &str, sd_version: &str) -> FhirPackage {
        let sd = make_sd(sd_version);
        let resources = vec![sd];

        let manifest = PackageManifest {
            name: name.to_string(),
            version: version.to_string(),
            canonical: None,
            url: None,
            homepage: None,
            title: None,
            description: String::new(),
            fhir_versions: vec![],
            dependencies: HashMap::new(),
            keywords: vec![],
            author: "test".to_string(),
            maintainers: vec![],
            package_type: None,
            jurisdiction: None,
            license: None,
            extra: serde_json::Map::new(),
        };

        FhirPackage::new(manifest, resources, vec![])
    }

    #[test]
    fn prefers_release_over_prerelease_when_latest() {
        let release_pkg = make_pkg("test-release", "1.0.0", "1.0.0");
        let ballot_pkg = make_pkg("test-ballot", "1.0.0-ballot", "1.0.0-ballot");

        let context = DefaultFhirContext::from_packages(vec![ballot_pkg, release_pkg]);
        let sd = FhirContext::get_structure_definition(
            &context,
            "http://hl7.org/fhir/StructureDefinition/Patient",
        )
        .unwrap()
        .unwrap();

        assert_eq!(sd.version.as_deref(), Some("1.0.0"));

        let ballot = context
            .get_resource_by_url(
                "http://hl7.org/fhir/StructureDefinition/Patient",
                Some("1.0.0-ballot"),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            ballot.get("version").and_then(|v| v.as_str()),
            Some("1.0.0-ballot")
        );
    }

    #[test]
    fn test_get_structure_definition_by_type() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test getting Patient SD by type name
        let sd = context
            .get_core_structure_definition_by_type("Patient")
            .unwrap();
        assert!(sd.is_some());
        assert_eq!(sd.unwrap().id.as_deref(), Some("Patient"));

        // Test getting Observation SD
        let sd = context
            .get_core_structure_definition_by_type("Observation")
            .unwrap();
        assert!(sd.is_some());
        assert_eq!(sd.unwrap().id.as_deref(), Some("Observation"));
    }

    #[test]
    fn test_get_structure_definition_from_resource() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test with resourceType
        let resource = json!({
            "resourceType": "Patient",
            "id": "123"
        });
        let sd = context
            .get_structure_definition_from_resource(&resource)
            .unwrap();
        assert!(sd.is_some());
        assert_eq!(sd.unwrap().id.as_deref(), Some("Patient"));

        // Test with meta.profile
        let resource = json!({
            "resourceType": "Patient",
            "meta": {
                "profile": ["http://hl7.org/fhir/StructureDefinition/Observation"]
            }
        });
        let sd = context
            .get_structure_definition_from_resource(&resource)
            .unwrap();
        assert!(sd.is_some());
        assert_eq!(sd.unwrap().id.as_deref(), Some("Observation"));
    }

    #[test]
    fn test_get_element_type() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test getting simple element type
        let elem_info = context.get_element_type("Patient", "id").unwrap();
        assert!(elem_info.is_some());
        let info = elem_info.unwrap();
        assert_eq!(info.path, "Patient.id");
        assert_eq!(info.type_codes, vec!["id"]);
        assert!(!info.is_choice);
        assert_eq!(info.min, 0);
        assert_eq!(info.max, Some(1));

        // Test getting array element type
        let elem_info = context.get_element_type("Patient", "name").unwrap();
        assert!(elem_info.is_some());
        let info = elem_info.unwrap();
        assert_eq!(info.path, "Patient.name");
        assert_eq!(info.type_codes, vec!["HumanName"]);
        assert!(info.is_array);
        assert_eq!(info.min, 0);
        assert_eq!(info.max, None); // "*" becomes None

        // Test getting non-existent element
        let elem_info = context.get_element_type("Patient", "nonExistent").unwrap();
        assert!(elem_info.is_none());
    }

    #[test]
    fn test_get_choice_expansions() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test getting choice expansions
        let expansions = context
            .get_choice_expansions("Observation", "value")
            .unwrap();
        assert!(expansions.is_some());
        let types = expansions.unwrap();
        assert!(types.contains(&"Quantity".to_string()));
        assert!(types.contains(&"string".to_string()));
        assert!(types.contains(&"CodeableConcept".to_string()));

        // Test getting non-choice element (should return None)
        let expansions = context.get_choice_expansions("Patient", "id").unwrap();
        assert!(expansions.is_none());
    }

    #[test]
    fn test_resolve_path_type() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test resolving simple path
        let elem_info = context.resolve_path_type("Patient", "id").unwrap();
        assert!(elem_info.is_some());
        let info = elem_info.unwrap();
        assert_eq!(info.path, "Patient.id");
        assert_eq!(info.type_codes, vec!["id"]);

        // Test resolving nested path
        let elem_info = context.resolve_path_type("Patient", "name.given").unwrap();
        assert!(elem_info.is_some());
        let info = elem_info.unwrap();
        // When navigating through "name" (type HumanName), we get the element from HumanName SD
        // So the path is "HumanName.given", not "Patient.name.given"
        assert_eq!(info.path, "HumanName.given");
        assert_eq!(info.type_codes, vec!["string"]);

        // Test resolving deeper nested path
        let elem_info = context.resolve_path_type("Patient", "name.family").unwrap();
        assert!(elem_info.is_some());
        let info = elem_info.unwrap();
        // Same as above - path is from HumanName SD
        assert_eq!(info.path, "HumanName.family");
        assert_eq!(info.type_codes, vec!["string"]);

        // Test resolving non-existent path
        let elem_info = context
            .resolve_path_type("Patient", "nonExistent.field")
            .unwrap();
        assert!(elem_info.is_none());
    }

    #[test]
    fn test_find_choice_base() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test finding choice base for choice variant
        let base = context.find_choice_base("Observation", "valueQuantity");
        assert_eq!(base, Some("Observation.value".to_string()));

        // Test finding choice base for non-choice field
        let base = context.find_choice_base("Patient", "id");
        assert!(base.is_none());
    }

    #[test]
    fn test_get_element_type_with_choice() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test getting choice element directly
        let elem_info = context.get_element_type("Observation", "value[x]").unwrap();
        assert!(elem_info.is_some());
        let info = elem_info.unwrap();
        assert!(info.is_choice);
        assert_eq!(info.path, "Observation.value[x]");
        assert!(info.type_codes.contains(&"Quantity".to_string()));
        assert!(info.type_codes.contains(&"string".to_string()));
        assert!(info.type_codes.contains(&"CodeableConcept".to_string()));
    }

    #[test]
    fn test_get_element_type_with_prefixed_field_name() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Ensure we do not double-append the base path when it's already present
        let elem_info = context
            .get_element_type("Patient", "Patient.id")
            .unwrap()
            .unwrap();
        assert_eq!(elem_info.path, "Patient.id");
        assert_eq!(elem_info.type_codes, vec!["id"]);
    }

    #[test]
    fn test_get_structure_definition_not_found() {
        let package = create_mock_package();
        let context = DefaultFhirContext::new(package);

        // Test getting non-existent type
        let result = context.get_element_type("NonExistent", "field");
        assert!(result.is_err());
        match result {
            Err(Error::StructureDefinitionNotFound(_)) => {}
            _ => panic!("Expected StructureDefinitionNotFound error"),
        }
    }
}
