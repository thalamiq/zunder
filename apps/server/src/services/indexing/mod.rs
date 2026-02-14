//! Indexing service - manages search parameter indexing.

use crate::models::Resource;
use crate::{db::IndexingRepository, Result};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use ferrum_fhirpath::{CompileOptions, Context, Engine as FhirPathEngine, Value as FhirPathValue};

mod resolver;

pub struct IndexingService {
    repo: IndexingRepository,
    pool: PgPool, // Still needed for complex insert operations and transactions
    fhirpath_engine: Arc<FhirPathEngine>,
    fhirpath_resolver: Arc<resolver::IndexingResourceResolver>,
    fhir_version: String,
    enable_text_search: bool,
    enable_content_search: bool,
    /// Cache of compiled FHIRPath plans by expression (indexing uses stable compile options).
    plan_cache: Arc<RwLock<HashMap<String, Arc<ferrum_fhirpath::vm::Plan>>>>,
    /// Cache of search parameters by resource type
    /// Key: resource_type, Value: Vec<SearchParameter>
    search_params_cache: Arc<RwLock<HashMap<String, Vec<SearchParameter>>>>,
    /// Batch size for regular indexing (reduces lock duration)
    batch_size: usize,
    /// Threshold to use COPY-based bulk indexing
    bulk_threshold: usize,
    /// Computed parameter hooks
    computed_hooks: crate::hooks::computed::HookRegistry,
}

impl IndexingService {
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn enable_text_search(&self) -> bool {
        self.enable_text_search
    }

    pub fn enable_content_search(&self) -> bool {
        self.enable_content_search
    }

    pub fn fhir_version(&self) -> &str {
        &self.fhir_version
    }

    pub fn new(
        pool: PgPool,
        fhir_version: &str,
        batch_size: usize,
        bulk_threshold: usize,
        enable_text_search: bool,
        enable_content_search: bool,
    ) -> Result<Self> {
        // IMPORTANT: Indexing runs inside DB transactions and must not perform additional
        // DB lookups during FHIRPath execution (can deadlock on small pools).
        //
        // We still need core StructureDefinitions so FHIRPath type operations like
        // `ofType(canonical)` / `ofType(uri)` work, so we use an in-memory core context.
        let core_context = crate::conformance::core_fhir_context(fhir_version)?;
        let fhirpath_resolver = Arc::new(resolver::IndexingResourceResolver::new_with_pool(
            pool.clone(),
            4096,
        ));
        let indexing_engine = Arc::new(FhirPathEngine::new(
            core_context,
            Some(fhirpath_resolver.clone()),
        ));

        let repo = IndexingRepository::new(pool.clone());

        Ok(Self {
            repo,
            pool,
            fhirpath_engine: indexing_engine,
            fhirpath_resolver,
            fhir_version: fhir_version.to_uppercase(),
            enable_text_search,
            enable_content_search,
            plan_cache: Arc::new(RwLock::new(HashMap::new())),
            search_params_cache: Arc::new(RwLock::new(HashMap::new())),
            batch_size,
            bulk_threshold,
            computed_hooks: crate::hooks::computed::HookRegistry::new(),
        })
    }

