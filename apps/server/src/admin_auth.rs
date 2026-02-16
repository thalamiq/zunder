//! Admin UI authentication (no external IdP required).
//!
//! Goals:
//! - Keep "getting started" friction low (optional, single shared password).
//! - Use an industry-standard pattern for admin panels: password login -> HttpOnly session cookie.
//! - Enforce authorization server-side for `/admin/*` endpoints.

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{sync::Arc, time::SystemTime};
use uuid::Uuid;

use crate::{state::AppState, Config};

const ADMIN_SESSION_COOKIE: &str = "tlq_admin_session";

#[derive(Debug, Clone)]
pub enum AdminAuthError {
    MissingSession,
    InvalidSession(String),
}

impl AdminAuthError {
    pub fn into_fhir_response(self) -> Response {
        let (status, diagnostics) = match self {
            Self::MissingSession => (
                StatusCode::UNAUTHORIZED,
                "Missing admin session".to_string(),
            ),
            Self::InvalidSession(msg) => (
                StatusCode::UNAUTHORIZED,
                format!("Invalid admin session: {msg}"),
            ),
        };

        let body = axum::Json(json!({
            "resourceType": "OperationOutcome",
            "issue": [{
                "severity": "error",
                "code": "login",
                "diagnostics": diagnostics
            }]
        }));

        let mut response = (status, body).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/fhir+json; charset=utf-8"),
        );
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        response
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdminSessionClaims {
    sub: String,
    iat: usize,
    exp: usize,
}

#[derive(Clone)]
pub struct AdminAuthManager {
    config: Arc<Config>,
    secret: Vec<u8>,
}

impl AdminAuthManager {
    pub fn new(config: Arc<Config>) -> Self {
        let secret = match &config.ui.session_secret {
            Some(s) if !s.is_empty() => s.as_bytes().to_vec(),
            _ => {
                if config.ui.password.is_some() {
                    tracing::warn!(
                        "UI auth enabled but `ui.session_secret` is not set; using ephemeral secret (sessions reset on restart)"
                    );
                }
                format!("{}{}", Uuid::new_v4(), Uuid::new_v4()).into_bytes()
            }
        };

        Self { config, secret }
    }

    pub fn requires_auth(&self) -> bool {
        self.config.ui.password.is_some()
    }

    pub fn verify_password(&self, provided: &str) -> bool {
        let Some(expected) = &self.config.ui.password else {
            return true;
        };
        constant_time_eq(expected.as_bytes(), provided.as_bytes())
    }

    pub fn cookie_name(&self) -> &'static str {
        ADMIN_SESSION_COOKIE
    }

    pub fn issue_session_cookie(&self, is_https: bool) -> Result<HeaderValue, AdminAuthError> {
        let now = now_epoch_seconds();
        let ttl = self.config.ui.session_ttl_seconds as usize;
        let claims = AdminSessionClaims {
            sub: "admin".to_string(),
            iat: now,
            exp: now.saturating_add(ttl),
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )
        .map_err(|e| AdminAuthError::InvalidSession(e.to_string()))?;

        let cookie = build_set_cookie(self.cookie_name(), &token, ttl, is_https);
        HeaderValue::from_str(&cookie).map_err(|e| AdminAuthError::InvalidSession(e.to_string()))
    }

    pub fn clear_session_cookie(&self, is_https: bool) -> HeaderValue {
        let cookie = build_clear_cookie(self.cookie_name(), is_https);
        HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
    }

    pub fn validate_session(&self, headers: &HeaderMap) -> Result<(), AdminAuthError> {
        // Allow Authorization: Bearer <token> as a fallback for non-browser clients.
        if let Some(authz) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            if let Some(token) = authz
                .strip_prefix("Bearer ")
                .or_else(|| authz.strip_prefix("bearer "))
            {
                return self.validate_jwt(token);
            }
        }

        let token = extract_cookie_value(headers, self.cookie_name())
            .ok_or(AdminAuthError::MissingSession)?;
        self.validate_jwt(&token)
    }

    fn validate_jwt(&self, token: &str) -> Result<(), AdminAuthError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        decode::<AdminSessionClaims>(token, &DecodingKey::from_secret(&self.secret), &validation)
            .map_err(|e| AdminAuthError::InvalidSession(e.to_string()))?;
        Ok(())
    }
}

/// Admin middleware for `/admin/*`.
///
/// Enforced only when `ui.password` is set. Otherwise, admin routes are open
/// (useful for local dev).
pub async fn admin_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.admin_auth.requires_auth() {
        return next.run(req).await;
    }

    let path = req.uri().path();
    // Support both with and without the `/admin` prefix (depending on nesting/layers).
    let is_public = matches!(
        path,
        "/admin/ui/config"
            | "/admin/ui/auth"
            | "/admin/ui/session"
            | "/admin/ui/logout"
            | "/ui/config"
            | "/ui/auth"
            | "/ui/session"
            | "/ui/logout"
    );
    if is_public || req.method() == axum::http::Method::OPTIONS {
        return next.run(req).await;
    }

    match state.admin_auth.validate_session(req.headers()) {
        Ok(()) => next.run(req).await,
        Err(e) => e.into_fhir_response(),
    }
}

fn now_epoch_seconds() -> usize {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn extract_cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        let (k, v) = part.split_once('=')?;
        if k.trim() == name {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn build_set_cookie(name: &str, value: &str, max_age_seconds: usize, is_https: bool) -> String {
    let mut cookie = format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/admin; Max-Age={}",
        name, value, max_age_seconds
    );
    if is_https {
        cookie.push_str("; Secure");
    }
    cookie
}

fn build_clear_cookie(name: &str, is_https: bool) -> String {
    let mut cookie = format!("{name}=; HttpOnly; SameSite=Lax; Path=/admin; Max-Age=0");
    if is_https {
        cookie.push_str("; Secure");
    }
    cookie
}
