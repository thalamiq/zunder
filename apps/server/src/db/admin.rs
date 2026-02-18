//! Admin repository - server statistics queries.

use crate::services::admin::{
    AuditEventAdminDetail, AuditEventAdminListItem, SearchHashCollisionStatus,
    SearchIndexTableStatus, SearchParameterAdminListItem, SearchParameterIndexingStatus,
    TransactionAdminDetail, TransactionAdminListItem, TransactionEntryItem,
};
use crate::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value as JsonValue;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTypeStats {
    pub resource_type: String,
    pub total_versions: i64,
    pub current_total: i64,
    pub current_active: i64,
    pub current_deleted: i64,
    pub last_updated: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct AdminRepository {
    pool: PgPool,
}

impl AdminRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn fetch_resource_type_stats(&self) -> Result<Vec<ResourceTypeStats>> {
        let rows = sqlx::query(
            r#"
            SELECT
                resource_type,
                COUNT(*)::BIGINT AS total_versions,
                COUNT(*) FILTER (WHERE is_current = true)::BIGINT AS current_total,
                COUNT(*) FILTER (WHERE is_current = true AND deleted = false)::BIGINT AS current_active,
                COUNT(*) FILTER (WHERE is_current = true AND deleted = true)::BIGINT AS current_deleted,
                MAX(last_updated) AS last_updated
            FROM resources
            GROUP BY resource_type
            ORDER BY resource_type
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ResourceTypeStats {
                resource_type: row.get("resource_type"),
                total_versions: row.get("total_versions"),
                current_total: row.get("current_total"),
                current_active: row.get("current_active"),
                current_deleted: row.get("current_deleted"),
                last_updated: row.get("last_updated"),
            })
            .collect())
    }

    pub async fn fetch_search_parameter_indexing_status(
        &self,
        resource_type: Option<&str>,
    ) -> Result<Vec<SearchParameterIndexingStatus>> {
        let status: Vec<SearchParameterIndexingStatus> =
            sqlx::query_as("SELECT * FROM get_search_parameter_indexing_status($1)")
                .bind(resource_type)
                .fetch_all(&self.pool)
                .await
                .map_err(crate::Error::Database)?;

        Ok(status)
    }

    pub async fn fetch_search_index_table_status(&self) -> Result<Vec<SearchIndexTableStatus>> {
        let status: Vec<SearchIndexTableStatus> =
            sqlx::query_as("SELECT * FROM check_search_index_status()")
                .fetch_all(&self.pool)
                .await
                .map_err(crate::Error::Database)?;
        Ok(status)
    }

    pub async fn fetch_search_hash_collisions(&self) -> Result<Vec<SearchHashCollisionStatus>> {
        let status: Vec<SearchHashCollisionStatus> =
            sqlx::query_as("SELECT * FROM check_hash_collisions()")
                .fetch_all(&self.pool)
                .await
                .map_err(crate::Error::Database)?;
        Ok(status)
    }

    pub async fn list_search_parameters(
        &self,
        q: Option<&str>,
        type_: Option<&str>,
        resource_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<SearchParameterAdminListItem>, i64)> {
        // Build dynamic query based on filters
        let mut where_clauses = vec![];
        let mut bind_count = 0;

        let mut query_str = String::from(
            r#"
            SELECT
                sp.id::text as id,
                sp.url,
                sp.code,
                json_build_array(sp.resource_type)::jsonb as base,
                sp.type,
                NULL::text as status,
                sp.expression,
                sp.description,
                TO_CHAR(sp.updated_at, 'YYYY-MM-DD"T"HH24:MI:SS"Z"') as last_updated,
                1::bigint as server_expected_bases,
                1::bigint as server_configured_bases,
                sp.active as server_active
            FROM search_parameters sp
            "#,
        );

        if let Some(_q) = q {
            bind_count += 1;
            where_clauses.push(format!(
                "(sp.code ILIKE ${} OR sp.url ILIKE ${0} OR sp.description ILIKE ${0})",
                bind_count
            ));
        }

        if let Some(_t) = type_ {
            bind_count += 1;
            where_clauses.push(format!("LOWER(sp.type) = LOWER(${})", bind_count));
        }

        if let Some(_rt) = resource_type {
            bind_count += 1;
            where_clauses.push(format!("sp.resource_type = ${}", bind_count));
        }

        if !where_clauses.is_empty() {
            query_str.push_str(" WHERE ");
            query_str.push_str(&where_clauses.join(" AND "));
        }

        query_str.push_str(" ORDER BY sp.code ASC, sp.resource_type ASC");

        // Count query
        let mut count_query = String::from(
            r#"
            SELECT COUNT(*)
            FROM search_parameters sp
            "#,
        );

        if !where_clauses.is_empty() {
            count_query.push_str(" WHERE ");
            count_query.push_str(&where_clauses.join(" AND "));
        }

        // Add pagination
        bind_count += 1;
        let limit_param = bind_count;
        bind_count += 1;
        let offset_param = bind_count;
        query_str.push_str(&format!(" LIMIT ${} OFFSET ${}", limit_param, offset_param));

        // Execute count query
        let mut count_q = sqlx::query_scalar::<_, i64>(&count_query);
        if let Some(q_val) = q {
            count_q = count_q.bind(format!("%{}%", q_val));
        }
        if let Some(t) = type_ {
            count_q = count_q.bind(t);
        }
        if let Some(rt) = resource_type {
            count_q = count_q.bind(rt);
        }

        let total = count_q
            .fetch_one(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        // Execute main query
        let mut main_q = sqlx::query_as::<_, SearchParameterAdminListItem>(&query_str);
        if let Some(q_val) = q {
            main_q = main_q.bind(format!("%{}%", q_val));
        }
        if let Some(t) = type_ {
            main_q = main_q.bind(t);
        }
        if let Some(rt) = resource_type {
            main_q = main_q.bind(rt);
        }
        main_q = main_q.bind(limit).bind(offset);

        let items = main_q
            .fetch_all(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        Ok((items, total))
    }

    pub async fn toggle_search_parameter_active(&self, id: i32) -> Result<bool> {
        // Get current active status
        let current_active = sqlx::query_scalar::<_, Option<bool>>(
            "SELECT active FROM search_parameters WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        let current_active = current_active.ok_or_else(|| {
            crate::Error::NotFound(format!("Search parameter with id {} not found", id))
        })?;

        let new_active = !current_active;

        // Update the search_parameter
        sqlx::query("UPDATE search_parameters SET active = $1, updated_at = NOW() WHERE id = $2")
            .bind(new_active)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        Ok(new_active)
    }

    pub async fn list_audit_events(
        &self,
        action: Option<&str>,
        outcome: Option<&str>,
        resource_type: Option<&str>,
        resource_id: Option<&str>,
        patient_id: Option<&str>,
        client_id: Option<&str>,
        user_id: Option<&str>,
        request_id: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<AuditEventAdminListItem>, i64)> {
        let mut where_clauses = vec![];
        let mut bind_count = 0;

        let mut query_str = String::from(
            r#"
            SELECT
                id,
                timestamp,
                action,
                http_method,
                fhir_action,
                resource_type,
                resource_id,
                patient_id,
                client_id,
                user_id,
                request_id,
                status_code,
                outcome,
                audit_event,
                details
            FROM audit_log
            "#,
        );

        if action.is_some() {
            bind_count += 1;
            where_clauses.push(format!("action = ${}", bind_count));
        }
        if outcome.is_some() {
            bind_count += 1;
            where_clauses.push(format!("outcome = ${}", bind_count));
        }
        if resource_type.is_some() {
            bind_count += 1;
            where_clauses.push(format!("resource_type = ${}", bind_count));
        }
        if resource_id.is_some() {
            bind_count += 1;
            where_clauses.push(format!("resource_id = ${}", bind_count));
        }
        if patient_id.is_some() {
            bind_count += 1;
            where_clauses.push(format!("patient_id = ${}", bind_count));
        }
        if client_id.is_some() {
            bind_count += 1;
            where_clauses.push(format!("client_id = ${}", bind_count));
        }
        if user_id.is_some() {
            bind_count += 1;
            where_clauses.push(format!("user_id = ${}", bind_count));
        }
        if request_id.is_some() {
            bind_count += 1;
            where_clauses.push(format!("request_id = ${}", bind_count));
        }

        if !where_clauses.is_empty() {
            query_str.push_str(" WHERE ");
            query_str.push_str(&where_clauses.join(" AND "));
        }

        query_str.push_str(" ORDER BY timestamp DESC, id DESC");

        // Count query
        let mut count_query = String::from("SELECT COUNT(*) FROM audit_log");
        if !where_clauses.is_empty() {
            count_query.push_str(" WHERE ");
            count_query.push_str(&where_clauses.join(" AND "));
        }

        // Add pagination
        bind_count += 1;
        let limit_param = bind_count;
        bind_count += 1;
        let offset_param = bind_count;
        query_str.push_str(&format!(" LIMIT ${} OFFSET ${}", limit_param, offset_param));

        let mut count_q = sqlx::query_scalar::<_, i64>(&count_query);
        if let Some(v) = action {
            count_q = count_q.bind(v);
        }
        if let Some(v) = outcome {
            count_q = count_q.bind(v);
        }
        if let Some(v) = resource_type {
            count_q = count_q.bind(v);
        }
        if let Some(v) = resource_id {
            count_q = count_q.bind(v);
        }
        if let Some(v) = patient_id {
            count_q = count_q.bind(v);
        }
        if let Some(v) = client_id {
            count_q = count_q.bind(v);
        }
        if let Some(v) = user_id {
            count_q = count_q.bind(v);
        }
        if let Some(v) = request_id {
            count_q = count_q.bind(v);
        }

        let total = count_q
            .fetch_one(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        let mut main_q = sqlx::query_as::<_, AuditEventAdminListItem>(&query_str);
        if let Some(v) = action {
            main_q = main_q.bind(v);
        }
        if let Some(v) = outcome {
            main_q = main_q.bind(v);
        }
        if let Some(v) = resource_type {
            main_q = main_q.bind(v);
        }
        if let Some(v) = resource_id {
            main_q = main_q.bind(v);
        }
        if let Some(v) = patient_id {
            main_q = main_q.bind(v);
        }
        if let Some(v) = client_id {
            main_q = main_q.bind(v);
        }
        if let Some(v) = user_id {
            main_q = main_q.bind(v);
        }
        if let Some(v) = request_id {
            main_q = main_q.bind(v);
        }
        main_q = main_q.bind(limit).bind(offset);

        let items = main_q
            .fetch_all(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        Ok((items, total))
    }

    pub async fn get_audit_event(&self, id: i64) -> Result<AuditEventAdminDetail> {
        let row = sqlx::query_as::<_, AuditEventAdminDetail>(
            r#"
            SELECT
                id,
                timestamp,
                action,
                http_method,
                fhir_action,
                resource_type,
                resource_id,
                version_id,
                patient_id,
                client_id,
                user_id,
                COALESCE(scopes, ARRAY[]::text[]) as scopes,
                token_type,
                client_ip,
                user_agent,
                request_id,
                status_code,
                outcome,
                audit_event,
                details
            FROM audit_log
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        row.ok_or_else(|| crate::Error::NotFound(format!("Audit event {} not found", id)))
    }

    pub async fn fetch_outgoing_references(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Vec<ReferenceEdge>> {
        let rows = sqlx::query_as::<_, ReferenceEdge>(
            r#"
            SELECT
                sr.resource_type AS source_type,
                sr.resource_id AS source_id,
                sr.parameter_name,
                sr.target_type,
                sr.target_id,
                sr.display
            FROM search_reference sr
            JOIN resources r
                ON r.resource_type = sr.resource_type
                AND r.id = sr.resource_id
                AND r.version_id = sr.version_id
                AND r.is_current = true
            WHERE sr.resource_type = $1
              AND sr.resource_id = $2
            LIMIT 200
            "#,
        )
        .bind(resource_type)
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        Ok(rows)
    }

    pub async fn fetch_incoming_references(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Vec<ReferenceEdge>> {
        let rows = sqlx::query_as::<_, ReferenceEdge>(
            r#"
            SELECT
                sr.resource_type AS source_type,
                sr.resource_id AS source_id,
                sr.parameter_name,
                sr.target_type,
                sr.target_id,
                sr.display
            FROM search_reference sr
            JOIN resources r
                ON r.resource_type = sr.resource_type
                AND r.id = sr.resource_id
                AND r.version_id = sr.version_id
                AND r.is_current = true
            WHERE sr.target_type = $1
              AND sr.target_id = $2
            LIMIT 200
            "#,
        )
        .bind(resource_type)
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        Ok(rows)
    }

    /// Fetch references between a set of resources (both source and target must be in the set).
    /// Used for bundle graph visualization.
    pub async fn fetch_batch_references(
        &self,
        keys: &[(String, String)], // (resource_type, resource_id)
    ) -> Result<Vec<ReferenceEdge>> {
        if keys.is_empty() {
            return Ok(vec![]);
        }

        // Build (resource_type, resource_id) tuples as a VALUES list for a CTE
        let mut values_parts = Vec::with_capacity(keys.len());
        let mut params: Vec<String> = Vec::with_capacity(keys.len() * 2);
        for (i, (rt, rid)) in keys.iter().enumerate() {
            let p1 = i * 2 + 1;
            let p2 = i * 2 + 2;
            values_parts.push(format!("(${}, ${})", p1, p2));
            params.push(rt.clone());
            params.push(rid.clone());
        }

        let query = format!(
            r#"
            WITH resource_set(resource_type, resource_id) AS (
                VALUES {}
            )
            SELECT
                sr.resource_type AS source_type,
                sr.resource_id AS source_id,
                sr.parameter_name,
                sr.target_type,
                sr.target_id,
                sr.display
            FROM search_reference sr
            JOIN resources r
                ON r.resource_type = sr.resource_type
                AND r.id = sr.resource_id
                AND r.version_id = sr.version_id
                AND r.is_current = true
            JOIN resource_set src
                ON src.resource_type = sr.resource_type
                AND src.resource_id = sr.resource_id
            JOIN resource_set tgt
                ON tgt.resource_type = sr.target_type
                AND tgt.resource_id = sr.target_id
            LIMIT 1000
            "#,
            values_parts.join(", ")
        );

        let mut q = sqlx::query_as::<_, ReferenceEdge>(&query);
        for p in &params {
            q = q.bind(p);
        }

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        Ok(rows)
    }

    pub async fn list_transactions(
        &self,
        bundle_type: Option<&str>,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<TransactionAdminListItem>, i64)> {
        let mut where_clauses = vec![];
        let mut bind_count = 0;

        let mut query_str = String::from(
            r#"
            SELECT
                id,
                type,
                status,
                entry_count,
                created_at,
                started_at,
                completed_at,
                error_message
            FROM fhir_transactions
            "#,
        );

        if bundle_type.is_some() {
            bind_count += 1;
            where_clauses.push(format!("type = ${}", bind_count));
        }
        if status.is_some() {
            bind_count += 1;
            where_clauses.push(format!("status = ${}", bind_count));
        }

        if !where_clauses.is_empty() {
            query_str.push_str(" WHERE ");
            query_str.push_str(&where_clauses.join(" AND "));
        }

        query_str.push_str(" ORDER BY created_at DESC");

        let mut count_query = String::from("SELECT COUNT(*) FROM fhir_transactions");
        if !where_clauses.is_empty() {
            count_query.push_str(" WHERE ");
            count_query.push_str(&where_clauses.join(" AND "));
        }

        bind_count += 1;
        let limit_param = bind_count;
        bind_count += 1;
        let offset_param = bind_count;
        query_str.push_str(&format!(" LIMIT ${} OFFSET ${}", limit_param, offset_param));

        let mut count_q = sqlx::query_scalar::<_, i64>(&count_query);
        if let Some(v) = bundle_type {
            count_q = count_q.bind(v);
        }
        if let Some(v) = status {
            count_q = count_q.bind(v);
        }

        let total = count_q
            .fetch_one(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        let mut main_q = sqlx::query_as::<_, TransactionAdminListItem>(&query_str);
        if let Some(v) = bundle_type {
            main_q = main_q.bind(v);
        }
        if let Some(v) = status {
            main_q = main_q.bind(v);
        }
        main_q = main_q.bind(limit).bind(offset);

        let items = main_q
            .fetch_all(&self.pool)
            .await
            .map_err(crate::Error::Database)?;

        Ok((items, total))
    }

    pub async fn get_transaction(&self, id: Uuid) -> Result<TransactionAdminDetail> {
        let row = sqlx::query_as::<_, TransactionAdminListItem>(
            r#"
            SELECT
                id,
                type,
                status,
                entry_count,
                created_at,
                started_at,
                completed_at,
                error_message
            FROM fhir_transactions
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        let transaction = row.ok_or_else(|| {
            crate::Error::NotFound(format!("Transaction {} not found", id))
        })?;

        let entries = sqlx::query_as::<_, TransactionEntryItem>(
            r#"
            SELECT
                entry_index,
                method,
                url,
                status,
                resource_type,
                resource_id,
                version_id,
                error_message
            FROM fhir_transaction_entries
            WHERE transaction_id = $1
            ORDER BY entry_index
            "#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        Ok(TransactionAdminDetail {
            id: transaction.id,
            bundle_type: transaction.bundle_type,
            status: transaction.status,
            entry_count: transaction.entry_count,
            created_at: transaction.created_at,
            started_at: transaction.started_at,
            completed_at: transaction.completed_at,
            error_message: transaction.error_message,
            entries,
        })
    }

    pub async fn fetch_terminology_summary(&self) -> Result<TerminologySummary> {
        let (codesystems, total_concepts, expansion_counts, valueset_count, conceptmap_count, closure_tables) = tokio::try_join!(
            // 1. CodeSystem stats
            async {
                let rows = sqlx::query(
                    "SELECT system, COUNT(*)::BIGINT as concept_count FROM codesystem_concepts GROUP BY system ORDER BY COUNT(*) DESC"
                )
                .fetch_all(&self.pool)
                .await?;
                Ok::<_, crate::Error>(rows.into_iter().map(|row| CodeSystemSummary {
                    url: row.get("system"),
                    concept_count: row.get("concept_count"),
                }).collect::<Vec<_>>())
            },
            // 2. Total concepts
            async {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*)::BIGINT FROM codesystem_concepts")
                    .fetch_one(&self.pool)
                    .await
                    .map_err(crate::Error::Database)
            },
            // 3. Expansion cache (total + active)
            async {
                let row = sqlx::query(
                    "SELECT COUNT(*)::BIGINT as total, COUNT(*) FILTER (WHERE expires_at IS NULL OR expires_at > NOW())::BIGINT as active FROM valueset_expansions"
                )
                .fetch_one(&self.pool)
                .await?;
                Ok::<_, crate::Error>((row.get::<i64, _>("total"), row.get::<i64, _>("active")))
            },
            // 4. ValueSet count
            async {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*)::BIGINT FROM resources WHERE resource_type = 'ValueSet' AND is_current AND NOT deleted"
                )
                .fetch_one(&self.pool)
                .await
                .map_err(crate::Error::Database)
            },
            // 5. ConceptMap count
            async {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(DISTINCT conceptmap_url)::BIGINT FROM conceptmap_groups"
                )
                .fetch_one(&self.pool)
                .await
                .map_err(crate::Error::Database)
            },
            // 6. Closure tables
            async {
                let rows = sqlx::query(
                    r#"
                    SELECT
                        ct.name,
                        ct.current_version,
                        ct.requires_reinit,
                        COALESCE(cc.cnt, 0)::BIGINT as concept_count,
                        COALESCE(cr.cnt, 0)::BIGINT as relation_count
                    FROM terminology_closure_tables ct
                    LEFT JOIN (
                        SELECT closure_name, COUNT(*)::BIGINT as cnt
                        FROM terminology_closure_concepts
                        GROUP BY closure_name
                    ) cc ON cc.closure_name = ct.name
                    LEFT JOIN (
                        SELECT closure_name, COUNT(*)::BIGINT as cnt
                        FROM terminology_closure_relations
                        GROUP BY closure_name
                    ) cr ON cr.closure_name = ct.name
                    ORDER BY ct.name
                    "#
                )
                .fetch_all(&self.pool)
                .await?;
                Ok::<_, crate::Error>(rows.into_iter().map(|row| ClosureTableSummary {
                    name: row.get("name"),
                    current_version: row.get("current_version"),
                    requires_reinit: row.get("requires_reinit"),
                    concept_count: row.get("concept_count"),
                    relation_count: row.get("relation_count"),
                }).collect::<Vec<_>>())
            },
        )?;

        let (cached_expansions, active_expansions) = expansion_counts;

        Ok(TerminologySummary {
            codesystems,
            total_concepts,
            cached_expansions,
            active_expansions,
            valueset_count,
            conceptmap_count,
            closure_tables,
        })
    }

    pub async fn fetch_compartment_memberships(&self) -> Result<Vec<CompartmentMembershipRecord>> {
        let rows = sqlx::query_as::<_, CompartmentMembershipRecord>(
            r#"
            SELECT
                compartment_type,
                resource_type,
                parameter_names,
                start_param,
                end_param,
                loaded_at
            FROM compartment_memberships
            ORDER BY compartment_type ASC, resource_type ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(crate::Error::Database)?;

        Ok(rows)
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CompartmentMembershipRecord {
    pub compartment_type: String,
    pub resource_type: String,
    pub parameter_names: Vec<String>,
    pub start_param: Option<String>,
    pub end_param: Option<String>,
    pub loaded_at: DateTime<Utc>,
}

// =============================================================================
// Terminology summary
// =============================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminologySummary {
    pub codesystems: Vec<CodeSystemSummary>,
    pub total_concepts: i64,
    pub cached_expansions: i64,
    pub active_expansions: i64,
    pub valueset_count: i64,
    pub conceptmap_count: i64,
    pub closure_tables: Vec<ClosureTableSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeSystemSummary {
    pub url: String,
    pub concept_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosureTableSummary {
    pub name: String,
    pub current_version: i32,
    pub requires_reinit: bool,
    pub concept_count: i64,
    pub relation_count: i64,
}

// =============================================================================
// Transaction Recorder (write path for batch/transaction tracking)
// =============================================================================

pub struct TransactionEntryRecord {
    pub entry_index: i32,
    pub method: String,
    pub url: String,
    pub status: Option<i32>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub version_id: Option<i32>,
    pub error_message: Option<String>,
}

#[derive(Clone)]
pub struct TransactionRecorder {
    pool: PgPool,
}

impl TransactionRecorder {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record_start(
        &self,
        id: Uuid,
        bundle_type: &str,
        entry_count: i32,
        metadata: Option<JsonValue>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO fhir_transactions (id, type, status, entry_count, started_at, metadata)
            VALUES ($1, $2, 'processing', $3, NOW(), $4)
            "#,
        )
        .bind(id)
        .bind(bundle_type)
        .bind(entry_count)
        .bind(metadata)
        .execute(&self.pool)
        .await
        .map_err(crate::Error::Database)?;
        Ok(())
    }

    pub async fn record_complete(
        &self,
        id: Uuid,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE fhir_transactions
            SET status = $2, completed_at = NOW(), error_message = $3
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(status)
        .bind(error_message)
        .execute(&self.pool)
        .await
        .map_err(crate::Error::Database)?;
        Ok(())
    }

    pub async fn record_entries(
        &self,
        transaction_id: Uuid,
        entries: &[TransactionEntryRecord],
    ) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut values_parts = Vec::with_capacity(entries.len());
        let mut param_idx = 0usize;
        for (i, _) in entries.iter().enumerate() {
            let base = i * 9;
            values_parts.push(format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8,
                base + 9,
            ));
            param_idx = base + 9;
        }
        let _ = param_idx;

        let query = format!(
            r#"
            INSERT INTO fhir_transaction_entries
                (transaction_id, entry_index, method, url, status, resource_type, resource_id, version_id, error_message)
            VALUES {}
            "#,
            values_parts.join(", ")
        );

        let mut q = sqlx::query(&query);
        for entry in entries {
            q = q
                .bind(transaction_id)
                .bind(entry.entry_index)
                .bind(&entry.method)
                .bind(&entry.url)
                .bind(entry.status)
                .bind(&entry.resource_type)
                .bind(&entry.resource_id)
                .bind(entry.version_id)
                .bind(&entry.error_message);
        }

        q.execute(&self.pool)
            .await
            .map_err(crate::Error::Database)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceEdge {
    pub source_type: String,
    pub source_id: String,
    pub parameter_name: String,
    pub target_type: String,
    pub target_id: String,
    pub display: Option<String>,
}
