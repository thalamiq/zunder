//! Configuration management for the FHIR server

use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub fhir: FhirConfig,
    pub workers: WorkerConfig,
    pub logging: LoggingConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_cors_origins")]
    pub cors_origins: Vec<String>,
    /// Maximum request body size in bytes. Prevents DoS via large payloads.
    /// Default: 10 MB
    #[serde(default = "default_max_request_body_size")]
    pub max_request_body_size: usize,
    /// Maximum response body size in bytes. Prevents huge bundle responses.
    /// Default: 50 MB
    #[serde(default = "default_max_response_body_size")]
    pub max_response_body_size: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_url")]
    pub url: String,
    /// Test database URL. If set, overrides `url` in test environments.
    /// Environment variable: `FHIR__DATABASE__TEST_DATABASE_URL`
    pub test_database_url: Option<String>,

    // API Server Pool Configuration
    #[serde(default = "default_pool_min_size")]
    pub pool_min_size: u32,
    #[serde(default = "default_pool_max_size")]
    pub pool_max_size: u32,
    #[serde(default = "default_pool_timeout")]
    pub pool_timeout_seconds: u64,

    // Worker Pool Configuration
    /// Worker pool min connections. Workers need fewer connections (LISTEN/NOTIFY + indexing).
    /// Default: 1
    #[serde(default = "default_worker_pool_min_size")]
    pub worker_pool_min_size: u32,
    /// Worker pool max connections. Workers need fewer connections than the API server.
    /// Default: 5
    #[serde(default = "default_worker_pool_max_size")]
    pub worker_pool_max_size: u32,
    /// Worker pool timeout in seconds. Default: 60
    #[serde(default = "default_worker_pool_timeout")]
    pub worker_pool_timeout_seconds: u64,

    /// Batch size for regular indexing (number of resources per transaction).
    /// Smaller batches reduce lock duration. Default: 50
    #[serde(default = "default_indexing_batch_size")]
    pub indexing_batch_size: usize,
    /// Threshold to use COPY-based bulk indexing (100-500x faster).
    /// Default: 200 resources
    #[serde(default = "default_indexing_bulk_threshold")]
    pub indexing_bulk_threshold: usize,
    /// Maximum query execution time in seconds. Queries exceeding this will be terminated.
    /// Prevents runaway queries from consuming resources. Default: 300 (5 minutes)
    #[serde(default = "default_statement_timeout")]
    pub statement_timeout_seconds: u64,
    /// Maximum time to wait for a lock in seconds. If exceeded, query fails fast.
    /// Prevents long waits on contended resources. Default: 30 seconds
    #[serde(default = "default_lock_timeout")]
    pub lock_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirConfig {
    #[serde(default = "default_fhir_version")]
    pub version: String,
    #[serde(default)]
    pub search: FhirSearchConfig,
    /// Feature flags for enabling/disabling FHIR interactions (read-only, no-delete, etc.).
    #[serde(default)]
    pub interactions: FhirInteractionsConfig,
    #[serde(default)]
    pub fhirpath: FhirPathConfig,
    #[serde(default)]
    pub default_packages: DefaultPackagesConfig,
    #[serde(default)]
    pub packages: Vec<FhirPackageConfig>,
    pub registry_url: Option<String>,
    /// Install internal packages from fhir_packages/ directory
    #[serde(default = "default_true")]
    pub install_internal_packages: bool,
    /// Optional directory containing internal FHIR packages (e.g. `zunder.fhir.server#1.0.0/package`).
    ///
    /// If not set, the server looks for `./fhir_packages` relative to the current working
    /// directory, then falls back to the compile-time crate directory.
    #[serde(default)]
    pub internal_packages_dir: Option<String>,
    #[serde(default = "default_format")]
    pub default_format: String,
    /// Default Prefer header return behavior when client doesn't specify one.
    /// Valid values: "minimal", "representation", "operationoutcome"
    /// Default: "representation" (return full resource)
    #[serde(default = "default_prefer_return")]
    pub default_prefer_return: String,
    /// Allow clients to create resources via PUT with client-defined IDs.
    /// Per FHIR spec: "Servers can choose whether or not to support client defined ids"
    /// When false, returns 405 Method Not Allowed if resource doesn't exist.
    /// Default: true (allow update-as-create)
    #[serde(default = "default_true")]
    pub allow_update_create: bool,
    /// When true, DELETE physically removes the resource and its history from storage.
    /// When false (default), DELETE is a soft delete that creates a deleted history entry.
    #[serde(default)]
    pub hard_delete: bool,
    #[serde(default)]
    pub capability_statement: CapabilityStatementConfig,
    #[serde(default)]
    pub referential_integrity: ReferentialIntegrityConfig,
}

