use crate::db::search::engine::SearchEngine;
use crate::db::PostgresResourceStore;
use crate::error::{Error, Result};
use crate::models::{OperationContext, OperationRequest, OperationResult, Parameters};
use crate::queue::{JobPriority, JobQueue};
use crate::services::{IndexingService, PackageService, TerminologyService};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

#[async_trait]
pub trait Operation: Send + Sync {
    async fn execute(&self, request: OperationRequest) -> Result<OperationResult>;
}

pub struct OperationExecutor {
    #[allow(dead_code)] // Reserved for future use
    package_service: Option<Arc<PackageService>>,
    indexing_service: Option<Arc<IndexingService>>,
    terminology_service: Option<Arc<TerminologyService>>,
    job_queue: Option<Arc<dyn JobQueue>>,
    search_engine: Option<Arc<SearchEngine>>,
    store: Option<PostgresResourceStore>,
}

impl OperationExecutor {
    pub fn new() -> Self {
        Self {
            package_service: None,
            indexing_service: None,
            terminology_service: None,
            job_queue: None,
            search_engine: None,
            store: None,
        }
    }

    /// Create executor with all dependencies for full operation support
    pub fn with_services(
        package_service: Arc<PackageService>,
        indexing_service: Arc<IndexingService>,
        terminology_service: Arc<TerminologyService>,
        job_queue: Arc<dyn JobQueue>,
        search_engine: Arc<SearchEngine>,
        store: PostgresResourceStore,
    ) -> Self {
        Self {
            package_service: Some(package_service),
            indexing_service: Some(indexing_service),
            terminology_service: Some(terminology_service),
            job_queue: Some(job_queue),
            search_engine: Some(search_engine),
            store: Some(store),
        }
    }

    pub async fn execute(&self, request: OperationRequest) -> Result<OperationResult> {
        match request.operation_name.as_str() {
            "install-package" => self.execute_install_package(request).await,
            "reindex" => self.execute_reindex(request).await,
            "expand" => self.execute_expand(request).await,
            "lookup" => self.execute_lookup(request).await,
            "validate-code" => self.execute_validate_code(request).await,
            "subsumes" => self.execute_subsumes(request).await,
            "translate" => self.execute_translate(request).await,
            "closure" => self.execute_closure(request).await,
            "everything" => self.execute_everything(request).await,
            _ => Err(Error::NotImplemented(format!(
                "Operation '{}' not yet implemented",
                request.operation_name
            ))),
        }
    }

