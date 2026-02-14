//! Runtime configuration API handlers

use crate::runtime_config::UpdateConfigRequest;
use crate::{state::AppState, Result};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

/// Returns 404 if runtime config is disabled in server config.
fn require_runtime_config(state: &AppState) -> std::result::Result<(), Response> {
    if !state.config.ui.runtime_config_enabled {
        return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Runtime configuration is disabled"
        }))).into_response());
    }
    Ok(())
}

/// Query parameters for listing configuration
#[derive(Debug, Deserialize)]
pub struct ListConfigQuery {
    pub category: Option<String>,
}

/// Query parameters for audit log
#[derive(Debug, Deserialize)]
pub struct AuditLogQuery {
    pub key: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// List all configuration entries
///
/// GET /admin/config
pub async fn list_config(
    State(state): State<AppState>,
    Query(query): Query<ListConfigQuery>,
) -> Result<Response> {
    if let Err(r) = require_runtime_config(&state) { return Ok(r); }
    let result = state
        .runtime_config_service
        .list_all(query.category.as_deref())
        .await?;

    Ok((StatusCode::OK, Json(result)).into_response())
}

/// Get a single configuration entry
///
/// GET /admin/config/:key
pub async fn get_config(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Response> {
    if let Err(r) = require_runtime_config(&state) { return Ok(r); }
    let result = state.runtime_config_service.get(&key).await?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

/// Update a configuration value
///
/// PUT /admin/config/:key
pub async fn update_config(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(request): Json<UpdateConfigRequest>,
) -> Result<Response> {
    if let Err(r) = require_runtime_config(&state) { return Ok(r); }
    let result = state.runtime_config_service.update(&key, request).await?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

/// Reset a configuration value to its default
///
/// POST /admin/config/:key/reset
pub async fn reset_config(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Response> {
    if let Err(r) = require_runtime_config(&state) { return Ok(r); }
    let result = state.runtime_config_service.reset(&key).await?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

/// Get configuration audit log
///
/// GET /admin/config/audit
pub async fn get_audit_log(
    State(state): State<AppState>,
    Query(query): Query<AuditLogQuery>,
) -> Result<Response> {
    if let Err(r) = require_runtime_config(&state) { return Ok(r); }
    let result = state
        .runtime_config_service
        .get_audit_log(query.key.as_deref(), query.limit, query.offset)
        .await?;

    Ok((StatusCode::OK, Json(result)).into_response())
}
