//! Batch processing service
//!
//! Implements FHIR batch bundle processing rules (3.2.0.13.1):
//! - Entries are processed independently (no interdependencies that cause change)
//! - Overall response is always HTTP 200 (handler-level), with per-entry status
//! - References to resources created within the same batch are non-conformant (no resolution)

use crate::{
    api::headers::parse_etag,
    db::PostgresResourceStore,
    hooks::ResourceHook,
    models::UpdateParams,
    queue::{JobPriority, JobQueue},
    runtime_config::RuntimeConfigCache,
    services::CrudService,
    Result,
};
use axum::http::StatusCode;
use json_patch::PatchErrorKind;
use serde_json::{json, Value as JsonValue};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use zunder_models::{Bundle, BundleEntry, BundleEntryResponse, BundleType};
use uuid::Uuid;

use crate::db::search::engine::SearchEngine;
use crate::services::conditional::{
    parse_form_urlencoded, parse_if_none_match_for_conditional_update, query_from_url,
    ConditionalService,
};

/// Prefer header return preference for response bundles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreferReturn {
    /// Return minimal response (status/location/etag only, no resource body)
    Minimal,
    /// Return full resource representation in response
    #[default]
    Representation,
    /// Return OperationOutcome in `entry.response.outcome`
    OperationOutcome,
}

#[derive(Debug, Clone, Default)]
pub struct BundleRequestOptions {
    pub prefer_return: PreferReturn,
    pub base_url: Option<String>,
}

pub struct BatchService {
    store: PostgresResourceStore,
    #[allow(dead_code)] // Kept for conformance hook processing
    hooks: Vec<Arc<dyn ResourceHook>>,
    job_queue: Arc<dyn JobQueue>,
    search_engine: Arc<SearchEngine>,
    conditional_service: ConditionalService,
    allow_update_create: bool,
    hard_delete: bool,
    runtime_config_cache: Option<Arc<RuntimeConfigCache>>,
    referential_integrity_mode: String,
}

impl BatchService {
    pub fn new(
        store: PostgresResourceStore,
        hooks: Vec<Arc<dyn ResourceHook>>,
        job_queue: Arc<dyn JobQueue>,
        search_engine: Arc<SearchEngine>,
        allow_update_create: bool,
        hard_delete: bool,
    ) -> Self {
        Self {
            store,
            hooks,
            job_queue,
            search_engine: search_engine.clone(),
            conditional_service: ConditionalService::new(search_engine),
            allow_update_create,
            hard_delete,
            runtime_config_cache: None,
            referential_integrity_mode: "lenient".to_string(),
        }
    }

    pub fn set_referential_integrity_mode(&mut self, mode: String) {
        self.referential_integrity_mode = mode;
    }

    pub fn new_with_runtime_config(
        store: PostgresResourceStore,
        hooks: Vec<Arc<dyn ResourceHook>>,
        job_queue: Arc<dyn JobQueue>,
        search_engine: Arc<SearchEngine>,
        allow_update_create: bool,
        hard_delete: bool,
        runtime_config_cache: Arc<RuntimeConfigCache>,
    ) -> Self {
        let mut service = Self::new(
            store,
            hooks,
            job_queue,
            search_engine,
            allow_update_create,
            hard_delete,
        );
        service.runtime_config_cache = Some(runtime_config_cache);
        service
    }

    /// Process a FHIR batch bundle (Bundle.type = batch).
    pub async fn process_bundle(&self, bundle_json: JsonValue) -> Result<JsonValue> {
        self.process_bundle_with_options(bundle_json, BundleRequestOptions::default())
            .await
    }

    pub async fn process_bundle_with_options(
        &self,
        bundle_json: JsonValue,
        options: BundleRequestOptions,
    ) -> Result<JsonValue> {
        let bundle: Bundle = serde_json::from_value(bundle_json.clone())
            .map_err(|e| crate::Error::InvalidResource(format!("Invalid Bundle: {}", e)))?;

        if bundle.bundle_type != BundleType::Batch {
            return Err(crate::Error::InvalidResource(format!(
                "Unsupported Bundle type: {:?}. BatchService requires type 'batch'",
                bundle.bundle_type
            )));
        }

        let response_bundle = self.process_batch(bundle, &options).await?;

        if let Err(e) = self.trigger_conformance_hooks(&response_bundle).await {
            tracing::warn!("Failed to trigger conformance hooks: {}", e);
        }

        let affected_resources = self.collect_affected_resources(&response_bundle);
        if !affected_resources.is_empty() {
            if let Err(e) = self.queue_indexing_jobs(affected_resources).await {
                tracing::warn!("Failed to queue indexing jobs: {}", e);
            }
        }

        serde_json::to_value(response_bundle).map_err(|e| {
            crate::Error::Internal(format!("Failed to serialize batch response bundle: {}", e))
        })
    }