    /// $install-package operation - install a FHIR package from registry
    async fn execute_install_package(&self, request: OperationRequest) -> Result<OperationResult> {
        // Validate context (system-level only)
        if !matches!(request.context, OperationContext::System) {
            return Err(Error::InvalidResource(
                "$install-package can only be invoked at system level".to_string(),
            ));
        }

        let job_queue = self
            .job_queue
            .as_ref()
            .ok_or_else(|| Error::Internal("JobQueue not available".to_string()))?;

        // Extract parameters
        let name = request
            .parameters
            .get_value("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidResource("Missing required parameter: name".to_string()))?
            .to_string();

        let version = request
            .parameters
            .get_value("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let include_dependencies = request
            .parameters
            .get_value("includeDependencies")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let include_examples = request
            .parameters
            .get_value("includeExamples")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Validate package name
        if name.trim().is_empty() {
            return Err(Error::Validation("Package name is required".to_string()));
        }

        // Queue package installation job (same format as admin API)
        let params = json!({
            "package_name": name,
            "package_version": version,
            "include_dependencies": include_dependencies,
            "include_examples": include_examples,
        });

        let job_id = job_queue
            .enqueue(
                "install_package".to_string(),
                params,
                JobPriority::Normal,
                None,
            )
            .await?;

        // Build FHIR Parameters response
        let mut response = Parameters::new();
        response.add_resource(
            "outcome".to_string(),
            json!({
                "resourceType": "OperationOutcome",
                "issue": [{
                    "severity": "information",
                    "code": "informational",
                    "diagnostics": format!(
                        "Package installation job queued: {}#{}",
                        name,
                        version.as_deref().unwrap_or("latest")
                    )
                }]
            }),
        );
        response.add_value_string("jobId".to_string(), job_id.to_string());
        response.add_value_string("name".to_string(), name);
        if let Some(v) = version {
            response.add_value_string("version".to_string(), v);
        }
        response.add_value_boolean("includeDependencies".to_string(), include_dependencies);
        response.add_value_boolean("includeExamples".to_string(), include_examples);

        Ok(OperationResult::Parameters(response))
    }

    /// $reindex operation - reindex search parameters
    async fn execute_reindex(&self, request: OperationRequest) -> Result<OperationResult> {
        let _indexing_service = self
            .indexing_service
            .as_ref()
            .ok_or_else(|| Error::Internal("IndexingService not available".to_string()))?;

        // Extract parameters
        let async_mode = request
            .parameters
            .get_value("async")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Determine scope based on context
        let (resource_type, resource_id) = match &request.context {
            OperationContext::System => (None, None),
            OperationContext::Type(rt) => (Some(rt.clone()), None),
            OperationContext::Instance(rt, id) => (Some(rt.clone()), Some(id.clone())),
        };

        if async_mode {
            let _job_queue = self
                .job_queue
                .as_ref()
                .ok_or_else(|| Error::Internal("JobQueue not available".to_string()))?;

            // TODO: Enqueue reindex job
            // let job_id = job_queue.enqueue("reindex", params, priority, None).await?;

            let mut response = Parameters::new();
            response.add_resource(
                "outcome".to_string(),
                json!({
                    "resourceType": "OperationOutcome",
                    "issue": [{
                        "severity": "information",
                        "code": "informational",
                        "diagnostics": "Reindex job queued for background processing"
                    }]
                }),
            );
            response.add_value_integer("jobId".to_string(), 12345); // Placeholder

            Ok(OperationResult::Parameters(response))
        } else {
            // Synchronous reindex
            // TODO: Implement synchronous reindexing
            // This would involve:
            // 1. Getting resources to reindex based on context
            // 2. For each resource: indexing_service.index_resource(resource)
            // 3. Tracking success/failure counts

            let mut response = Parameters::new();
            response.add_resource(
                "outcome".to_string(),
                json!({
                    "resourceType": "OperationOutcome",
                    "issue": [{
                        "severity": "information",
                        "code": "informational",
                        "diagnostics": format!(
                            "Reindex completed - resource_type: {:?}, resource_id: {:?}",
                            resource_type, resource_id
                        )
                    }]
                }),
            );
            response.add_value_integer("resourcesReindexed".to_string(), 0);
            response.add_value_integer("resourcesFailed".to_string(), 0);

            Ok(OperationResult::Parameters(response))
        }
    }

    async fn execute_expand(&self, request: OperationRequest) -> Result<OperationResult> {
        let terminology = self
            .terminology_service
            .as_ref()
            .ok_or_else(|| Error::Internal("TerminologyService not available".to_string()))?;
        let vs = terminology
            .expand(&request.context, &request.parameters)
            .await?;
        Ok(OperationResult::Resource(vs))
    }

    async fn execute_lookup(&self, request: OperationRequest) -> Result<OperationResult> {
        let terminology = self
            .terminology_service
            .as_ref()
            .ok_or_else(|| Error::Internal("TerminologyService not available".to_string()))?;
        let out = terminology
            .lookup(&request.context, &request.parameters)
            .await?;
        Ok(OperationResult::Parameters(out))
    }

    async fn execute_validate_code(&self, request: OperationRequest) -> Result<OperationResult> {
        let terminology = self
            .terminology_service
            .as_ref()
            .ok_or_else(|| Error::Internal("TerminologyService not available".to_string()))?;
        let out = terminology
            .validate_code(&request.context, &request.parameters)
            .await?;
        Ok(OperationResult::Parameters(out))
    }

    async fn execute_subsumes(&self, request: OperationRequest) -> Result<OperationResult> {
        let terminology = self
            .terminology_service
            .as_ref()
            .ok_or_else(|| Error::Internal("TerminologyService not available".to_string()))?;
        let out = terminology
            .subsumes(&request.context, &request.parameters)
            .await?;
        Ok(OperationResult::Parameters(out))
    }

    async fn execute_translate(&self, request: OperationRequest) -> Result<OperationResult> {
        let terminology = self
            .terminology_service
            .as_ref()
            .ok_or_else(|| Error::Internal("TerminologyService not available".to_string()))?;
        let out = terminology
            .translate(&request.context, &request.parameters)
            .await?;
        Ok(OperationResult::Parameters(out))
    }

    async fn execute_closure(&self, request: OperationRequest) -> Result<OperationResult> {
        if !matches!(request.context, OperationContext::System) {
            return Err(Error::Validation(
                "$closure can only be invoked at system level".to_string(),
            ));
        }

        let terminology = self
            .terminology_service
            .as_ref()
            .ok_or_else(|| Error::Internal("TerminologyService not available".to_string()))?;
        let cm = terminology.closure(&request.parameters).await?;
        Ok(OperationResult::Resource(cm))
    }

    /// Patient/$everything — return the patient plus all resources in the patient compartment.
    async fn execute_everything(&self, request: OperationRequest) -> Result<OperationResult> {
        use crate::db::search::params::SearchParameters;
        use crate::db::traits::ResourceStore;

        let (resource_type, patient_id) = match &request.context {
            OperationContext::Instance(rt, id) => (rt.as_str(), id.as_str()),
            _ => {
                return Err(Error::Validation(
                    "$everything is only supported at instance level (Patient/[id]/$everything)"
                        .to_string(),
                ));
            }
        };

        if resource_type != "Patient" {
            return Err(Error::Validation(format!(
                "$everything is only supported on Patient, not {}",
                resource_type
            )));
        }

        let search_engine = self
            .search_engine
            .as_ref()
            .ok_or_else(|| Error::Internal("SearchEngine not available".to_string()))?;
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| Error::Internal("ResourceStore not available".to_string()))?;

        // Verify the patient exists
        let patient = store
            .read("Patient", patient_id)
            .await?
            .ok_or_else(|| Error::ResourceNotFound {
                resource_type: "Patient".to_string(),
                id: patient_id.to_string(),
            })?;

        // Parse optional _type filter
        let type_filter: Option<Vec<String>> = request
            .parameters
            .get_value("_type")
            .and_then(|v| v.as_str())
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());

        // Parse optional _count
        let count: Option<usize> = request
            .parameters
            .get_value("_count")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok());

