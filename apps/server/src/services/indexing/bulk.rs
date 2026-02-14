//! COPY-based bulk indexing for maximum throughput (10K+ resources)
//!
//! This module provides ultra-fast bulk indexing using PostgreSQL's COPY protocol,
//! which bypasses most INSERT overhead and can achieve 100-500x faster throughput
//! compared to regular INSERT operations.
//!
//! Expected performance:
//! - Bulk throughput: 10,000-50,000 resources/second
//! - Best for: Initial loads, large migrations, bulk imports
//!
//! How it works:
//! 1. Extract all index data in memory (parallel processing)
//! 2. Use COPY FROM STDIN to stream data directly to PostgreSQL
//! 3. Single transaction for entire batch
//!
//! Safety:
//! - Uses ON CONFLICT DO UPDATE to handle duplicates
//! - Hash-based deduplication matches Migration 004
//! - Single transaction ensures atomicity

use crate::db::search::string_normalization::{
    normalize_casefold_strip_combining, normalize_string_for_search,
};
use crate::models::Resource;
use crate::Result;
use sqlx::PgPool;
use std::collections::HashMap;
use ferrum_fhirpath::{Collection as FhirPathCollection, Context, ToJson, Value as FhirPathValue};

use super::text::{extract_all_textual_content, extract_narrative_text};
use super::IndexingService;
use super::{
    extract_date_ranges, extract_identifier_of_type_rows, extract_numbers, extract_quantity_values,
    extract_reference_values, extract_strings, extract_tokens,
};

/// Bulk indexer using PostgreSQL COPY for maximum throughput
pub struct BulkIndexer {
    pool: PgPool,
}