    async fn process_batch(
        &self,
        bundle: Bundle,
        options: &BundleRequestOptions,
    ) -> Result<Bundle> {
        let entries = bundle.entry.unwrap_or_default();
        let mut response_entries = vec![default_bundle_entry(); entries.len()];

        if entries.is_empty() {
            return Ok(Bundle {
                resource_type: "Bundle".to_string(),
                id: Some(Uuid::new_v4().to_string()),
                bundle_type: BundleType::BatchResponse,
                timestamp: None,
                total: None,
                link: None,
                entry: Some(vec![]),
                signature: None,
                extensions: HashMap::new(),
            });
        }

        // Pre-validate interdependencies (SHOULD per spec).
        let pre_errors = validate_batch_independence(&entries)?;

        // Process in the same canonical order as transactions, even though batch
        // entries must not have interdependencies.
        let (delete_indices, post_indices, put_patch_indices, get_indices) =
            partition_entries_for_processing(&entries)?;
        let mut ordered = Vec::with_capacity(entries.len());
        ordered.extend(delete_indices);
        ordered.extend(post_indices);
        ordered.extend(put_patch_indices);
        ordered.extend(get_indices);

        let mut crud = if let Some(cache) = &self.runtime_config_cache {
            CrudService::new_with_policy_and_runtime_config(
                self.store.clone(),
                self.allow_update_create,
                self.hard_delete,
                cache.clone(),
            )
        } else {
            CrudService::new_with_policy(
                self.store.clone(),
                self.allow_update_create,
                self.hard_delete,
            )
        };
        crud.set_referential_integrity_mode(self.referential_integrity_mode.clone());

        for index in ordered {
            if let Some(err) = pre_errors.get(&index) {
                response_entries[index] =
                    create_error_entry(entries[index].full_url.as_deref(), err);
                continue;
            }

            let entry = &entries[index];
            let response_entry = match self
                .process_entry(
                    &mut crud,
                    entry,
                    index,
                    options.prefer_return,
                    options.base_url.as_deref(),
                )
                .await
            {
                Ok(e) => e,
                Err(err) => create_error_entry(entry.full_url.as_deref(), &err),
            };

            response_entries[index] = response_entry;
        }

        Ok(Bundle {
            resource_type: "Bundle".to_string(),
            id: Some(Uuid::new_v4().to_string()),
            bundle_type: BundleType::BatchResponse,
            timestamp: None,
            total: None,
            link: None,
            entry: Some(response_entries),
            signature: None,
            extensions: HashMap::new(),
        })
    }

