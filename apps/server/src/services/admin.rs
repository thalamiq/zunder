//! Admin service - server statistics and diagnostics.

use crate::{
    db::admin::{
        AdminRepository, CompartmentMembershipRecord, ReferenceEdge, ResourceTypeStats,
        TerminologySummary,
    },
    Result,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTypeStatsTotals {
    pub resource_type_count: i64,
    pub total_versions: i64,
    pub current_total: i64,
    pub current_active: i64,
    pub current_deleted: i64,
    pub last_updated: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTypeStatsReport {
    pub resource_types: Vec<ResourceTypeStats>,
    pub totals: ResourceTypeStatsTotals,
}

pub struct AdminService {
    repo: AdminRepository,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventListQuery {
    pub action: Option<String>,
    pub outcome: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub patient_id: Option<String>,
    pub client_id: Option<String>,
    pub user_id: Option<String>,
    pub request_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventListResponse {
    pub items: Vec<AuditEventAdminListItem>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventAdminListItem {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub http_method: String,
    pub fhir_action: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub patient_id: Option<String>,
    pub client_id: Option<String>,
    pub user_id: Option<String>,
    pub request_id: Option<String>,
    pub status_code: i32,
    pub outcome: String,
    #[sqlx(json)]
    pub audit_event: JsonValue,
    #[sqlx(json)]
    pub details: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventAdminDetail {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub http_method: String,
    pub fhir_action: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub version_id: Option<i32>,
    pub patient_id: Option<String>,
    pub client_id: Option<String>,
    pub user_id: Option<String>,
    pub scopes: Vec<String>,
    pub token_type: String,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
    pub status_code: i32,
    pub outcome: String,
    #[sqlx(json)]
    pub audit_event: JsonValue,
    #[sqlx(json)]
    pub details: Option<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchParameterListQuery {
    pub q: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub resource_type: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchParameterListResponse {
    pub items: Vec<SearchParameterAdminListItem>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SearchParameterAdminListItem {
    pub id: String,
    pub url: Option<String>,
    pub code: Option<String>,
    #[sqlx(json)]
    pub base: Vec<String>,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub type_: Option<String>,
    pub status: Option<String>,
    pub expression: Option<String>,
    pub description: Option<String>,
    pub last_updated: Option<String>,
    pub server_expected_bases: i64,
    pub server_configured_bases: i64,
    pub server_active: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexTableStatus {
    pub table_name: String,
    pub row_count: i64,
    pub is_unlogged: bool,
    pub size_pretty: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SearchHashCollisionStatus {
    pub table_name: String,
    pub collision_count: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SearchParameterIndexingStatus {
    pub resource_type: String,
    pub version_number: i32,
    pub param_count: i32,
    pub current_hash: String,
    pub last_parameter_change: DateTime<Utc>,
    pub total_resources: i64,
    pub indexed_with_current: i64,
    pub indexed_with_old: i64,
    pub never_indexed: i64,
    pub coverage_percent: f64,
    pub indexing_needed: bool,
    pub oldest_indexed_at: Option<DateTime<Utc>>,
    pub newest_indexed_at: Option<DateTime<Utc>>,
}

// =============================================================================
// Transaction tracking types
// =============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionListQuery {
    pub bundle_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionListResponse {
    pub items: Vec<TransactionAdminListItem>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TransactionAdminListItem {
    pub id: Uuid,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub bundle_type: String,
    pub status: String,
    pub entry_count: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionAdminDetail {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub bundle_type: String,
    pub status: String,
    pub entry_count: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub entries: Vec<TransactionEntryItem>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TransactionEntryItem {
    pub entry_index: i32,
    pub method: String,
    pub url: String,
    pub status: Option<i32>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub version_id: Option<i32>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceReferencesResponse {
    pub resource_type: String,
    pub resource_id: String,
    pub outgoing: Vec<ReferenceEdge>,
    pub incoming: Vec<ReferenceEdge>,
}

impl AdminService {
    pub fn new(repo: AdminRepository) -> Self {
        Self { repo }
    }

    pub async fn get_resource_references(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<ResourceReferencesResponse> {
        let (outgoing, incoming) = tokio::try_join!(
            self.repo.fetch_outgoing_references(resource_type, id),
            self.repo.fetch_incoming_references(resource_type, id),
        )?;

        Ok(ResourceReferencesResponse {
            resource_type: resource_type.to_string(),
            resource_id: id.to_string(),
            outgoing,
            incoming,
        })
    }

    pub async fn get_batch_references(
        &self,
        keys: Vec<(String, String)>,
    ) -> Result<Vec<ReferenceEdge>> {
        self.repo.fetch_batch_references(&keys).await
    }

    pub async fn list_audit_events(
        &self,
        query: AuditEventListQuery,
    ) -> Result<AuditEventListResponse> {
        let limit = query.limit.unwrap_or(100).clamp(1, 1000);
        let offset = query.offset.unwrap_or(0).max(0);

        let action = query
            .action
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let outcome = query
            .outcome
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        let resource_type = query
            .resource_type
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let resource_id = query
            .resource_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let patient_id = query
            .patient_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let client_id = query
            .client_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let user_id = query
            .user_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let request_id = query
            .request_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        let (items, total) = self
            .repo
            .list_audit_events(
                action.as_deref(),
                outcome.as_deref(),
                resource_type,
                resource_id,
                patient_id,
                client_id,
                user_id,
                request_id,
                limit,
                offset,
            )
            .await?;

        Ok(AuditEventListResponse { items, total })
    }

    pub async fn get_audit_event(&self, id: i64) -> Result<AuditEventAdminDetail> {
        self.repo.get_audit_event(id).await
    }

    pub async fn resource_type_stats(&self) -> Result<ResourceTypeStatsReport> {
        let resource_types = self.repo.fetch_resource_type_stats().await?;
        let totals = ResourceTypeStatsTotals::from_stats(&resource_types);

        Ok(ResourceTypeStatsReport {
            resource_types,
            totals,
        })
    }

    pub async fn search_parameter_indexing_status(
        &self,
        resource_type: Option<&str>,
    ) -> Result<Vec<SearchParameterIndexingStatus>> {
        self.repo
            .fetch_search_parameter_indexing_status(resource_type)
            .await
    }

    pub async fn search_index_table_status(&self) -> Result<Vec<SearchIndexTableStatus>> {
        self.repo.fetch_search_index_table_status().await
    }

    pub async fn search_hash_collisions(&self) -> Result<Vec<SearchHashCollisionStatus>> {
        self.repo.fetch_search_hash_collisions().await
    }

    pub async fn list_search_parameters(
        &self,
        query: SearchParameterListQuery,
    ) -> Result<SearchParameterListResponse> {
        let limit = query.limit.unwrap_or(100).clamp(1, 1000);
        let offset = query.offset.unwrap_or(0).max(0);

        let q = query.q.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty());
        let type_ = query
            .type_
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let resource_type = query
            .resource_type
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        let (items, total) = self
            .repo
            .list_search_parameters(q, type_.as_deref(), resource_type, limit, offset)
            .await?;

        Ok(SearchParameterListResponse { items, total })
    }

    pub async fn toggle_search_parameter_active(&self, id: i32) -> Result<bool> {
        self.repo.toggle_search_parameter_active(id).await
    }

    pub async fn terminology_summary(&self) -> Result<TerminologySummary> {
        self.repo.fetch_terminology_summary().await
    }

    pub async fn compartment_memberships(&self) -> Result<Vec<CompartmentMembershipRecord>> {
        self.repo.fetch_compartment_memberships().await
    }

    pub async fn list_transactions(
        &self,
        query: TransactionListQuery,
    ) -> Result<TransactionListResponse> {
        let limit = query.limit.unwrap_or(100).clamp(1, 1000);
        let offset = query.offset.unwrap_or(0).max(0);

        let bundle_type = query
            .bundle_type
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let status = query
            .status
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        let (items, total) = self
            .repo
            .list_transactions(bundle_type.as_deref(), status.as_deref(), limit, offset)
            .await?;

        Ok(TransactionListResponse { items, total })
    }

    pub async fn get_transaction(&self, id: Uuid) -> Result<TransactionAdminDetail> {
        self.repo.get_transaction(id).await
    }
}

impl ResourceTypeStatsTotals {
    fn from_stats(stats: &[ResourceTypeStats]) -> Self {
        let mut total_versions = 0i64;
        let mut current_total = 0i64;
        let mut current_active = 0i64;
        let mut current_deleted = 0i64;
        let mut last_updated: Option<DateTime<Utc>> = None;

        for stat in stats {
            total_versions += stat.total_versions;
            current_total += stat.current_total;
            current_active += stat.current_active;
            current_deleted += stat.current_deleted;

            if let Some(updated) = stat.last_updated.as_ref() {
                let updated = *updated;
                last_updated = Some(match last_updated {
                    Some(existing) => existing.max(updated),
                    None => updated,
                });
            }
        }

        Self {
            resource_type_count: stats.len() as i64,
            total_versions,
            current_total,
            current_active,
            current_deleted,
            last_updated,
        }
    }
}