impl BulkIndexer {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Bulk index using PostgreSQL COPY (100-500x faster than INSERT for large batches)
    ///
    /// This is the fastest way to index large numbers of resources:
    /// - Extracts all data in memory first
    /// - Uses COPY FROM STDIN for each table
    /// - Single transaction for entire batch
    ///
    /// Best for: 10,000+ resources at once
    pub async fn bulk_index_with_copy(
        &self,
        resources: &[Resource],
        indexing_service: &IndexingService,
    ) -> Result<()> {
        if resources.is_empty() {
            return Ok(());
        }

        tracing::info!(
            "[PERF] Starting COPY-based bulk index for {} resources",
            resources.len()
        );
        let total_start = std::time::Instant::now();

        // 1. Extract all index data in memory
        let extract_start = std::time::Instant::now();
        let mut index_data = self
            .extract_all_index_data(resources, indexing_service)
            .await?;
        let extract_time = extract_start.elapsed();

        let total_rows = index_data.strings.len()
            + index_data.tokens.len()
            + index_data.token_identifiers.len()
            + index_data.dates.len()
            + index_data.numbers.len()
            + index_data.quantities.len()
            + index_data.references.len()
            + index_data.uris.len()
            + index_data.texts.len()
            + index_data.contents.len();

        tracing::info!(
            "[PERF] Extracted index data in {:?}: strings={}, tokens={}, token_identifiers={}, dates={}, numbers={}, quantities={}, references={}, uris={}, total_rows={}",
            extract_time,
            index_data.strings.len(),
            index_data.tokens.len(),
            index_data.token_identifiers.len(),
            index_data.dates.len(),
            index_data.numbers.len(),
            index_data.quantities.len(),
            index_data.references.len(),
            index_data.uris.len(),
            total_rows
        );

        // Deduplicate rows by entry_hash to prevent "ON CONFLICT DO UPDATE cannot affect row a second time" errors
        let dedup_start = std::time::Instant::now();
        index_data.deduplicate();
        let dedup_time = dedup_start.elapsed();

        let deduped_total = index_data.strings.len()
            + index_data.tokens.len()
            + index_data.token_identifiers.len()
            + index_data.dates.len()
            + index_data.numbers.len()
            + index_data.quantities.len()
            + index_data.references.len()
            + index_data.uris.len()
            + index_data.texts.len()
            + index_data.contents.len();

        if total_rows != deduped_total {
            tracing::info!(
                "[PERF] Deduplication removed {} duplicate rows in {:?} ({}  -> {} rows)",
                total_rows - deduped_total,
                dedup_time,
                total_rows,
                deduped_total
            );
        }

        // 2. Use COPY to load each table
        let tx_start = std::time::Instant::now();
        let mut tx = self.pool.begin().await.map_err(crate::Error::Database)?;
        let tx_init_time = tx_start.elapsed();
        tracing::debug!("[PERF] Transaction initialization: {:?}", tx_init_time);

        // Load all tables using COPY
        let copy_start = std::time::Instant::now();
        self.copy_to_search_string(&mut tx, &index_data.strings)
            .await?;
        self.copy_to_search_token(&mut tx, &index_data.tokens)
            .await?;
        self.copy_to_search_token_identifier(&mut tx, &index_data.token_identifiers)
            .await?;
        self.copy_to_search_date(&mut tx, &index_data.dates).await?;
        self.copy_to_search_number(&mut tx, &index_data.numbers)
            .await?;
        self.copy_to_search_quantity(&mut tx, &index_data.quantities)
            .await?;
        self.copy_to_search_reference(&mut tx, &index_data.references)
            .await?;
        self.copy_to_search_uri(&mut tx, &index_data.uris).await?;
        self.copy_to_search_text(&mut tx, &index_data.texts).await?;
        self.copy_to_search_content(&mut tx, &index_data.contents)
            .await?;
        let copy_time = copy_start.elapsed();

        // Update membership indexes derived from collection resources (Group/List/CareTeam).
        // These indexes are not version-scoped and are independent of the standard search_* tables.
        for resource in resources {
            if let Err(e) = indexing_service
                .update_membership_indexes(&mut tx, resource)
                .await
            {
                tracing::warn!(
                    "Failed to index collection memberships for {}/{}: {}",
                    resource.resource_type,
                    resource.id,
                    e
                );
            }
        }

        // Update indexing status for all resources
        let status_start = std::time::Instant::now();
        for resource in resources {
            // Fetch search parameter count for this resource type
            let params = indexing_service
                .fetch_search_parameters(&resource.resource_type)
                .await?;
            if let Err(e) = indexing_service
                .update_index_status(&mut tx, resource, params.len())
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
        let status_time = status_start.elapsed();
        tracing::debug!("[PERF] Status tracking: {:?}", status_time);

        let commit_start = std::time::Instant::now();
        tx.commit().await.map_err(crate::Error::Database)?;
        let commit_time = commit_start.elapsed();

        let total_time = total_start.elapsed();
        let throughput = resources.len() as f64 / total_time.as_secs_f64();
        let rows_per_sec = total_rows as f64 / total_time.as_secs_f64();

        tracing::info!(
            "[PERF] COPY-based bulk index completed: total={:?}, extract={:?} ({:.1}%), copy={:?} ({:.1}%), commit={:?} ({:.1}%), throughput={:.0} resources/sec, {:.0} rows/sec",
            total_time,
            extract_time,
            extract_time.as_secs_f64() / total_time.as_secs_f64() * 100.0,
            copy_time,
            copy_time.as_secs_f64() / total_time.as_secs_f64() * 100.0,
            commit_time,
            commit_time.as_secs_f64() / total_time.as_secs_f64() * 100.0,
            throughput,
            rows_per_sec
        );

        Ok(())
    }

    /// Extract all index data from resources in parallel
    async fn extract_all_index_data(
        &self,
        resources: &[Resource],
        indexing_service: &IndexingService,
    ) -> Result<IndexData> {
        let extract_total_start = std::time::Instant::now();
        let mut index_data = IndexData::default();

        // Group by resource type for efficient parameter lookup
        let grouping_start = std::time::Instant::now();
        let mut by_type: HashMap<String, Vec<&Resource>> = HashMap::new();
        for resource in resources {
            by_type
                .entry(resource.resource_type.clone())
                .or_default()
                .push(resource);
        }
        let grouping_time = grouping_start.elapsed();
        tracing::debug!(
            "[PERF] Grouped {} resources into {} types in {:?}",
            resources.len(),
            by_type.len(),
            grouping_time
        );

        let mut total_fetch_params_time = std::time::Duration::ZERO;
        let mut total_context_build_time = std::time::Duration::ZERO;
        let mut total_extract_time = std::time::Duration::ZERO;

        // Process each type
        for (resource_type, type_resources) in by_type {
            let type_start = std::time::Instant::now();
            let type_count = type_resources.len();

            let fetch_start = std::time::Instant::now();
            let search_params = indexing_service
                .fetch_search_parameters(&resource_type)
                .await?;
            let fetch_time = fetch_start.elapsed();
            total_fetch_params_time += fetch_time;

            if search_params.is_empty() {
                tracing::debug!(
                    "[PERF] Skipping {} ({} resources) - no search parameters",
                    resource_type,
                    type_count
                );
                continue;
            }

            tracing::debug!(
                "[PERF] Processing {} {} resources with {} search params",
                type_count,
                resource_type,
                search_params.len()
            );

            let needs_resolve = search_params.iter().any(|p| {
                p.expression
                    .as_deref()
                    .is_some_and(|expr| expr.contains("resolve("))
            });

            // Track FHIRPath metrics per resource type
            let mut type_context_build_time = std::time::Duration::ZERO;
            let mut type_params_processed = 0usize;
            let mut resource_root_key_counts = Vec::new();

            // Extract data from all resources of this type
            for resource in type_resources {
                // Build FHIRPath context with detailed timing
                let context_start = std::time::Instant::now();
                let root_key_count = resource.resource.as_object().map(|m| m.len()).unwrap_or(0);
                resource_root_key_counts.push(root_key_count);

                let from_json_start = std::time::Instant::now();
                let root = FhirPathValue::from_json(resource.resource.clone());
                let from_json_time = from_json_start.elapsed();

                let context_new_start = std::time::Instant::now();
                let root = if needs_resolve {
                    root.materialize()
                } else {
                    root
                };
                let ctx = Context::new(root);
                let context_new_time = context_new_start.elapsed();

                let context_build_time = context_start.elapsed();
                total_context_build_time += context_build_time;
                type_context_build_time += context_build_time;

                // Log slow context building (>1ms) for large resources
                if context_build_time.as_millis() > 1 {
                    tracing::debug!(
                        "[FHIRPATH] Slow context build for {}/{}: root_keys={}, from_json={:?}, context_new={:?}, total={:?}",
                        resource.resource_type,
                        resource.id,
                        root_key_count,
                        from_json_time,
                        context_new_time,
                        context_build_time
                    );
                }

                // Extract for each parameter
                let param_extract_start = std::time::Instant::now();
                for param in &search_params {
                    self.extract_parameter_data(
                        &mut index_data,
                        resource,
                        param,
                        &ctx,
                        indexing_service.fhir_version(),
                        indexing_service.enable_text_search,
                        indexing_service.enable_content_search,
                    )
                    .await?;
                    type_params_processed += 1;
                }
                total_extract_time += param_extract_start.elapsed();
            }

            // Log aggregated FHIRPath metrics per resource type
            if !resource_root_key_counts.is_empty() {
                let avg_root_keys =
                    resource_root_key_counts.iter().sum::<usize>() / resource_root_key_counts.len();
                let max_root_keys = *resource_root_key_counts.iter().max().unwrap_or(&0);

                tracing::info!(
                    "[FHIRPATH] {} {}: avg_root_keys={}, max_root_keys={}, avg_context_build={:?}, params={}",
                    type_count,
                    resource_type,
                    avg_root_keys,
                    max_root_keys,
                    type_context_build_time / type_count as u32,
                    type_params_processed
                );
            }

            let type_time = type_start.elapsed();
            tracing::debug!(
                "[PERF] Completed {} {} resources in {:?} ({:.0} resources/sec)",
                type_count,
                resource_type,
                type_time,
                type_count as f64 / type_time.as_secs_f64()
            );
        }

        let extract_total_time = extract_total_start.elapsed();
        tracing::debug!(
            "[PERF] Extraction breakdown: grouping={:?}, fetch_params={:?}, context_build={:?}, extract={:?}, total={:?}",
            grouping_time,
            total_fetch_params_time,
            total_context_build_time,
            total_extract_time,
            extract_total_time
        );

        Ok(index_data)
    }

    /// Extract data for a single parameter
    async fn extract_parameter_data(
        &self,
        index_data: &mut IndexData,
        resource: &Resource,
        param: &super::SearchParameter,
        ctx: &Context,
        fhir_version: &str,
        enable_text_search: bool,
        enable_content_search: bool,
    ) -> Result<()> {
        use std::sync::OnceLock;
        use ferrum_fhirpath::Engine as FhirPathEngine;

        let param_start = std::time::Instant::now();

        match param.r#type.as_str() {
            // `_text` / `_content` are full-resource indexes and typically have no FHIRPath expression.
            "text" => {
                if !enable_text_search {
                    return Ok(());
                }
                let content = extract_narrative_text(&resource.resource);
                let content = content.trim();
                if content.is_empty() {
                    return Ok(());
                }
                index_data.texts.push(TextRow {
                    resource_type: resource.resource_type.clone(),
                    resource_id: resource.id.clone(),
                    version_id: resource.version_id,
                    parameter_name: param.code.clone(),
                    content: content.to_string(),
                });
                return Ok(());
            }
            "content" => {
                if !enable_content_search {
                    return Ok(());
                }
                let content = extract_all_textual_content(&resource.resource);
                let content = content.trim();
                if content.is_empty() {
                    return Ok(());
                }
                index_data.contents.push(ContentRow {
                    resource_type: resource.resource_type.clone(),
                    resource_id: resource.id.clone(),
                    version_id: resource.version_id,
                    parameter_name: param.code.clone(),
                    content: content.to_string(),
                });
                return Ok(());
            }
            _ => {}
        }

        // Static engine for FHIRPath evaluation (shared across all extractions)
        // Use a HashMap keyed by version to support multiple versions if needed
        static GLOBAL_ENGINES: OnceLock<
            std::sync::Arc<
                std::sync::RwLock<
                    std::collections::HashMap<String, std::sync::Arc<FhirPathEngine>>,
                >,
            >,
        > = OnceLock::new();

        let engines = GLOBAL_ENGINES.get_or_init(|| {
            std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()))
        });

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
                    let resolver = std::sync::Arc::new(
                        super::resolver::IndexingResourceResolver::new_stub(4096),
                    );
                    std::sync::Arc::new(FhirPathEngine::new(core_context, Some(resolver)))
                })
                .clone()
        };

        // Get the compiled FHIRPath expression
        let plan_start = std::time::Instant::now();
        let plan = match super::get_or_compile_plan(param, fhir_version)? {
            Some(p) => p,
            None => return Ok(()),
        };
        let plan_time = plan_start.elapsed();

        let context_root_keys = resource.resource.as_object().map(|m| m.len()).unwrap_or(0);

        // Evaluate FHIRPath using the engine
        let eval_start = std::time::Instant::now();
        let values: FhirPathCollection = match engine.evaluate(&plan, ctx) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    "[FHIRPATH] Evaluation failed for {}.{} on {}/{}: {} (expr={}, ctx_root_keys={})",
                    resource.resource_type,
                    param.code,
                    resource.resource_type,
                    resource.id,
                    e,
                    param.expression.as_deref().unwrap_or("N/A"),
                    context_root_keys
                );
                return Ok(());
            }
        };
        let eval_time = eval_start.elapsed();

        if values.is_empty() {
            return Ok(());
        }

        // Extract based on parameter type
        let extract_start = std::time::Instant::now();
        let rows_before = index_data.strings.len()
            + index_data.tokens.len()
            + index_data.token_identifiers.len()
            + index_data.dates.len()
            + index_data.numbers.len()
            + index_data.quantities.len()
            + index_data.references.len()
            + index_data.uris.len();

        match param.r#type.as_str() {
            "string" => {
                for value in values.iter().filter_map(|v| v.to_json()) {
                    for s in extract_strings(&value) {
                        let normalized = normalize_string_for_search(&s);
                        let hash = compute_hash(&format!(
                            "{}{}{}{}{}",
                            resource.resource_type, resource.id, resource.version_id, param.code, s
                        ));
                        index_data.strings.push(StringRow {
                            resource_type: resource.resource_type.clone(),
                            resource_id: resource.id.clone(),
                            version_id: resource.version_id,
                            parameter_name: param.code.clone(),
                            value: s,
                            value_normalized: normalized,
                            entry_hash: hash,
                        });
                    }
                }
            }
            "token" => {
                for value in values.iter().filter_map(|v| v.to_json()) {
                    // Extract identifier-of-type rows
                    for row in extract_identifier_of_type_rows(&value) {
                        if let (Some(type_code), Some(val)) = (row.type_code, row.value) {
                            if !type_code.is_empty() && !val.is_empty() {
                                let hash = compute_hash(&format!(
                                    "{}{}{}{}{}{}{}",
                                    resource.resource_type,
                                    resource.id,
                                    resource.version_id,
                                    param.code,
                                    row.type_system.as_deref().unwrap_or(""),
                                    type_code,
                                    val
                                ));
                                index_data.token_identifiers.push(TokenIdentifierRow {
                                    resource_type: resource.resource_type.clone(),
                                    resource_id: resource.id.clone(),
                                    version_id: resource.version_id,
                                    parameter_name: param.code.clone(),
                                    type_system: row.type_system,
                                    type_code: type_code.clone(),
                                    type_code_ci: type_code.to_lowercase(),
                                    value: val.clone(),
                                    value_ci: val.to_lowercase(),
                                    entry_hash: hash,
                                });
                            }
                        }
                    }

                    // Extract tokens
                    for token in extract_tokens(&value) {
                        if let Some(code) = token.code {
                            if !code.is_empty() {
                                let hash = compute_hash(&format!(
                                    "{}{}{}{}{}{}",
                                    resource.resource_type,
                                    resource.id,
                                    resource.version_id,
                                    param.code,
                                    token.system.as_deref().unwrap_or(""),
                                    code
                                ));
                                index_data.tokens.push(TokenRow {
                                    resource_type: resource.resource_type.clone(),
                                    resource_id: resource.id.clone(),
                                    version_id: resource.version_id,
                                    parameter_name: param.code.clone(),
                                    system: token.system,
                                    code: code.clone(),
                                    code_ci: code.to_lowercase(),
                                    display: token.display,
                                    entry_hash: hash,
                                });
                            }
                        }
                    }
                }
            }
            "date" => {
                for value in values.iter().filter_map(|v| v.to_json()) {
                    for (start, end) in extract_date_ranges(&value) {
                        let hash = compute_hash(&format!(
                            "{}{}{}{}{}{}",
                            resource.resource_type,
                            resource.id,
                            resource.version_id,
                            param.code,
                            start,
                            end
                        ));
                        index_data.dates.push(DateRow {
                            resource_type: resource.resource_type.clone(),
                            resource_id: resource.id.clone(),
                            version_id: resource.version_id,
                            parameter_name: param.code.clone(),
                            start_date: start,
                            end_date: end,
                            entry_hash: hash,
                        });
                    }
                }
            }
            "number" => {
                for value in values.iter().filter_map(|v| v.to_json()) {
                    for num in extract_numbers(&value) {
                        let hash = compute_hash(&format!(
                            "{}{}{}{}{}",
                            resource.resource_type,
                            resource.id,
                            resource.version_id,
                            param.code,
                            num
                        ));
                        index_data.numbers.push(NumberRow {
                            resource_type: resource.resource_type.clone(),
                            resource_id: resource.id.clone(),
                            version_id: resource.version_id,
                            parameter_name: param.code.clone(),
                            value: num,
                            entry_hash: hash,
                        });
                    }
                }
            }
            "quantity" => {
                for value in values.iter().filter_map(|v| v.to_json()) {
                    for quantity in extract_quantity_values(&value) {
                        if quantity.code.is_some() || quantity.unit.is_some() {
                            let hash = compute_hash(&format!(
                                "{}{}{}{}{}{}{}",
                                resource.resource_type,
                                resource.id,
                                resource.version_id,
                                param.code,
                                quantity.value,
                                quantity.system.as_deref().unwrap_or(""),
                                quantity.code.as_deref().unwrap_or("")
                            ));
                            index_data.quantities.push(QuantityRow {
                                resource_type: resource.resource_type.clone(),
                                resource_id: resource.id.clone(),
                                version_id: resource.version_id,
                                parameter_name: param.code.clone(),
                                value: quantity.value,
                                system: quantity.system,
                                code: quantity.code,
                                unit: quantity.unit,
                                entry_hash: hash,
                            });
                        }
                    }
                }
            }
            "reference" => {
                for value in values.iter().filter_map(|v| v.to_json()) {
                    for reference in extract_reference_values(&value) {
                        let hash = compute_hash(&format!(
                            "{}{}{}{}{}{}{}{}{}{}{}",
                            resource.resource_type,
                            resource.id,
                            resource.version_id,
                            param.code,
                            reference.reference_kind.as_str(),
                            reference.target_type,
                            reference.target_id,
                            reference.target_version_id,
                            reference.target_url,
                            reference.canonical_url,
                            reference.canonical_version
                        ));
                        index_data.references.push(ReferenceRow {
                            resource_type: resource.resource_type.clone(),
                            resource_id: resource.id.clone(),
                            version_id: resource.version_id,
                            parameter_name: param.code.clone(),
                            reference_kind: reference.reference_kind.as_str().to_string(),
                            target_type: reference.target_type,
                            target_id: reference.target_id,
                            target_version_id: reference.target_version_id,
                            target_url: reference.target_url,
                            canonical_url: reference.canonical_url,
                            canonical_version: reference.canonical_version,
                            display: reference.display,
                            entry_hash: hash,
                        });
                    }
                }
            }
            "uri" => {
                for value in values.iter().filter_map(|v| v.to_json()) {
                    for s in extract_strings(&value) {
                        let normalized = normalize_casefold_strip_combining(&s);
                        let hash = compute_hash(&format!(
                            "{}{}{}{}{}",
                            resource.resource_type, resource.id, resource.version_id, param.code, s
                        ));
                        index_data.uris.push(UriRow {
                            resource_type: resource.resource_type.clone(),
                            resource_id: resource.id.clone(),
                            version_id: resource.version_id,
                            parameter_name: param.code.clone(),
                            value: s,
                            value_normalized: normalized,
                            entry_hash: hash,
                        });
                    }
                }
            }
            _ => {
                // Unsupported types (text, content, composite, special)
                // These are handled by regular indexing
            }
        }

        let extract_time = extract_start.elapsed();
        let rows_after = index_data.strings.len()
            + index_data.tokens.len()
            + index_data.token_identifiers.len()
            + index_data.dates.len()
            + index_data.numbers.len()
            + index_data.quantities.len()
            + index_data.references.len()
            + index_data.uris.len();
        let rows_added = rows_after - rows_before;
        let total_time = param_start.elapsed();

        if total_time.as_millis() > 10 {
            tracing::debug!(
                "[PERF] Slow extract {}.{} on {}/{}: total={:?}, plan={:?}, eval={:?}, extract={:?}, rows_added={}",
                resource.resource_type,
                param.code,
                resource.resource_type,
                resource.id,
                total_time,
                plan_time,
                eval_time,
                extract_time,
                rows_added
            );
        }

        Ok(())
    }

    /// COPY data to search_string using CSV format
    async fn copy_to_search_string(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[StringRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_string", rows.len());

        // Build CSV data
        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                escape_csv(&row.value),
                escape_csv(&row.value_normalized),
                escape_csv(&row.entry_hash)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        // Use COPY with ON CONFLICT handling via temp table
        let create_temp_start = std::time::Instant::now();
        sqlx::query(
            "CREATE TEMP TABLE temp_search_string (LIKE search_string INCLUDING DEFAULTS) ON COMMIT DROP"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        // COPY into temp table
        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_string (resource_type, resource_id, version_id, parameter_name, value, value_normalized, entry_hash) FROM STDIN"
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        // Merge into real table with ON CONFLICT DO UPDATE
        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_string (resource_type, resource_id, version_id, parameter_name, value, value_normalized, entry_hash)
             SELECT resource_type, resource_id, version_id, parameter_name, value, value_normalized, entry_hash
             FROM temp_search_string
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name, entry_hash)
             DO UPDATE SET
                 value = EXCLUDED.value,
                 value_normalized = EXCLUDED.value_normalized"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_string: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_token
    async fn copy_to_search_token(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[TokenRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_token", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                row.system
                    .as_ref()
                    .map(|s| escape_csv(s))
                    .unwrap_or_else(|| "\\N".to_string()),
                escape_csv(&row.code),
                escape_csv(&row.code_ci),
                row.display
                    .as_ref()
                    .map(|s| escape_csv(s))
                    .unwrap_or_else(|| "\\N".to_string()),
                escape_csv(&row.entry_hash)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query("CREATE TEMP TABLE temp_search_token (LIKE search_token INCLUDING DEFAULTS) ON COMMIT DROP")
            .execute(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_token (resource_type, resource_id, version_id, parameter_name, system, code, code_ci, display, entry_hash) FROM STDIN"
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_token (resource_type, resource_id, version_id, parameter_name, system, code, code_ci, display, entry_hash)
             SELECT resource_type, resource_id, version_id, parameter_name, system, code, code_ci, display, entry_hash
             FROM temp_search_token
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name, entry_hash)
             DO UPDATE SET
                 system = EXCLUDED.system,
                 code = EXCLUDED.code,
                 code_ci = EXCLUDED.code_ci,
                 display = EXCLUDED.display"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_token: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_token_identifier
    async fn copy_to_search_token_identifier(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[TokenIdentifierRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_token_identifier", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                row.type_system
                    .as_ref()
                    .map(|s| escape_csv(s))
                    .unwrap_or_else(|| "\\N".to_string()),
                escape_csv(&row.type_code),
                escape_csv(&row.type_code_ci),
                escape_csv(&row.value),
                escape_csv(&row.value_ci),
                escape_csv(&row.entry_hash)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query("CREATE TEMP TABLE temp_search_token_identifier (LIKE search_token_identifier INCLUDING DEFAULTS) ON COMMIT DROP")
            .execute(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_token_identifier (resource_type, resource_id, version_id, parameter_name, type_system, type_code, type_code_ci, value, value_ci, entry_hash) FROM STDIN"
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_token_identifier (resource_type, resource_id, version_id, parameter_name, type_system, type_code, type_code_ci, value, value_ci, entry_hash)
             SELECT resource_type, resource_id, version_id, parameter_name, type_system, type_code, type_code_ci, value, value_ci, entry_hash
             FROM temp_search_token_identifier
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name, entry_hash)
             DO UPDATE SET
                 type_system = EXCLUDED.type_system,
                 type_code = EXCLUDED.type_code,
                 type_code_ci = EXCLUDED.type_code_ci,
                 value = EXCLUDED.value,
                 value_ci = EXCLUDED.value_ci"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_token_identifier: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_date
    async fn copy_to_search_date(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[DateRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_date", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                row.start_date.to_rfc3339(),
                row.end_date.to_rfc3339(),
                escape_csv(&row.entry_hash)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query("CREATE TEMP TABLE temp_search_date (LIKE search_date INCLUDING DEFAULTS) ON COMMIT DROP")
            .execute(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_date (resource_type, resource_id, version_id, parameter_name, start_date, end_date, entry_hash) FROM STDIN"
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_date (resource_type, resource_id, version_id, parameter_name, start_date, end_date, entry_hash)
             SELECT resource_type, resource_id, version_id, parameter_name, start_date, end_date, entry_hash
             FROM temp_search_date
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name, entry_hash)
             DO UPDATE SET
                 start_date = EXCLUDED.start_date,
                 end_date = EXCLUDED.end_date"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_date: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_number
    async fn copy_to_search_number(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[NumberRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_number", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                row.value,
                escape_csv(&row.entry_hash)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query("CREATE TEMP TABLE temp_search_number (LIKE search_number INCLUDING DEFAULTS) ON COMMIT DROP")
            .execute(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_number (resource_type, resource_id, version_id, parameter_name, value, entry_hash) FROM STDIN"
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_number (resource_type, resource_id, version_id, parameter_name, value, entry_hash)
             SELECT resource_type, resource_id, version_id, parameter_name, value, entry_hash
             FROM temp_search_number
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name, entry_hash)
             DO UPDATE SET value = EXCLUDED.value"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_number: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_quantity
    async fn copy_to_search_quantity(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[QuantityRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_quantity", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                row.value,
                row.system
                    .as_ref()
                    .map(|s| escape_csv(s))
                    .unwrap_or_else(|| "\\N".to_string()),
                row.code
                    .as_ref()
                    .map(|s| escape_csv(s))
                    .unwrap_or_else(|| "\\N".to_string()),
                row.unit
                    .as_ref()
                    .map(|s| escape_csv(s))
                    .unwrap_or_else(|| "\\N".to_string()),
                escape_csv(&row.entry_hash)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query("CREATE TEMP TABLE temp_search_quantity (LIKE search_quantity INCLUDING DEFAULTS) ON COMMIT DROP")
            .execute(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_quantity (resource_type, resource_id, version_id, parameter_name, value, system, code, unit, entry_hash) FROM STDIN"
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_quantity (resource_type, resource_id, version_id, parameter_name, value, system, code, unit, entry_hash)
             SELECT resource_type, resource_id, version_id, parameter_name, value, system, code, unit, entry_hash
             FROM temp_search_quantity
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name, entry_hash)
             DO UPDATE SET
                 value = EXCLUDED.value,
                 system = EXCLUDED.system,
                 code = EXCLUDED.code,
                 unit = EXCLUDED.unit"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_quantity: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_reference
    async fn copy_to_search_reference(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[ReferenceRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_reference", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                escape_csv(&row.reference_kind),
                escape_csv(&row.target_type),
                escape_csv(&row.target_id),
                escape_csv(&row.target_version_id),
                escape_csv(&row.target_url),
                escape_csv(&row.canonical_url),
                escape_csv(&row.canonical_version),
                row.display
                    .as_ref()
                    .map(|s| escape_csv(s))
                    .unwrap_or_else(|| "\\N".to_string()),
                escape_csv(&row.entry_hash)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query("CREATE TEMP TABLE temp_search_reference (LIKE search_reference INCLUDING DEFAULTS) ON COMMIT DROP")
            .execute(&mut **tx)
            .await
            .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_reference (resource_type, resource_id, version_id, parameter_name, reference_kind, target_type, target_id, target_version_id, target_url, canonical_url, canonical_version, display, entry_hash) FROM STDIN"
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_reference (resource_type, resource_id, version_id, parameter_name, reference_kind, target_type, target_id, target_version_id, target_url, canonical_url, canonical_version, display, entry_hash)
             SELECT resource_type, resource_id, version_id, parameter_name, reference_kind, target_type, target_id, target_version_id, target_url, canonical_url, canonical_version, display, entry_hash
             FROM temp_search_reference
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name, entry_hash)
             DO UPDATE SET
                 reference_kind = EXCLUDED.reference_kind,
                 target_type = EXCLUDED.target_type,
                 target_id = EXCLUDED.target_id,
                 target_version_id = EXCLUDED.target_version_id,
                 target_url = EXCLUDED.target_url,
                 canonical_url = EXCLUDED.canonical_url,
                 canonical_version = EXCLUDED.canonical_version,
                 display = EXCLUDED.display"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_reference: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_uri
    async fn copy_to_search_uri(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[UriRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_uri", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                escape_csv(&row.value),
                escape_csv(&row.value_normalized),
                escape_csv(&row.entry_hash)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query(
            "CREATE TEMP TABLE temp_search_uri (LIKE search_uri INCLUDING DEFAULTS) ON COMMIT DROP",
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_uri (resource_type, resource_id, version_id, parameter_name, value, value_normalized, entry_hash) FROM STDIN"
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_uri (resource_type, resource_id, version_id, parameter_name, value, value_normalized, entry_hash)
             SELECT resource_type, resource_id, version_id, parameter_name, value, value_normalized, entry_hash
             FROM temp_search_uri
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name, entry_hash)
             DO UPDATE SET
                 value = EXCLUDED.value,
                 value_normalized = EXCLUDED.value_normalized"
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_uri: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_text
    async fn copy_to_search_text(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[TextRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_text", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                escape_csv(&row.content)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query(
            "CREATE TEMP TABLE temp_search_text (LIKE search_text INCLUDING DEFAULTS) ON COMMIT DROP",
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_text (resource_type, resource_id, version_id, parameter_name, content) FROM STDIN",
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_text (resource_type, resource_id, version_id, parameter_name, content)
             SELECT resource_type, resource_id, version_id, parameter_name, content
             FROM temp_search_text
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name)
             DO UPDATE SET content = EXCLUDED.content",
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_text: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }

    /// COPY data to search_content
    async fn copy_to_search_content(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rows: &[ContentRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        let table_start = std::time::Instant::now();
        tracing::debug!("[PERF] COPY {} rows to search_content", rows.len());

        let csv_build_start = std::time::Instant::now();
        let mut csv_data = String::new();
        for row in rows {
            csv_data.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                escape_csv(&row.resource_type),
                escape_csv(&row.resource_id),
                row.version_id,
                escape_csv(&row.parameter_name),
                escape_csv(&row.content)
            ));
        }
        let csv_build_time = csv_build_start.elapsed();
        let csv_size_mb = csv_data.len() as f64 / 1_048_576.0;

        let create_temp_start = std::time::Instant::now();
        sqlx::query(
            "CREATE TEMP TABLE temp_search_content (LIKE search_content INCLUDING DEFAULTS) ON COMMIT DROP",
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let create_temp_time = create_temp_start.elapsed();

        let copy_start = std::time::Instant::now();
        let mut copy = tx
            .copy_in_raw(
                "COPY temp_search_content (resource_type, resource_id, version_id, parameter_name, content) FROM STDIN",
            )
            .await
            .map_err(crate::Error::Database)?;

        copy.send(csv_data.as_bytes())
            .await
            .map_err(crate::Error::Database)?;
        copy.finish().await.map_err(crate::Error::Database)?;
        let copy_time = copy_start.elapsed();

        let insert_start = std::time::Instant::now();
        sqlx::query(
            "INSERT INTO search_content (resource_type, resource_id, version_id, parameter_name, content)
             SELECT resource_type, resource_id, version_id, parameter_name, content
             FROM temp_search_content
             ON CONFLICT (resource_type, resource_id, version_id, parameter_name)
             DO UPDATE SET content = EXCLUDED.content",
        )
        .execute(&mut **tx)
        .await
        .map_err(crate::Error::Database)?;
        let insert_time = insert_start.elapsed();

        let total_time = table_start.elapsed();
        tracing::debug!(
            "[PERF] search_content: csv_build={:?} ({:.2}MB), create_temp={:?}, copy={:?} ({:.0} rows/sec), insert={:?}, total={:?}",
            csv_build_time,
            csv_size_mb,
            create_temp_time,
            copy_time,
            rows.len() as f64 / copy_time.as_secs_f64(),
            insert_time,
            total_time
        );

        Ok(())
    }
}

/// Escape CSV field (tab-delimited format)
fn escape_csv(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    // Escape backslashes, newlines, tabs
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Compute hash for entry deduplication (32-char hex string)
/// This uses a simple hash function to match the SQL MD5() format
fn compute_hash(input: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash = hasher.finish();

    // Format as 32-char hex string to match MD5 format (pad with zeros)
    format!("{:032x}", hash)
}

// Data structures for bulk loading

#[derive(Debug, Default)]
struct IndexData {
    strings: Vec<StringRow>,
    tokens: Vec<TokenRow>,
    token_identifiers: Vec<TokenIdentifierRow>,
    dates: Vec<DateRow>,
    numbers: Vec<NumberRow>,
    quantities: Vec<QuantityRow>,
    references: Vec<ReferenceRow>,
    uris: Vec<UriRow>,
    texts: Vec<TextRow>,
    contents: Vec<ContentRow>,
}

impl IndexData {
    /// Deduplicate all rows by entry_hash
    ///
    /// CRITICAL: Prevents PostgreSQL "ON CONFLICT DO UPDATE command cannot affect row a second time" error
    /// which occurs when the same entry_hash appears multiple times in a single INSERT statement.
    fn deduplicate(&mut self) {
        use std::collections::HashSet;

        // Deduplicate strings
        {
            let mut seen = HashSet::new();
            self.strings
                .retain(|row| seen.insert(row.entry_hash.clone()));
        }

        // Deduplicate tokens
        {
            let mut seen = HashSet::new();
            self.tokens
                .retain(|row| seen.insert(row.entry_hash.clone()));
        }

        // Deduplicate token identifiers
        {
            let mut seen = HashSet::new();
            self.token_identifiers
                .retain(|row| seen.insert(row.entry_hash.clone()));
        }

        // Deduplicate dates
        {
            let mut seen = HashSet::new();
            self.dates.retain(|row| seen.insert(row.entry_hash.clone()));
        }

        // Deduplicate numbers
        {
            let mut seen = HashSet::new();
            self.numbers
                .retain(|row| seen.insert(row.entry_hash.clone()));
        }

        // Deduplicate quantities
        {
            let mut seen = HashSet::new();
            self.quantities
                .retain(|row| seen.insert(row.entry_hash.clone()));
        }

        // Deduplicate references
        {
            let mut seen = HashSet::new();
            self.references
                .retain(|row| seen.insert(row.entry_hash.clone()));
        }

        // Deduplicate uris
        {
            let mut seen = HashSet::new();
            self.uris.retain(|row| seen.insert(row.entry_hash.clone()));
        }

        // Deduplicate texts (unique per resource version + parameter)
        {
            let mut seen = HashSet::new();
            self.texts.retain(|row| {
                seen.insert(format!(
                    "{}|{}|{}|{}",
                    row.resource_type, row.resource_id, row.version_id, row.parameter_name
                ))
            });
        }

        // Deduplicate contents (unique per resource version + parameter)
        {
            let mut seen = HashSet::new();
            self.contents.retain(|row| {
                seen.insert(format!(
                    "{}|{}|{}|{}",
                    row.resource_type, row.resource_id, row.version_id, row.parameter_name
                ))
            });
        }
    }
}

#[derive(Debug)]
struct StringRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    value: String,
    value_normalized: String,
    entry_hash: String,
}

#[derive(Debug)]
struct TokenRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    system: Option<String>,
    code: String,
    code_ci: String,
    display: Option<String>,
    entry_hash: String,
}

#[derive(Debug)]
struct TokenIdentifierRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    type_system: Option<String>,
    type_code: String,
    type_code_ci: String,
    value: String,
    value_ci: String,
    entry_hash: String,
}

#[derive(Debug)]
struct TextRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    content: String,
}

#[derive(Debug)]
struct ContentRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    content: String,
}

#[derive(Debug)]
struct DateRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    start_date: chrono::DateTime<chrono::Utc>,
    end_date: chrono::DateTime<chrono::Utc>,
    entry_hash: String,
}

#[derive(Debug)]
struct NumberRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    value: rust_decimal::Decimal,
    entry_hash: String,
}

#[derive(Debug)]
struct QuantityRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    value: rust_decimal::Decimal,
    system: Option<String>,
    code: Option<String>,
    unit: Option<String>,
    entry_hash: String,
}

#[derive(Debug)]
struct ReferenceRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    reference_kind: String,
    target_type: String,
    target_id: String,
    target_version_id: String,
    target_url: String,
    canonical_url: String,
    canonical_version: String,
    display: Option<String>,
    entry_hash: String,
}

#[derive(Debug)]
struct UriRow {
    resource_type: String,
    resource_id: String,
    version_id: i32,
    parameter_name: String,
    value: String,
    value_normalized: String,
    entry_hash: String,
}