    async fn process_entry(
        &self,
        crud: &mut CrudService,
        entry: &BundleEntry,
        index: usize,
        prefer_return: PreferReturn,
        base_url: Option<&str>,
    ) -> Result<BundleEntry> {
        let request = entry.request.as_ref().ok_or_else(|| {
            crate::Error::InvalidResource(format!("Batch entry {} missing request", index))
        })?;

        let method = request.method.to_uppercase();
        let parsed_url = ParsedUrl::parse(&request.url);
        let query_items = query_from_url(&request.url)
            .map(parse_form_urlencoded)
            .transpose()?
            .unwrap_or_default();

        match method.as_str() {
            "POST" => {
                let resource_type = parsed_url.resource_type.ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Batch entry {} POST missing resource type in request.url",
                        index
                    ))
                })?;

                let mut resource = entry.resource.clone().ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Batch entry {} POST missing resource",
                        index
                    ))
                })?;

                let result = if let Some(if_none_exist_raw) = request.if_none_exist.as_deref() {
                    let query = if_none_exist_raw.trim().trim_start_matches('?');
                    let query_items = parse_form_urlencoded(query)?;

                    match self
                        .conditional_service
                        .conditional_create(&resource_type, &query_items, base_url, false)
                        .await?
                    {
                        crate::services::conditional::ConditionalCreateResult::NoMatch => {
                            crate::services::conditional_references::resolve_conditional_references(
                                self.search_engine.as_ref(),
                                &mut resource,
                                base_url,
                            )
                            .await?;
                            crud.create_resource(&resource_type, resource, None).await?
                        }
                        crate::services::conditional::ConditionalCreateResult::MatchFound {
                            id,
                        } => {
                            let existing = crud.read_resource(&resource_type, &id).await?;

                            let outcome = serde_json::json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!(
                                        "Resource matched existing resource with ID {}",
                                        existing.id
                                    )
                                }]
                            });

                            return Ok(BundleEntry {
                                full_url: entry.full_url.clone(),
                                request: None,
                                response: Some(BundleEntryResponse {
                                    status: status_line(StatusCode::OK),
                                    location: Some(format!(
                                        "{}/{}/_history/{}",
                                        resource_type, existing.id, existing.version_id
                                    )),
                                    etag: Some(format!("W/\"{}\"", existing.version_id)),
                                    last_modified: Some(existing.last_updated.to_rfc3339()),
                                    outcome: match prefer_return {
                                        PreferReturn::OperationOutcome => Some(outcome),
                                        _ => None,
                                    },
                                    extensions: HashMap::new(),
                                }),
                                resource: match prefer_return {
                                    PreferReturn::Representation => Some(existing.resource),
                                    _ => None,
                                },
                                search: None,
                                extensions: HashMap::new(),
                            });
                        }
                    }
                } else {
                    crate::services::conditional_references::resolve_conditional_references(
                        self.search_engine.as_ref(),
                        &mut resource,
                        base_url,
                    )
                    .await?;
                    crud.create_resource(&resource_type, resource, None).await?
                };

                let status = match result.operation {
                    ResourceOperation::Created => StatusCode::CREATED,
                    ResourceOperation::NoOp => StatusCode::OK,
                    _ => StatusCode::CREATED,
                };

                Ok(BundleEntry {
                    full_url: entry.full_url.clone(),
                    request: None,
                    response: Some(BundleEntryResponse {
                        status: status_line(status),
                        location: Some(format!(
                            "{}/{}/_history/{}",
                            resource_type, result.resource.id, result.resource.version_id
                        )),
                        etag: Some(format!("W/\"{}\"", result.resource.version_id)),
                        last_modified: Some(result.resource.last_updated.to_rfc3339()),
                        outcome: match prefer_return {
                            PreferReturn::OperationOutcome => Some(serde_json::json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!(
                                        "Resource {} successfully with ID {}",
                                        match result.operation {
                                            ResourceOperation::Created => "created",
                                            ResourceOperation::NoOp => "matched existing resource",
                                            _ => "processed"
                                        },
                                        result.resource.id
                                    )
                                }]
                            })),
                            _ => None,
                        },
                        extensions: HashMap::new(),
                    }),
                    resource: match prefer_return {
                        PreferReturn::Representation => Some(result.resource.resource),
                        _ => None,
                    },
                    search: None,
                    extensions: HashMap::new(),
                })
            }
            "PUT" => {
                let resource_type = parsed_url.resource_type.ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Batch entry {} PUT missing resource type in request.url",
                        index
                    ))
                })?;

                let mut resource = entry.resource.clone().ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Batch entry {} PUT missing resource",
                        index
                    ))
                })?;

                let result = if let Some(resource_id) = parsed_url.resource_id {
                    let if_match = request.if_match.as_deref().and_then(parse_etag);
                    let update_params = if if_match.is_some() {
                        Some(UpdateParams { if_match })
                    } else {
                        None
                    };

                    crate::services::conditional_references::resolve_conditional_references(
                        self.search_engine.as_ref(),
                        &mut resource,
                        base_url,
                    )
                    .await?;
                    crud.update_resource(&resource_type, &resource_id, resource, update_params)
                        .await?
                } else {
                    // Conditional update: PUT {type}?{criteria}
                    if query_items.is_empty() {
                        return Err(crate::Error::InvalidResource(format!(
                            "Batch entry {} PUT missing resource id and conditional criteria in request.url",
                            index
                        )));
                    }

                    let id_in_body = resource
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let if_none_match = parse_if_none_match_for_conditional_update(
                        request.if_none_match.as_deref(),
                    )?;

                    let resolution = self
                        .conditional_service
                        .resolve_conditional_target(
                            crud,
                            &resource_type,
                            &query_items,
                            base_url,
                            false,
                            id_in_body.as_deref(),
                        )
                        .await?;
                    self.conditional_service
                        .check_if_none_match(
                            crud,
                            &resource_type,
                            resolution.target_id.as_deref(),
                            resolution.target_existed,
                            if_none_match,
                        )
                        .await?;

                    crate::services::conditional_references::resolve_conditional_references(
                        self.search_engine.as_ref(),
                        &mut resource,
                        base_url,
                    )
                    .await?;
                    match resolution.target_id {
                        None => {
                            // No matches and no client id: create.
                            crud.create_resource(&resource_type, resource, None).await?
                        }
                        Some(id) => {
                            // Match or client-supplied id: update (or update-as-create).
                            crud.update_resource(&resource_type, &id, resource, None)
                                .await?
                        }
                    }
                };

                let status = match result.operation {
                    ResourceOperation::Created => StatusCode::CREATED,
                    ResourceOperation::Updated => StatusCode::OK,
                    _ => StatusCode::OK,
                };

                Ok(BundleEntry {
                    full_url: entry.full_url.clone(),
                    request: None,
                    response: Some(BundleEntryResponse {
                        status: status_line(status),
                        location: Some(format!(
                            "{}/{}/_history/{}",
                            resource_type, result.resource.id, result.resource.version_id
                        )),
                        etag: Some(format!("W/\"{}\"", result.resource.version_id)),
                        last_modified: Some(result.resource.last_updated.to_rfc3339()),
                        outcome: match prefer_return {
                            PreferReturn::OperationOutcome => Some(serde_json::json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!(
                                        "Resource {} successfully with ID {}",
                                        match result.operation {
                                            ResourceOperation::Created => "created",
                                            ResourceOperation::Updated => "updated",
                                            _ => "processed"
                                        },
                                        result.resource.id
                                    )
                                }]
                            })),
                            _ => None,
                        },
                        extensions: HashMap::new(),
                    }),
                    resource: match prefer_return {
                        PreferReturn::Representation => Some(result.resource.resource),
                        _ => None,
                    },
                    search: None,
                    extensions: HashMap::new(),
                })
            }
            "PATCH" => {
                let resource_type = parsed_url.resource_type.ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Batch entry {} PATCH missing resource type in request.url",
                        index
                    ))
                })?;

                let binary = entry.resource.clone().ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Batch entry {} PATCH missing resource (Binary)",
                        index
                    ))
                })?;
                let patch = parse_json_patch_from_binary(&binary)?;

                let if_match = request.if_match.as_deref().and_then(parse_etag);
                let update_params = if if_match.is_some() {
                    Some(UpdateParams { if_match })
                } else {
                    None
                };

                let resource_id = if let Some(resource_id) = parsed_url.resource_id {
                    resource_id
                } else {
                    // Conditional patch: PATCH {type}?{criteria}
                    if query_items.is_empty() {
                        return Err(crate::Error::InvalidResource(format!(
                            "Batch entry {} PATCH missing resource id and conditional criteria in request.url",
                            index
                        )));
                    }

                    let resolution = self
                        .conditional_service
                        .resolve_conditional_target(
                            crud,
                            &resource_type,
                            &query_items,
                            base_url,
                            false,
                            None,
                        )
                        .await?;

                    let Some(id) = resolution.target_id else {
                        return Err(crate::Error::NotFound(
                            "No resources match conditional patch criteria".to_string(),
                        ));
                    };

                    id
                };

                let current = crud.read_resource(&resource_type, &resource_id).await?;
                if let Some(expected_version) = if_match {
                    if current.version_id != expected_version {
                        return Err(crate::Error::VersionConflict {
                            expected: expected_version,
                            actual: current.version_id,
                        });
                    }
                }

                let mut patched = current.resource.clone();
                json_patch::patch(&mut patched, &patch.0).map_err(|e| match e.kind {
                    PatchErrorKind::TestFailed => crate::Error::UnprocessableEntity(e.to_string()),
                    _ => crate::Error::InvalidResource(e.to_string()),
                })?;

                let obj = patched.as_object_mut().ok_or_else(|| {
                    crate::Error::InvalidResource(
                        "Patched resource must be a JSON object".to_string(),
                    )
                })?;
                obj.insert(
                    "resourceType".to_string(),
                    serde_json::json!(&resource_type),
                );
                obj.insert("id".to_string(), serde_json::json!(&resource_id));
                obj.remove("text");

                crate::services::conditional_references::resolve_conditional_references(
                    self.search_engine.as_ref(),
                    &mut patched,
                    base_url,
                )
                .await?;

                let result = crud
                    .update_resource(&resource_type, &resource_id, patched, update_params)
                    .await?;

                Ok(BundleEntry {
                    full_url: entry.full_url.clone(),
                    request: None,
                    response: Some(BundleEntryResponse {
                        status: status_line(StatusCode::OK),
                        location: Some(format!(
                            "{}/{}/_history/{}",
                            resource_type, result.resource.id, result.resource.version_id
                        )),
                        etag: Some(format!("W/\"{}\"", result.resource.version_id)),
                        last_modified: Some(result.resource.last_updated.to_rfc3339()),
                        outcome: match prefer_return {
                            PreferReturn::OperationOutcome => Some(serde_json::json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!(
                                        "Resource patched successfully with ID {}",
                                        result.resource.id
                                    )
                                }]
                            })),
                            _ => None,
                        },
                        extensions: HashMap::new(),
                    }),
                    resource: match prefer_return {
                        PreferReturn::Representation => Some(result.resource.resource),
                        _ => None,
                    },
                    search: None,
                    extensions: HashMap::new(),
                })
            }
            "GET" | "HEAD" => {
                let resource_type = parsed_url.resource_type.ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Batch entry {} GET missing resource type in request.url",
                        index
                    ))
                })?;

                let Some(resource_id) = parsed_url.resource_id else {
                    return Ok(empty_searchset_entry());
                };

                let resource = crud.read_resource(&resource_type, &resource_id).await?;

                Ok(BundleEntry {
                    full_url: Some(format!("{}/{}", resource_type, resource_id)),
                    request: None,
                    response: Some(BundleEntryResponse {
                        status: status_line(StatusCode::OK),
                        location: None,
                        etag: Some(format!("W/\"{}\"", resource.version_id)),
                        last_modified: Some(resource.last_updated.to_rfc3339()),
                        outcome: match prefer_return {
                            PreferReturn::OperationOutcome if method != "HEAD" => {
                                Some(serde_json::json!({
                                    "resourceType": "OperationOutcome",
                                    "issue": [{
                                        "severity": "information",
                                        "code": "informational",
                                        "diagnostics": format!(
                                            "Read successful: {}/{}",
                                            resource_type, resource_id
                                        )
                                    }]
                                }))
                            }
                            _ => None,
                        },
                        extensions: HashMap::new(),
                    }),
                    resource: if method == "HEAD" {
                        None
                    } else {
                        match prefer_return {
                            PreferReturn::Representation => Some(resource.resource),
                            _ => None,
                        }
                    },
                    search: None,
                    extensions: HashMap::new(),
                })
            }
            "DELETE" => {
                let (resource_type, resource_id) = match (
                    parsed_url.resource_type,
                    parsed_url.resource_id,
                ) {
                    (Some(rt), Some(id)) => (rt, id),
                    (rt_opt, None) => {
                        // Conditional delete: DELETE {type}?{criteria} or DELETE ?{criteria}
                        if query_items.is_empty() {
                            return Err(crate::Error::InvalidResource(format!(
                                "Batch entry {} DELETE missing resource id and conditional criteria in request.url",
                                index
                            )));
                        }

                        let Some(resolved_rt) = rt_opt else {
                            return Err(crate::Error::InvalidResource(format!(
                                "Batch entry {} conditional DELETE requires a resource type in request.url",
                                index
                            )));
                        };

                        let resolution = self
                            .conditional_service
                            .resolve_conditional_target(
                                crud,
                                &resolved_rt,
                                &query_items,
                                base_url,
                                false,
                                None,
                            )
                            .await?;

                        let Some(resolved_id) = resolution.target_id else {
                            return Err(crate::Error::NotFound(
                                "No resources match conditional delete criteria".to_string(),
                            ));
                        };

                        (resolved_rt, resolved_id)
                    }
                    (None, Some(_)) => {
                        unreachable!("ParsedUrl cannot have id without resource type")
                    }
                };

                if let Some(expected_version) = request.if_match.as_deref().and_then(parse_etag) {
                    let current = crud.read_resource(&resource_type, &resource_id).await?;
                    if current.version_id != expected_version {
                        return Err(crate::Error::VersionConflict {
                            expected: expected_version,
                            actual: current.version_id,
                        });
                    }
                }

                let version_id = crud.delete_resource(&resource_type, &resource_id).await?;

                Ok(BundleEntry {
                    full_url: entry.full_url.clone(),
                    request: None,
                    response: Some(BundleEntryResponse {
                        status: status_line(StatusCode::NO_CONTENT),
                        location: None,
                        etag: version_id.map(|v| format!("W/\"{}\"", v)),
                        last_modified: None,
                        outcome: match prefer_return {
                            PreferReturn::OperationOutcome => Some(serde_json::json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!(
                                        "Resource deleted successfully: {}/{}",
                                        resource_type, resource_id
                                    )
                                }]
                            })),
                            _ => None,
                        },
                        extensions: HashMap::new(),
                    }),
                    resource: None,
                    search: None,
                    extensions: HashMap::new(),
                })
            }
            _ => Err(crate::Error::InvalidResource(format!(
                "Batch entry {} has unsupported method {}",
                index, method
            ))),
        }
    }

    fn collect_affected_resources(&self, bundle: &Bundle) -> HashMap<String, Vec<String>> {
        let mut resources: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(entries) = &bundle.entry {
            for entry in entries {
                if let Some(response) = &entry.response {
                    let status = response.status.as_str();
                    if status.starts_with("200") || status.starts_with("201") {
                        let parsed_from_location = response.location.as_ref().and_then(|loc| {
                            let parts: Vec<&str> =
                                loc.split('/').filter(|s| !s.is_empty()).collect();
                            match (parts.first(), parts.get(1)) {
                                (Some(rt), Some(id)) => Some((rt.to_string(), id.to_string())),
                                _ => None,
                            }
                        });

                        let parsed_from_resource = entry.resource.as_ref().and_then(|r| {
                            let rt = r.get("resourceType")?.as_str()?.to_string();
                            let id = r.get("id")?.as_str()?.to_string();
                            Some((rt, id))
                        });

                        if let Some((rt, id)) = parsed_from_location.or(parsed_from_resource) {
                            resources.entry(rt).or_default().push(id);
                        }
                    }
                }
            }
        }

        resources
    }

    async fn queue_indexing_jobs(&self, resources: HashMap<String, Vec<String>>) -> Result<()> {
        for (resource_type, resource_ids) in resources {
            if resource_ids.is_empty() {
                continue;
            }

            let parameters = json!({
                "resource_type": resource_type,
                "resource_ids": resource_ids,
            });

            if let Err(e) = self
                .job_queue
                .enqueue(
                    "index_search".to_string(),
                    parameters,
                    JobPriority::Normal,
                    None,
                )
                .await
            {
                tracing::warn!("Failed to queue indexing job: {}", e);
            }
        }

        Ok(())
    }

    async fn trigger_conformance_hooks(&self, bundle: &Bundle) -> Result<()> {
        use crate::models::Resource;

        let mut conformance_ids: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(entries) = &bundle.entry {
            for entry in entries {
                if let Some(response) = &entry.response {
                    let status = response.status.as_str();
                    if !(status.starts_with("200") || status.starts_with("201")) {
                        continue;
                    }

                    let parsed = response.location.as_ref().and_then(|loc| {
                        let parts: Vec<&str> = loc.split('/').filter(|s| !s.is_empty()).collect();
                        match (parts.first(), parts.get(1)) {
                            (Some(rt), Some(id)) => Some((rt.to_string(), id.to_string())),
                            _ => None,
                        }
                    });

                    let Some((rt, id)) = parsed else {
                        continue;
                    };

                    if crate::conformance::is_conformance_resource_type(&rt) {
                        conformance_ids.entry(rt).or_default().push(id);
                    }
                }
            }
        }

        if conformance_ids.is_empty() {
            return Ok(());
        }

        let mut all_conformance: Vec<Resource> = Vec::new();
        for (resource_type, ids) in conformance_ids {
            let resources = self
                .store
                .load_resources_batch(&resource_type, &ids)
                .await?;
            all_conformance.extend(resources);
        }

        for resource in &all_conformance {
            for hook in &self.hooks {
                if let Err(e) = hook.on_updated(resource).await {
                    tracing::warn!(
                        "Hook failed for conformance resource {}/{}: {}",
                        resource.resource_type,
                        resource.id,
                        e
                    );
                }
            }
        }

        Ok(())
    }
}

