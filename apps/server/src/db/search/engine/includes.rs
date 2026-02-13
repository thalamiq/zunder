use super::{params, JsonValue, SearchEngine, SearchParameters};
use crate::Result;
use sqlx::PgConnection;
use std::collections::HashSet;

impl SearchEngine {
    /// Fetch included resources based on `_include` and `_revinclude`.
    pub(super) async fn fetch_includes(
        &self,
        conn: &mut PgConnection,
        resources: &[JsonValue],
        params: &SearchParameters,
    ) -> Result<Vec<JsonValue>> {
        let mut processed: HashSet<(String, String)> = HashSet::new();
        for r in resources {
            if let (Some(rt), Some(id)) = (
                r.get("resourceType").and_then(|v| v.as_str()),
                r.get("id").and_then(|v| v.as_str()),
            ) {
                processed.insert((rt.to_string(), id.to_string()));
            }
        }

        let mut included = Vec::new();

        // Non-iterating includes apply only to the matching resources.
        for spec in params.include.iter().filter(|s| !s.iterate) {
            self.collect_includes(
                conn,
                spec,
                false,
                resources,
                &mut processed,
                &mut included,
                0,
            )
            .await?;
        }
        for spec in params.revinclude.iter().filter(|s| !s.iterate) {
            self.collect_includes(
                conn,
                spec,
                true,
                resources,
                &mut processed,
                &mut included,
                0,
            )
            .await?;
        }

        // Iterating includes apply to included resources as well as matching resources.
        // We loop to a fixpoint because multiple `:iterate` directives can feed each other.
        const MAX_ITERATE_PASSES: usize = 3;
        for _pass in 0..MAX_ITERATE_PASSES {
            let before = processed.len();
            let mut sources = Vec::with_capacity(resources.len() + included.len());
            sources.extend_from_slice(resources);
            sources.extend_from_slice(&included);

            for spec in params.include.iter().filter(|s| s.iterate) {
                self.collect_includes(
                    conn,
                    spec,
                    false,
                    &sources,
                    &mut processed,
                    &mut included,
                    0,
                )
                .await?;
            }
            for spec in params.revinclude.iter().filter(|s| s.iterate) {
                self.collect_includes(conn, spec, true, &sources, &mut processed, &mut included, 0)
                    .await?;
            }

            if processed.len() == before {
                break;
            }
        }

        Ok(included)
    }

