//! CRUD service - business logic for resource operations
//!
//! FHIR REST spec-compliant implementation (inlined from fhir-rest)

use crate::{
    db::{PostgresResourceStore, ResourceStore},
    hooks::ResourceHook,
    models::{
        is_known_resource_type, CreateParams, HistoryEntry, HistoryMethod, HistoryResult, Resource,
        ResourceOperation, ResourceResult, UpdateParams,
    },
    queue::{JobPriority, JobQueue},
    runtime_config::{ConfigKey, RuntimeConfigCache},
    services::IndexingService,
    Error, Result,
};
use chrono::Utc;
use json_patch::PatchErrorKind;
use serde_json::Value as JsonValue;
use std::sync::Arc;
use uuid::Uuid;

pub struct CrudService {
    store: PostgresResourceStore,
    hooks: Vec<Arc<dyn ResourceHook>>,
    job_queue: Option<Arc<dyn JobQueue>>,
    indexing_service: Option<Arc<IndexingService>>,
    allow_update_create: bool,
    hard_delete: bool,
    runtime_config_cache: Option<Arc<RuntimeConfigCache>>,
    referential_integrity_mode: String,
}

impl CrudService {
    pub fn new(store: PostgresResourceStore) -> Self {
        Self::new_with_policy(store, true, false)
    }

    pub fn new_with_policy(
        store: PostgresResourceStore,
        allow_update_create: bool,
        hard_delete: bool,
    ) -> Self {
        Self {
            store,
            hooks: Vec::new(),
            job_queue: None,
            indexing_service: None,
            allow_update_create,
            hard_delete,
            runtime_config_cache: None,
            referential_integrity_mode: "lenient".to_string(),
        }
    }

    pub fn new_with_policy_and_runtime_config(
        store: PostgresResourceStore,
        allow_update_create: bool,
        hard_delete: bool,
        runtime_config_cache: Arc<RuntimeConfigCache>,
    ) -> Self {
        let mut service = Self::new_with_policy(store, allow_update_create, hard_delete);
        service.runtime_config_cache = Some(runtime_config_cache);
        service
    }

    pub fn with_hooks(store: PostgresResourceStore, hooks: Vec<Arc<dyn ResourceHook>>) -> Self {
        Self::with_hooks_and_policy(store, hooks, true, false)
    }

    pub fn with_hooks_and_policy(
        store: PostgresResourceStore,
        hooks: Vec<Arc<dyn ResourceHook>>,
        allow_update_create: bool,
        hard_delete: bool,
    ) -> Self {
        Self {
            store,
            hooks,
            job_queue: None,
            indexing_service: None,
            allow_update_create,
            hard_delete,
            runtime_config_cache: None,
            referential_integrity_mode: "lenient".to_string(),
        }
    }

    pub fn with_hooks_and_indexing(
        store: PostgresResourceStore,
        hooks: Vec<Arc<dyn ResourceHook>>,
        job_queue: Arc<dyn JobQueue>,
        indexing_service: Arc<IndexingService>,
        allow_update_create: bool,
        hard_delete: bool,
    ) -> Self {
        Self {
            store,
            hooks,
            job_queue: Some(job_queue),
            indexing_service: Some(indexing_service),
            allow_update_create,
            hard_delete,
            runtime_config_cache: None,
            referential_integrity_mode: "lenient".to_string(),
        }
    }

    pub fn with_hooks_and_indexing_and_runtime_config(
        store: PostgresResourceStore,
        hooks: Vec<Arc<dyn ResourceHook>>,
        job_queue: Arc<dyn JobQueue>,
        indexing_service: Arc<IndexingService>,
        allow_update_create: bool,
        hard_delete: bool,
        runtime_config_cache: Arc<RuntimeConfigCache>,
    ) -> Self {
        let mut service = Self::with_hooks_and_indexing(
            store,
            hooks,
            job_queue,
            indexing_service,
            allow_update_create,
            hard_delete,
        );
        service.runtime_config_cache = Some(runtime_config_cache);
        service
    }