// =============================================================================
// Batch validation (no interdependency + no resolution)
// =============================================================================

fn validate_batch_independence(entries: &[BundleEntry]) -> Result<HashMap<usize, crate::Error>> {
    let mut errors: HashMap<usize, crate::Error> = HashMap::new();

    // 0) References to resources created within the same batch are non-conformant.
    let created_full_urls: HashSet<String> = entries
        .iter()
        .filter_map(|e| {
            let method = e
                .request
                .as_ref()
                .map(|r| r.method.to_uppercase())
                .unwrap_or_default();
            if method == "POST" {
                e.full_url.clone()
            } else {
                None
            }
        })
        .collect();

    if !created_full_urls.is_empty() {
        for (idx, entry) in entries.iter().enumerate() {
            let Some(resource) = &entry.resource else {
                continue;
            };
            if resource_references_any_fullurl(resource, &created_full_urls) {
                errors.insert(
                    idx,
                    crate::Error::InvalidResource(
                        "References between resources in a batch are non-conformant (no resolution)"
                            .to_string(),
                    ),
                );
            }
        }
    }

    // 1) Change interdependencies (PUT/PATCH/DELETE on same identity) are non-conformant.
    let mut mutating_identities: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        let Some(request) = &entry.request else {
            continue;
        };
        let method = request.method.to_uppercase();
        if !(method == "PUT" || method == "PATCH" || method == "DELETE") {
            continue;
        }

        if let Some(identity) = ParsedUrl::parse(&request.url).identity() {
            mutating_identities.entry(identity).or_default().push(idx);
        }
    }

    for (identity, indices) in mutating_identities {
        if indices.len() <= 1 {
            continue;
        }

        for idx in indices {
            errors.entry(idx).or_insert_with(|| {
                crate::Error::InvalidResource(format!(
                    "Batch interdependency detected: multiple change interactions target {}",
                    identity
                ))
            });
        }
    }

    Ok(errors)
}