    pub(super) async fn collect_includes(
        &self,
        conn: &mut PgConnection,
        spec: &params::IncludeParam,
        is_reverse: bool,
        source_resources: &[JsonValue],
        processed: &mut HashSet<(String, String)>,
        out: &mut Vec<JsonValue>,
        depth: usize,
    ) -> Result<()> {
        const MAX_DEPTH: usize = 3;
        let mut current_depth = depth;
        let mut current_sources: Vec<JsonValue> = source_resources.to_vec();

        loop {
            if current_sources.is_empty() {
                return Ok(());
            }
            if spec.iterate && current_depth >= MAX_DEPTH {
                return Ok(());
            }

            let mut src_types = Vec::new();
            let mut src_ids = Vec::new();
            for r in &current_sources {
                let Some(rt) = r.get("resourceType").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(id) = r.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                // For _include, source_type filters which source resources to follow refs from.
                // For _revinclude, source_type is the type of resources TO include (not the sources),
                // so we skip this filter — the SQL query filters sr.resource_type instead.
                if !is_reverse && spec.source_type != "*" && spec.source_type != rt {
                    continue;
                }
                src_types.push(rt.to_string());
                src_ids.push(id.to_string());
            }

            if src_types.is_empty() {
                return Ok(());
            }

            let included: Vec<JsonValue> = if is_reverse {
                // Find resources that reference our sources.
                // Track bind parameter index; $1/$2 are always src_types/src_ids.
                let mut next_bind = 3u32;
                let mut sql = String::from(
                    r#"
                    SELECT DISTINCT r.resource
                    FROM search_reference sr
                    INNER JOIN UNNEST($1::text[], $2::text[]) AS tgt(ttype, tid)
                        ON sr.target_type = tgt.ttype AND sr.target_id = tgt.tid
                    INNER JOIN resources r
                        ON r.resource_type = sr.resource_type AND r.id = sr.resource_id AND r.version_id = sr.version_id
                    WHERE r.is_current = true AND r.deleted = false
                    "#,
                );

                // Filter by the source resource type (e.g. only Condition rows, not all)
                let filter_source_type = spec.source_type != "*";
                if filter_source_type {
                    sql.push_str(&format!(" AND sr.resource_type = ${next_bind}"));
                    next_bind += 1;
                }

                if spec.param != "*" {
                    sql.push_str(&format!(" AND sr.parameter_name = ${next_bind}"));
                    next_bind += 1;
                }
                if spec.target_type.is_some() {
                    sql.push_str(&format!(" AND sr.target_type = ${next_bind}"));
                    // next_bind += 1; // last bind
                }

                let mut q = sqlx::query_scalar::<_, JsonValue>(&sql)
                    .bind(&src_types)
                    .bind(&src_ids);
                if filter_source_type {
                    q = q.bind(spec.source_type.clone());
                }
                if spec.param != "*" {
                    q = q.bind(spec.param.clone());
                }
                if let Some(tt) = &spec.target_type {
                    q = q.bind(tt.clone());
                }
                q.fetch_all(&mut *conn)
                    .await
                    .map_err(crate::Error::Database)?
            } else {
                // Follow references from our sources.
                let mut sql = String::from(
                    r#"
                    SELECT DISTINCT r.resource
                    FROM resources src
                    INNER JOIN UNNEST($1::text[], $2::text[]) AS s(rtype, rid)
                        ON src.resource_type = s.rtype AND src.id = s.rid
                    INNER JOIN search_reference sr
                        ON sr.resource_type = src.resource_type AND sr.resource_id = src.id AND sr.version_id = src.version_id
                    INNER JOIN resources r
                        ON r.resource_type = sr.target_type AND r.id = sr.target_id
                    WHERE src.is_current = true AND src.deleted = false
                      AND r.is_current = true AND r.deleted = false
                    "#,
                );

                if spec.param == "*" && spec.target_type.is_none() {
                    sqlx::query_scalar::<_, JsonValue>(&sql)
                        .bind(&src_types)
                        .bind(&src_ids)
                        .fetch_all(&mut *conn)
                        .await
                        .map_err(crate::Error::Database)?
                } else if spec.param == "*" {
                    sql.push_str(" AND sr.target_type = $3");
                    sqlx::query_scalar::<_, JsonValue>(&sql)
                        .bind(&src_types)
                        .bind(&src_ids)
                        .bind(spec.target_type.clone().unwrap())
                        .fetch_all(&mut *conn)
                        .await
                        .map_err(crate::Error::Database)?
                } else if spec.target_type.is_none() {
                    sql.push_str(" AND sr.parameter_name = $3");
                    sqlx::query_scalar::<_, JsonValue>(&sql)
                        .bind(&src_types)
                        .bind(&src_ids)
                        .bind(spec.param.clone())
                        .fetch_all(&mut *conn)
                        .await
                        .map_err(crate::Error::Database)?
                } else {
                    sql.push_str(" AND sr.parameter_name = $3 AND sr.target_type = $4");
                    sqlx::query_scalar::<_, JsonValue>(&sql)
                        .bind(&src_types)
                        .bind(&src_ids)
                        .bind(spec.param.clone())
                        .bind(spec.target_type.clone().unwrap())
                        .fetch_all(&mut *conn)
                        .await
                        .map_err(crate::Error::Database)?
                }
            };

            let mut newly_added = Vec::new();
            for r in included {
                let Some(rt) = r.get("resourceType").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(id) = r.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let key = (rt.to_string(), id.to_string());
                if processed.insert(key) {
                    newly_added.push(r.clone());
                    out.push(r);
                }
            }

            if !spec.iterate || newly_added.is_empty() {
                return Ok(());
            }

            current_sources = newly_added;
            current_depth += 1;
        }
    }
}
