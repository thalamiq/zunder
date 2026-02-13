//! PostgreSQL-backed `ResourceStore` implementation

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value as JsonValue;
use sqlx::{PgPool, Row};

use crate::{
    db::traits::ResourceStore,
    models::{HistoryEntry, HistoryMethod, HistoryResult, Resource},
    Error, Result,
};

/// PostgreSQL-backed ResourceStore implementation
#[derive(Clone)]
pub struct PostgresResourceStore {
    pub(crate) pool: PgPool,
}

impl PostgresResourceStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn history_type_resources(
        &self,
        resource_type: &str,
        count: Option<i32>,
        since: Option<chrono::DateTime<chrono::Utc>>,
        at: Option<chrono::DateTime<chrono::Utc>>,
        sort_ascending: bool,
    ) -> Result<Vec<Resource>> {
        // _at: for each resource of this type, return the version that was current at the instant.
        if let Some(at_instant) = at {
            let limit = count.unwrap_or(100);
            let order = if sort_ascending { "ASC" } else { "DESC" };

            let sql = "SELECT DISTINCT ON (id) id, resource_type, version_id, resource, last_updated, deleted
                 FROM resources
                 WHERE resource_type = $1 AND last_updated <= $2
                 ORDER BY id, version_id DESC".to_string();
            // Wrap in an outer query for ordering and LIMIT
            let sql = format!(
                "SELECT * FROM ({sql}) sub ORDER BY last_updated {order}, id ASC LIMIT $3"
            );

            let rows = sqlx::query(&sql)
                .bind(resource_type)
                .bind(at_instant)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(Error::Database)?;

            return Ok(rows
                .into_iter()
                .map(|r| Resource {
                    id: r.get("id"),
                    resource_type: r.get("resource_type"),
                    version_id: r.get("version_id"),
                    resource: r.get("resource"),
                    last_updated: r.get("last_updated"),
                    deleted: r.get("deleted"),
                })
                .collect());
        }

        let limit = count.unwrap_or(100);
        let order = if sort_ascending { "ASC" } else { "DESC" };

        // Note: `order` is injected from a boolean and is not user-controlled.
        let sql = format!(
            "SELECT id, resource_type, version_id, resource, last_updated, deleted
             FROM resources
             WHERE resource_type = $1
               AND ($2::TIMESTAMPTZ IS NULL OR last_updated >= $2)
             ORDER BY last_updated {order}, id ASC, version_id {order}
             LIMIT $3"
        );

        let rows = sqlx::query(&sql)
            .bind(resource_type)
            .bind(since)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(Error::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| Resource {
                id: r.get("id"),
                resource_type: r.get("resource_type"),
                version_id: r.get("version_id"),
                resource: r.get("resource"),
                last_updated: r.get("last_updated"),
                deleted: r.get("deleted"),
            })
            .collect())
    }

    pub async fn history_system_resources(
        &self,
        count: Option<i32>,
        since: Option<chrono::DateTime<chrono::Utc>>,
        at: Option<chrono::DateTime<chrono::Utc>>,
        sort_ascending: bool,
    ) -> Result<Vec<Resource>> {
        // _at: for each resource across all types, return the version that was current at the instant.
        if let Some(at_instant) = at {
            let limit = count.unwrap_or(100);
            let order = if sort_ascending { "ASC" } else { "DESC" };

            let sql = "SELECT DISTINCT ON (resource_type, id) id, resource_type, version_id, resource, last_updated, deleted
                 FROM resources
                 WHERE last_updated <= $1
                 ORDER BY resource_type, id, version_id DESC".to_string();
            let sql = format!(
                "SELECT * FROM ({sql}) sub ORDER BY last_updated {order}, resource_type ASC, id ASC LIMIT $2"
            );

            let rows = sqlx::query(&sql)
                .bind(at_instant)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(Error::Database)?;

            return Ok(rows
                .into_iter()
                .map(|r| Resource {
                    id: r.get("id"),
                    resource_type: r.get("resource_type"),
                    version_id: r.get("version_id"),
                    resource: r.get("resource"),
                    last_updated: r.get("last_updated"),
                    deleted: r.get("deleted"),
                })
                .collect());
        }

        let limit = count.unwrap_or(100);
        let order = if sort_ascending { "ASC" } else { "DESC" };

        // Note: `order` is injected from a boolean and is not user-controlled.
        let sql = format!(
            "SELECT id, resource_type, version_id, resource, last_updated, deleted
             FROM resources
             WHERE ($1::TIMESTAMPTZ IS NULL OR last_updated >= $1)
             ORDER BY last_updated {order}, resource_type ASC, id ASC, version_id {order}
             LIMIT $2"
        );

        let rows = sqlx::query(&sql)
            .bind(since)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(Error::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| Resource {
                id: r.get("id"),
                resource_type: r.get("resource_type"),
                version_id: r.get("version_id"),
                resource: r.get("resource"),
                last_updated: r.get("last_updated"),
                deleted: r.get("deleted"),
            })
            .collect())
    }

    pub async fn list_current_by_canonical_url(
        &self,
        canonical_url: &str,
    ) -> Result<Vec<JsonValue>> {
        let rows = sqlx::query(
            "SELECT resource
             FROM resources
             WHERE is_current = TRUE
               AND deleted = FALSE
               AND url = $1

             UNION ALL

             SELECT resource
             FROM resources
             WHERE is_current = TRUE
               AND deleted = FALSE
               AND url IS NULL
               AND resource->>'url' = $1",
        )
        .bind(canonical_url)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::Database)?;

        Ok(rows.into_iter().map(|r| r.get("resource")).collect())
    }

    pub async fn get_by_canonical_url_and_version(
        &self,
        canonical_url: &str,
        version: &str,
    ) -> Result<Option<JsonValue>> {
        let row = sqlx::query(
            "SELECT resource
             FROM (
                 SELECT resource, is_current, last_updated, version_id
                 FROM resources
                 WHERE deleted = FALSE
                   AND url = $1
                   AND resource->>'version' = $2

                 UNION ALL

                 SELECT resource, is_current, last_updated, version_id
                 FROM resources
                 WHERE deleted = FALSE
                   AND url IS NULL
                   AND resource->>'url' = $1
                   AND resource->>'version' = $2
             ) candidates
             ORDER BY is_current DESC, last_updated DESC, version_id DESC
             LIMIT 1",
        )
        .bind(canonical_url)
        .bind(version)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::Database)?;

        Ok(row.map(|r| r.get("resource")))
    }

    pub(crate) fn extract_url(resource: &JsonValue) -> Option<String> {
        resource
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    pub(crate) fn extract_meta_source(resource: &JsonValue) -> Option<String> {
        resource
            .get("meta")
            .and_then(|m| m.get("source"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    pub(crate) fn extract_meta_tags(resource: &JsonValue) -> Option<Vec<String>> {
        let tags = resource
            .get("meta")
            .and_then(|m| m.get("tag"))
            .and_then(|v| v.as_array())?;

        let mut codes: Vec<String> = tags
            .iter()
            .filter_map(|tag| {
                tag.get("code")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        if codes.is_empty() {
            return None;
        }

        codes.sort();
        codes.dedup();
        Some(codes)
    }

    pub(crate) fn extract_meta_last_updated(resource: &JsonValue) -> Option<chrono::DateTime<Utc>> {
        let raw = resource
            .get("meta")
            .and_then(|m| m.get("lastUpdated"))
            .and_then(|v| v.as_str())?;
        chrono::DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }

    /// Load multiple resources in a single query for batch processing
    ///
    /// This is used by background workers to efficiently load resources
    /// that need to be indexed after batch/transaction operations.
    pub async fn load_resources_batch(
        &self,
        resource_type: &str,
        resource_ids: &[String],
    ) -> Result<Vec<Resource>> {
        if resource_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            "SELECT id, resource_type, version_id, resource, last_updated, deleted
             FROM resources
             WHERE resource_type = $1
               AND id = ANY($2)
               AND is_current = true
               AND deleted = false
             ORDER BY id",
        )
        .bind(resource_type)
        .bind(resource_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::Database)?;

        let resources: Vec<Resource> = rows
            .into_iter()
            .map(|row| Resource {
                id: row.get("id"),
                resource_type: row.get("resource_type"),
                version_id: row.get("version_id"),
                resource: row.get("resource"),
                last_updated: row.get("last_updated"),
                deleted: row.get("deleted"),
            })
            .collect();

        Ok(resources)
    }

    /// Check which of the given `(resource_type, id)` pairs exist as current, non-deleted resources.
    ///
    /// Returns a set of `(resource_type, id)` pairs that exist.
    pub async fn check_resources_exist(
        &self,
        refs: &[(String, String)],
    ) -> Result<std::collections::HashSet<(String, String)>> {
        if refs.is_empty() {
            return Ok(std::collections::HashSet::new());
        }

        let types: Vec<&str> = refs.iter().map(|(t, _)| t.as_str()).collect();
        let ids: Vec<&str> = refs.iter().map(|(_, id)| id.as_str()).collect();

        let rows = sqlx::query(
            "SELECT r.resource_type, r.id
             FROM UNNEST($1::text[], $2::text[]) AS input(resource_type, id)
             JOIN resources r ON r.resource_type = input.resource_type
                             AND r.id = input.id
                             AND r.is_current = true
                             AND r.deleted = false",
        )
        .bind(&types)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::Database)?;

        let mut existing = std::collections::HashSet::new();
        for row in rows {
            let rt: String = row.get("resource_type");
            let id: String = row.get("id");
            existing.insert((rt, id));
        }
        Ok(existing)
    }

    /// Find resources that reference a given target via the `search_reference` index.
    ///
    /// Returns `Vec<(resource_type, resource_id)>` of referencing resources, limited to `limit`.
    pub async fn find_referencing_resources(
        &self,
        target_type: &str,
        target_id: &str,
        limit: i64,
    ) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            "SELECT DISTINCT sr.resource_type, sr.resource_id
             FROM search_reference sr
             JOIN resources r ON r.resource_type = sr.resource_type
                             AND r.id = sr.resource_id
                             AND r.is_current = true
                             AND r.deleted = false
             WHERE sr.target_type = $1
               AND sr.target_id = $2
             LIMIT $3",
        )
        .bind(target_type)
        .bind(target_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::Database)?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let rt: String = row.get("resource_type");
                let id: String = row.get("resource_id");
                (rt, id)
            })
            .collect())
    }

    /// Physically delete a resource and its full version history.
    ///
    /// Returns the number of rows removed from the `resources` table.
    pub async fn hard_delete(&self, resource_type: &str, id: &str) -> Result<u64> {
        let mut tx = self.pool.begin().await.map_err(Error::Database)?;

        let resources_deleted = sqlx::query(
            "DELETE FROM resources
             WHERE resource_type = $1 AND id = $2",
        )
        .bind(resource_type)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(Error::Database)?
        .rows_affected();

        sqlx::query(
            "DELETE FROM resource_versions
             WHERE resource_type = $1 AND id = $2",
        )
        .bind(resource_type)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(Error::Database)?;

        tx.commit().await.map_err(Error::Database)?;
        Ok(resources_deleted)
    }

    /// Physically delete a specific historical version of a resource.
    ///
    /// If the deleted version was the current one, the newest remaining version is promoted to
    /// `is_current = true`. If no versions remain, the `resource_versions` row is removed.
    pub async fn hard_delete_version(
        &self,
        resource_type: &str,
        id: &str,
        version_id: i32,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(crate::Error::Database)?;

        let row = sqlx::query(
            "SELECT is_current
             FROM resources
             WHERE resource_type = $1 AND id = $2 AND version_id = $3
             FOR UPDATE",
        )
        .bind(resource_type)
        .bind(id)
        .bind(version_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(crate::Error::Database)?;

        let Some(row) = row else {
            let exists =
                sqlx::query("SELECT 1 FROM resources WHERE resource_type = $1 AND id = $2")
                    .bind(resource_type)
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(crate::Error::Database)?;

            if exists.is_some() {
                return Err(crate::Error::VersionNotFound {
                    resource_type: resource_type.to_string(),
                    id: id.to_string(),
                    version_id,
                });
            }
            return Err(crate::Error::ResourceNotFound {
                resource_type: resource_type.to_string(),
                id: id.to_string(),
            });
        };

        let was_current: bool = row.get("is_current");

        sqlx::query(
            "DELETE FROM resources
             WHERE resource_type = $1 AND id = $2 AND version_id = $3",
        )
        .bind(resource_type)
        .bind(id)
        .bind(version_id)
        .execute(&mut *tx)
        .await
        .map_err(crate::Error::Database)?;

        if was_current {
            let newest = sqlx::query(
                "SELECT version_id
                 FROM resources
                 WHERE resource_type = $1 AND id = $2
                 ORDER BY version_id DESC
                 LIMIT 1
                 FOR UPDATE",
            )
            .bind(resource_type)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(crate::Error::Database)?;

            if let Some(newest) = newest {
                let promoted_version: i32 = newest.get("version_id");
                sqlx::query(
                    "UPDATE resources
                     SET is_current = true
                     WHERE resource_type = $1 AND id = $2 AND version_id = $3",
                )
                .bind(resource_type)
                .bind(id)
                .bind(promoted_version)
                .execute(&mut *tx)
                .await
                .map_err(crate::Error::Database)?;
            } else {
                let _ = sqlx::query(
                    "DELETE FROM resource_versions
                     WHERE resource_type = $1 AND id = $2",
                )
                .bind(resource_type)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(crate::Error::Database)?;
            }
        }

        tx.commit().await.map_err(crate::Error::Database)?;
        Ok(())
    }
}