fn resource_references_any_fullurl(resource: &JsonValue, full_urls: &HashSet<String>) -> bool {
    match resource {
        JsonValue::Object(map) => {
            if let Some(JsonValue::String(reference)) = map.get("reference") {
                if full_urls.contains(reference) {
                    return true;
                }
            }
            map.values()
                .any(|v| resource_references_any_fullurl(v, full_urls))
        }
        JsonValue::Array(arr) => arr
            .iter()
            .any(|v| resource_references_any_fullurl(v, full_urls)),
        _ => false,
    }
}

fn partition_entries_for_processing(
    entries: &[BundleEntry],
) -> Result<(Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>)> {
    let mut delete_indices = Vec::new();
    let mut post_indices = Vec::new();
    let mut put_patch_indices = Vec::new();
    let mut get_indices = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        let request = entry.request.as_ref().ok_or_else(|| {
            crate::Error::InvalidResource(format!("Batch entry {} missing request", index))
        })?;

        let method = request.method.to_uppercase();
        match method.as_str() {
            "DELETE" => delete_indices.push(index),
            "POST" => post_indices.push(index),
            "PUT" | "PATCH" => put_patch_indices.push(index),
            "GET" | "HEAD" => get_indices.push(index),
            _ => {
                get_indices.push(index); // Let per-entry processing produce a proper error
            }
        }
    }

    Ok((delete_indices, post_indices, put_patch_indices, get_indices))
}