/// Configuration for enabling/disabling specific FHIR interactions.
///
/// Defaults are permissive (all interactions enabled). Use this to make a deployment read-only,
/// disable batch/transaction, or disable compartment search, etc.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FhirInteractionsConfig {
    #[serde(default)]
    pub instance: FhirInstanceInteractionsConfig,
    #[serde(default)]
    pub type_level: FhirTypeInteractionsConfig,
    #[serde(default)]
    pub system: FhirSystemInteractionsConfig,
    #[serde(default)]
    pub compartment: FhirCompartmentInteractionsConfig,
    /// `$operation` endpoints (system/type/instance). Not part of the core interaction list,
    /// but useful to disable in public deployments.
    #[serde(default)]
    pub operations: FhirOperationsInteractionsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirInstanceInteractionsConfig {
    /// `GET /{type}/{id}` and `HEAD /{type}/{id}`
    #[serde(default = "default_true")]
    pub read: bool,
    /// `GET /{type}/{id}/_history/{vid}` and `HEAD /{type}/{id}/_history/{vid}`
    #[serde(default = "default_true")]
    pub vread: bool,
    /// `PUT /{type}/{id}`
    #[serde(default = "default_true")]
    pub update: bool,
    /// `PATCH /{type}/{id}`
    #[serde(default = "default_true")]
    pub patch: bool,
    /// `DELETE /{type}/{id}`
    #[serde(default = "default_true")]
    pub delete: bool,
    /// `GET /{type}/{id}/_history`
    #[serde(default = "default_true")]
    pub history: bool,
    /// `DELETE /{type}/{id}/_history`
    #[serde(default = "default_true")]
    pub delete_history: bool,
    /// `DELETE /{type}/{id}/_history/{vid}`
    #[serde(default = "default_true")]
    pub delete_history_version: bool,
}

