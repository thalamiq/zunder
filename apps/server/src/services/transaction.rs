//! Transaction processing service
//!
//! Implements FHIR transaction bundle processing per specification:
//! - 3.2.0.13.2 Transaction Processing Rules
//! - 3.2.0.13.3 Replacing hyperlinks and full-urls

use crate::{
    api::headers::parse_etag,
    db::{
        PostgresResourceStore, PostgresTransactionContext, ResourceTransaction, TransactionContext,
    },
    hooks::ResourceHook,
    runtime_config::{ConfigKey, RuntimeConfigCache},
    services::IndexingService,
    Result,
};
use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use serde_json::{json, Value as JsonValue};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use zunder_context::FhirContext;
use zunder_models::{Bundle, BundleEntry, BundleEntryResponse, BundleType, StructureDefinition};
use uuid::Uuid;

use super::batch::{BundleRequestOptions, PreferReturn};
use crate::db::search::engine::SearchEngine;
use crate::services::conditional::{
    build_conditional_search_params_from_items, parse_form_urlencoded,
    parse_if_none_match_for_conditional_update, query_from_url, ConditionalService,
};

const EXT_RESOLVE_AS_VERSION_SPECIFIC: &str =
    "http://hl7.org/fhir/StructureDefinition/resolve-as-version-specific";

#[derive(Debug, Default)]
struct TransactionIndexingActions {
    upserted: HashMap<String, Vec<String>>,
    deleted: HashMap<String, Vec<String>>,
}

impl TransactionIndexingActions {
    fn add_deleted(&mut self, resource_type: &str, id: &str) {
        self.deleted
            .entry(resource_type.to_string())
            .or_default()
            .push(id.to_string());
        self.dedupe();
    }

    fn add_written(&mut self, written: &[WrittenResource]) {
        for w in written {
            self.upserted
                .entry(w.resource_type.clone())
                .or_default()
                .push(w.id.clone());
        }
        self.dedupe();
    }

    fn dedupe(&mut self) {
        for ids in self.upserted.values_mut() {
            ids.sort();
            ids.dedup();
        }
        for ids in self.deleted.values_mut() {
            ids.sort();
            ids.dedup();
        }
    }
}

pub struct TransactionService {
    store: PostgresResourceStore,
    hooks: Vec<Arc<dyn ResourceHook>>,
    indexing_service: Arc<IndexingService>,
    fhir_context: Arc<dyn FhirContext>,
    search_engine: Arc<SearchEngine>,
    conditional_service: ConditionalService,
    allow_update_create: bool,
    hard_delete: bool,
    runtime_config_cache: Option<Arc<RuntimeConfigCache>>,
    referential_integrity_mode: String,
}