#[async_trait]
impl ResourceStore for PostgresResourceStore {
    async fn create(&self, resource_type: &str, resource: JsonValue) -> Result<Resource> {
        // Extract ID from the resource JSON (populated by the service layer)
        let id = resource
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidResource("Missing id field".to_string()))?
            .to_string();

        // Atomically get next version_id
        let version_row = sqlx::query(
            "INSERT INTO resource_versions (resource_type, id, next_version)
             VALUES ($1, $2, 1)
             ON CONFLICT (resource_type, id)
             DO UPDATE SET next_version = resource_versions.next_version + 1
             RETURNING next_version",
        )
        .bind(resource_type)
        .bind(&id)
        .fetch_one(&self.pool)
        .await
        .map_err(Error::Database)?;

        let version_id: i32 = version_row.get("next_version");
        let now = Self::extract_meta_last_updated(&resource).unwrap_or_else(Utc::now);
        let url = Self::extract_url(&resource);
        let meta_source = Self::extract_meta_source(&resource);
        let meta_tags = Self::extract_meta_tags(&resource);

        sqlx::query(
            "INSERT INTO resources (id, resource_type, version_id, resource, last_updated, url, meta_source, meta_tags, deleted, is_current)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, false, true)",
        )
        .bind(&id)
        .bind(resource_type)
        .bind(version_id)
        .bind(&resource)
        .bind(now)
        .bind(url)
        .bind(meta_source)
        .bind(meta_tags)
        .execute(&self.pool)
        .await
        .map_err(Error::Database)?;