    pub fn set_referential_integrity_mode(&mut self, mode: String) {
        self.referential_integrity_mode = mode;
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

    /// Create a new resource (POST /{resourceType})
    ///
    /// Spec-compliant behavior:
    /// - Generates server-assigned ID (UUID)
    /// - Populates meta.versionId = 1
    /// - Populates meta.lastUpdated
    ///
    /// NOTE: Conditional create (If-None-Exist) should be handled at the handler level
    /// using SearchEngine, not in this service method. The service layer doesn't have
    /// access to SearchEngine and cannot perform proper search operations.
    pub async fn create_resource(
        &self,
        resource_type: &str,
        mut resource: JsonValue,
        params: Option<CreateParams>,
    ) -> Result<ResourceResult> {
        self.validate_resource_type_name(resource_type)?;

        // Validate resource type matches
        self.validate_resource_type(&resource, resource_type)?;

        // NOTE: Conditional operations (If-None-Exist) are now handled at the handler level
        // where SearchEngine is available. This service method should only be called after
        // the handler has already resolved any conditional logic.
        if let Some(create_params) = params {
            if create_params.if_none_exist.is_some() {
                return Err(Error::Internal(
                    "Conditional create (If-None-Exist) must be handled at handler level using SearchEngine. \
                     Do not pass conditional params to service layer.".to_string()
                ));
            }
        }

        // Generate server-assigned ID
        let id = Uuid::new_v4().to_string();

        // Populate meta
        self.populate_meta(&mut resource, &id, 1, Utc::now());

        // Referential integrity check (strict mode)
        if self.is_strict_referential_integrity() {
            self.validate_references(&resource).await?;
        }

        // Create in store
        let created = self.store.create(resource_type, resource).await?;

        // Trigger hooks
        for hook in &self.hooks {
            hook.on_created(&created).await?;
        }

        self.queue_indexing_job(resource_type, vec![created.id.clone()])
            .await;

        Ok(ResourceResult {
            resource: created,
            operation: ResourceOperation::Created,
        })
    }

    /// Read a resource (GET /{resourceType}/{id})
    ///
    /// Spec-compliant behavior:
    /// - Returns current version only
    /// - Returns 404 if not found
    /// - Returns 410 Gone if deleted
    pub async fn read_resource(&self, resource_type: &str, id: &str) -> Result<Resource> {
        self.validate_resource_type_name(resource_type)?;

        match self.store.read(resource_type, id).await? {
            Some(resource) => {
                if resource.deleted {
                    Err(Error::ResourceDeleted {
                        resource_type: resource_type.to_string(),
                        id: id.to_string(),
                        version_id: Some(resource.version_id),
                    })
                } else {
                    Ok(resource)
                }
            }
            None => Err(Error::ResourceNotFound {
                resource_type: resource_type.to_string(),
                id: id.to_string(),
            }),
        }
    }

    /// Delete all history for a resource (DELETE /{resourceType}/{id}/_history).
    ///
    /// This is a destructive "purge" operation and is only allowed when `hard_delete` is enabled.
    pub async fn delete_resource_history(&self, resource_type: &str, id: &str) -> Result<()> {
        self.validate_resource_type_name(resource_type)?;

        if !self.hard_delete_effective().await {
            return Err(Error::MethodNotAllowed(
                "Deleting resource history requires hard_delete=true".to_string(),
            ));
        }

        let deleted = self.store.hard_delete(resource_type, id).await?;
        if deleted == 0 {
            return Err(Error::ResourceNotFound {
                resource_type: resource_type.to_string(),
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Delete a specific version from a resource history (DELETE /{resourceType}/{id}/_history/{vid}).
    ///
    /// If the deleted version was the current one, the newest remaining version becomes current.
    /// This is a destructive operation and is only allowed when `hard_delete` is enabled.
    pub async fn delete_resource_history_version(
        &self,
        resource_type: &str,
        id: &str,
        version_id: i32,
    ) -> Result<()> {
        self.validate_resource_type_name(resource_type)?;

        if !self.hard_delete_effective().await {
            return Err(Error::MethodNotAllowed(
                "Deleting resource history requires hard_delete=true".to_string(),
            ));
        }

        self.store
            .hard_delete_version(resource_type, id, version_id)
            .await
    }

    /// Update a resource (PUT /{resourceType}/{id})
    ///
    /// Spec-compliant behavior:
    /// - Creates new version (increments versionId)
    /// - Updates meta.lastUpdated
    /// - Handles If-Match conditional update
    /// - Returns 201 if resource didn't exist (create via update)
    /// - Returns 200 if resource was updated
    pub async fn update_resource(
        &self,
        resource_type: &str,
        id: &str,
        mut resource: JsonValue,
        params: Option<UpdateParams>,
    ) -> Result<ResourceResult> {
        self.validate_resource_type_name(resource_type)?;

        // Validate ID matches URL (FHIR spec SHALL requirement)
        // "If no id element is provided, or the id disagrees with the id in the URL,
        // the server SHALL respond with an HTTP 400 Bad Request error code"
        if let Some(id_value) = resource.get("id") {
            if let Some(id_str) = id_value.as_str() {
                if id_str != id {
                    return Err(Error::InvalidResource(format!(
                        "Resource id '{}' does not match URL id '{}'",
                        id_str, id
                    )));
                }
            } else {
                return Err(Error::InvalidResource(
                    "Resource id must be a string".to_string(),
                ));
            }
        }
        // If no id provided, that's OK - we'll populate it below

        // Validate resource type matches
        self.validate_resource_type(&resource, resource_type)?;

        // Handle conditional update (If-Match)
        if let Some(update_params) = params {
            if let Some(expected_version) = update_params.if_match {
                // Check current version
                if let Some(current) = self.store.read(resource_type, id).await? {
                    validate_version_match(&current, expected_version)?;
                } else {
                    return Err(Error::ResourceNotFound {
                        resource_type: resource_type.to_string(),
                        id: id.to_string(),
                    });
                }
            }
        }

        // Check if resource exists
        let operation = match self.store.read(resource_type, id).await? {
            Some(existing) => {
                // Update existing resource
                let new_version = existing.version_id + 1;
                self.populate_meta(&mut resource, id, new_version, Utc::now());
                ResourceOperation::Updated
            }
            None => {
                // Resource doesn't exist - check if update-as-create is allowed
                // Per FHIR spec: "405 Method Not Allowed - the resource did not exist
                // prior to the update, and the server does not allow client defined ids"
                if !self.allow_update_create_effective().await {
                    return Err(Error::MethodNotAllowed(
                        "Server does not allow client-defined resource ids. \
                        Use POST to create resources with server-assigned ids."
                            .to_string(),
                    ));
                }
                // Create via update (PUT with client-specified ID)
                self.populate_meta(&mut resource, id, 1, Utc::now());
                ResourceOperation::Created
            }
        };

        // Referential integrity check (strict mode)
        if self.is_strict_referential_integrity() {
            self.validate_references(&resource).await?;
        }

        // Perform update/upsert
        let updated = self.store.upsert(resource_type, id, resource).await?;

        // Trigger hooks
        for hook in &self.hooks {
            hook.on_updated(&updated).await?;
        }

        if matches!(
            operation,
            ResourceOperation::Created | ResourceOperation::Updated
        ) {
            self.queue_indexing_job(resource_type, vec![updated.id.clone()])
                .await;
        }

        Ok(ResourceResult {
            resource: updated,
            operation,
        })
    }

    /// Patch a resource (PATCH /{resourceType}/{id}) using JSON Patch (RFC 6902)
    ///
    /// Spec-compliant behavior:
    /// - Applies patch to the server's current representation
    /// - Supports version contention via If-Match (UpdateParams.if_match)
    /// - Processes the result as an update (new version, lastUpdated, hooks, indexing)
    /// - Returns 404 if the resource does not exist
    pub async fn patch_resource_json_patch(
        &self,
        resource_type: &str,
        id: &str,
        patch: json_patch::Patch,
        params: Option<UpdateParams>,
    ) -> Result<ResourceResult> {
        self.validate_resource_type_name(resource_type)?;

        let current = self.read_resource(resource_type, id).await?;

        // Resource contention (If-Match)
        if let Some(update_params) = params {
            if let Some(expected_version) = update_params.if_match {
                validate_version_match(&current, expected_version)?;
            }
        }

        let mut patched = current.resource.clone();
        json_patch::patch(&mut patched, &patch.0).map_err(|e| match e.kind {
            PatchErrorKind::TestFailed => Error::UnprocessableEntity(e.to_string()),
            _ => Error::InvalidResource(e.to_string()),
        })?;

        // Prevent changing the resource identity via PATCH.
        let obj = patched.as_object_mut().ok_or_else(|| {
            Error::InvalidResource("Patched resource must be a JSON object".to_string())
        })?;
        obj.insert("resourceType".to_string(), serde_json::json!(resource_type));
        obj.insert("id".to_string(), serde_json::json!(id));
        // Narrative safety: PATCH changes data without updating narrative.
        // To ensure the narrative is not clinically unsafe, drop it after applying the patch.
        // (FHIR spec allows servers to delete narrative if they cannot safely maintain it.)
        obj.remove("text");

        // Update-as-create is not allowed for PATCH: we already resolved `current`.
        let new_version = current.version_id + 1;
        self.populate_meta(&mut patched, id, new_version, Utc::now());

        // Referential integrity check (strict mode)
        if self.is_strict_referential_integrity() {
            self.validate_references(&patched).await?;
        }

        // Persist and trigger side effects like a normal update.
        let updated = self.store.upsert(resource_type, id, patched).await?;

        for hook in &self.hooks {
            hook.on_updated(&updated).await?;
        }

        self.queue_indexing_job(resource_type, vec![updated.id.clone()])
            .await;

        Ok(ResourceResult {
            resource: updated,
            operation: ResourceOperation::Updated,
        })
    }

    /// Delete a resource (DELETE /{resourceType}/{id})
    ///
    /// Spec-compliant behavior:
    /// - Request is idempotent: deleting a missing/already-deleted resource succeeds
    /// - Soft delete (default): creates new version with deleted=true
    /// - Hard delete (optional): physically removes all versions
    /// - Returns Optional version ID (ETag MAY be returned)
    /// - Returns 204 No Content on success
    pub async fn delete_resource(&self, resource_type: &str, id: &str) -> Result<Option<i32>> {
        self.validate_resource_type_name(resource_type)?;

        let current = self.store.read(resource_type, id).await?;

        // Nothing to delete: return success with no ETag.
        let Some(current) = current else {
            return Ok(None);
        };

        // Referential integrity check on delete (strict mode)
        if self.is_strict_referential_integrity() && !current.deleted {
            self.validate_no_references_to(resource_type, id).await?;
        }

        if self.hard_delete_effective().await {
            let _rows_deleted = self.store.hard_delete(resource_type, id).await?;

            if !current.deleted {
                for hook in &self.hooks {
                    hook.on_deleted(resource_type, id, current.version_id)
                        .await?;
                }
            }

            if let Some(indexing_service) = &self.indexing_service {
                if let Err(e) = indexing_service
                    .remove_resource_index(resource_type, id)
                    .await
                {
                    tracing::warn!(
                        "Failed to remove search index for hard-deleted {}/{}: {}",
                        resource_type,
                        id,
                        e
                    );
                }
            }

            // No new version is created, but we can return the last known version for optional ETag.
            return Ok(Some(current.version_id));
        }

        // Already deleted: no-op.
        if current.deleted {
            return Ok(Some(current.version_id));
        }

        // Perform soft delete (creates deleted history entry)
        let new_version = self.store.delete(resource_type, id).await?;

        for hook in &self.hooks {
            hook.on_deleted(resource_type, id, new_version).await?;
        }

        if let Some(indexing_service) = &self.indexing_service {
            if let Err(e) = indexing_service
                .remove_resource_index(resource_type, id)
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

        Ok(Some(new_version))
    }

    /// Read a specific version (GET /{resourceType}/{id}/_history/{vid})
    ///
    /// Spec-compliant behavior:
    /// - Returns 410 Gone if the version represents a deletion
    /// - Returns 404 if version doesn't exist
    pub async fn vread_resource(
        &self,
        resource_type: &str,
        id: &str,
        version_id: i32,
    ) -> Result<Resource> {
        self.validate_resource_type_name(resource_type)?;

        let resource = self.store.vread(resource_type, id, version_id).await?;

        // Per FHIR spec: return 410 Gone if this version was a deletion
        if resource.deleted {
            return Err(Error::ResourceDeleted {
                resource_type: resource_type.to_string(),
                id: id.to_string(),
                version_id: Some(resource.version_id),
            });
        }

        Ok(resource)
    }

    /// Get resource history (GET /{resourceType}/{id}/_history)
    ///
    /// Spec-compliant behavior:
    /// - Returns all versions (newest first)
    /// - Includes deleted versions
    /// - Supports _count, _since and _at parameters
    pub async fn resource_history(
        &self,
        resource_type: &str,
        id: &str,
        count: Option<i32>,
        since: Option<chrono::DateTime<chrono::Utc>>,
        at: Option<chrono::DateTime<chrono::Utc>>,
        sort_ascending: bool,
    ) -> Result<HistoryResult> {
        self.validate_resource_type_name(resource_type)?;

        self.store
            .history(resource_type, id, count, since, at, sort_ascending)
            .await
    }

    /// Get type-wide history (GET /{resourceType}/_history)
    ///
    /// Spec-compliant behavior:
    /// - Includes all versions of all resources of the given type (including deletions)
    /// - Supports `_count`, `_since`, `_at`, and `_sort` (via handler validation)
    pub async fn type_history(
        &self,
        resource_type: &str,
        count: Option<i32>,
        since: Option<chrono::DateTime<chrono::Utc>>,
        at: Option<chrono::DateTime<chrono::Utc>>,
        sort_ascending: bool,
    ) -> Result<HistoryResult> {
        self.validate_resource_type_name(resource_type)?;

        let resources = self
            .store
            .history_type_resources(resource_type, count, since, at, sort_ascending)
            .await?;

        let entries = resources
            .into_iter()
            .map(|resource| {
                let method = if resource.deleted {
                    HistoryMethod::Delete
                } else if resource.version_id == 1 {
                    HistoryMethod::Post
                } else {
                    HistoryMethod::Put
                };
                HistoryEntry { resource, method }
            })
            .collect();

        Ok(HistoryResult {
            entries,
            total: None,
        })
    }

    /// Get system-wide history (GET /_history)
    ///
    /// Spec-compliant behavior:
    /// - Includes all versions of all resources (including deletions)
    /// - Supports `_count`, `_since`, `_at`, and `_sort` (via handler validation)
    pub async fn system_history(
        &self,
        count: Option<i32>,
        since: Option<chrono::DateTime<chrono::Utc>>,
        at: Option<chrono::DateTime<chrono::Utc>>,
        sort_ascending: bool,
    ) -> Result<HistoryResult> {
        let resources = self
            .store
            .history_system_resources(count, since, at, sort_ascending)
            .await?;

        let entries = resources
            .into_iter()
            .map(|resource| {
                let method = if resource.deleted {
                    HistoryMethod::Delete
                } else if resource.version_id == 1 {
                    HistoryMethod::Post
                } else {
                    HistoryMethod::Put
                };
                HistoryEntry { resource, method }
            })
            .collect();

        Ok(HistoryResult {
            entries,
            total: None,
        })
    }

    fn is_strict_referential_integrity(&self) -> bool {
        self.referential_integrity_mode == "strict"
    }

    /// Validate that all relative references in the resource point to existing resources.
    async fn validate_references(&self, resource: &JsonValue) -> Result<()> {
        let mut relative_refs = std::collections::HashSet::new();
        super::referential_integrity::collect_relative_refs(resource, &mut relative_refs);
        let relative_refs: Vec<(String, String)> = relative_refs.into_iter().collect();

        if relative_refs.is_empty() {
            return Ok(());
        }

        // Allow self-references: filter out refs that point to the resource itself
        let self_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let self_id = resource.get("id").and_then(|v| v.as_str()).unwrap_or("");

        let refs_to_check: Vec<(String, String)> = relative_refs
            .into_iter()
            .filter(|(rt, id)| !(rt == self_type && id == self_id))
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
            return Err(Error::BusinessRule(format!(
                "Referential integrity violation: the following referenced resources do not exist: {}",
                missing.join(", ")
            )));
        }

        Ok(())
    }

    /// Check that no other resources reference this resource before deletion.
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
            return Err(Error::BusinessRule(format!(
                "Referential integrity violation: cannot delete {}/{} because it is referenced by: {}",
                resource_type, id, refs.join(", ")
            )));
        }

        Ok(())
    }

    fn validate_resource_type_name(&self, resource_type: &str) -> Result<()> {
        if !is_known_resource_type(resource_type) {
            return Err(Error::Validation(format!(
                "Invalid resource type: {}",
                resource_type
            )));
        }

        Ok(())
    }

    /// Validate that resource type in JSON matches the endpoint
    fn validate_resource_type(&self, resource: &JsonValue, expected_type: &str) -> Result<()> {
        let resource_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidResource("Missing resourceType field".to_string()))?;

        if resource_type != expected_type {
            return Err(Error::InvalidResource(format!(
                "Resource type mismatch: expected {}, got {}",
                expected_type, resource_type
            )));
        }

        Ok(())
    }

    /// Populate resource meta fields per FHIR spec
    ///
    /// Per FHIR spec: "If the request body includes a meta, the server SHALL ignore
    /// the provided versionId and lastUpdated values."
    fn populate_meta(
        &self,
        resource: &mut JsonValue,
        id: &str,
        version_id: i32,
        last_updated: chrono::DateTime<Utc>,
    ) {
        if let Some(obj) = resource.as_object_mut() {
            // Set id
            obj.insert("id".to_string(), serde_json::json!(id));

            // Create or update meta
            let meta = obj
                .entry("meta".to_string())
                .or_insert_with(|| serde_json::json!({}));

            if let Some(meta_obj) = meta.as_object_mut() {
                // Log if client provided versionId or lastUpdated (which we'll ignore)
                if meta_obj.contains_key("versionId") || meta_obj.contains_key("lastUpdated") {
                    tracing::debug!(
                        "Ignoring client-provided meta.versionId and/or meta.lastUpdated \
                        (server will populate these values)"
                    );
                }

                // Overwrite with server-controlled values
                meta_obj.insert(
                    "versionId".to_string(),
                    serde_json::json!(version_id.to_string()),
                );
                // Truncate to microsecond precision to match PostgreSQL timestamptz storage.
                // Without this, nanosecond-precision timestamps (common on Linux) cause
                // cursor-based pagination mismatches: SQLx truncates nanos when writing
                // the last_updated column, but PostgreSQL rounds them when casting the
                // cursor string back to timestamptz.
                let us = (last_updated.timestamp_subsec_nanos() / 1_000) * 1_000;
                let last_updated_us = chrono::DateTime::from_timestamp(last_updated.timestamp(), us)
                    .unwrap_or(last_updated);
                meta_obj.insert(
                    "lastUpdated".to_string(),
                    serde_json::json!(last_updated_us.to_rfc3339()),
                );
            }
        }
    }

    async fn queue_indexing_job(&self, resource_type: &str, resource_ids: Vec<String>) {
        let Some(job_queue) = &self.job_queue else {
            return;
        };

        if resource_ids.is_empty() {
            return;
        }

        let parameters = serde_json::json!({
            "resource_type": resource_type,
            "resource_ids": resource_ids,
        });

        if let Err(e) = job_queue
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
}

// ============================================================================
// Conditional operation helpers
// ============================================================================
//
// NOTE: Conditional operations (If-None-Exist, conditional update, etc.) are now
// handled at the handler level using SearchEngine. The service layer does not
// have access to SearchEngine and should not perform search operations.
//
// See src/services/conditional.rs for the centralized conditional operation logic
// that will be used by handlers.

/// Validate version for conditional update (If-Match)
///
/// Per FHIR spec:
/// - If version matches: proceed with update
/// - If version doesn't match: return 409 Conflict
fn validate_version_match(resource: &Resource, expected_version: i32) -> Result<()> {
    if resource.version_id != expected_version {
        return Err(Error::VersionConflict {
            expected: expected_version,
            actual: resource.version_id,
        });
    }
    Ok(())
}