impl Default for FhirInstanceInteractionsConfig {
    fn default() -> Self {
        Self {
            read: true,
            vread: true,
            update: true,
            patch: true,
            delete: true,
            history: true,
            delete_history: true,
            delete_history_version: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirTypeInteractionsConfig {
    /// `POST /{type}`
    #[serde(default = "default_true")]
    pub create: bool,
    /// Conditional create via `If-None-Exist` on `POST /{type}`
    #[serde(default = "default_true")]
    pub conditional_create: bool,
    /// `GET /{type}` and `POST /{type}/_search`
    #[serde(default = "default_true")]
    pub search: bool,
    /// `GET /{type}/_history`
    #[serde(default = "default_true")]
    pub history: bool,
    /// `PUT /{type}` (conditional update)
    #[serde(default = "default_true")]
    pub conditional_update: bool,
    /// `PATCH /{type}` (conditional patch)
    #[serde(default = "default_true")]
    pub conditional_patch: bool,
    /// `DELETE /{type}` (conditional delete)
    #[serde(default = "default_true")]
    pub conditional_delete: bool,
}

impl Default for FhirTypeInteractionsConfig {
    fn default() -> Self {
        Self {
            create: true,
            conditional_create: true,
            search: true,
            history: true,
            conditional_update: true,
            conditional_patch: true,
            conditional_delete: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirSystemInteractionsConfig {
    /// `GET /metadata`
    #[serde(default = "default_true")]
    pub capabilities: bool,
    /// `GET /` and `POST /_search`
    #[serde(default = "default_true")]
    pub search: bool,
    /// `GET /_history`
    #[serde(default = "default_true")]
    pub history: bool,
    /// `DELETE /` (conditional delete across all types)
    #[serde(default = "default_true")]
    pub delete: bool,
    /// `POST /` with Bundle.type=batch
    #[serde(default = "default_true")]
    pub batch: bool,
    /// `POST /` with Bundle.type=transaction
    #[serde(default = "default_true")]
    pub transaction: bool,
    /// `POST /` with Bundle.type=history (replication)
    #[serde(default = "default_true")]
    pub history_bundle: bool,
}

impl Default for FhirSystemInteractionsConfig {
    fn default() -> Self {
        Self {
            capabilities: true,
            search: true,
            history: true,
            delete: true,
            batch: true,
            transaction: true,
            history_bundle: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirCompartmentInteractionsConfig {
    /// Variant searches:
    /// - `GET /{compartment_type}/{id}/*`
    /// - `POST /{compartment_type}/{id}/_search`
    /// - `GET /{compartment_type}/{id}/{type}`
    /// - `POST /{compartment_type}/{id}/{type}/_search`
    #[serde(default = "default_true")]
    pub search: bool,
}

impl Default for FhirCompartmentInteractionsConfig {
    fn default() -> Self {
        Self { search: true }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirOperationsInteractionsConfig {
    #[serde(default = "default_true")]
    pub system: bool,
    #[serde(default = "default_true")]
    pub type_level: bool,
    #[serde(default = "default_true")]
    pub instance: bool,
}

impl Default for FhirOperationsInteractionsConfig {
    fn default() -> Self {
        Self {
            system: true,
            type_level: true,
            instance: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirSearchConfig {
    /// Enable `_text` search parameter (narrative full-text).
    #[serde(default = "default_true")]
    pub enable_text: bool,
    /// Enable `_content` search parameter (whole-resource full-text).
    #[serde(default = "default_true")]
    pub enable_content: bool,
    /// Default page size when _count is not specified.
    /// Default: 20
    #[serde(default = "default_search_default_count")]
    pub default_count: usize,
    /// Maximum allowed _count value to prevent overly large result sets.
    /// Requests exceeding this will return "too-costly" error.
    /// Default: 1000
    #[serde(default = "default_search_max_count")]
    pub max_count: usize,
    /// Maximum total results across all pages (_maxresults cap).
    /// Default: 10000
    #[serde(default = "default_search_max_total_results")]
    pub max_total_results: usize,
    /// Maximum depth for _include:iterate and _revinclude:iterate.
    /// Prevents infinite recursion. Default: 3
    #[serde(default = "default_search_max_include_depth")]
    pub max_include_depth: usize,
    /// Maximum number of _include/_revinclude parameters allowed.
    /// Default: 10
    #[serde(default = "default_search_max_includes")]
    pub max_includes: usize,
    /// SearchParameter.status values treated as active.
    /// Default: ["draft", "active"]
    #[serde(default = "default_search_parameter_active_statuses")]
    pub search_parameter_active_statuses: Vec<String>,
}

impl Default for FhirSearchConfig {
    fn default() -> Self {
        Self {
            enable_text: true,
            enable_content: true,
            default_count: default_search_default_count(),
            max_count: default_search_max_count(),
            max_total_results: default_search_max_total_results(),
            max_include_depth: default_search_max_include_depth(),
            max_includes: default_search_max_includes(),
            search_parameter_active_statuses: default_search_parameter_active_statuses(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirPathConfig {
    /// Enable resolve() function for FHIR references in FHIRPath expressions.
    /// When false, resolve() returns empty collection. Default: true
    #[serde(default = "default_true")]
    pub enable_resolve: bool,

    /// Enable HTTP resolution for external absolute URLs.
    /// SECURITY WARNING: When enabled, FHIRPath expressions can fetch arbitrary URLs.
    /// Only enable in trusted environments. Default: false
    #[serde(default)]
    pub enable_external_http: bool,

    /// Per-request LRU cache size for resolved resources.
    /// Cache is scoped to single FHIRPath evaluation to prevent redundant DB queries.
    /// Default: 100
    #[serde(default = "default_resolve_cache_size")]
    pub resolve_cache_size: usize,

    /// HTTP timeout for external reference resolution in seconds.
    /// Only applies when enable_external_http is true. Default: 5
    #[serde(default = "default_http_timeout")]
    pub http_timeout_seconds: u64,
}

impl Default for FhirPathConfig {
    fn default() -> Self {
        Self {
            enable_resolve: true,
            enable_external_http: false,
            resolve_cache_size: default_resolve_cache_size(),
            http_timeout_seconds: default_http_timeout(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirPackageConfig {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub install_examples: bool,
    #[serde(flatten)]
    pub filter: ResourceTypeFilter,
}

/// Resource type filtering configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResourceTypeFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_resource_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_resource_types: Option<Vec<String>>,
}

impl ResourceTypeFilter {
    /// Validate that both include and exclude are not specified together
    pub fn validate(&self) -> Result<(), String> {
        if self.include_resource_types.is_some() && self.exclude_resource_types.is_some() {
            return Err(
                "Cannot specify both include_resource_types and exclude_resource_types".to_string(),
            );
        }
        Ok(())
    }

    /// Check if a resource type should be included based on the filter
    pub fn should_include(&self, resource_type: &str) -> bool {
        if let Some(include_list) = &self.include_resource_types {
            return include_list.iter().any(|rt| rt == resource_type);
        }
        if let Some(exclude_list) = &self.exclude_resource_types {
            return !exclude_list.iter().any(|rt| rt == resource_type);
        }
        true // No filter = include all
    }

    /// Check if any filter is active
    pub fn is_active(&self) -> bool {
        self.include_resource_types.is_some() || self.exclude_resource_types.is_some()
    }
}

/// Configuration for a default package (core, extensions, terminology)
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultPackageConfig {
    #[serde(default = "default_true")]
    pub install: bool,
    #[serde(default)]
    pub install_examples: bool,
    #[serde(flatten)]
    pub filter: ResourceTypeFilter,
}

impl Default for DefaultPackageConfig {
    fn default() -> Self {
        Self {
            install: true,
            install_examples: false,
            filter: ResourceTypeFilter::default(),
        }
    }
}

/// Default configuration for core package (excludes Bundle resources)
fn default_core_package_config() -> DefaultPackageConfig {
    DefaultPackageConfig {
        install: true,
        install_examples: false,
        filter: ResourceTypeFilter {
            include_resource_types: Some(
                [
                    "StructureDefinition",
                    "CodeSystem",
                    "ValueSet",
                    "SearchParameter",
                    "CompartmentDefinition",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ),
            exclude_resource_types: None,
        },
    }
}

/// Default configuration for extensions package (excludes Bundle resources)
fn default_extensions_package_config() -> DefaultPackageConfig {
    DefaultPackageConfig {
        install: false,
        install_examples: false,
        filter: ResourceTypeFilter {
            include_resource_types: Some(
                ["StructureDefinition"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            exclude_resource_types: None,
        },
    }
}

/// Default configuration for terminology package (excludes Bundle resources)
fn default_terminology_package_config() -> DefaultPackageConfig {
    DefaultPackageConfig {
        install: false,
        install_examples: false,
        filter: ResourceTypeFilter {
            include_resource_types: Some(
                ["ValueSet", "CodeSystem", "ConceptMap"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            exclude_resource_types: None,
        },
    }
}

/// Configuration for all default packages
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultPackagesConfig {
    #[serde(default = "default_core_package_config")]
    pub core: DefaultPackageConfig,
    #[serde(default = "default_extensions_package_config")]
    pub extensions: DefaultPackageConfig,
    #[serde(default = "default_terminology_package_config")]
    pub terminology: DefaultPackageConfig,
}

impl Default for DefaultPackagesConfig {
    fn default() -> Self {
        Self {
            core: default_core_package_config(),
            extensions: default_extensions_package_config(),
            terminology: default_terminology_package_config(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CapabilityStatementConfig {
    #[serde(default = "default_cs_id")]
    pub id: String,
    #[serde(default = "default_cs_name")]
    pub name: String,
    #[serde(default = "default_cs_title")]
    pub title: String,
    #[serde(default = "default_cs_publisher")]
    pub publisher: String,
    #[serde(default = "default_cs_description")]
    pub description: String,
    #[serde(default = "default_cs_software_name")]
    pub software_name: String,
    #[serde(default = "default_cs_software_version")]
    pub software_version: String,
    pub contact_email: Option<String>,
    /// List of supported resource types
    /// If empty, all resource types from loaded StructureDefinitions will be used
    #[serde(default)]
    pub supported_resources: Vec<String>,
}

/// Referential integrity enforcement configuration.
///
/// Controls whether the server validates that references point to existing resources
/// and prevents deletion of resources that are referenced by others.
#[derive(Debug, Clone, Deserialize)]
pub struct ReferentialIntegrityConfig {
    /// Enforcement mode:
    /// - "lenient" (default): no checks, current behavior
    /// - "strict": reject writes with broken refs, reject deletes of referenced resources
    #[serde(default = "default_referential_integrity_mode")]
    pub mode: String,
}

impl Default for ReferentialIntegrityConfig {
    fn default() -> Self {
        Self {
            mode: default_referential_integrity_mode(),
        }
    }
}

fn default_referential_integrity_mode() -> String {
    "lenient".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Run workers in the same process as the API server.
    /// When true, the server spawns background worker tasks at startup (simpler deployment).
    /// When false, use the separate `fhir-worker` binary (independently scalable).
    #[serde(default = "default_true")]
    pub embedded: bool,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_jobs: usize,
    /// Initial reconnect delay (seconds) when the worker loses the DB job listener connection.
    #[serde(default = "default_worker_reconnect_initial_seconds")]
    pub reconnect_initial_seconds: u64,
    /// Maximum reconnect delay (seconds) for exponential backoff.
    #[serde(default = "default_worker_reconnect_max_seconds")]
    pub reconnect_max_seconds: u64,
    /// Random jitter ratio applied to reconnect delays (0.0 - 1.0).
    /// Example: 0.2 -> +/-20% jitter.
    #[serde(default = "default_worker_reconnect_jitter_ratio")]
    pub reconnect_jitter_ratio: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Use JSON formatting for logs (recommended for production)
    #[serde(default)]
    pub json: bool,

    /// Enable file logging in addition to console
    #[serde(default)]
    pub file_enabled: bool,

    /// Directory for log files (default: ./logs)
    #[serde(default = "default_log_directory")]
    pub file_directory: String,

    /// Log file prefix (default: fhir-server)
    #[serde(default = "default_log_file_prefix")]
    pub file_prefix: String,

    /// Log rotation: daily, hourly, minutely, never (default: daily)
    #[serde(default = "default_log_rotation")]
    pub file_rotation: String,

    #[serde(default)]
    pub audit: AuditConfig,

    /// Enable OpenTelemetry integration
    #[serde(default)]
    pub opentelemetry_enabled: bool,

    /// OpenTelemetry Collector endpoint (OTLP/gRPC)
    #[serde(default = "default_otlp_endpoint")]
    pub otlp_endpoint: String,

    /// Trace sampling ratio (0.0 - 1.0): 1.0 = always, 0.1 = 10%
    #[serde(default = "default_trace_sample_ratio")]
    pub trace_sample_ratio: f64,

    /// Service name for OpenTelemetry
    #[serde(default = "default_service_name")]
    pub service_name: String,

    /// Deployment environment (dev, staging, prod)
    #[serde(default = "default_environment")]
    pub deployment_environment: String,

    /// Service version (defaults to cargo package version)
    pub service_version: Option<String>,

    /// OTLP export timeout in seconds
    #[serde(default = "default_otlp_timeout")]
    pub otlp_timeout_seconds: u64,
}

/// Fine-grained control for what to audit from the FHIR API.
#[derive(Debug, Clone, Deserialize)]
pub struct AuditConfig {
    /// Master switch for audit logging.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Record successful interactions (HTTP < 400).
    #[serde(default = "default_true")]
    pub include_success: bool,
    /// Record authorization failures (HTTP 401/403).
    #[serde(default = "default_true")]
    pub include_authz_failure: bool,
    /// Record processing failures (HTTP >= 400 excluding 401/403).
    #[serde(default = "default_true")]
    pub include_processing_failure: bool,

    #[serde(default)]
    pub interactions: AuditInteractionsConfig,

    /// For search interactions, capture full raw HTTP request (headers+body) into
    /// `AuditEvent.entity.query` as base64binary.
    #[serde(default = "default_true")]
    pub capture_search_query: bool,

    /// On failures, try to capture OperationOutcome from the response.
    #[serde(default = "default_true")]
    pub capture_operation_outcome: bool,

    /// For search interactions, parse the response bundle and emit one AuditEvent per resolved
    /// patient id (best-practice). When false, emits a single AuditEvent per request.
    #[serde(default = "default_true")]
    pub per_patient_events_for_search: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            include_success: true,
            include_authz_failure: true,
            include_processing_failure: true,
            interactions: AuditInteractionsConfig::default(),
            capture_search_query: true,
            capture_operation_outcome: true,
            per_patient_events_for_search: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuditInteractionsConfig {
    #[serde(default = "default_true")]
    pub read: bool,
    #[serde(default = "default_true")]
    pub vread: bool,
    #[serde(default = "default_true")]
    pub history: bool,
    #[serde(default = "default_true")]
    pub search: bool,
    #[serde(default = "default_true")]
    pub create: bool,
    #[serde(default = "default_true")]
    pub update: bool,
    #[serde(default = "default_true")]
    pub patch: bool,
    #[serde(default = "default_true")]
    pub delete: bool,
    /// Audit `/metadata` (CapabilityStatement) requests.
    /// Defaults to false since these are often polled for health/connection checks.
    #[serde(default)]
    pub capabilities: bool,
    #[serde(default = "default_true")]
    pub operation: bool,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default = "default_true")]
    pub transaction: bool,
    #[serde(default = "default_true")]
    pub export: bool,
}

impl Default for AuditInteractionsConfig {
    fn default() -> Self {
        Self {
            read: true,
            vread: true,
            history: true,
            search: true,
            create: true,
            update: true,
            patch: true,
            delete: true,
            capabilities: false, // Off by default - often polled for health checks
            operation: true,
            batch: true,
            transaction: true,
            export: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    /// Enable authentication/authorization for protected routes.
    ///
    /// When enabled, the server acts as an OAuth2/OIDC resource server:
    /// - Clients authenticate with an external IdP (e.g. Keycloak)
    /// - The FHIR server validates the presented access token on each request
    #[serde(default)]
    pub enabled: bool,

    /// When true, requests to protected routes without a valid token are rejected (401).
    /// When false, requests without a token are treated as anonymous.
    #[serde(default = "default_true")]
    pub required: bool,

    /// Paths that never require authentication (exact matches).
    ///
    /// Defaults include health checks and FHIR metadata endpoints to support discovery.
    #[serde(default = "default_auth_public_paths")]
    pub public_paths: Vec<String>,

    #[serde(default)]
    pub oidc: OidcAuthConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            required: true,
            public_paths: default_auth_public_paths(),
            oidc: OidcAuthConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OidcAuthConfig {
    /// OIDC issuer URL (the `iss` claim), e.g. `https://keycloak.example.com/realms/tlq`.
    pub issuer_url: Option<String>,

    /// Expected audience (the `aud` claim). Typically your API/client identifier.
    pub audience: Option<String>,

    /// Override for the JWKS URI. If not set, the server will derive it from
    /// `issuer_url` via OIDC discovery.
    pub jwks_url: Option<String>,

    /// JWKS cache TTL in seconds. Used when fetching signing keys from the IdP.
    #[serde(default = "default_oidc_jwks_cache_ttl_seconds")]
    pub jwks_cache_ttl_seconds: u64,

    /// HTTP timeout in seconds for OIDC discovery / JWKS fetch.
    #[serde(default = "default_oidc_http_timeout_seconds")]
    pub http_timeout_seconds: u64,
}

impl Default for OidcAuthConfig {
    fn default() -> Self {
        Self {
            issuer_url: None,
            audience: None,
            jwks_url: None,
            jwks_cache_ttl_seconds: default_oidc_jwks_cache_ttl_seconds(),
            http_timeout_seconds: default_oidc_http_timeout_seconds(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiConfig {
    /// Enable admin UI
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Admin UI title
    #[serde(default = "default_ui_title")]
    pub title: String,

    /// Admin password (enables admin authentication when set).
    /// Can be set via `FHIR__UI__PASSWORD`.
    #[serde(default)]
    pub password: Option<String>,

    /// Secret used to sign admin UI sessions (cookie).
    ///
    /// If not set, the server generates an ephemeral secret at startup
    /// (sessions become invalid on restart).
    ///
    /// Recommended: set this in production via `FHIR__UI__SESSION_SECRET` to a long random value.
    #[serde(default)]
    pub session_secret: Option<String>,

    /// Admin session TTL in seconds.
    #[serde(default = "default_ui_session_ttl_seconds")]
    pub session_ttl_seconds: u64,

    /// Enable runtime configuration API (settings page in admin UI).
    /// When false, the /admin/config endpoints return 404 and the settings page is hidden.
    #[serde(default = "default_true")]
    pub runtime_config_enabled: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            title: default_ui_title(),
            password: None,
            session_secret: None,
            session_ttl_seconds: default_ui_session_ttl_seconds(),
            runtime_config_enabled: true,
        }
    }
}

// Default values
fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_cors_origins() -> Vec<String> {
    vec!["http://localhost:3000".to_string()]
}

fn default_max_request_body_size() -> usize {
    10 * 1024 * 1024 // 10 MB
}

fn default_max_response_body_size() -> usize {
    50 * 1024 * 1024 // 50 MB
}

fn default_database_url() -> String {
    "postgresql://fhir:fhir@localhost/fhir".to_string()
}

fn default_pool_min_size() -> u32 {
    2 // API server: lower idle usage
}

fn default_pool_max_size() -> u32 {
    20 // API server: conservative default for multi-process deployments
}

fn default_pool_timeout() -> u64 {
    60 // API server: longer timeout for load spikes
}

fn default_worker_pool_min_size() -> u32 {
    1 // Worker: minimal idle connections
}

fn default_worker_pool_max_size() -> u32 {
    5 // Worker: needs fewer connections (LISTEN/NOTIFY + indexing)
}

fn default_worker_pool_timeout() -> u64 {
    60 // Worker: longer timeout for long-running indexing
}

fn default_indexing_batch_size() -> usize {
    50
}

fn default_indexing_bulk_threshold() -> usize {
    200
}

fn default_fhir_version() -> String {
    "R4".to_string()
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_poll_interval() -> u64 {
    5
}

fn default_max_concurrent() -> usize {
    1
}

fn default_worker_reconnect_initial_seconds() -> u64 {
    1
}

fn default_worker_reconnect_max_seconds() -> u64 {
    30
}

fn default_worker_reconnect_jitter_ratio() -> f64 {
    0.2
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_directory() -> String {
    "./logs".to_string()
}

fn default_log_file_prefix() -> String {
    "fhir-server".to_string()
}

fn default_log_rotation() -> String {
    "daily".to_string()
}

fn default_otlp_endpoint() -> String {
    "http://localhost:4317".to_string()
}

fn default_trace_sample_ratio() -> f64 {
    1.0 // Always sample in development
}

fn default_service_name() -> String {
    "fhir-server".to_string()
}

fn default_environment() -> String {
    "development".to_string()
}

fn default_otlp_timeout() -> u64 {
    10
}

fn default_format() -> String {
    "json".to_string()
}

fn default_prefer_return() -> String {
    "representation".to_string()
}

fn default_statement_timeout() -> u64 {
    300
}

fn default_lock_timeout() -> u64 {
    30
}

fn default_cs_id() -> String {
    "zunder-capability-statement".to_string()
}

fn default_cs_name() -> String {
    "zunder-capability-statement".to_string()
}

fn default_cs_title() -> String {
    "Zunder FHIR Server".to_string()
}

fn default_cs_publisher() -> String {
    "ThalamiQ".to_string()
}

fn default_cs_description() -> String {
    "Capability Statement for the Zunder FHIR Server".to_string()
}

fn default_cs_software_name() -> String {
    env!("CARGO_PKG_NAME").to_string()
}

fn default_cs_software_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn default_search_default_count() -> usize {
    20
}

fn default_search_max_count() -> usize {
    1000
}

fn default_search_max_total_results() -> usize {
    10000
}

fn default_search_max_include_depth() -> usize {
    3
}

fn default_search_max_includes() -> usize {
    10
}

fn default_search_parameter_active_statuses() -> Vec<String> {
    vec!["draft".to_string(), "active".to_string()]
}

fn default_resolve_cache_size() -> usize {
    100
}

fn default_http_timeout() -> u64 {
    5
}

fn default_ui_title() -> String {
    "FHIR Admin".to_string()
}

fn default_ui_session_ttl_seconds() -> u64 {
    12 * 60 * 60
}

fn default_auth_public_paths() -> Vec<String> {
    vec![
        "/health".to_string(),
        "/".to_string(),
        "/favicon.ico".to_string(),
        // FHIR discovery endpoints are typically public.
        "/fhir/metadata".to_string(),
        "/fhir/metadata/".to_string(),
        "/fhir/.well-known/smart-configuration".to_string(),
    ]
}

fn default_oidc_jwks_cache_ttl_seconds() -> u64 {
    300
}

fn default_oidc_http_timeout_seconds() -> u64 {
    5
}

impl Default for CapabilityStatementConfig {
    fn default() -> Self {
        Self {
            id: default_cs_id(),
            name: default_cs_name(),
            title: default_cs_title(),
            publisher: default_cs_publisher(),
            description: default_cs_description(),
            software_name: default_cs_software_name(),
            software_version: default_cs_software_version(),
            contact_email: None,
            supported_resources: Vec::new(),
        }
    }
}

impl Config {
    /// Load configuration from environment and config files
    pub fn load() -> anyhow::Result<Self> {
        // Load .env file if present
        dotenvy::dotenv().ok();

        let config = config::Config::builder()
            // Start with defaults
            .set_default("server.host", default_host())?
            .set_default("server.port", default_port())?
            .set_default(
                "server.max_request_body_size",
                default_max_request_body_size() as i64,
            )?
            .set_default(
                "server.max_response_body_size",
                default_max_response_body_size() as i64,
            )?
            .set_default("database.url", default_database_url())?
            .set_default("database.pool_min_size", default_pool_min_size())?
            .set_default("database.pool_max_size", default_pool_max_size())?
            .set_default("database.pool_timeout_seconds", default_pool_timeout())?
            .set_default(
                "database.worker_pool_min_size",
                default_worker_pool_min_size(),
            )?
            .set_default(
                "database.worker_pool_max_size",
                default_worker_pool_max_size(),
            )?
            .set_default(
                "database.worker_pool_timeout_seconds",
                default_worker_pool_timeout(),
            )?
            .set_default("fhir.version", default_fhir_version())?
            .set_default("fhir.search.enable_text", default_true())?
            .set_default("fhir.search.enable_content", default_true())?
            .set_default(
                "fhir.search.default_count",
                default_search_default_count() as i64,
            )?
            .set_default("fhir.search.max_count", default_search_max_count() as i64)?
            .set_default(
                "fhir.search.max_total_results",
                default_search_max_total_results() as i64,
            )?
            .set_default(
                "fhir.search.max_include_depth",
                default_search_max_include_depth() as i64,
            )?
            .set_default(
                "fhir.search.max_includes",
                default_search_max_includes() as i64,
            )?
            .set_default("fhir.default_format", default_format())?
            .set_default("fhir.default_prefer_return", default_prefer_return())?
            .set_default("fhir.allow_update_create", default_true())?
            .set_default("fhir.hard_delete", default_false())?
            .set_default("fhir.referential_integrity.mode", default_referential_integrity_mode())?
            .set_default("workers.enabled", default_true())?
            .set_default("workers.embedded", default_true())?
            .set_default("workers.poll_interval_seconds", default_poll_interval())?
            .set_default(
                "workers.max_concurrent_jobs",
                default_max_concurrent() as i64,
            )?
            .set_default(
                "workers.reconnect_initial_seconds",
                default_worker_reconnect_initial_seconds(),
            )?
            .set_default(
                "workers.reconnect_max_seconds",
                default_worker_reconnect_max_seconds(),
            )?
            .set_default(
                "workers.reconnect_jitter_ratio",
                default_worker_reconnect_jitter_ratio(),
            )?
            .set_default("logging.level", default_log_level())?
            .set_default("logging.json", false)?
            .set_default("logging.file_enabled", false)?
            .set_default("logging.file_directory", default_log_directory())?
            .set_default("logging.file_prefix", default_log_file_prefix())?
            .set_default("logging.file_rotation", default_log_rotation())?
            .set_default("logging.audit.enabled", default_true())?
            .set_default("logging.audit.include_success", default_true())?
            .set_default("logging.audit.include_authz_failure", default_true())?
            .set_default("logging.audit.include_processing_failure", default_true())?
            .set_default("logging.audit.capture_search_query", default_true())?
            .set_default("logging.audit.capture_operation_outcome", default_true())?
            .set_default(
                "logging.audit.per_patient_events_for_search",
                default_true(),
            )?
            .set_default("logging.audit.interactions.read", default_true())?
            .set_default("logging.audit.interactions.vread", default_true())?
            .set_default("logging.audit.interactions.history", default_true())?
            .set_default("logging.audit.interactions.search", default_true())?
            .set_default("logging.audit.interactions.create", default_true())?
            .set_default("logging.audit.interactions.update", default_true())?
            .set_default("logging.audit.interactions.patch", default_true())?
            .set_default("logging.audit.interactions.delete", default_true())?
            .set_default("logging.audit.interactions.capabilities", false)?
            .set_default("logging.audit.interactions.operation", default_true())?
            .set_default("logging.audit.interactions.batch", default_true())?
            .set_default("logging.audit.interactions.transaction", default_true())?
            .set_default("logging.audit.interactions.export", default_true())?
            .set_default("ui.enabled", default_true())?
            .set_default("ui.title", default_ui_title())?
            .set_default(
                "ui.session_ttl_seconds",
                default_ui_session_ttl_seconds() as i64,
            )?
            .set_default("ui.runtime_config_enabled", default_true())?
            .set_default("auth.enabled", false)?
            .set_default("auth.required", default_true())?
            .set_default(
                "auth.oidc.jwks_cache_ttl_seconds",
                default_oidc_jwks_cache_ttl_seconds() as i64,
            )?
            .set_default(
                "auth.oidc.http_timeout_seconds",
                default_oidc_http_timeout_seconds() as i64,
            )?
            // Add config file if exists
            .add_source(config::File::with_name("config").required(false))
            // Override with environment variables
            // Uses double underscore (__) to map to nested config structure
            // Example: FHIR__DATABASE__URL → config.database.url
            // Arrays use comma separator: FHIR__SERVER__CORS_ORIGINS=https://a.com,https://b.com
            .add_source(
                config::Environment::with_prefix("FHIR")
                    .prefix_separator("__")
                    .separator("__")
                    .list_separator(",")
                    // Explicitly specify which keys are lists to prevent other values
                    // from being incorrectly parsed as arrays
                    .with_list_parse_key("server.cors_origins")
                    .with_list_parse_key("fhir.search.search_parameter_active_statuses")
                    .with_list_parse_key("fhir.capability_statement.supported_resources")
                    .with_list_parse_key("auth.public_paths")
                    .try_parsing(true),
            )
            .build()?;

        let mut config: Self = config.try_deserialize()?;

        // Convenience escape hatch: allow DATABASE_URL to set `database.url` when no explicit
        // FHIR__DATABASE__URL override is present.
        if std::env::var("FHIR__DATABASE__URL").is_err() {
            if let Ok(url) = std::env::var("DATABASE_URL") {
                config.database.url = url;
            }
        }

        Ok(config)
    }

    pub fn socket_addr(&self) -> anyhow::Result<SocketAddr> {
        let addr = format!("{}:{}", self.server.host, self.server.port);
        Ok(addr.parse()?)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate default package filters
        self.fhir.default_packages.core.filter.validate()?;
        self.fhir.default_packages.extensions.filter.validate()?;
        self.fhir.default_packages.terminology.filter.validate()?;

        // Validate custom package filters
        for pkg in &self.fhir.packages {
            pkg.filter.validate().map_err(|e| {
                format!(
                    "Package {}#{} has invalid filter: {}",
                    pkg.name, pkg.version, e
                )
            })?;
        }

        if self.workers.poll_interval_seconds == 0 {
            return Err("workers.poll_interval_seconds must be > 0".to_string());
        }
        if self.workers.reconnect_initial_seconds == 0 {
            return Err("workers.reconnect_initial_seconds must be > 0".to_string());
        }
        if self.workers.reconnect_max_seconds < self.workers.reconnect_initial_seconds {
            return Err(
                "workers.reconnect_max_seconds must be >= workers.reconnect_initial_seconds"
                    .to_string(),
            );
        }
        if !(0.0..=1.0).contains(&self.workers.reconnect_jitter_ratio) {
            return Err("workers.reconnect_jitter_ratio must be between 0.0 and 1.0".to_string());
        }

        if self.auth.enabled {
            if self
                .auth
                .oidc
                .issuer_url
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            {
                return Err("auth.oidc.issuer_url must be set when auth.enabled=true".to_string());
            }
            if self
                .auth
                .oidc
                .audience
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            {
                return Err("auth.oidc.audience must be set when auth.enabled=true".to_string());
            }
            if self.auth.oidc.http_timeout_seconds == 0 {
                return Err("auth.oidc.http_timeout_seconds must be > 0".to_string());
            }
        }

        if self.ui.session_ttl_seconds == 0 {
            return Err("ui.session_ttl_seconds must be > 0".to_string());
        }

        Ok(())
    }
}