// =============================================================================
// Response helpers
// =============================================================================

fn status_line(status: StatusCode) -> String {
    match status.canonical_reason() {
        Some(reason) => format!("{} {}", status.as_u16(), reason),
        None => status.as_u16().to_string(),
    }
}

fn status_to_fhir_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "invalid",
        StatusCode::NOT_FOUND => "not-found",
        StatusCode::GONE => "deleted",
        StatusCode::CONFLICT => "conflict",
        StatusCode::PRECONDITION_FAILED => "conflict",
        StatusCode::UNPROCESSABLE_ENTITY => "processing",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "not-supported",
        _ => "exception",
    }
}

fn error_status(err: &crate::Error) -> StatusCode {
    match err {
        crate::Error::ResourceNotFound { .. } => StatusCode::NOT_FOUND,
        crate::Error::VersionNotFound { .. } => StatusCode::NOT_FOUND,
        crate::Error::NotFound(_) => StatusCode::NOT_FOUND,
        crate::Error::ResourceDeleted { .. } => StatusCode::GONE,
        crate::Error::InvalidResource(_)
        | crate::Error::Validation(_)
        | crate::Error::InvalidReference(_) => StatusCode::BAD_REQUEST,
        crate::Error::BusinessRule(_) => StatusCode::CONFLICT,
        crate::Error::VersionConflict { .. } | crate::Error::PreconditionFailed(_) => {
            StatusCode::PRECONDITION_FAILED
        }
        crate::Error::MethodNotAllowed(_) => StatusCode::METHOD_NOT_ALLOWED,
        crate::Error::Search(_) => StatusCode::BAD_REQUEST,
        crate::Error::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        crate::Error::UnprocessableEntity(_) => StatusCode::UNPROCESSABLE_ENTITY,
        crate::Error::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
        crate::Error::TooCostly(_) => StatusCode::FORBIDDEN,
        crate::Error::Database(_)
        | crate::Error::JobQueue(_)
        | crate::Error::FhirContext(_)
        | crate::Error::FhirPath(_)
        | crate::Error::ExternalReference(_)
        | crate::Error::Internal(_)
        | crate::Error::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn create_error_entry(full_url: Option<&str>, err: &crate::Error) -> BundleEntry {
    let status = error_status(err);
    let outcome = json!({
        "resourceType": "OperationOutcome",
        "issue": [{
            "severity": "error",
            "code": status_to_fhir_code(status),
            "diagnostics": err.to_string()
        }]
    });

    BundleEntry {
        full_url: full_url.map(|s| s.to_string()),
        request: None,
        response: Some(BundleEntryResponse {
            status: status_line(status),
            location: None,
            etag: None,
            last_modified: None,
            outcome: Some(outcome),
            extensions: HashMap::new(),
        }),
        resource: None,
        search: None,
        extensions: HashMap::new(),
    }
}

fn default_bundle_entry() -> BundleEntry {
    BundleEntry {
        full_url: None,
        request: None,
        response: None,
        resource: None,
        search: None,
        extensions: HashMap::new(),
    }
}

fn empty_searchset_entry() -> BundleEntry {
    BundleEntry {
        full_url: None,
        request: None,
        response: Some(BundleEntryResponse {
            status: status_line(StatusCode::OK),
            location: None,
            etag: None,
            last_modified: None,
            outcome: None,
            extensions: HashMap::new(),
        }),
        resource: Some(json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 0,
            "entry": []
        })),
        search: None,
        extensions: HashMap::new(),
    }
}