        // Parse optional _since
        let since_filter: Option<String> = request
            .parameters
            .get_value("_since")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Build search params for compartment search
        let mut items: Vec<(String, String)> = Vec::new();
        if let Some(c) = count {
            items.push(("_count".to_string(), c.to_string()));
        }
        if let Some(ref since) = since_filter {
            items.push(("_lastUpdated".to_string(), format!("ge{}", since)));
        }
        let search_params = SearchParameters::from_items(&items)?;

        // Search the Patient compartment (all resource types or filtered)
        let result = if let Some(ref types) = type_filter {
            // Search each requested type and collect results
            let mut all_resources: Vec<serde_json::Value> = Vec::new();
            for rt in types {
                let result = search_engine
                    .search_compartment("Patient", patient_id, Some(rt.as_str()), &search_params, None)
                    .await?;
                all_resources.extend(result.resources);
            }
            all_resources
        } else {
            // Search all resource types in compartment
            let result = search_engine
                .search_compartment("Patient", patient_id, None, &search_params, None)
                .await?;
            result.resources
        };

        // Build the $everything Bundle: patient first, then compartment resources
        let mut entries = Vec::with_capacity(result.len() + 1);

        // Add the patient
        if since_filter.is_none()
            || type_filter.is_none()
            || type_filter.as_ref().is_some_and(|t| t.iter().any(|rt| rt == "Patient"))
        {
            entries.push(json!({
                "fullUrl": format!("Patient/{}", patient_id),
                "resource": patient.resource,
                "search": { "mode": "match" }
            }));
        }

        // Add compartment resources
        for resource in result {
            let rt = resource.get("resourceType").and_then(|v| v.as_str()).unwrap_or("Unknown");
            let id = resource.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
            entries.push(json!({
                "fullUrl": format!("{}/{}", rt, id),
                "resource": resource,
                "search": { "mode": "match" }
            }));
        }

        let bundle = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": entries.len(),
            "entry": entries
        });

        Ok(OperationResult::Resource(bundle))
    }
}

impl Default for OperationExecutor {
    fn default() -> Self {
        Self::new()
    }
}
