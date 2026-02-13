//! Audit middleware for FHIR REST interactions

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, Uri, Version},
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

use crate::auth::Principal;
use crate::request_context::RequestContext;
use crate::state::AppState;

fn extract_client_ip(headers: &HeaderMap) -> Option<String> {
    // Prefer common proxy headers.
    // - X-Forwarded-For: comma-separated list (client, proxy1, proxy2)
    // - X-Real-IP: single IP
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(s) = xff.to_str() {
            if let Some(first) = s.split(',').next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

#[derive(Debug, Clone)]
struct FhirInteraction {
    // Restful interaction code (http://hl7.org/fhir/restful-interaction)
    interaction: String,
    // FHIR audit action (C/R/U/D/E)
    action: String,
    resource_type: Option<String>,
    resource_id: Option<String>,
    compartment_patient_id: Option<String>,
    is_search: bool,
}

fn strip_fhir_prefix(path: &str) -> &str {
    path.strip_prefix("/fhir").unwrap_or(path)
}

fn parse_fhir_interaction(method: &Method, uri: &Uri) -> Option<FhirInteraction> {
    let path = strip_fhir_prefix(uri.path());
    let query_present = uri.query().map(|q| !q.is_empty()).unwrap_or(false);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Skip SMART config (non-FHIR interaction).
    if segments
        .first()
        .is_some_and(|s| *s == ".well-known" || *s == ".well-known%2Fsmart-configuration")
        || path.contains(".well-known/smart-configuration")
    {
        return None;
    }

    let interaction = match (method.as_str(), segments.as_slice()) {
        ("GET", ["metadata", ..]) => "capabilities".to_string(),
        ("GET", ["_history", ..]) => "history".to_string(),
        ("DELETE", ["_history", ..]) => "delete".to_string(),
        (_, [seg]) if seg.starts_with('$') => {
            if *seg == "$export" {
                "export".to_string()
            } else {
                "operation".to_string()
            }
        }
        (_, [_, seg]) if seg.starts_with('$') => {
            if *seg == "$export" {
                "export".to_string()
            } else {
                "operation".to_string()
            }
        }
        (_, [_, _, seg]) if seg.starts_with('$') => {
            if *seg == "$export" {
                "export".to_string()
            } else {
                "operation".to_string()
            }
        }
        ("POST", ["_search", ..]) => "search".to_string(),
        ("GET", []) => "search".to_string(),
        ("GET", ["_search", ..]) => "search".to_string(),
        ("POST", []) => "batch".to_string(), // system-level batch/transaction
        ("DELETE", []) => "delete".to_string(), // system-level delete
        ("GET", [_resource_type]) => {
            if query_present || path.contains("_search") {
                "search".to_string()
            } else {
                // Type-level GET without query is still a search in FHIR.
                "search".to_string()
            }
        }
        ("POST", [_resource_type]) => "create".to_string(),
        ("PUT", [_resource_type]) => "update".to_string(), // conditional update
        ("PATCH", [_resource_type]) => "patch".to_string(), // conditional patch
        ("DELETE", [_resource_type]) => "delete".to_string(), // conditional delete
        // Type-level history: /{resourceType}/_history
        ("GET", [_resource_type, "_history"]) => "history".to_string(),
        ("GET", [_resource_type, _id]) => "read".to_string(),
        ("HEAD", [_resource_type, _id]) => "read".to_string(),
        ("PUT", [_resource_type, _id]) => "update".to_string(),
        ("PATCH", [_resource_type, _id]) => "patch".to_string(),
        ("DELETE", [_resource_type, _id]) => "delete".to_string(),
        ("GET", [_resource_type, _id, "_history"]) => "history".to_string(),
        ("DELETE", [_resource_type, _id, "_history"]) => "delete".to_string(), // delete history
        ("GET", [_resource_type, _id, "_history", _vid]) => "vread".to_string(),
        ("HEAD", [_resource_type, _id, "_history", _vid]) => "vread".to_string(),
        ("DELETE", [_resource_type, _id, "_history", _vid]) => "delete".to_string(),
        // Compartment search variants.
        ("GET", [_compartment_type, _compartment_id, "*"]) => "search".to_string(),
        ("GET", [_compartment_type, _compartment_id, _resource_type]) => "search".to_string(),
        ("POST", [_compartment_type, _compartment_id, "_search"]) => "search".to_string(),
        ("POST", [_compartment_type, _compartment_id, _resource_type, "_search"]) => {
            "search".to_string()
        }
        _ => "operation".to_string(),
    };

    let action = match interaction.as_str() {
        "create" => "C",
        "read" | "vread" | "history" => "R",
        "update" | "patch" => "U",
        "delete" => "D",
        "search" | "batch" | "transaction" | "operation" | "capabilities" => "E",
        _ => "E",
    }
    .to_string();

    // Resource and compartment detection (best-effort).
    let (resource_type, resource_id, compartment_patient_id) = match segments.as_slice() {
        // /{resourceType}/{id}
        [rt, id] => (Some((*rt).to_string()), Some((*id).to_string()), None),
        // /{resourceType}/{id}/_history...
        [rt, id, ..] if *rt != "_history" && *rt != "_search" => {
            (Some((*rt).to_string()), Some((*id).to_string()), None)
        }
        // /{resourceType}
        [rt] if !rt.starts_with('_') && !rt.starts_with('$') && *rt != "metadata" => {
            (Some((*rt).to_string()), None, None)
        }
        // Compartment: /Patient/{id}/...
        [compartment_type, compartment_id, ..] if *compartment_type == "Patient" => {
            (None, None, Some((*compartment_id).to_string()))
        }
        _ => (None, None, None),
    };

    let is_search = interaction == "search";

    Some(FhirInteraction {
        interaction,
        action,
        resource_type,
        resource_id,
        compartment_patient_id,
        is_search,
    })
}

fn http_version_string(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2.0",
        Version::HTTP_3 => "HTTP/3.0",
        _ => "HTTP/1.1",
    }
}

fn build_raw_http_message(
    method: &Method,
    uri: &Uri,
    version: Version,
    headers: &HeaderMap,
    body: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();

    let path_and_query = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(uri.path());

    out.extend_from_slice(method.as_str().as_bytes());
    out.extend_from_slice(b" ");
    out.extend_from_slice(path_and_query.as_bytes());
    out.extend_from_slice(b" ");
    out.extend_from_slice(http_version_string(version).as_bytes());
    out.extend_from_slice(b"\r\n");

    for (name, value) in headers.iter() {
        out.extend_from_slice(name.as_str().as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

fn parse_location_target(location: &str) -> Option<(String, String)> {
    // Location is typically "{resourceType}/{id}/_history/{vid}" (relative).
    let trimmed = location.trim().trim_start_matches('/');
    let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() >= 2 {
        return Some((segments[0].to_string(), segments[1].to_string()));
    }
    None
}

fn extract_patient_ids_from_bundle(bundle: &serde_json::Value) -> Vec<String> {
    let Some(entries) = bundle.get("entry").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut ids = std::collections::BTreeSet::new();

    for entry in entries {
        let Some(resource) = entry.get("resource") else {
            continue;
        };

        // Direct Patient resources.
        if resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .is_some_and(|rt| rt == "Patient")
        {
            if let Some(id) = resource.get("id").and_then(|v| v.as_str()) {
                ids.insert(id.to_string());
            }
            continue;
        }

        // Common patient references: subject / patient.
        for key in ["subject", "patient"] {
            if let Some(reference) = resource
                .get(key)
                .and_then(|v| v.get("reference"))
                .and_then(|v| v.as_str())
            {
                if let Some(rest) = reference.strip_prefix("Patient/") {
                    let id = rest.split('/').next().unwrap_or(rest);
                    if !id.is_empty() {
                        ids.insert(id.to_string());
                    }
                }
            }
        }
    }

    ids.into_iter().collect()
}

/// Audit middleware for FHIR REST interactions.
///
/// Emits FHIR `AuditEvent` records into the internal `audit_log` when audit logging is enabled.
///
/// Search interactions additionally capture the full raw HTTP request (headers + body) as base64,
/// per the AuditEvent `entity.query` guidance.
pub async fn audit_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.audit_service.enabled().await {
        return next.run(req).await;
    }

    let Some(mut interaction) = parse_fhir_interaction(req.method(), req.uri()) else {
        return next.run(req).await;
    };

    let should_peek_bundle_type = interaction.interaction == "batch"
        && req.method() == Method::POST
        && req.uri().path() == "/"
        && (state.audit_service.should_audit_interaction("batch").await
            || state
                .audit_service
                .should_audit_interaction("transaction")
                .await);

    // If we don't need to peek the request body to disambiguate batch/transaction and this
    // interaction is disabled, short-circuit early.
    if !should_peek_bundle_type
        && !state
            .audit_service
            .should_audit_interaction(&interaction.interaction)
            .await
    {
        return next.run(req).await;
    }

    let request_id = req
        .extensions()
        .get::<RequestContext>()
        .map(|c| c.request_id.clone());
    let principal = req.extensions().get::<Principal>().cloned();
    let client_ip = extract_client_ip(req.headers());
    let user_agent = extract_user_agent(req.headers());
    let request_method = req.method().as_str().to_string();

    // Capture full raw request for search events (base64), including headers and body.
    let should_capture_query = interaction.is_search
        && state.audit_service.should_audit_interaction("search").await
        && state.audit_service.capture_search_query().await;
    // For system-level POST, peek Bundle.type to distinguish batch vs transaction.
    let (parts, body) = req.into_parts();
    let (req_for_next, query_b64, request_harmonized_url) =
        if should_capture_query || should_peek_bundle_type {
            match axum::body::to_bytes(body, state.config.server.max_request_body_size).await {
                Ok(body_bytes) => {
                    if should_peek_bundle_type
                        && parts
                            .headers
                            .get("content-type")
                            .and_then(|v| v.to_str().ok())
                            .is_some_and(|ct| ct.contains("json"))
                    {
                        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                            if v.get("resourceType")
                                .and_then(|rt| rt.as_str())
                                .is_some_and(|rt| rt == "Bundle")
                                && v.get("type")
                                    .and_then(|t| t.as_str())
                                    .is_some_and(|t| t == "transaction")
                            {
                                interaction.interaction = "transaction".to_string();
                            }
                        }
                    }
                    if should_capture_query {
                        let raw = build_raw_http_message(
                            &parts.method,
                            &parts.uri,
                            parts.version,
                            &parts.headers,
                            &body_bytes,
                        );
                        let b64 = BASE64_STANDARD.encode(raw);
                        let harmonized = parts
                            .uri
                            .path_and_query()
                            .map(|pq| pq.as_str().to_string())
                            .unwrap_or_else(|| parts.uri.path().to_string());
                        (
                            Request::from_parts(parts, Body::from(body_bytes)),
                            Some(b64),
                            Some(harmonized),
                        )
                    } else {
                        (
                            Request::from_parts(parts, Body::from(body_bytes)),
                            None,
                            None,
                        )
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to read request body for audit search capture: {}",
                        e
                    );
                    (Request::from_parts(parts, Body::empty()), None, None)
                }
            }
        } else {
            (Request::from_parts(parts, body), None, None::<String>)
        };

    let mut response = next.run(req_for_next).await;

    let status = response.status();
    let status_u16 = status.as_u16();

    // Re-evaluate gating with the finalized interaction and response status.
    if !state
        .audit_service
        .should_audit_interaction(&interaction.interaction)
        .await
        || !state.audit_service.should_audit_status(status_u16).await
    {
        return response;
    }

    // Best-effort parse for:
    // - OperationOutcome on failures (SHOULD)
    // - Bundle patient extraction on search success (SHOULD)
    let should_parse_response_json = (status_u16 >= 400
        && state.audit_service.capture_operation_outcome().await)
        || (interaction.is_search
            && status == StatusCode::OK
            && state.audit_service.per_patient_events_for_search().await);
    let mut response_json: Option<serde_json::Value> = None;
    if should_parse_response_json {
        let (parts, body) = response.into_parts();
        match axum::body::to_bytes(body, state.config.server.max_response_body_size).await {
            Ok(bytes) => {
                if parts
                    .headers
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|ct| ct.contains("json"))
                {
                    response_json = serde_json::from_slice(&bytes).ok();
                }
                response = Response::from_parts(parts, Body::from(bytes));
            }
            Err(e) => {
                tracing::warn!("Failed to read response body for audit capture: {}", e);
                response = Response::from_parts(parts, Body::empty());
            }
        }
    }

    // Resolve target resource (best-effort) from path or Location header.
    let location_target = response
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_location_target);

    let target = interaction
        .resource_type
        .clone()
        .zip(interaction.resource_id.clone())
        .or(location_target);

    // Resolve patients (best-effort).
    let mut patient_ids: Vec<String> = Vec::new();
    if let Some(pid) = principal.as_ref().and_then(|p| p.patient.clone()) {
        patient_ids.push(pid);
    } else if let Some(pid) = interaction.compartment_patient_id.clone() {
        patient_ids.push(pid);
    } else if let Some((rt, id)) = target.as_ref() {
        if rt == "Patient" {
            patient_ids.push(id.clone());
        }
    }

    if patient_ids.is_empty()
        && interaction.is_search
        && status == StatusCode::OK
        && state.audit_service.per_patient_events_for_search().await
    {
        if let Some(json) = response_json.as_ref() {
            if json
                .get("resourceType")
                .and_then(|v| v.as_str())
                .is_some_and(|rt| rt == "Bundle")
            {
                patient_ids = extract_patient_ids_from_bundle(json);
            }
        }
    }

    // OperationOutcome for failures.
    let operation_outcome = state
        .audit_service
        .capture_operation_outcome()
        .await
        .then_some(())
        .and(response_json.as_ref())
        .and_then(|json| {
            json.get("resourceType")
                .and_then(|v| v.as_str())
                .is_some_and(|rt| rt == "OperationOutcome")
                .then(|| json.clone())
        });

    // Create 1..n AuditEvent records (one per patient when known, per best practice).
    let patients = if state.audit_service.per_patient_events_for_search().await
        && interaction.is_search
        && !patient_ids.is_empty()
    {
        patient_ids.into_iter().map(Some).collect()
    } else {
        vec![None]
    };

    for patient_id in patients {
        let input = crate::services::audit::HttpAuditInput {
            method: request_method.clone(),
            interaction: interaction.interaction.clone(),
            action: interaction.action.clone(),
            status: status_u16,
            request_id: request_id.clone(),
            principal: principal.clone(),
            client_ip: client_ip.clone(),
            user_agent: user_agent.clone(),
            target: target.clone(),
            patient_id,
            query_base64: query_b64.clone(),
            query_harmonized: request_harmonized_url.clone(),
            operation_outcome: operation_outcome.clone(),
        };
        state.audit_service.enqueue_http(input).await;
    }

    response
}