// =============================================================================
// JSON Patch payload (Binary) helper
// =============================================================================

use base64::{engine::general_purpose::STANDARD, Engine as _};

fn parse_json_patch_from_binary(binary: &JsonValue) -> Result<json_patch::Patch> {
    let resource_type = binary
        .get("resourceType")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if resource_type != "Binary" {
        return Err(crate::Error::InvalidResource(
            "Batch PATCH requires a Binary resource payload".to_string(),
        ));
    }

    let content_type = binary
        .get("contentType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if content_type != "application/json-patch+json" {
        return Err(crate::Error::UnsupportedMediaType(format!(
            "Unsupported PATCH Binary.contentType '{}'. Supported: application/json-patch+json",
            content_type
        )));
    }

    let data_b64 = binary
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::Error::InvalidResource("Binary.data missing".to_string()))?;

    let bytes = STANDARD.decode(data_b64).map_err(|e| {
        crate::Error::InvalidResource(format!("Invalid base64 in Binary.data: {}", e))
    })?;

    serde_json::from_slice::<json_patch::Patch>(&bytes)
        .map_err(|e| crate::Error::InvalidResource(format!("Invalid JSON Patch document: {}", e)))
}

// =============================================================================
// URL parsing
// =============================================================================

#[derive(Debug, Clone)]
struct ParsedUrl {
    resource_type: Option<String>,
    resource_id: Option<String>,
}

impl ParsedUrl {
    fn parse(raw: &str) -> Self {
        let mut path = raw;

        if let Some((p, _q)) = path.split_once('?') {
            path = p;
        }

        if let Some(scheme_idx) = path.find("://") {
            let after_scheme = &path[scheme_idx + 3..];
            path = after_scheme.split_once('/').map(|(_, p)| p).unwrap_or("");
        }

        let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if let Some(history_idx) = parts.iter().position(|p| *p == "_history") {
            parts.truncate(history_idx);
        }

        match parts.len() {
            0 => Self {
                resource_type: None,
                resource_id: None,
            },
            1 => Self {
                resource_type: parts.last().map(|s| s.to_string()),
                resource_id: None,
            },
            _ => Self {
                resource_type: parts.get(parts.len() - 2).map(|s| s.to_string()),
                resource_id: parts.last().map(|s| s.to_string()),
            },
        }
    }

    fn identity(&self) -> Option<String> {
        match (&self.resource_type, &self.resource_id) {
            (Some(rt), Some(id)) => Some(format!("{}/{}", rt, id)),
            _ => None,
        }
    }
}

// fhir_models BundleEntryResponse doesn't include ResourceOperation; use crate model.
use crate::models::ResourceOperation;