impl TransactionService {
    pub fn new(
        store: PostgresResourceStore,
        hooks: Vec<Arc<dyn ResourceHook>>,
        indexing_service: Arc<IndexingService>,
        fhir_context: Arc<dyn FhirContext>,
        search_engine: Arc<SearchEngine>,
        allow_update_create: bool,
        hard_delete: bool,
    ) -> Self {
        Self {
            store,
            hooks,
            indexing_service,
            fhir_context,
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
        indexing_service: Arc<IndexingService>,
        fhir_context: Arc<dyn FhirContext>,
        search_engine: Arc<SearchEngine>,
        allow_update_create: bool,
        hard_delete: bool,
        runtime_config_cache: Arc<RuntimeConfigCache>,
    ) -> Self {
        let mut service = Self::new(
            store,
            hooks,
            indexing_service,
            fhir_context,
            search_engine,
            allow_update_create,
            hard_delete,
        );
        service.runtime_config_cache = Some(runtime_config_cache);
        service
    }

    async fn allow_update_create_effective(&self) -> bool {
        if let Some(cache) = &self.runtime_config_cache {
            return cache.get(ConfigKey::BehaviorAllowUpdateCreate).await;
        }
        self.allow_update_create
    }

    async fn hard_delete_effective(&self) -> bool {
        if let Some(cache) = &self.runtime_config_cache {
            return cache.get(ConfigKey::BehaviorHardDelete).await;
        }
        self.hard_delete
    }

    pub async fn process_bundle_with_options(
        &self,
        bundle_json: JsonValue,
        options: BundleRequestOptions,
    ) -> Result<JsonValue> {
        tracing::debug!("Transaction service: Deserializing bundle JSON");
        let bundle: Bundle = serde_json::from_value(bundle_json.clone())
            .map_err(|e| crate::Error::InvalidResource(format!("Invalid Bundle: {}", e)))?;

        if bundle.bundle_type != BundleType::Transaction {
            return Err(crate::Error::InvalidResource(format!(
                "Unsupported Bundle type: {:?}. TransactionService requires type 'transaction'",
                bundle.bundle_type
            )));
        }

        let (response_bundle, indexing_actions) =
            self.process_transaction(bundle, &options).await?;

        // Only after successful commit: trigger hooks + inline indexing
        if let Err(e) = self.trigger_conformance_hooks(&response_bundle).await {
            tracing::warn!("Failed to trigger conformance hooks: {}", e);
        }

        if let Err(e) = self
            .apply_inline_indexing(&response_bundle, indexing_actions)
            .await
        {
            tracing::warn!("Failed to apply inline transaction indexing: {}", e);
        }

        serde_json::to_value(response_bundle).map_err(|e| {
            crate::Error::Internal(format!(
                "Failed to serialize transaction response bundle: {}",
                e
            ))
        })
    }

    async fn process_transaction(
        &self,
        bundle: Bundle,
        options: &BundleRequestOptions,
    ) -> Result<(Bundle, TransactionIndexingActions)> {
        tracing::debug!("process_transaction: Starting transaction processing");
        let entries = bundle.entry.unwrap_or_default();
        tracing::debug!("process_transaction: Got {} entries", entries.len());

        // It's not an error for a transaction to have no resources.
        if entries.is_empty() {
            return Ok((
                Bundle {
                    resource_type: "Bundle".to_string(),
                    id: Some(Uuid::new_v4().to_string()),
                    bundle_type: BundleType::TransactionResponse,
                    timestamp: None,
                    total: None,
                    link: None,
                    entry: Some(vec![]),
                    signature: None,
                    extensions: HashMap::new(),
                },
                TransactionIndexingActions::default(),
            ));
        }

        tracing::debug!("process_transaction: Validating bundle");
        validate_transaction_bundle(&entries)?;

        tracing::debug!("process_transaction: Partitioning entries");
        let (delete_indices, post_indices, put_patch_indices, get_indices) =
            partition_transaction_entries(&entries)?;

        tracing::debug!("process_transaction: Checking identity overlaps");
        check_identity_overlaps(&entries, &delete_indices, &post_indices, &put_patch_indices)?;

        tracing::debug!("process_transaction: Creating URL rewriter");
        let mut url_rewriter = UrlRewriter::new(self.fhir_context.clone());
        tracing::debug!("process_transaction: Seeding non-POST mappings");
        url_rewriter.seed_non_post_mappings(&entries)?;
        tracing::debug!("process_transaction: Reserving POST IDs");
        url_rewriter.reserve_post_ids(&entries, &post_indices)?;
        tracing::debug!("process_transaction: URL rewriter setup complete");

        let mut response_entries = vec![default_bundle_entry(); entries.len()];
        let mut produced_versions: HashMap<String, i32> = HashMap::new();
        let mut written_resources: Vec<WrittenResource> = Vec::new();
        let mut indexing_actions = TransactionIndexingActions::default();
        let mut tx = self.store.begin_transaction().await?;

        // Process in required order
        for &index in &delete_indices {
            match self
                .process_entry(
                    &mut tx,
                    &entries[index],
                    index,
                    &mut url_rewriter,
                    &mut produced_versions,
                    &mut written_resources,
                    &mut indexing_actions,
                    options.prefer_return,
                    options.base_url.as_deref(),
                )
                .await
            {
                Ok(response) => response_entries[index] = response,
                Err(err) => {
                    let _ = tx.rollback().await;
                    return Err(with_entry_context(err, index));
                }
            }
        }

        for &index in &post_indices {
            match self
                .process_entry(
                    &mut tx,
                    &entries[index],
                    index,
                    &mut url_rewriter,
                    &mut produced_versions,
                    &mut written_resources,
                    &mut indexing_actions,
                    options.prefer_return,
                    options.base_url.as_deref(),
                )
                .await
            {
                Ok(response) => response_entries[index] = response,
                Err(err) => {
                    let _ = tx.rollback().await;
                    return Err(with_entry_context(err, index));
                }
            }
        }

        for &index in &put_patch_indices {
            match self
                .process_entry(
                    &mut tx,
                    &entries[index],
                    index,
                    &mut url_rewriter,
                    &mut produced_versions,
                    &mut written_resources,
                    &mut indexing_actions,
                    options.prefer_return,
                    options.base_url.as_deref(),
                )
                .await
            {
                Ok(response) => response_entries[index] = response,
                Err(err) => {
                    let _ = tx.rollback().await;
                    return Err(with_entry_context(err, index));
                }
            }
        }

        // 3.2.0.13.4 Version specific references and updates:
        // Upgrade versionless references that requested resolve-as-version-specific to
        // version-specific references to the versions produced by this transaction.
        finalize_version_specific_references(
            &mut tx,
            &produced_versions,
            &mut written_resources,
            &mut response_entries,
        )
        .await?;

        for &index in &get_indices {
            match self
                .process_entry(
                    &mut tx,
                    &entries[index],
                    index,
                    &mut url_rewriter,
                    &mut produced_versions,
                    &mut written_resources,
                    &mut indexing_actions,
                    options.prefer_return,
                    options.base_url.as_deref(),
                )
                .await
            {
                Ok(response) => response_entries[index] = response,
                Err(err) => {
                    let _ = tx.rollback().await;
                    return Err(with_entry_context(err, index));
                }
            }
        }

        tx.commit().await?;

        indexing_actions.add_written(&written_resources);

        Ok((
            Bundle {
                resource_type: "Bundle".to_string(),
                id: Some(Uuid::new_v4().to_string()),
                bundle_type: BundleType::TransactionResponse,
                timestamp: None,
                total: None,
                link: None,
                entry: Some(response_entries),
                signature: None,
                extensions: HashMap::new(),
            },
            indexing_actions,
        ))
    }

    async fn process_entry(
        &self,
        tx: &mut PostgresTransactionContext,
        entry: &BundleEntry,
        index: usize,
        url_rewriter: &mut UrlRewriter,
        produced_versions: &mut HashMap<String, i32>,
        written_resources: &mut Vec<WrittenResource>,
        indexing_actions: &mut TransactionIndexingActions,
        prefer_return: PreferReturn,
        base_url: Option<&str>,
    ) -> Result<BundleEntry> {
        // Build known IDs from resources already written in this transaction
        let known_ids: std::collections::HashSet<(String, String)> = written_resources
            .iter()
            .map(|w| (w.resource_type.clone(), w.id.clone()))
            .collect();
        let request = entry.request.as_ref().ok_or_else(|| {
            crate::Error::InvalidResource(format!("Transaction entry {} missing request", index))
        })?;

        let method = request.method.to_uppercase();
        let parsed_url = ParsedUrl::parse(&request.url);
        let query_items = query_from_url(&request.url)
            .map(parse_form_urlencoded)
            .transpose()?
            .unwrap_or_default();

        match method.as_str() {
            "DELETE" => {
                let (resource_type, resource_id, is_conditional) = match (
                    parsed_url.resource_type,
                    parsed_url.resource_id,
                ) {
                    (Some(rt), Some(id)) => (rt, id, false),
                    (rt_opt, None) => {
                        // Conditional delete: DELETE {type}?{criteria} or DELETE ?{criteria}
                        if query_items.is_empty() {
                            return Err(crate::Error::InvalidResource(format!(
                                "Transaction entry {} DELETE missing resource id and conditional criteria in request.url",
                                index
                            )));
                        }

                        let Some(resolved_rt) = rt_opt else {
                            return Err(crate::Error::InvalidResource(format!(
                                "Transaction entry {} conditional DELETE requires a resource type in request.url",
                                index
                            )));
                        };

                        let search_params =
                            build_conditional_search_params_from_items(&query_items)?;
                        let search_result = {
                            let conn = tx.tx_mut()?;
                            self.search_engine
                                .search_with_connection(
                                    conn,
                                    Some(&resolved_rt),
                                    &search_params,
                                    base_url,
                                )
                                .await?
                        };

                        let resolution = self
                            .conditional_service
                            .resolve_conditional_target_from_matches(
                                tx,
                                &resolved_rt,
                                None,
                                &search_result.resources,
                            )
                            .await?;

                        let Some(resolved_id) = resolution.target_id else {
                            return Err(crate::Error::NotFound(
                                "No resources match conditional delete criteria".to_string(),
                            ));
                        };

                        (resolved_rt, resolved_id, true)
                    }
                    (None, Some(_)) => {
                        unreachable!("ParsedUrl cannot have id without resource type")
                    }
                };

                if let Some(expected_version) = request.if_match.as_deref().and_then(parse_etag) {
                    let current =
                        tx.read(&resource_type, &resource_id)
                            .await?
                            .ok_or_else(|| crate::Error::ResourceNotFound {
                                resource_type: resource_type.clone(),
                                id: resource_id.clone(),
                            })?;
                    if current.version_id != expected_version {
                        return Err(crate::Error::VersionConflict {
                            expected: expected_version,
                            actual: current.version_id,
                        });
                    }
                }

                // Referential integrity check on delete (strict mode)
                if self.is_strict_referential_integrity() {
                    let existing_check = tx.read(&resource_type, &resource_id).await?;
                    if let Some(ref e) = existing_check {
                        if !e.deleted {
                            self.validate_no_references_to(&resource_type, &resource_id).await?;
                        }
                    }
                }

                let hard_delete = self.hard_delete_effective().await;
                let existing = tx.read(&resource_type, &resource_id).await?;
                let version_id = match existing {
                    None => {
                        if is_conditional {
                            return Err(crate::Error::ResourceNotFound {
                                resource_type: resource_type.clone(),
                                id: resource_id.clone(),
                            });
                        }
                        None
                    }
                    Some(existing) if hard_delete => {
                        let version_id = existing.version_id;
                        let _ = tx.hard_delete(&resource_type, &resource_id).await?;
                        Some(version_id)
                    }
                    Some(existing) if existing.deleted => Some(existing.version_id),
                    Some(_) => Some(tx.delete(&resource_type, &resource_id).await?),
                };

                if version_id.is_some() {
                    indexing_actions.add_deleted(&resource_type, &resource_id);
                }

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
            "POST" => {
                tracing::debug!("Processing POST entry {} for resource", index);
                let resource_type = parsed_url.resource_type.ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Transaction entry {} POST missing resource type in request.url",
                        index
                    ))
                })?;

                let mut resource = entry.resource.clone().ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Transaction entry {} POST missing resource",
                        index
                    ))
                })?;

                tracing::debug!("Rewriting resource for entry {}", index);
                url_rewriter.rewrite_resource(&mut resource)?;
                self.resolve_conditional_references_in_transaction(tx, &mut resource, base_url)
                    .await?;
                tracing::debug!("Resource rewrite complete for entry {}", index);

                if let Some(if_none_exist_raw) = request.if_none_exist.as_deref() {
                    tracing::debug!(
                        "Processing conditional create for entry {} with if_none_exist: {}",
                        index,
                        if_none_exist_raw
                    );
                    let query = if_none_exist_raw.trim().trim_start_matches('?');
                    let query_items = parse_form_urlencoded(query)?;
                    if query_items.is_empty() {
                        return Err(crate::Error::Validation(
                            "Transaction conditional create requires If-None-Exist search parameters"
                                .to_string(),
                        ));
                    }

                    let search_params = build_conditional_search_params_from_items(&query_items)?;
                    tracing::debug!(
                        "Starting conditional search for entry {} with params: {:?}",
                        index,
                        search_params
                    );
                    let search_result = {
                        let conn = tx.tx_mut()?;
                        tracing::debug!(
                            "Got transaction connection, calling search_with_connection for entry {}",
                            index
                        );
                        let result = self
                            .search_engine
                            .search_with_connection(
                                conn,
                                Some(&resource_type),
                                &search_params,
                                base_url,
                            )
                            .await?;
                        tracing::debug!(
                            "Search completed for entry {}, found {} results",
                            index,
                            result.resources.len()
                        );
                        result
                    };

                    match self
                        .conditional_service
                        .conditional_create_from_matches(&search_result.resources)?
                    {
                        crate::services::conditional::ConditionalCreateResult::NoMatch => {
                            /* proceed with create below */
                        }
                        crate::services::conditional::ConditionalCreateResult::MatchFound {
                            id,
                        } => {
                            let existing =
                                tx.read(&resource_type, &id).await?.ok_or_else(|| {
                                    crate::Error::ResourceNotFound {
                                        resource_type: resource_type.clone(),
                                        id: id.clone(),
                                    }
                                })?;

                            if let Some(full_url) = &entry.full_url {
                                url_rewriter
                                    .mapping
                                    .insert(full_url.clone(), format!("{}/{}", resource_type, &id));
                            }

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
                }

                let id = url_rewriter.reserved_post_id(index).ok_or_else(|| {
                    crate::Error::Internal("Missing reserved POST id".to_string())
                })?;
                populate_meta(&mut resource, &id, 1, Utc::now());

                // Referential integrity check (strict mode)
                if self.is_strict_referential_integrity() {
                    self.validate_references_in_transaction(&resource, &known_ids).await?;
                }

                let created = tx.create(&resource_type, resource).await?;

                produced_versions.insert(
                    format!("{}/{}", resource_type, created.id),
                    created.version_id,
                );
                written_resources.push(WrittenResource {
                    entry_index: index,
                    resource_type: resource_type.clone(),
                    id: created.id.clone(),
                    version_id: created.version_id,
                    resource: created.resource.clone(),
                });

                Ok(BundleEntry {
                    full_url: entry.full_url.clone(),
                    request: None,
                    response: Some(BundleEntryResponse {
                        status: status_line(StatusCode::CREATED),
                        location: Some(format!(
                            "{}/{}/_history/{}",
                            resource_type, created.id, created.version_id
                        )),
                        etag: Some(format!("W/\"{}\"", created.version_id)),
                        last_modified: Some(created.last_updated.to_rfc3339()),
                        outcome: match prefer_return {
                            PreferReturn::OperationOutcome => Some(serde_json::json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!(
                                        "Resource created successfully with ID {}",
                                        created.id
                                    )
                                }]
                            })),
                            _ => None,
                        },
                        extensions: HashMap::new(),
                    }),
                    resource: match prefer_return {
                        PreferReturn::Representation => Some(created.resource),
                        _ => None,
                    },
                    search: None,
                    extensions: HashMap::new(),
                })
            }
            "PUT" => {
                let resource_type = parsed_url.resource_type.ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Transaction entry {} PUT missing resource type in request.url",
                        index
                    ))
                })?;

                let mut resource = entry.resource.clone().ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Transaction entry {} PUT missing resource",
                        index
                    ))
                })?;

                url_rewriter.rewrite_resource(&mut resource)?;
                self.resolve_conditional_references_in_transaction(tx, &mut resource, base_url)
                    .await?;

                let Some(resource_id) = parsed_url.resource_id else {
                    // Conditional update: PUT {type}?{criteria}
                    if query_items.is_empty() {
                        return Err(crate::Error::InvalidResource(format!(
                            "Transaction entry {} PUT missing resource id and conditional criteria in request.url",
                            index
                        )));
                    }

                    let search_params = build_conditional_search_params_from_items(&query_items)?;
                    let search_result = {
                        let conn = tx.tx_mut()?;
                        self.search_engine
                            .search_with_connection(
                                conn,
                                Some(&resource_type),
                                &search_params,
                                base_url,
                            )
                            .await?
                    };

                    let id_in_body = resource
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let if_none_match = parse_if_none_match_for_conditional_update(
                        request.if_none_match.as_deref(),
                    )?;

                    let resolution = self
                        .conditional_service
                        .resolve_conditional_target_from_matches(
                            tx,
                            &resource_type,
                            id_in_body.as_deref(),
                            &search_result.resources,
                        )
                        .await?;
                    self.conditional_service
                        .check_if_none_match(
                            tx,
                            &resource_type,
                            resolution.target_id.as_deref(),
                            resolution.target_existed,
                            if_none_match,
                        )
                        .await?;

                    match resolution.target_id {
                        None => {
                            let id = Uuid::new_v4().to_string();
                            populate_meta(&mut resource, &id, 1, Utc::now());
                            if let Some(obj) = resource.as_object_mut() {
                                obj.insert("resourceType".to_string(), json!(resource_type));
                                obj.insert("id".to_string(), json!(id));
                            }
                            if let Some(full_url) = &entry.full_url {
                                url_rewriter
                                    .mapping
                                    .insert(full_url.clone(), format!("{}/{}", resource_type, &id));
                            }

                            // Referential integrity check (strict mode)
                            if self.is_strict_referential_integrity() {
                                self.validate_references_in_transaction(&resource, &known_ids).await?;
                            }

                            let created = tx.create(&resource_type, resource).await?;

                            produced_versions.insert(
                                format!("{}/{}", resource_type, created.id),
                                created.version_id,
                            );
                            written_resources.push(WrittenResource {
                                entry_index: index,
                                resource_type: resource_type.clone(),
                                id: created.id.clone(),
                                version_id: created.version_id,
                                resource: created.resource.clone(),
                            });

                            return Ok(BundleEntry {
                                full_url: entry.full_url.clone(),
                                request: None,
                                response: Some(BundleEntryResponse {
                                    status: status_line(StatusCode::CREATED),
                                    location: Some(format!(
                                        "{}/{}/_history/{}",
                                        resource_type, created.id, created.version_id
                                    )),
                                    etag: Some(format!("W/\"{}\"", created.version_id)),
                                    last_modified: Some(created.last_updated.to_rfc3339()),
                                    outcome: match prefer_return {
                                        PreferReturn::OperationOutcome => Some(serde_json::json!({
                                            "resourceType": "OperationOutcome",
                                            "issue": [{
                                                "severity": "information",
                                                "code": "informational",
                                                "diagnostics": format!(
                                                    "Resource created successfully with ID {}",
                                                    created.id
                                                )
                                            }]
                                        })),
                                        _ => None,
                                    },
                                    extensions: HashMap::new(),
                                }),
                                resource: match prefer_return {
                                    PreferReturn::Representation => Some(created.resource),
                                    _ => None,
                                },
                                search: None,
                                extensions: HashMap::new(),
                            });
                        }
                        Some(id) => {
                            let current = tx.read(&resource_type, &id).await?;
                            let status = match current {
                                Some(existing) => {
                                    let new_version = existing.version_id + 1;
                                    populate_meta(&mut resource, &id, new_version, Utc::now());
                                    StatusCode::OK
                                }
                                None => {
                                    if !self.allow_update_create_effective().await {
                                        return Err(crate::Error::MethodNotAllowed(
                                            "Server does not allow client-defined resource ids. Use POST to create resources."
                                                .to_string(),
                                        ));
                                    }
                                    populate_meta(&mut resource, &id, 1, Utc::now());
                                    StatusCode::CREATED
                                }
                            };

                            if let Some(obj) = resource.as_object_mut() {
                                obj.insert("resourceType".to_string(), json!(resource_type));
                                obj.insert("id".to_string(), json!(id));
                            }
                            if let Some(full_url) = &entry.full_url {
                                url_rewriter
                                    .mapping
                                    .insert(full_url.clone(), format!("{}/{}", resource_type, &id));
                            }

                            // Referential integrity check (strict mode)
                            if self.is_strict_referential_integrity() {
                                self.validate_references_in_transaction(&resource, &known_ids).await?;
                            }

                            let updated = tx.upsert(&resource_type, &id, resource).await?;

                            produced_versions.insert(
                                format!("{}/{}", resource_type, updated.id),
                                updated.version_id,
                            );
                            written_resources.push(WrittenResource {
                                entry_index: index,
                                resource_type: resource_type.clone(),
                                id: updated.id.clone(),
                                version_id: updated.version_id,
                                resource: updated.resource.clone(),
                            });

                            return Ok(BundleEntry {
                                full_url: entry.full_url.clone(),
                                request: None,
                                response: Some(BundleEntryResponse {
                                    status: status_line(status),
                                    location: Some(format!(
                                        "{}/{}/_history/{}",
                                        resource_type, updated.id, updated.version_id
                                    )),
                                    etag: Some(format!("W/\"{}\"", updated.version_id)),
                                    last_modified: Some(updated.last_updated.to_rfc3339()),
                                    outcome: match prefer_return {
                                        PreferReturn::OperationOutcome => Some(serde_json::json!({
                                            "resourceType": "OperationOutcome",
                                            "issue": [{
                                                "severity": "information",
                                                "code": "informational",
                                                "diagnostics": format!(
                                                    "Resource {} successfully with ID {}",
                                                    if status == StatusCode::CREATED {
                                                        "created"
                                                    } else {
                                                        "updated"
                                                    },
                                                    updated.id
                                                )
                                            }]
                                        })),
                                        _ => None,
                                    },
                                    extensions: HashMap::new(),
                                }),
                                resource: match prefer_return {
                                    PreferReturn::Representation => Some(updated.resource),
                                    _ => None,
                                },
                                search: None,
                                extensions: HashMap::new(),
                            });
                        }
                    }
                };

                // If-Match (version aware update)
                if let Some(if_match) = &request.if_match {
                    if let Some(expected) = parse_etag(if_match) {
                        let current =
                            tx.read(&resource_type, &resource_id)
                                .await?
                                .ok_or_else(|| crate::Error::ResourceNotFound {
                                    resource_type: resource_type.clone(),
                                    id: resource_id.clone(),
                                })?;
                        if current.version_id != expected {
                            return Err(crate::Error::VersionConflict {
                                expected,
                                actual: current.version_id,
                            });
                        }
                    }
                }

                let current = tx.read(&resource_type, &resource_id).await?;
                let status = match current {
                    Some(existing) => {
                        let new_version = existing.version_id + 1;
                        populate_meta(&mut resource, &resource_id, new_version, Utc::now());
                        StatusCode::OK
                    }
                    None => {
                        if !self.allow_update_create_effective().await {
                            return Err(crate::Error::MethodNotAllowed(
                                "Server does not allow client-defined resource ids. Use POST to create resources."
                                    .to_string(),
                            ));
                        }
                        populate_meta(&mut resource, &resource_id, 1, Utc::now());
                        StatusCode::CREATED
                    }
                };

                // Ensure resource identity matches URL
                if let Some(obj) = resource.as_object_mut() {
                    obj.insert("resourceType".to_string(), json!(resource_type));
                    obj.insert("id".to_string(), json!(resource_id));
                }

                // Referential integrity check (strict mode)
                if self.is_strict_referential_integrity() {
                    self.validate_references_in_transaction(&resource, &known_ids).await?;
                }

                let updated = tx.upsert(&resource_type, &resource_id, resource).await?;

                produced_versions.insert(
                    format!("{}/{}", resource_type, updated.id),
                    updated.version_id,
                );
                written_resources.push(WrittenResource {
                    entry_index: index,
                    resource_type: resource_type.clone(),
                    id: updated.id.clone(),
                    version_id: updated.version_id,
                    resource: updated.resource.clone(),
                });

                Ok(BundleEntry {
                    full_url: entry.full_url.clone(),
                    request: None,
                    response: Some(BundleEntryResponse {
                        status: status_line(status),
                        location: Some(format!(
                            "{}/{}/_history/{}",
                            resource_type, updated.id, updated.version_id
                        )),
                        etag: Some(format!("W/\"{}\"", updated.version_id)),
                        last_modified: Some(updated.last_updated.to_rfc3339()),
                        outcome: match prefer_return {
                            PreferReturn::OperationOutcome => Some(serde_json::json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!(
                                        "Resource {} successfully with ID {}",
                                        if status == StatusCode::CREATED {
                                            "created"
                                        } else {
                                            "updated"
                                        },
                                        updated.id
                                    )
                                }]
                            })),
                            _ => None,
                        },
                        extensions: HashMap::new(),
                    }),
                    resource: match prefer_return {
                        PreferReturn::Representation => Some(updated.resource),
                        _ => None,
                    },
                    search: None,
                    extensions: HashMap::new(),
                })
            }
            "PATCH" => {
                // Support JSON Patch in transaction using Binary payload per spec.
                let resource_type = parsed_url.resource_type.ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Transaction entry {} PATCH missing resource type in request.url",
                        index
                    ))
                })?;
                let resource_id = if let Some(id) = parsed_url.resource_id {
                    id
                } else {
                    // Conditional patch: PATCH {type}?{criteria}
                    if query_items.is_empty() {
                        return Err(crate::Error::InvalidResource(format!(
                            "Transaction entry {} PATCH missing resource id and conditional criteria in request.url",
                            index
                        )));
                    }

                    let search_params = build_conditional_search_params_from_items(&query_items)?;
                    let search_result = {
                        let conn = tx.tx_mut()?;
                        self.search_engine
                            .search_with_connection(
                                conn,
                                Some(&resource_type),
                                &search_params,
                                base_url,
                            )
                            .await?
                    };

                    let resolution = self
                        .conditional_service
                        .resolve_conditional_target_from_matches(
                            tx,
                            &resource_type,
                            None,
                            &search_result.resources,
                        )
                        .await?;

                    let Some(id) = resolution.target_id else {
                        return Err(crate::Error::NotFound(
                            "No resources match conditional patch criteria".to_string(),
                        ));
                    };
                    if let Some(full_url) = &entry.full_url {
                        url_rewriter
                            .mapping
                            .insert(full_url.clone(), format!("{}/{}", resource_type, &id));
                    }
                    id
                };

                let binary = entry.resource.clone().ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Transaction entry {} PATCH missing resource (Binary)",
                        index
                    ))
                })?;
                let patch = parse_json_patch_from_binary(&binary)?;

                let current = tx
                    .read(&resource_type, &resource_id)
                    .await?
                    .ok_or_else(|| crate::Error::ResourceNotFound {
                        resource_type: resource_type.clone(),
                        id: resource_id.clone(),
                    })?;
                if current.deleted {
                    return Err(crate::Error::ResourceDeleted {
                        resource_type: resource_type.clone(),
                        id: resource_id.clone(),
                        version_id: Some(current.version_id),
                    });
                }

                if let Some(if_match) = &request.if_match {
                    if let Some(expected) = parse_etag(if_match) {
                        if current.version_id != expected {
                            return Err(crate::Error::VersionConflict {
                                expected,
                                actual: current.version_id,
                            });
                        }
                    }
                }

                let mut patched = current.resource.clone();
                json_patch::patch(&mut patched, &patch.0).map_err(|e| match e.kind {
                    json_patch::PatchErrorKind::TestFailed => {
                        crate::Error::UnprocessableEntity(e.to_string())
                    }
                    _ => crate::Error::InvalidResource(e.to_string()),
                })?;

                url_rewriter.rewrite_resource(&mut patched)?;
                self.resolve_conditional_references_in_transaction(tx, &mut patched, base_url)
                    .await?;

                let new_version = current.version_id + 1;
                populate_meta(&mut patched, &resource_id, new_version, Utc::now());
                if let Some(obj) = patched.as_object_mut() {
                    obj.insert("resourceType".to_string(), json!(resource_type));
                    obj.insert("id".to_string(), json!(resource_id));
                    // Narrative safety: PATCH changes data without updating narrative.
                    // Drop it to avoid clinically unsafe narrative.
                    obj.remove("text");
                }

                // Referential integrity check (strict mode)
                if self.is_strict_referential_integrity() {
                    self.validate_references_in_transaction(&patched, &known_ids).await?;
                }

                let updated = tx.upsert(&resource_type, &resource_id, patched).await?;

                produced_versions.insert(
                    format!("{}/{}", resource_type, updated.id),
                    updated.version_id,
                );
                written_resources.push(WrittenResource {
                    entry_index: index,
                    resource_type: resource_type.clone(),
                    id: updated.id.clone(),
                    version_id: updated.version_id,
                    resource: updated.resource.clone(),
                });

                Ok(BundleEntry {
                    full_url: entry.full_url.clone(),
                    request: None,
                    response: Some(BundleEntryResponse {
                        status: status_line(StatusCode::OK),
                        location: Some(format!(
                            "{}/{}/_history/{}",
                            resource_type, updated.id, updated.version_id
                        )),
                        etag: Some(format!("W/\"{}\"", updated.version_id)),
                        last_modified: Some(updated.last_updated.to_rfc3339()),
                        outcome: match prefer_return {
                            PreferReturn::OperationOutcome => Some(serde_json::json!({
                                "resourceType": "OperationOutcome",
                                "issue": [{
                                    "severity": "information",
                                    "code": "informational",
                                    "diagnostics": format!(
                                        "Resource patched successfully: {}/{}",
                                        resource_type, resource_id
                                    )
                                }]
                            })),
                            _ => None,
                        },
                        extensions: HashMap::new(),
                    }),
                    resource: match prefer_return {
                        PreferReturn::Representation => Some(updated.resource),
                        _ => None,
                    },
                    search: None,
                    extensions: HashMap::new(),
                })
            }
            "GET" | "HEAD" => {
                let resource_type = parsed_url.resource_type.ok_or_else(|| {
                    crate::Error::InvalidResource(format!(
                        "Transaction entry {} GET missing resource type in request.url",
                        index
                    ))
                })?;
                let Some(resource_id) = parsed_url.resource_id else {
                    // Search isn't implemented here.
                    return Ok(empty_searchset_entry());
                };

                let resource = tx
                    .read(&resource_type, &resource_id)
                    .await?
                    .ok_or_else(|| crate::Error::ResourceNotFound {
                        resource_type: resource_type.clone(),
                        id: resource_id.clone(),
                    })?;
                if resource.deleted {
                    return Err(crate::Error::ResourceDeleted {
                        resource_type: resource_type.clone(),
                        id: resource_id.clone(),
                        version_id: Some(resource.version_id),
                    });
                }

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
            _ => Err(crate::Error::InvalidResource(format!(
                "Unsupported HTTP method in transaction: {}",
                method
            ))),
        }
    }

    fn is_strict_referential_integrity(&self) -> bool {
        self.referential_integrity_mode == "strict"
    }

    /// Validate references in a resource within a transaction context.
    ///
    /// `known_ids` contains `(resource_type, id)` pairs for resources created earlier
    /// in this transaction, which should be treated as existing.
    async fn validate_references_in_transaction(
        &self,
        resource: &JsonValue,
        known_ids: &std::collections::HashSet<(String, String)>,
    ) -> Result<()> {
        let mut relative_refs_set = std::collections::HashSet::new();
        super::referential_integrity::collect_relative_refs(resource, &mut relative_refs_set);
        let relative_refs: Vec<(String, String)> = relative_refs_set.into_iter().collect();

        if relative_refs.is_empty() {
            return Ok(());
        }

        // Allow self-references
        let self_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let self_id = resource.get("id").and_then(|v| v.as_str()).unwrap_or("");

        let refs_to_check: Vec<(String, String)> = relative_refs
            .into_iter()
            .filter(|(rt, id)| {
                // Skip self-references
                if rt == self_type && id == self_id {
                    return false;
                }
                // Skip references to resources created earlier in this transaction
                !known_ids.contains(&(rt.clone(), id.clone()))
            })
            .collect();

        if refs_to_check.is_empty() {
            return Ok(());
        }

        let existing = self.store.check_resources_exist(&refs_to_check).await?;
        let missing: Vec<String> = refs_to_check
            .iter()
            .filter(|pair| !existing.contains(pair))
            .map(|(rt, id)| format!("{}/{}", rt, id))
            .collect();

        if !missing.is_empty() {
            return Err(crate::Error::BusinessRule(format!(
                "Referential integrity violation: the following referenced resources do not exist: {}",
                missing.join(", ")
            )));
        }

        Ok(())
    }

    /// Check that no other resources reference this resource before deletion in a transaction.
    async fn validate_no_references_to(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<()> {
        let referencing = self
            .store
            .find_referencing_resources(resource_type, id, 5)
            .await?;

        if !referencing.is_empty() {
            let refs: Vec<String> = referencing
                .iter()
                .map(|(rt, rid)| format!("{}/{}", rt, rid))
                .collect();
            return Err(crate::Error::BusinessRule(format!(
                "Referential integrity violation: cannot delete {}/{} because it is referenced by: {}",
                resource_type, id, refs.join(", ")
            )));
        }

        Ok(())
    }

    async fn resolve_conditional_references_in_transaction(
        &self,
        tx: &mut PostgresTransactionContext,
        resource: &mut JsonValue,
        base_url: Option<&str>,
    ) -> Result<()> {
        let conn = tx.tx_mut()?;
        crate::services::conditional_references::resolve_conditional_references_with_connection(
            &self.search_engine,
            conn,
            resource,
            base_url,
        )
        .await
    }

    async fn apply_inline_indexing(
        &self,
        response_bundle: &Bundle,
        indexing: TransactionIndexingActions,
    ) -> Result<()> {
        // Seed fullUrl -> identity mapping so FHIRPath `resolve()` can follow transaction-scoped
        // references that still use `Bundle.entry.fullUrl` values (e.g., `urn:uuid:...`).
        if let Some(entries) = &response_bundle.entry {
            let mut mapping = HashMap::new();
            for entry in entries {
                let Some(full_url) = &entry.full_url else {
                    continue;
                };
                let Some(response) = &entry.response else {
                    continue;
                };
                let Some(location) = &response.location else {
                    continue;
                };
                if let Some(identity) = ParsedUrl::parse(location).identity() {
                    mapping.insert(full_url.clone(), identity);
                }
            }
            if !mapping.is_empty() {
                self.indexing_service.seed_full_url_mapping(mapping);
            }
        }

        for (resource_type, resource_ids) in indexing.deleted {
            for id in resource_ids {
                if let Err(e) = self
                    .indexing_service
                    .remove_resource_index(&resource_type, &id)
                    .await
                {
                    tracing::warn!(
                        "Failed to remove search index for deleted {}/{}: {}",
                        resource_type,
                        id,
                        e
                    );
                }
            }
        }

        if indexing.upserted.is_empty() {
            return Ok(());
        }

        let mut resources = Vec::new();
        for (resource_type, resource_ids) in indexing.upserted {
            let mut loaded = self
                .store
                .load_resources_batch(&resource_type, &resource_ids)
                .await?;
            resources.append(&mut loaded);
        }

        self.indexing_service
            .index_resources_auto(&resources)
            .await?;
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
// Transaction helpers
// =============================================================================

fn validate_transaction_bundle(entries: &[BundleEntry]) -> Result<()> {
    let mut seen_full_urls = HashSet::new();

    for (i, entry) in entries.iter().enumerate() {
        let request = entry.request.as_ref().ok_or_else(|| {
            crate::Error::InvalidResource(format!("Transaction entry {} missing request", i))
        })?;
        let method = request.method.to_uppercase();

        if (method == "POST" || method == "PUT" || method == "PATCH") && entry.resource.is_none() {
            return Err(crate::Error::InvalidResource(format!(
                "Transaction entry {} with method {} missing resource",
                i, method
            )));
        }

        if let Some(full_url) = &entry.full_url {
            if !seen_full_urls.insert(full_url.clone()) {
                return Err(crate::Error::InvalidResource(format!(
                    "Duplicate fullUrl in transaction at entry {}: {}",
                    i, full_url
                )));
            }
        }
    }

    Ok(())
}

fn partition_transaction_entries(
    entries: &[BundleEntry],
) -> Result<(Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>)> {
    let mut delete_indices = Vec::new();
    let mut post_indices = Vec::new();
    let mut put_patch_indices = Vec::new();
    let mut get_indices = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        let request = entry.request.as_ref().ok_or_else(|| {
            crate::Error::InvalidResource(format!("Transaction entry {} missing request", index))
        })?;

        let method = request.method.to_uppercase();
        match method.as_str() {
            "DELETE" => delete_indices.push(index),
            "POST" => post_indices.push(index),
            "PUT" | "PATCH" => put_patch_indices.push(index),
            "GET" | "HEAD" => get_indices.push(index),
            _ => {
                return Err(crate::Error::InvalidResource(format!(
                    "Unsupported HTTP method in transaction: {}",
                    method
                )));
            }
        }
    }

    Ok((delete_indices, post_indices, put_patch_indices, get_indices))
}

fn check_identity_overlaps(
    entries: &[BundleEntry],
    delete_indices: &[usize],
    post_indices: &[usize],
    put_indices: &[usize],
) -> Result<()> {
    let mut identities = HashSet::new();

    for &idx in delete_indices.iter().chain(post_indices).chain(put_indices) {
        let entry = &entries[idx];
        let request = entry.request.as_ref().ok_or_else(|| {
            crate::Error::InvalidResource(format!("Transaction entry {} missing request", idx))
        })?;

        let method = request.method.to_uppercase();
        if method == "POST" {
            // POST identities are server assigned; treat duplicate fullUrl as invalid elsewhere.
            continue;
        }

        let parsed = ParsedUrl::parse(&request.url);
        if let Some(id) = parsed.identity() {
            if !identities.insert(id.clone()) {
                return Err(crate::Error::InvalidResource(format!(
                    "Transaction identity overlap detected for {}",
                    id
                )));
            }
        }
    }

    Ok(())
}

fn with_entry_context(err: crate::Error, index: usize) -> crate::Error {
    match err {
        crate::Error::InvalidResource(msg) => {
            crate::Error::InvalidResource(format!("Transaction entry {}: {}", index, msg))
        }
        crate::Error::Validation(msg) => {
            crate::Error::Validation(format!("Transaction entry {}: {}", index, msg))
        }
        crate::Error::BusinessRule(msg) => {
            crate::Error::BusinessRule(format!("Transaction entry {}: {}", index, msg))
        }
        crate::Error::PreconditionFailed(msg) => {
            crate::Error::PreconditionFailed(format!("Transaction entry {}: {}", index, msg))
        }
        other => other,
    }
}

fn status_line(status: StatusCode) -> String {
    match status.canonical_reason() {
        Some(reason) => format!("{} {}", status.as_u16(), reason),
        None => status.as_u16().to_string(),
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

fn populate_meta(resource: &mut JsonValue, id: &str, version_id: i32, now: chrono::DateTime<Utc>) {
    if let Some(obj) = resource.as_object_mut() {
        obj.insert("id".to_string(), json!(id));

        let meta = obj.entry("meta".to_string()).or_insert_with(|| json!({}));
        if let Some(meta_obj) = meta.as_object_mut() {
            meta_obj.insert("versionId".to_string(), json!(version_id.to_string()));
            meta_obj.insert("lastUpdated".to_string(), json!(now.to_rfc3339()));
        }
    }
}

#[derive(Debug, Clone)]
struct WrittenResource {
    entry_index: usize,
    resource_type: String,
    id: String,
    version_id: i32,
    resource: JsonValue,
}

async fn finalize_version_specific_references(
    tx: &mut PostgresTransactionContext,
    produced_versions: &HashMap<String, i32>,
    written_resources: &mut [WrittenResource],
    response_entries: &mut [BundleEntry],
) -> Result<()> {
    for written in written_resources.iter_mut() {
        let mut changed = false;
        apply_resolve_as_version_specific(&mut written.resource, produced_versions, &mut changed);

        if !changed {
            continue;
        }

        tx.update_current_resource_json(
            &written.resource_type,
            &written.id,
            written.version_id,
            &written.resource,
        )
        .await?;

        if let Some(entry) = response_entries.get_mut(written.entry_index) {
            if entry.resource.is_some() {
                entry.resource = Some(written.resource.clone());
            }
        }
    }

    Ok(())
}

fn apply_resolve_as_version_specific(
    value: &mut JsonValue,
    produced_versions: &HashMap<String, i32>,
    changed: &mut bool,
) {
    match value {
        JsonValue::Object(map) => {
            // Reference datatype shape: { "reference": "...", "extension": [...] }
            if let Some(JsonValue::String(reference)) = map.get("reference").cloned() {
                let has_resolve_extension = map
                    .get("extension")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter().any(|ext| {
                            ext.get("url")
                                .and_then(|u| u.as_str())
                                .is_some_and(|u| u == EXT_RESOLVE_AS_VERSION_SPECIFIC)
                        })
                    })
                    .unwrap_or(false);

                if has_resolve_extension {
                    // SHALL remove the extension as part of resolution.
                    if let Some(JsonValue::Array(ext_arr)) = map.get_mut("extension") {
                        let before_len = ext_arr.len();
                        ext_arr.retain(|ext| {
                            ext.get("url")
                                .and_then(|u| u.as_str())
                                .map(|u| u != EXT_RESOLVE_AS_VERSION_SPECIFIC)
                                .unwrap_or(true)
                        });
                        if ext_arr.len() != before_len {
                            *changed = true;
                        }
                        if ext_arr.is_empty() {
                            map.remove("extension");
                        }
                    }

                    // If the reference is versionless and the transaction produced a new version
                    // of the target resource, rewrite it to a version-specific reference.
                    let (base_ref, frag) = reference
                        .split_once('#')
                        .map_or((reference.as_str(), None), |(b, f)| (b, Some(f)));

                    if !base_ref.contains("/_history/") {
                        if let Some(identity) = ParsedUrl::parse(base_ref).identity() {
                            if let Some(version_id) = produced_versions.get(&identity) {
                                let mut upgraded = format!("{}/_history/{}", identity, version_id);
                                if let Some(frag) = frag {
                                    upgraded.push('#');
                                    upgraded.push_str(frag);
                                }
                                map.insert("reference".to_string(), JsonValue::String(upgraded));
                                *changed = true;
                            }
                        }
                    }
                }
            }

            for v in map.values_mut() {
                apply_resolve_as_version_specific(v, produced_versions, changed);
            }
        }
        JsonValue::Array(arr) => {
            for item in arr.iter_mut() {
                apply_resolve_as_version_specific(item, produced_versions, changed);
            }
        }
        _ => {}
    }
}

fn parse_json_patch_from_binary(binary: &JsonValue) -> Result<json_patch::Patch> {
    let resource_type = binary
        .get("resourceType")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if resource_type != "Binary" {
        return Err(crate::Error::InvalidResource(
            "Transaction PATCH requires a Binary resource payload".to_string(),
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
// URL replacement / fullUrl mapping
// =============================================================================

struct UrlRewriter {
    fhir_context: Arc<dyn FhirContext>,
    mapping: HashMap<String, String>,
    reserved_post_ids: HashMap<usize, String>,
    canonical_cache: HashMap<String, HashSet<String>>,
}

impl UrlRewriter {
    fn new(fhir_context: Arc<dyn FhirContext>) -> Self {
        Self {
            fhir_context,
            mapping: HashMap::new(),
            reserved_post_ids: HashMap::new(),
            canonical_cache: HashMap::new(),
        }
    }

    fn seed_non_post_mappings(&mut self, entries: &[BundleEntry]) -> Result<()> {
        for entry in entries {
            let Some(full_url) = &entry.full_url else {
                continue;
            };
            let Some(request) = &entry.request else {
                continue;
            };
            let method = request.method.to_uppercase();
            if method == "POST" {
                continue;
            }

            let parsed = ParsedUrl::parse(&request.url);
            if let Some(identity) = parsed.identity() {
                self.mapping.insert(full_url.clone(), identity);
            }
        }
        Ok(())
    }

    fn reserve_post_ids(&mut self, entries: &[BundleEntry], post_indices: &[usize]) -> Result<()> {
        for &idx in post_indices {
            let entry = &entries[idx];
            let request = entry.request.as_ref().ok_or_else(|| {
                crate::Error::InvalidResource(format!("Transaction entry {} missing request", idx))
            })?;
            let parsed = ParsedUrl::parse(&request.url);
            let resource_type = parsed.resource_type.clone().ok_or_else(|| {
                crate::Error::InvalidResource(format!(
                    "Transaction entry {} POST missing resource type in request.url",
                    idx
                ))
            })?;

            let id = Uuid::new_v4().to_string();
            self.reserved_post_ids.insert(idx, id.clone());

            if let Some(full_url) = &entry.full_url {
                self.mapping
                    .insert(full_url.clone(), format!("{}/{}", resource_type, id));
            }
        }
        Ok(())
    }

    fn reserved_post_id(&self, index: usize) -> Option<String> {
        self.reserved_post_ids.get(&index).cloned()
    }

    fn rewrite_resource(&mut self, resource: &mut JsonValue) -> Result<()> {
        if self.mapping.is_empty() {
            return Ok(());
        }

        let resource_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let canonical_paths = self.canonical_paths_for_resource(&resource_type)?;
        rewrite_json_value(resource, &self.mapping, &canonical_paths, &mut Vec::new());
        Ok(())
    }

    fn canonical_paths_for_resource(&mut self, resource_type: &str) -> Result<HashSet<String>> {
        if let Some(cached) = self.canonical_cache.get(resource_type) {
            return Ok(cached.clone());
        }

        let Some(sd) = self
            .fhir_context
            .get_core_structure_definition_by_type(resource_type)
            .map_err(|e| crate::Error::FhirContext(e.to_string()))?
        else {
            self.canonical_cache
                .insert(resource_type.to_string(), HashSet::new());
            return Ok(HashSet::new());
        };

        let paths = collect_canonical_paths(&sd);
        self.canonical_cache
            .insert(resource_type.to_string(), paths.clone());
        Ok(paths)
    }
}

fn collect_canonical_paths(sd: &StructureDefinition) -> HashSet<String> {
    let mut paths = HashSet::new();

    let Some(snapshot) = &sd.snapshot else {
        return paths;
    };

    for element in &snapshot.element {
        let Some(types) = &element.types else {
            continue;
        };
        if !types.iter().any(|t| t.code == "canonical") {
            continue;
        }

        // Convert "Patient.meta.profile" -> "meta.profile"
        if let Some((_, tail)) = element.path.split_once('.') {
            paths.insert(tail.to_string());
        }
    }

    paths
}

fn rewrite_json_value(
    value: &mut JsonValue,
    mapping: &HashMap<String, String>,
    canonical_paths: &HashSet<String>,
    path: &mut Vec<String>,
) {
    match value {
        JsonValue::Object(map) => {
            for (k, v) in map.iter_mut() {
                // Choice: skip replacement under canonical element paths.
                path.push(k.clone());
                let dot_path = path.join(".");
                if canonical_paths.contains(&dot_path) {
                    // Don't rewrite canonical values.
                    path.pop();
                    continue;
                }

                rewrite_json_value(v, mapping, canonical_paths, path);
                path.pop();
            }
        }
        JsonValue::Array(arr) => {
            for item in arr.iter_mut() {
                rewrite_json_value(item, mapping, canonical_paths, path);
            }
        }
        JsonValue::String(s) => {
            if let Some(updated) = rewrite_string(s, mapping) {
                *s = updated;
            }
        }
        _ => {}
    }
}

fn rewrite_string(input: &str, mapping: &HashMap<String, String>) -> Option<String> {
    if mapping.is_empty() {
        return None;
    }

    // Exact match first.
    if let Some(replacement) = mapping.get(input) {
        return Some(replacement.clone());
    }

    // Fragment-aware replacement: replace base before '#'.
    if let Some((base, frag)) = input.split_once('#') {
        if let Some(replacement) = mapping.get(base) {
            return Some(format!("{}#{}", replacement, frag));
        }
    }

    // Generic replacement for narrative/markdown and other string fields.
    let mut out = input.to_string();
    let mut changed = false;
    for (from, to) in mapping {
        if out.contains(from) {
            out = out.replace(from, to);
            changed = true;
        }
    }

    if changed {
        Some(out)
    } else {
        None
    }
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

        // Remove query string
        if let Some((p, _q)) = path.split_once('?') {
            path = p;
        }

        // Strip scheme + host
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