    /// Acquire advisory lock for resource to prevent concurrent indexing.
    ///
    /// Uses PostgreSQL transaction-level advisory locks (pg_advisory_xact_lock) to ensure
    /// only one worker can index a specific resource at a time. The lock is automatically
    /// released when the transaction commits or rolls back.
    ///
    /// This prevents the 102-second lock waits caused by:
    /// - IndexingWorker + SearchParameterWorker indexing same resource simultaneously
    /// - Foreign key constraint validation conflicts
    /// - ON CONFLICT DO UPDATE serialization
    async fn acquire_indexing_lock(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<()> {
        IndexingRepository::acquire_indexing_lock(tx, resource_type, resource_id).await
    }

    /// Update resource_search_index_status to track indexing coverage
    pub(super) async fn update_index_status(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource: &Resource,
        param_count: usize,
    ) -> Result<()> {
        self.repo
            .update_index_status(tx, resource, param_count)
            .await
    }

    fn get_or_compile_plan(
        &self,
        expr: &str,
    ) -> Result<(Arc<ferrum_fhirpath::vm::Plan>, bool, std::time::Duration)> {
        {
            let cache = self.plan_cache.read().unwrap();
            if let Some(plan) = cache.get(expr) {
                return Ok((plan.clone(), true, std::time::Duration::ZERO));
            }
        }

        let compile_start = std::time::Instant::now();
        let plan = self
            .fhirpath_engine
            .compile_with_options(
                expr,
                CompileOptions {
                    base_type: None,
                    strict: false,
                },
            )
            .map_err(|e| crate::Error::FhirPath(e.to_string()))?;
        let compile_time = compile_start.elapsed();

        {
            let mut cache = self.plan_cache.write().unwrap();
            cache.insert(expr.to_string(), plan.clone());
        }

        Ok((plan, false, compile_time))
    }

    /// Invalidate the cache for a specific resource type, or clear all if None
    pub fn invalidate_cache(&self, resource_type: Option<&str>) {
        let mut cache = self.search_params_cache.write().unwrap();
        if let Some(rt) = resource_type {
            // Base-type parameters (Resource/DomainResource) affect all resource types.
            if rt == "Resource" || rt == "DomainResource" {
                cache.clear();
                tracing::debug!(
                    "Cleared all search parameter cache entries (base type updated: {})",
                    rt
                );
            } else {
                cache.remove(rt);
                tracing::debug!(
                    "Invalidated search parameter cache for resource type: {}",
                    rt
                );
            }
        } else {
            cache.clear();
            tracing::debug!("Cleared all search parameter cache entries");
        }
    }

    /// Seed mapping from transaction `Bundle.entry.fullUrl` to resolved identity (`ResourceType/id`).
    ///
    /// This is used as an additional fallback for `resolve()` during indexing when resources
    /// still contain fullUrl-based references (e.g., `urn:uuid:...`) after transaction handling.
    pub fn seed_full_url_mapping<I>(&self, mapping: I)
    where
        I: IntoIterator<Item = (String, String)>,
    {
        self.fhirpath_resolver.seed_full_url_mapping(mapping);
    }

    /// Index a single resource
    pub async fn index_resource(&self, resource: &Resource) -> Result<()> {
        let search_params = self
            .fetch_search_parameters(&resource.resource_type)
            .await?;

        let needs_membership_indexes = matches!(
            resource.resource_type.as_str(),
            "CareTeam" | "Group" | "List"
        );

        if search_params.is_empty() && !needs_membership_indexes {
            return Ok(());
        }

        let needs_resolve = search_params.iter().any(|p| {
            p.expression
                .as_deref()
                .is_some_and(|expr| expr.contains("resolve("))
        });

        // Pre-warm reference resolution cache if any parameters for this type use `resolve()`.
        // This runs outside of the write transaction to avoid deadlocks on small pools.
        if needs_resolve {
            if let Err(e) = self
                .fhirpath_resolver
                .prewarm_cache_for_resource(&resource.resource)
                .await
            {
                tracing::debug!(
                    "FHIRPath resolver prewarm failed for {}/{}: {}",
                    resource.resource_type,
                    resource.id,
                    e
                );
            }
        }

        let mut tx = self.pool.begin().await.map_err(crate::Error::Database)?;

        // Acquire advisory lock to prevent concurrent indexing
        Self::acquire_indexing_lock(&mut tx, &resource.resource_type, &resource.id).await?;

        if !search_params.is_empty() {
            // Clear old entries
            self.clear_search_entries(
                &mut tx,
                &resource.resource_type,
                &resource.id,
                resource.version_id,
            )
            .await?;

            // Build the FHIRPath runtime context once per resource (expensive on large resources).
            let root = FhirPathValue::from_json(resource.resource.clone());
            // NOTE: `ferrum_fhirpath::Value::from_json()` uses lazy objects. The current
            // `resolve()` implementation expects reference objects to be materialized, so we
            // materialize the root only when at least one expression uses `resolve()`.
            let root = if needs_resolve {
                root.materialize()
            } else {
                root
            };
            let ctx = Context::new(root);

            // Extract and insert for each parameter
            for param in &search_params {
                if let Err(e) = self.process_parameter(&mut tx, resource, param, &ctx).await {
                    tracing::warn!("Failed to index parameter {}: {}", param.code, e);
                }
            }
        }

        // Update `_in` / `_list` membership indexes derived from collection resources.
        self.update_membership_indexes(&mut tx, resource).await?;

        // Update indexing status before commit
        self.update_index_status(&mut tx, resource, search_params.len())
            .await?;

        tx.commit().await.map_err(crate::Error::Database)?;
        Ok(())
    }

    /// Index multiple resources of the same type in a single transaction
    ///
    /// More efficient than calling index_resource repeatedly:
    /// - Single transaction for all resources
    /// - Search parameters fetched once per type
    /// - Reduced database round-trips
    pub async fn index_resources_batch(&self, resources: &[Resource]) -> Result<()> {
        if resources.is_empty() {
            return Ok(());
        }

        let batch_start = std::time::Instant::now();

        // Group by resource type
        let mut by_type: HashMap<String, Vec<&Resource>> = HashMap::new();
        for resource in resources {
            by_type
                .entry(resource.resource_type.clone())
                .or_default()
                .push(resource);
        }

        tracing::debug!(
            "Grouped {} resources into {} types in {:?}",
            resources.len(),
            by_type.len(),
            batch_start.elapsed()
        );

        // Fetch search parameters for all types
        let mut params_by_type: HashMap<String, Vec<SearchParameter>> = HashMap::new();
        let mut resolve_by_type: HashMap<String, bool> = HashMap::new();
        for resource_type in by_type.keys() {
            let search_params = self.fetch_search_parameters(resource_type).await?;
            if !search_params.is_empty()
                || matches!(resource_type.as_str(), "CareTeam" | "Group" | "List")
            {
                let needs_resolve = search_params.iter().any(|p| {
                    p.expression
                        .as_deref()
                        .is_some_and(|expr| expr.contains("resolve("))
                });
                resolve_by_type.insert(resource_type.clone(), needs_resolve);
                params_by_type.insert(resource_type.clone(), search_params);
            }
        }

        // Pre-warm reference resolution cache for resources whose search parameters use `resolve()`.
        // This happens before we open the batch write transaction.
        for (resource_type, type_resources) in &by_type {
            let Some(search_params) = params_by_type.get(resource_type) else {
                continue;
            };
            let needs_resolve = resolve_by_type
                .get(resource_type)
                .copied()
                .unwrap_or_else(|| {
                    search_params.iter().any(|p| {
                        p.expression
                            .as_deref()
                            .is_some_and(|expr| expr.contains("resolve("))
                    })
                });
            if !needs_resolve {
                continue;
            }
            for resource in type_resources {
                if let Err(e) = self
                    .fhirpath_resolver
                    .prewarm_cache_for_resource(&resource.resource)
                    .await
                {
                    tracing::debug!(
                        "FHIRPath resolver prewarm failed for {}/{}: {}",
                        resource.resource_type,
                        resource.id,
                        e
                    );
                }
            }
        }

        // CRITICAL: Single transaction for ENTIRE batch (not per-type)
        let tx_start = std::time::Instant::now();
        let mut tx = self.pool.begin().await.map_err(crate::Error::Database)?;
        tracing::debug!("Started batch transaction in {:?}", tx_start.elapsed());

        // Track time spent in different operations
        let mut total_clear_time = std::time::Duration::ZERO;
        let mut total_process_time = std::time::Duration::ZERO;

        // Process all resources across all types in single transaction
        for (resource_type, type_resources) in &by_type {
            let type_start = std::time::Instant::now();

            let search_params = match params_by_type.get(resource_type) {
                Some(params) => params,
                None => {
                    tracing::debug!("No search parameters for {}, skipping", resource_type);
                    continue;
                }
            };

            // Build parameter codes set for smart DELETE detection
            let param_codes: std::collections::HashSet<String> =
                search_params.iter().map(|p| p.code.clone()).collect();

            let count = type_resources.len();
            tracing::info!(
                "Processing {} {} resources with {} search params",
                count,
                resource_type,
                search_params.len()
            );

            // OPTIMIZATION: Batch fetch all old parameters for all resources of this type at once
            // This eliminates the N+1 query problem (1 query instead of N queries)
            let clear_start = std::time::Instant::now();
            let resource_ids: Vec<&str> = type_resources.iter().map(|r| r.id.as_str()).collect();
            let old_params_map = self
                .batch_fetch_old_parameters(&mut tx, resource_type, &resource_ids)
                .await?;
            total_clear_time += clear_start.elapsed();

            // Process all resources of this type
            for resource in type_resources {
                // Acquire advisory lock to prevent concurrent indexing of same resource
                // This eliminates 102-second lock waits when IndexingWorker + SearchParameterWorker
                // try to index the same resource simultaneously
                Self::acquire_indexing_lock(&mut tx, &resource.resource_type, &resource.id).await?;

                // Smart DELETE: only delete if parameters were removed
                let clear_start = std::time::Instant::now();

                // Get the pre-fetched old parameters for this resource
                let old_params = old_params_map
                    .get(resource.id.as_str())
                    .map(|set| set.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                if let Err(e) = self
                    .clear_search_entries_batch(
                        &mut tx,
                        &resource.resource_type,
                        &resource.id,
                        &old_params,
                        &param_codes,
                    )
                    .await
                {
                    tracing::warn!(
                        "Failed to clear search entries for {}/{}: {}",
                        resource.resource_type,
                        resource.id,
                        e
                    );
                    continue;
                }
                total_clear_time += clear_start.elapsed();

                if !search_params.is_empty() {
                    // Build the FHIRPath runtime context once per resource (expensive on large resources).
                    let root = FhirPathValue::from_json(resource.resource.clone());
                    let needs_resolve =
                        resolve_by_type.get(resource_type).copied().unwrap_or(false);
                    let root = if needs_resolve {
                        root.materialize()
                    } else {
                        root
                    };
                    let ctx = Context::new(root);

                    // Extract and insert for each parameter
                    let process_start = std::time::Instant::now();
                    for param in search_params {
                        if let Err(e) = self.process_parameter(&mut tx, resource, param, &ctx).await
                        {
                            tracing::warn!(
                                "Failed to index parameter {} for {}/{}: {}",
                                param.code,
                                resource.resource_type,
                                resource.id,
                                e
                            );
                        }
                    }
                    total_process_time += process_start.elapsed();
                }

                // Update `_in` / `_list` membership indexes derived from collection resources.
                if let Err(e) = self.update_membership_indexes(&mut tx, resource).await {
                    tracing::warn!(
                        "Failed to index collection memberships for {}/{}: {}",
                        resource.resource_type,
                        resource.id,
                        e
                    );
                }
            }

            tracing::debug!(
                "Completed {} {} resources in {:?}",
                count,
                resource_type,
                type_start.elapsed()
            );
        }

        // Update indexing status for all resources
        for (resource_type, type_resources) in &by_type {
            let search_params = match params_by_type.get(resource_type) {
                Some(params) => params,
                None => continue,
            };

            for resource in type_resources {
                if let Err(e) = self
                    .update_index_status(&mut tx, resource, search_params.len())
                    .await
                {
                    tracing::warn!(
                        "Failed to update index status for {}/{}: {}",
                        resource.resource_type,
                        resource.id,
                        e
                    );
                }
            }
        }

        // Single commit for all resources
        let commit_start = std::time::Instant::now();
        let batch_duration = batch_start.elapsed();

        // Warn about long-running transactions (potential contention)
        if batch_duration.as_secs() > 1 {
            tracing::warn!(
                "Long-running indexing transaction for {} resources: {:?} (clear={:?}, process={:?})",
                resources.len(),
                batch_duration,
                total_clear_time,
                total_process_time
            );
        }

        tx.commit().await.map_err(crate::Error::Database)?;
        let commit_time = commit_start.elapsed();

        tracing::info!(
            "Batch indexed {} resources: clear={:?}, process={:?}, commit={:?}, total={:?}",
            resources.len(),
            total_clear_time,
            total_process_time,
            commit_time,
            batch_duration
        );

        Ok(())
    }

    /// Auto-batching: automatically chooses best indexing strategy based on batch size
    ///
    /// Strategy selection:
    /// - < 1,000 resources: Regular batch indexing (INSERT with ON CONFLICT DO UPDATE)
    /// - >= 10,000 resources: COPY-based bulk loading (100-500x faster)
    /// - 1,000-9,999 resources: Split into 1,000-resource batches
    ///
    /// Expected performance:
    /// - Small batches (<1K): 2,000-5,000 resources/sec
    /// - Large batches (>=10K): 10,000-50,000 resources/sec
    pub async fn index_resources_auto(&self, resources: &[Resource]) -> Result<()> {
        if resources.is_empty() {
            return Ok(());
        }

        let total = resources.len();

        if total >= self.bulk_threshold {
            // Use COPY for massive batches (100-500x faster)
            tracing::info!(
                "Using COPY-based bulk indexing for {} resources (threshold: {})",
                total,
                self.bulk_threshold
            );
            let bulk_indexer = BulkIndexer::new(self.pool.clone());
            bulk_indexer.bulk_index_with_copy(resources, self).await
        } else if total > self.batch_size {
            // Split into optimal batches and process in sequence
            tracing::info!(
                "Splitting {} resources into batches of {}",
                total,
                self.batch_size
            );
            for (i, chunk) in resources.chunks(self.batch_size).enumerate() {
                tracing::debug!(
                    "Processing batch {}/{}",
                    i + 1,
                    total.div_ceil(self.batch_size)
                );
                self.index_resources_batch(chunk).await?;
            }
            Ok(())
        } else {
            // Normal batch indexing for small batches
            self.index_resources_batch(resources).await
        }
    }

    pub(super) async fn fetch_search_parameters(
        &self,
        resource_type: &str,
    ) -> Result<Vec<SearchParameter>> {
        // Check cache first
        {
            let cache = self.search_params_cache.read().unwrap();
            if let Some(params) = cache.get(resource_type) {
                return Ok(params.clone());
            }
        }

        // Cache miss - fetch from database
        // Component metadata (component_code, component_type) is preloaded at write time
        // This avoids expensive JOINs during indexing operations
        let mut params = sqlx::query_as::<_, SearchParameter>(
            r#"
            SELECT
                sp.id,
                sp.code,
                sp.type,
                sp.expression,
                CASE
                    WHEN sp.type = 'composite' THEN (
                        SELECT jsonb_agg(
                            jsonb_build_object(
                                'position', c.position,
                                'definition_url', c.definition_url,
                                'expression', c.expression,
                                'component_code', c.component_code,
                                'component_type', c.component_type
                            )
                            ORDER BY c.position
                        )
                        FROM search_parameter_components c
                        WHERE c.search_parameter_id = sp.id
                          AND c.component_code IS NOT NULL
                          AND c.component_type IS NOT NULL
                    )
                    ELSE NULL
                END AS components
            FROM search_parameters sp
            WHERE
                sp.active = TRUE
                AND (sp.resource_type = $1 OR sp.resource_type IN ('DomainResource', 'Resource'))
            ORDER BY
                CASE
                    WHEN sp.resource_type = $1 THEN 0
                    WHEN sp.resource_type = 'DomainResource' THEN 1
                    WHEN sp.resource_type = 'Resource' THEN 2
                    ELSE 3
                END,
                sp.id
            "#,
        )
        .bind(resource_type)
        .fetch_all(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        // If a parameter is defined for both the concrete resource type and a base type
        // (Resource/DomainResource), prefer the concrete definition.
        let mut seen = std::collections::HashSet::new();
        params.retain(|p| seen.insert(p.code.clone()));

        // Store in cache
        {
            let mut cache = self.search_params_cache.write().unwrap();
            cache.insert(resource_type.to_string(), params.clone());
        }

        Ok(params)
    }

    /// Batch fetch old parameter names for multiple resources in a single query.
    /// Returns a map of resource_id -> set of parameter names.
    ///
    /// This eliminates the N+1 query problem when indexing batches of resources.
    async fn batch_fetch_old_parameters(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_type: &str,
        resource_ids: &[&str],
    ) -> Result<HashMap<String, std::collections::HashSet<String>>> {
        if resource_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut q = String::from(
            r#"
            SELECT resource_id, parameter_name FROM (
                SELECT resource_id, parameter_name FROM search_token
                WHERE resource_type = $1 AND resource_id = ANY($2)
                UNION
                SELECT resource_id, parameter_name FROM search_string
                WHERE resource_type = $1 AND resource_id = ANY($2)
                UNION
                SELECT resource_id, parameter_name FROM search_date
                WHERE resource_type = $1 AND resource_id = ANY($2)
                UNION
                SELECT resource_id, parameter_name FROM search_number
                WHERE resource_type = $1 AND resource_id = ANY($2)
                UNION
                SELECT resource_id, parameter_name FROM search_quantity
                WHERE resource_type = $1 AND resource_id = ANY($2)
                UNION
                SELECT resource_id, parameter_name FROM search_reference
                WHERE resource_type = $1 AND resource_id = ANY($2)
                UNION
                SELECT resource_id, parameter_name FROM search_uri
                WHERE resource_type = $1 AND resource_id = ANY($2)
"#,
        );

        if self.enable_text_search {
            q.push_str(
                r#"
                UNION
                SELECT resource_id, parameter_name FROM search_text
                WHERE resource_type = $1 AND resource_id = ANY($2)
"#,
            );
        }
        if self.enable_content_search {
            q.push_str(
                r#"
                UNION
                SELECT resource_id, parameter_name FROM search_content
                WHERE resource_type = $1 AND resource_id = ANY($2)
"#,
            );
        }

        q.push_str(
            r#"
                UNION
                SELECT resource_id, parameter_name FROM search_composite
                WHERE resource_type = $1 AND resource_id = ANY($2)
            ) AS all_params
            "#,
        );

        // Single query to fetch all parameter names for all resources
        let rows = sqlx::query(&q)
            .bind(resource_type)
            .bind(resource_ids)
            .fetch_all(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;

        // Build map of resource_id -> set of parameter names
        let mut result: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
        for row in rows {
            let resource_id: String = row.try_get("resource_id").map_err(crate::Error::Database)?;
            let parameter_name: String = row
                .try_get("parameter_name")
                .map_err(crate::Error::Database)?;
            result
                .entry(resource_id)
                .or_default()
                .insert(parameter_name);
        }

        Ok(result)
    }

    /// Clear search entries using pre-fetched old parameters (batch-optimized version).
    /// Only deletes parameters that were removed from the resource.
    async fn clear_search_entries_batch(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_type: &str,
        id: &str,
        old_params: &[&str],
        current_param_codes: &std::collections::HashSet<String>,
    ) -> Result<()> {
        // Find parameters that were removed (exist in index but not in current resource)
        let removed_params: Vec<_> = old_params
            .iter()
            .filter(|p| !current_param_codes.contains(**p))
            .collect();

        if removed_params.is_empty() {
            // Common case: No parameters removed, skip DELETE entirely!
            // ON CONFLICT DO UPDATE in insert functions will handle value updates.
            tracing::trace!(
                "Skipping DELETE for {}/{} - no parameters removed",
                resource_type,
                id
            );
            return Ok(());
        }

        // Rare case: Some parameters were removed, delete only those
        tracing::debug!(
            "Deleting {} removed parameters for {}/{}: {:?}",
            removed_params.len(),
            resource_type,
            id,
            removed_params
        );

        for param in removed_params {
            // Delete from all tables for this specific parameter
            // Much faster than deleting from all tables unconditionally
            let mut del = String::from(
                "DELETE FROM search_token WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_string WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_date WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_number WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_quantity WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_reference WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_uri WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;",
            );
            if self.enable_text_search {
                del.push_str(
                    "\n                 DELETE FROM search_text WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;",
                );
            }
            if self.enable_content_search {
                del.push_str(
                    "\n                 DELETE FROM search_content WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;",
                );
            }
            del.push_str(
                "\n                 DELETE FROM search_composite WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;",
            );

            sqlx::query(&del)
                .bind(resource_type)
                .bind(id)
                .bind(*param)
                .execute(&mut **tx)
                .await
                .map_err(crate::Error::Database)?;
        }

        Ok(())
    }

    /// Smart DELETE strategy: Only delete when parameters are removed from resource.
    /// 95% of reindexing operations can skip DELETE entirely by using ON CONFLICT DO UPDATE.
    ///
    /// NOTE: This is the single-resource version. Use batch_fetch_old_parameters +
    /// clear_search_entries_batch for better performance when indexing multiple resources.
    #[allow(dead_code)]
    async fn clear_search_entries_if_needed(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_type: &str,
        id: &str,
        current_param_codes: &std::collections::HashSet<String>,
    ) -> Result<()> {
        // Query all currently indexed parameter names for this resource
        let mut q = String::from(
            "SELECT DISTINCT parameter_name FROM search_token
             WHERE resource_type = $1 AND resource_id = $2
             UNION
             SELECT DISTINCT parameter_name FROM search_string
             WHERE resource_type = $1 AND resource_id = $2
             UNION
             SELECT DISTINCT parameter_name FROM search_date
             WHERE resource_type = $1 AND resource_id = $2
             UNION
             SELECT DISTINCT parameter_name FROM search_number
             WHERE resource_type = $1 AND resource_id = $2
             UNION
             SELECT DISTINCT parameter_name FROM search_quantity
             WHERE resource_type = $1 AND resource_id = $2
             UNION
             SELECT DISTINCT parameter_name FROM search_reference
             WHERE resource_type = $1 AND resource_id = $2
             UNION
             SELECT DISTINCT parameter_name FROM search_uri
             WHERE resource_type = $1 AND resource_id = $2",
        );
        if self.enable_text_search {
            q.push_str(
                "\n             UNION\n             SELECT DISTINCT parameter_name FROM search_text\n             WHERE resource_type = $1 AND resource_id = $2",
            );
        }
        if self.enable_content_search {
            q.push_str(
                "\n             UNION\n             SELECT DISTINCT parameter_name FROM search_content\n             WHERE resource_type = $1 AND resource_id = $2",
            );
        }
        q.push_str(
            "\n             UNION\n             SELECT DISTINCT parameter_name FROM search_composite\n             WHERE resource_type = $1 AND resource_id = $2",
        );

        let old_params = sqlx::query_scalar::<_, String>(&q)
            .bind(resource_type)
            .bind(id)
            .fetch_all(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;

        // Find parameters that were removed (exist in index but not in current resource)
        let removed_params: Vec<_> = old_params
            .into_iter()
            .filter(|p| !current_param_codes.contains(p))
            .collect();

        if removed_params.is_empty() {
            // Common case: No parameters removed, skip DELETE entirely!
            // ON CONFLICT DO UPDATE in insert functions will handle value updates.
            tracing::trace!(
                "Skipping DELETE for {}/{} - no parameters removed",
                resource_type,
                id
            );
            return Ok(());
        }

        // Rare case: Some parameters were removed, delete only those
        tracing::debug!(
            "Deleting {} removed parameters for {}/{}: {:?}",
            removed_params.len(),
            resource_type,
            id,
            removed_params
        );

        for param in removed_params {
            // Delete from all tables for this specific parameter
            // Much faster than deleting from all tables unconditionally
            let mut del = String::from(
                "DELETE FROM search_token WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_string WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_date WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_number WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_quantity WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_reference WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;
                 DELETE FROM search_uri WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;",
            );
            if self.enable_text_search {
                del.push_str(
                    "\n                 DELETE FROM search_text WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;",
                );
            }
            if self.enable_content_search {
                del.push_str(
                    "\n                 DELETE FROM search_content WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;",
                );
            }
            del.push_str(
                "\n                 DELETE FROM search_composite WHERE resource_type = $1 AND resource_id = $2 AND parameter_name = $3;",
            );

            sqlx::query(&del)
                .bind(resource_type)
                .bind(id)
                .bind(&param)
                .execute(&mut **tx)
                .await
                .map_err(crate::Error::Database)?;
        }

        Ok(())
    }

    /// Legacy DELETE function - kept for backwards compatibility and full reindexing.
    /// Use clear_search_entries_if_needed() for normal operations.
    #[allow(dead_code)]
    async fn clear_search_entries(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_type: &str,
        id: &str,
        version: i32,
    ) -> Result<()> {
        let mut ctes: Vec<&'static str> = vec![
            "del_string AS (DELETE FROM search_string WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            "del_token AS (DELETE FROM search_token WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            "del_token_identifier AS (DELETE FROM search_token_identifier WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            "del_date AS (DELETE FROM search_date WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            "del_number AS (DELETE FROM search_number WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            "del_reference AS (DELETE FROM search_reference WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            "del_composite AS (DELETE FROM search_composite WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            "del_uri AS (DELETE FROM search_uri WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
        ];
        if self.enable_text_search {
            ctes.push(
                "del_text AS (DELETE FROM search_text WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            );
        }
        if self.enable_content_search {
            ctes.push(
                "del_content AS (DELETE FROM search_content WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3)",
            );
        }

        let q = format!(
            "WITH {} DELETE FROM search_quantity WHERE resource_type = $1 AND resource_id = $2 AND version_id = $3",
            ctes.join(", ")
        );

        sqlx::query(&q)
            .bind(resource_type)
            .bind(id)
            .bind(version)
            .execute(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;

        Ok(())
    }

    async fn process_parameter(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource: &Resource,
        param: &SearchParameter,
        ctx: &Context,
    ) -> Result<()> {
        let param_start = std::time::Instant::now();

        // Check for computed parameter hook
        if let Some(hook) = self
            .computed_hooks
            .find_index_hook(&resource.resource_type, &param.code)
        {
            hook.index(tx, resource, param, ctx, &self.fhirpath_engine)
                .await?;
            return Ok(());
        }

        match param.r#type.as_str() {
            "composite" => {
                let insert_start = std::time::Instant::now();
                self.insert_composite_values(tx, resource, param).await?;
                tracing::trace!(
                    "  {} composite insert: {:?}",
                    param.code,
                    insert_start.elapsed()
                );
                return Ok(());
            }
            // `_text` / `_content` are full-resource indexes and typically have no FHIRPath expression.
            "text" => {
                if !self.enable_text_search {
                    return Ok(());
                }
                let insert_start = std::time::Instant::now();
                self.insert_text_values(tx, resource, &param.code).await?;
                tracing::trace!("  {} text insert: {:?}", param.code, insert_start.elapsed());
                return Ok(());
            }
            "content" => {
                if !self.enable_content_search {
                    return Ok(());
                }
                let insert_start = std::time::Instant::now();
                self.insert_content_values(tx, resource, &param.code)
                    .await?;
                tracing::trace!(
                    "  {} content insert: {:?}",
                    param.code,
                    insert_start.elapsed()
                );
                return Ok(());
            }
            _ => {}
        }

        if param.expression.is_none() {
            return Ok(());
        }

        let expression = param.expression.as_ref().unwrap();

        // Compile once per distinct expression (per-process cache) and execute against this resource.
        let (plan, plan_hit, compile_time) = self.get_or_compile_plan(expression)?;
        let eval_start = std::time::Instant::now();
        let collection = self
            .fhirpath_engine
            .evaluate(&plan, ctx)
            .map_err(|e| crate::Error::FhirPath(e.to_string()))?;
        let eval_time = eval_start.elapsed();
        let result_count = collection.len();

        let insert_start = std::time::Instant::now();
        let insert_stats = match param.r#type.as_str() {
            "string" => {
                self.insert_string_values(tx, resource, &param.code, &collection)
                    .await?
            }
            "token" => {
                self.insert_token_values(tx, resource, &param.code, &collection)
                    .await?
            }
            "date" => {
                self.insert_date_values(tx, resource, &param.code, &collection)
                    .await?
            }
            "number" => {
                self.insert_number_values(tx, resource, &param.code, &collection)
                    .await?
            }
            "quantity" => {
                self.insert_quantity_values(tx, resource, &param.code, &collection)
                    .await?
            }
            "reference" => {
                self.insert_reference_values(tx, resource, &param.code, &collection)
                    .await?
            }
            "uri" => {
                self.insert_uri_values(tx, resource, &param.code, &collection)
                    .await?
            }
            _ => insert::InsertStats::default(), // Skip unsupported types
        };
        let insert_time = insert_start.elapsed();

        // Log any parameter that takes >2ms (to identify bottlenecks)
        let total_time = param_start.elapsed();
        if total_time.as_millis() > 2 {
            tracing::info!(
                "  Slow parameter {}.{}: compile={:?} (hit={}), eval={:?} (n={}), insert={:?} (rows={}, aux_rows={}), total={:?}",
                resource.resource_type,
                param.code,
                compile_time,
                plan_hit,
                eval_time,
                result_count,
                insert_time,
                insert_stats.rows,
                insert_stats.aux_rows,
                total_time
            );
        }

        Ok(())
    }

    pub async fn remove_resource_index(&self, resource_type: &str, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(crate::Error::Database)?;

        sqlx::query(
            "WITH del_string AS (DELETE FROM search_string WHERE resource_type = $1 AND resource_id = $2),
             del_token AS (DELETE FROM search_token WHERE resource_type = $1 AND resource_id = $2),
             del_token_identifier AS (DELETE FROM search_token_identifier WHERE resource_type = $1 AND resource_id = $2),
             del_date AS (DELETE FROM search_date WHERE resource_type = $1 AND resource_id = $2),
             del_number AS (DELETE FROM search_number WHERE resource_type = $1 AND resource_id = $2),
             del_reference AS (DELETE FROM search_reference WHERE resource_type = $1 AND resource_id = $2),
             del_composite AS (DELETE FROM search_composite WHERE resource_type = $1 AND resource_id = $2),
             del_uri AS (DELETE FROM search_uri WHERE resource_type = $1 AND resource_id = $2),
             del_text AS (DELETE FROM search_text WHERE resource_type = $1 AND resource_id = $2),
             del_content AS (DELETE FROM search_content WHERE resource_type = $1 AND resource_id = $2)
             DELETE FROM search_quantity WHERE resource_type = $1 AND resource_id = $2",
        )
        .bind(resource_type)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(crate::Error::Database)?;

        tx.commit().await.map_err(crate::Error::Database)?;
        Ok(())
    }
}

#[derive(sqlx::FromRow, Clone)]
pub(crate) struct SearchParameter {
    #[allow(dead_code)]
    id: i32,
    pub(crate) code: String,
    #[sqlx(rename = "type")]
    pub(super) r#type: String,
    pub(super) expression: Option<String>,
    pub(super) components: Option<serde_json::Value>,
}

/// Helper to get or compile FHIRPath plan for a search parameter (used by bulk indexer)
pub(super) fn get_or_compile_plan(
    param: &SearchParameter,
    fhir_version: &str,
) -> Result<Option<Arc<ferrum_fhirpath::vm::Plan>>> {
    use std::sync::OnceLock;

    // Static engine for plan compilation (bulk indexing doesn't have access to IndexingService)
    // Use a HashMap keyed by version to support multiple versions if needed
    static GLOBAL_PLAN_CACHE: OnceLock<Arc<RwLock<HashMap<String, Arc<ferrum_fhirpath::vm::Plan>>>>> =
        OnceLock::new();
    static GLOBAL_ENGINES: OnceLock<Arc<RwLock<HashMap<String, Arc<FhirPathEngine>>>>> =
        OnceLock::new();

    let cache = GLOBAL_PLAN_CACHE.get_or_init(|| Arc::new(RwLock::new(HashMap::new())));
    let engines = GLOBAL_ENGINES.get_or_init(|| Arc::new(RwLock::new(HashMap::new())));

    // Get or create engine for this version
    let engine = {
        let mut engines_write = engines.write().unwrap();
        engines_write
            .entry(fhir_version.to_string())
            .or_insert_with(|| {
                let core_context = crate::conformance::core_fhir_context(fhir_version)
                    .unwrap_or_else(|_| {
                        panic!("Failed to load FHIR context for version: {}", fhir_version)
                    });
                let resolver = Arc::new(resolver::IndexingResourceResolver::new_stub(4096));
                Arc::new(FhirPathEngine::new(core_context, Some(resolver)))
            })
            .clone()
    };

    let expression = match &param.expression {
        Some(expr) => expr,
        None => return Ok(None),
    };

    // Check cache first
    {
        let cache_read = cache.read().unwrap();
        if let Some(plan) = cache_read.get(expression) {
            return Ok(Some(plan.clone()));
        }
    }

    // Compile and cache
    let plan = engine
        .compile_with_options(
            expression,
            CompileOptions {
                base_type: None,
                strict: false,
            },
        )
        .map_err(|e| crate::Error::FhirPath(e.to_string()))?;

    {
        let mut cache_write = cache.write().unwrap();
        cache_write.insert(expression.to_string(), plan.clone());
    }

    Ok(Some(plan))
}

mod bulk;
mod composite;
mod extract;
mod insert;
mod membership;
mod text;

pub use bulk::BulkIndexer;
pub(crate) use extract::extract_date_ranges;
use extract::*;
use membership::rebuild_memberships_for_resource;

impl IndexingService {
    pub(super) async fn update_membership_indexes(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource: &Resource,
    ) -> Result<()> {
        rebuild_memberships_for_resource(tx, resource).await
    }
}