        Ok(Resource {
            id,
            resource_type: resource_type.to_string(),
            version_id,
            resource,
            last_updated: now,
            deleted: false,
        })
    }

    async fn upsert(&self, resource_type: &str, id: &str, resource: JsonValue) -> Result<Resource> {
        // Atomically get next version_id
        let version_row = sqlx::query(
            "INSERT INTO resource_versions (resource_type, id, next_version)
             VALUES ($1, $2, 1)
             ON CONFLICT (resource_type, id)
             DO UPDATE SET next_version = resource_versions.next_version + 1
             RETURNING next_version",
        )
        .bind(resource_type)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(Error::Database)?;

        let version_id: i32 = version_row.get("next_version");
        let now = Self::extract_meta_last_updated(&resource).unwrap_or_else(Utc::now);
        let url = Self::extract_url(&resource);
        let meta_source = Self::extract_meta_source(&resource);
        let meta_tags = Self::extract_meta_tags(&resource);

        // Mark old versions as not current (if any exist)
        // The unique index will prevent multiple current versions
        sqlx::query(
            "UPDATE resources SET is_current = false
             WHERE resource_type = $1 AND id = $2 AND is_current = true",
        )
        .bind(resource_type)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(Error::Database)?;

        // Insert new version
        sqlx::query(
            "INSERT INTO resources (id, resource_type, version_id, resource, last_updated, url, meta_source, meta_tags, deleted, is_current)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, false, true)",
        )
        .bind(id)
        .bind(resource_type)
        .bind(version_id)
        .bind(&resource)
        .bind(now)
        .bind(url)
        .bind(meta_source)
        .bind(meta_tags)
        .execute(&self.pool)
        .await
        .map_err(Error::Database)?;

        Ok(Resource {
            id: id.to_string(),
            resource_type: resource_type.to_string(),
            version_id,
            resource,
            last_updated: now,
            deleted: false,
        })
    }

    async fn read(&self, resource_type: &str, id: &str) -> Result<Option<Resource>> {
        let row = sqlx::query(
            "SELECT id, resource_type, version_id, resource, last_updated, deleted
             FROM resources
             WHERE resource_type = $1 AND id = $2 AND is_current = true",
        )
        .bind(resource_type)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::Database)?;

        Ok(row.map(|r| Resource {
            id: r.get("id"),
            resource_type: r.get("resource_type"),
            version_id: r.get("version_id"),
            resource: r.get("resource"),
            last_updated: r.get("last_updated"),
            deleted: r.get("deleted"),
        }))
    }

    async fn update(
        &self,
        resource_type: &str,
        id: &str,
        resource: JsonValue,
        expected_version: Option<i32>,
    ) -> Result<Resource> {
        // Get current version
        let current = sqlx::query(
            "SELECT version_id FROM resources
             WHERE resource_type = $1 AND id = $2 AND is_current = true",
        )
        .bind(resource_type)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::Database)?
        .ok_or_else(|| Error::ResourceNotFound {
            resource_type: resource_type.to_string(),
            id: id.to_string(),
        })?;

        let current_version: i32 = current.get("version_id");

        // Check version if expected
        if let Some(expected) = expected_version {
            if current_version != expected {
                return Err(Error::VersionConflict {
                    expected,
                    actual: current_version,
                });
            }
        }

        // Atomically get next version_id
        let version_row = sqlx::query(
            "INSERT INTO resource_versions (resource_type, id, next_version)
             VALUES ($1, $2, 1)
             ON CONFLICT (resource_type, id)
             DO UPDATE SET next_version = resource_versions.next_version + 1
             RETURNING next_version",
        )
        .bind(resource_type)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(Error::Database)?;

        let new_version: i32 = version_row.get("next_version");
        let now = Self::extract_meta_last_updated(&resource).unwrap_or_else(Utc::now);
        let url = Self::extract_url(&resource);
        let meta_source = Self::extract_meta_source(&resource);
        let meta_tags = Self::extract_meta_tags(&resource);

        // Mark current as not current
        sqlx::query(
            "UPDATE resources SET is_current = false
             WHERE resource_type = $1 AND id = $2 AND is_current = true",
        )
        .bind(resource_type)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(Error::Database)?;

        // Insert new version
        sqlx::query(
            "INSERT INTO resources (id, resource_type, version_id, resource, last_updated, url, meta_source, meta_tags, deleted, is_current)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, false, true)",
        )
        .bind(id)
        .bind(resource_type)
        .bind(new_version)
        .bind(&resource)
        .bind(now)
        .bind(url)
        .bind(meta_source)
        .bind(meta_tags)
        .execute(&self.pool)
        .await
        .map_err(Error::Database)?;

        Ok(Resource {
            id: id.to_string(),
            resource_type: resource_type.to_string(),
            version_id: new_version,
            resource,
            last_updated: now,
            deleted: false,
        })
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<i32> {
        // Get current version
        let current = sqlx::query(
            "SELECT version_id, deleted FROM resources
             WHERE resource_type = $1 AND id = $2 AND is_current = true",
        )
        .bind(resource_type)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::Database)?
        .ok_or_else(|| Error::ResourceNotFound {
            resource_type: resource_type.to_string(),
            id: id.to_string(),
        })?;

        let current_version: i32 = current.get("version_id");
        let is_deleted: bool = current.get("deleted");
        if is_deleted {
            return Ok(current_version);
        }

        // Atomically get next version_id
        let version_row = sqlx::query(
            "INSERT INTO resource_versions (resource_type, id, next_version)
             VALUES ($1, $2, 1)
             ON CONFLICT (resource_type, id)
             DO UPDATE SET next_version = resource_versions.next_version + 1
             RETURNING next_version",
        )
        .bind(resource_type)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(Error::Database)?;

        let new_version: i32 = version_row.get("next_version");
        let now = Utc::now();

        // Mark current as not current
        sqlx::query(
            "UPDATE resources SET is_current = false
             WHERE resource_type = $1 AND id = $2 AND is_current = true",
        )
        .bind(resource_type)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(Error::Database)?;

        // Insert deleted version
        let resource = serde_json::json!({
            "resourceType": resource_type,
            "id": id
        });

        sqlx::query(
            "INSERT INTO resources (id, resource_type, version_id, resource, last_updated, url, meta_source, meta_tags, deleted, is_current)
             VALUES ($1, $2, $3, $4, $5, NULL, NULL, NULL, true, true)",
        )
        .bind(id)
        .bind(resource_type)
        .bind(new_version)
        .bind(resource)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(Error::Database)?;

        Ok(new_version)
    }

    async fn vread(&self, resource_type: &str, id: &str, version_id: i32) -> Result<Resource> {
        let row = sqlx::query(
            "SELECT id, resource_type, version_id, resource, last_updated, deleted
             FROM resources
             WHERE resource_type = $1 AND id = $2 AND version_id = $3",
        )
        .bind(resource_type)
        .bind(id)
        .bind(version_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::Database)?
        .ok_or_else(|| Error::VersionNotFound {
            resource_type: resource_type.to_string(),
            id: id.to_string(),
            version_id,
        })?;

        Ok(Resource {
            id: row.get("id"),
            resource_type: row.get("resource_type"),
            version_id: row.get("version_id"),
            resource: row.get("resource"),
            last_updated: row.get("last_updated"),
            deleted: row.get("deleted"),
        })
    }

    async fn history(
        &self,
        resource_type: &str,
        id: &str,
        count: Option<i32>,
        since: Option<chrono::DateTime<chrono::Utc>>,
        at: Option<chrono::DateTime<chrono::Utc>>,
        sort_ascending: bool,
    ) -> Result<HistoryResult> {
        // _at: return only the version that was current at the given instant.
        // That is the version with the highest last_updated <= _at.
        if let Some(at_instant) = at {
            let row = sqlx::query(
                "SELECT id, resource_type, version_id, resource, last_updated, deleted
                 FROM resources
                 WHERE resource_type = $1 AND id = $2 AND last_updated <= $3
                 ORDER BY version_id DESC
                 LIMIT 1",
            )
            .bind(resource_type)
            .bind(id)
            .bind(at_instant)
            .fetch_optional(&self.pool)
            .await
            .map_err(Error::Database)?;

            let entries = match row {
                Some(r) => {
                    let version_id: i32 = r.get("version_id");
                    let deleted: bool = r.get("deleted");
                    let method = if deleted {
                        HistoryMethod::Delete
                    } else if version_id == 1 {
                        HistoryMethod::Post
                    } else {
                        HistoryMethod::Put
                    };
                    vec![HistoryEntry {
                        resource: Resource {
                            id: r.get("id"),
                            resource_type: r.get("resource_type"),
                            version_id,
                            resource: r.get("resource"),
                            last_updated: r.get("last_updated"),
                            deleted,
                        },
                        method,
                    }]
                }
                None => vec![],
            };

            let total = entries.len() as i64;
            return Ok(HistoryResult {
                entries,
                total: Some(total),
            });
        }

        let limit = count.unwrap_or(100);
        let order = if sort_ascending { "ASC" } else { "DESC" };

        // Note: `order` is injected from a boolean and is not user-controlled.
        let sql = format!(
            "SELECT id, resource_type, version_id, resource, last_updated, deleted
             FROM resources
             WHERE resource_type = $1 AND id = $2
               AND ($3::TIMESTAMPTZ IS NULL OR last_updated >= $3)
             ORDER BY last_updated {order}, version_id {order}
             LIMIT $4"
        );

        let rows = sqlx::query(&sql)
            .bind(resource_type)
            .bind(id)
            .bind(since)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(Error::Database)?;

        let entries = rows
            .into_iter()
            .map(|r| {
                let version_id: i32 = r.get("version_id");
                let deleted: bool = r.get("deleted");

                let method = if deleted {
                    HistoryMethod::Delete
                } else if version_id == 1 {
                    HistoryMethod::Post
                } else {
                    HistoryMethod::Put
                };

                HistoryEntry {
                    resource: Resource {
                        id: r.get("id"),
                        resource_type: r.get("resource_type"),
                        version_id,
                        resource: r.get("resource"),
                        last_updated: r.get("last_updated"),
                        deleted,
                    },
                    method,
                }
            })
            .collect();

        // Get total count
        let total_row = sqlx::query(
            "SELECT COUNT(*) as count
             FROM resources
             WHERE resource_type = $1 AND id = $2
               AND ($3::TIMESTAMPTZ IS NULL OR last_updated >= $3)",
        )
        .bind(resource_type)
        .bind(id)
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(Error::Database)?;

        let total: i64 = total_row.get("count");

        Ok(HistoryResult {
            entries,
            total: Some(total),
        })
    }

    async fn search(
        &self,
        _resource_type: &str,
        _search_params: &[(String, String)],
    ) -> Result<Vec<Resource>> {
        // This method is deprecated and should not be used.
        // All search operations should use SearchEngine instead, which provides
        // proper search parameter parsing, indexing, and result formatting.
        //
        // If you're seeing this error, update your code to use:
        //   state.search_engine.search(Some(&resource_type), &params, base_url).await
        Err(crate::Error::Internal(
            "ResourceStore::search is not implemented. Use SearchEngine for all search operations."
                .to_string(),
        ))
    }

    async fn load_resources_batch(
        &self,
        resource_type: &str,
        ids: &[String],
    ) -> Result<Vec<Resource>> {
        PostgresResourceStore::load_resources_batch(self, resource_type, ids).await
    }
}
