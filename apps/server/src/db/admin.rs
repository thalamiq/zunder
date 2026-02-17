//! Admin repository - server statistics queries.

use crate::services::admin::{
    AuditEventAdminDetail, AuditEventAdminListItem, SearchHashCollisionStatus,
    SearchIndexTableStatus, SearchParameterAdminListItem, SearchParameterIndexingStatus,
};
use crate::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row};

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
