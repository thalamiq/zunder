//! Admin statistics handlers.

use crate::services::admin::{AuditEventListQuery, SearchParameterListQuery};
use crate::{state::AppState, Result};
use axum::{
    extract::Query,
    extract::{Path, State},
    http::header,
    http::HeaderMap,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

pub async fn get_resource_type_stats(State(state): State<AppState>) -> Result<Response> {
    let stats = state.admin_service.resource_type_stats().await?;

    Ok((StatusCode::OK, Json(stats)).into_response())
}

pub async fn get_search_parameter_indexing_status(
    State(state): State<AppState>,
) -> Result<Response> {
    let status = state
        .admin_service
        .search_parameter_indexing_status(None)
        .await?;

    Ok((StatusCode::OK, Json(status)).into_response())
}

pub async fn get_search_parameter_indexing_status_by_type(
    State(state): State<AppState>,
    Path(resource_type): Path<String>,
) -> Result<Response> {
    let status = state
        .admin_service
        .search_parameter_indexing_status(Some(&resource_type))
        .await?;

    Ok((StatusCode::OK, Json(status)).into_response())
}

pub async fn get_search_index_table_status(State(state): State<AppState>) -> Result<Response> {
    let status = state.admin_service.search_index_table_status().await?;
    Ok((StatusCode::OK, Json(status)).into_response())
}

pub async fn get_search_hash_collisions(State(state): State<AppState>) -> Result<Response> {
    let status = state.admin_service.search_hash_collisions().await?;
    Ok((StatusCode::OK, Json(status)).into_response())
}

pub async fn list_search_parameters(
    State(state): State<AppState>,
    Query(query): Query<SearchParameterListQuery>,
) -> Result<Response> {
    let result = state.admin_service.list_search_parameters(query).await?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

pub async fn toggle_search_parameter_active(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Response> {
    let active = state
        .admin_service
        .toggle_search_parameter_active(id)
        .await?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "active": active })),
    )
        .into_response())
}

pub async fn list_audit_events(
    State(state): State<AppState>,
    Query(query): Query<AuditEventListQuery>,
) -> Result<Response> {
    let result = state.admin_service.list_audit_events(query).await?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

pub async fn get_audit_event(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let result = state.admin_service.get_audit_event(id).await?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

/// Public UI configuration (excludes password)
#[derive(Debug, Serialize)]
pub struct UiConfigResponse {
    pub enabled: bool,
    pub title: String,
    pub requires_auth: bool,
    pub runtime_config_enabled: bool,
}

/// Get UI configuration (public endpoint)
pub async fn get_ui_config(State(state): State<AppState>) -> Result<Response> {
    let config = &state.config.ui;

    let response = UiConfigResponse {
        enabled: config.enabled,
        title: config.title.clone(),
        requires_auth: config.password.is_some(),
        runtime_config_enabled: config.runtime_config_enabled,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Authentication request
#[derive(Debug, Deserialize)]
pub struct AuthRequest {
    pub password: String,
}

/// Authentication response
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub authenticated: bool,
    pub token: Option<String>,
}

/// Authenticate with admin password
pub async fn authenticate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AuthRequest>,
) -> Result<Response> {
    let config = &state.config.ui;

    // Check if password is configured
    if config.password.is_none() {
        // No password configured = no auth required
        return Ok((
            StatusCode::OK,
            Json(AuthResponse {
                authenticated: true,
                token: None,
            }),
        )
            .into_response());
    };

    // Validate password
    if state.admin_auth.verify_password(&req.password) {
        let is_https = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("https"))
            .unwrap_or(false);

        let mut response = (
            StatusCode::OK,
            Json(AuthResponse {
                authenticated: true,
                token: None,
            }),
        )
            .into_response();

        if let Ok(set_cookie) = state.admin_auth.issue_session_cookie(is_https) {
            response
                .headers_mut()
                .insert(header::SET_COOKIE, set_cookie);
        }
        Ok(response)
    } else {
        Ok((
            StatusCode::UNAUTHORIZED,
            Json(AuthResponse {
                authenticated: false,
                token: None,
            }),
        )
            .into_response())
    }
}

/// Check if the current request has a valid admin session.
pub async fn get_ui_session(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    if !state.admin_auth.requires_auth() {
        return Ok((
            StatusCode::OK,
            Json(AuthResponse {
                authenticated: true,
                token: None,
            }),
        )
            .into_response());
    }

    match state.admin_auth.validate_session(&headers) {
        Ok(()) => Ok((
            StatusCode::OK,
            Json(AuthResponse {
                authenticated: true,
                token: None,
            }),
        )
            .into_response()),
        Err(e) => Ok(e.into_fhir_response()),
    }
}

/// Logout (clears the admin session cookie).
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    let is_https = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false);

    let mut response = (
        StatusCode::OK,
        Json(AuthResponse {
            authenticated: false,
            token: None,
        }),
    )
        .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        state.admin_auth.clear_session_cookie(is_https),
    );
    Ok(response)
}

pub async fn get_compartment_memberships(State(state): State<AppState>) -> Result<Response> {
    let memberships = state.admin_service.compartment_memberships().await?;
    Ok((StatusCode::OK, Json(memberships)).into_response())
}
