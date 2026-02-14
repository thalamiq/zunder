//! Conformance resource access for `fhir-context`.

use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use std::{collections::HashMap, sync::OnceLock};
use ferrum_context::{
    ConformanceResourceProvider, DefaultFhirContext, Error as ContextError,
    FallbackConformanceProvider, FhirContext, FlexibleFhirContext, Result as ContextResult,
};
use ferrum_registry_client::RegistryClient;

use crate::db::PostgresResourceStore;
use crate::Result;

/// Global cache for core FHIR contexts, keyed by FHIR version (e.g. "R4").
/// Populated by `load_core_fhir_context`, read by `core_fhir_context`.
static CORE_CTX: OnceLock<std::sync::Mutex<HashMap<String, Arc<dyn FhirContext>>>> = OnceLock::new();

pub struct DbConformanceProvider {
    store: PostgresResourceStore,
}

impl DbConformanceProvider {
    pub fn new(pool: PgPool) -> Self {
        Self {
            store: PostgresResourceStore::new(pool),
        }
    }
}

pub fn db_backed_fhir_context(pool: PgPool) -> Result<Arc<dyn FhirContext>> {
    let provider: Arc<dyn ConformanceResourceProvider> = Arc::new(DbConformanceProvider::new(pool));
    let ctx =
        FlexibleFhirContext::new(provider).map_err(|e| crate::Error::FhirContext(e.to_string()))?;
    Ok(Arc::new(ctx))
}

pub fn db_backed_fhir_context_with_fallback(
    pool: PgPool,
    fallback: Arc<dyn ConformanceResourceProvider>,
) -> Result<Arc<dyn FhirContext>> {
    let db: Arc<dyn ConformanceResourceProvider> = Arc::new(DbConformanceProvider::new(pool));
    let provider: Arc<dyn ConformanceResourceProvider> =
        Arc::new(FallbackConformanceProvider::new(db, fallback));
    let ctx =
        FlexibleFhirContext::new(provider).map_err(|e| crate::Error::FhirContext(e.to_string()))?;
    Ok(Arc::new(ctx))
}

/// Create an empty FhirContext that doesn't query the database.
///
/// Used for FHIRPath evaluation during indexing to avoid sync/async deadlocks.
/// The TypePass will not be able to resolve types, but FHIRPath evaluation will
/// still work correctly (it will just use dynamic typing at runtime).
pub fn empty_fhir_context() -> Result<Arc<dyn FhirContext>> {
    let provider: Arc<dyn ConformanceResourceProvider> = Arc::new(EmptyConformanceProvider);
    let ctx =
        FlexibleFhirContext::new(provider).map_err(|e| crate::Error::FhirContext(e.to_string()))?;
    Ok(Arc::new(ctx))
}

/// Load the core FHIR package into memory via the registry client and cache it globally.
///
/// Downloads the package from the Simplifier registry if not already cached by the
/// `RegistryClient`. Builds a `DefaultFhirContext` in memory and stores it in a global
/// so subsequent calls for the same version return immediately.
///
/// This should be called once during application startup.
pub async fn load_core_fhir_context(fhir_version: &str) -> Result<Arc<dyn FhirContext>> {
    let cache_map = CORE_CTX.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Some(ctx) = cache_map.lock().unwrap().get(fhir_version).cloned() {
        return Ok(ctx);
    }

    let (core_name, core_version) = match fhir_version {
        "R4" => ("hl7.fhir.r4.core", "4.0.1"),
        "R4B" => ("hl7.fhir.r4b.core", "4.3.0"),
        "R5" => ("hl7.fhir.r5.core", "5.0.0"),
        other => {
            return Err(crate::Error::Internal(format!(
                "Unsupported FHIR version: {}",
                other
            )));
        }
    };

    tracing::info!(
        package = core_name,
        version = core_version,
        "Loading core FHIR package via registry client..."
    );

    let client = RegistryClient::new(None);
    let package = client
        .load_or_download_package(core_name, core_version)
        .await
        .map_err(|e| {
            crate::Error::FhirContext(format!(
                "Failed to load core package {}#{}: {}",
                core_name, core_version, e
            ))
        })?;

    let ctx: Arc<dyn FhirContext> = Arc::new(DefaultFhirContext::new(package));
    cache_map
        .lock()
        .unwrap()
        .insert(fhir_version.to_string(), ctx.clone());

    tracing::info!(
        package = core_name,
        version = core_version,
        "Core FHIR context loaded successfully"
    );

    Ok(ctx)
}

/// Get the previously-loaded core FHIR context for a given version.
///
/// This is a synchronous getter intended for use in `spawn_blocking` contexts
/// (e.g. indexing). Panics if `load_core_fhir_context` was not called first.
pub fn core_fhir_context(fhir_version: &str) -> Result<Arc<dyn FhirContext>> {
    let cache_map = CORE_CTX.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    cache_map
        .lock()
        .unwrap()
        .get(fhir_version)
        .cloned()
        .ok_or_else(|| {
            crate::Error::Internal(format!(
                "Core FHIR context for version '{}' not loaded. Call load_core_fhir_context() at startup first.",
                fhir_version
            ))
        })
}

/// Empty conformance provider that returns no resources.
/// Used to create a FhirContext that doesn't make database calls.
struct EmptyConformanceProvider;

#[async_trait]
impl ConformanceResourceProvider for EmptyConformanceProvider {
    async fn list_by_canonical(&self, _canonical_url: &str) -> ContextResult<Vec<Arc<Value>>> {
        Ok(Vec::new())
    }

    async fn get_by_canonical_and_version(
        &self,
        _canonical_url: &str,
        _version: &str,
    ) -> ContextResult<Option<Arc<Value>>> {
        Ok(None)
    }
}

#[async_trait]
impl ConformanceResourceProvider for DbConformanceProvider {
    async fn list_by_canonical(&self, canonical_url: &str) -> ContextResult<Vec<Arc<Value>>> {
        let resources = self
            .store
            .list_current_by_canonical_url(canonical_url)
            .await
            .map_err(|e| ContextError::ConformanceStore(e.to_string()))?;

        Ok(resources.into_iter().map(Arc::new).collect())
    }

    async fn get_by_canonical_and_version(
        &self,
        canonical_url: &str,
        version: &str,
    ) -> ContextResult<Option<Arc<Value>>> {
        let resource = self
            .store
            .get_by_canonical_url_and_version(canonical_url, version)
            .await
            .map_err(|e| ContextError::ConformanceStore(e.to_string()))?;

        Ok(resource.map(Arc::new))
    }
}

/// Check if a resource type is a conformance resource that should trigger hooks
/// when created or updated in batch/transaction operations.
///
/// These resource types require special processing (e.g., updating search indexes,
/// rebuilding compartment memberships, etc.) when installed from packages.
///
/// # Examples
///
/// ```
/// use ferrum::conformance::is_conformance_resource_type;
///
/// assert!(is_conformance_resource_type("SearchParameter"));
/// assert!(is_conformance_resource_type("CompartmentDefinition"));
/// assert!(!is_conformance_resource_type("Patient"));
/// ```
pub fn is_conformance_resource_type(resource_type: &str) -> bool {
    matches!(
        resource_type,
        "SearchParameter"
            | "StructureDefinition"
            | "CodeSystem"
            | "ValueSet"
            | "CompartmentDefinition"
    )
}
