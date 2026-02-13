#![allow(unused)]
#[allow(unused)]
mod support;

use anyhow::Context as _;
use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use serde_json::json;
use support::*;

#[tokio::test]
async fn compartment_search_variant_endpoints_are_routed() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let patient = json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "Smith", "given": ["Eve"] }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created: serde_json::Value = serde_json::from_slice(&body)?;
            let id = created
                .get("id")
                .and_then(|v| v.as_str())
                .context("created Patient has id")?
                .to_string();

            // Minimal membership: Patient is in the Patient compartment via the "{def}" rule.
            sqlx::query(
                "INSERT INTO compartment_memberships (compartment_type, resource_type, parameter_names) VALUES ($1, $2, ARRAY['{def}'])",
            )
            .bind("Patient")
            .bind("Patient")
            .execute(&app.state.db_pool)
            .await?;

            // GET [base]/[Compartment]/[id]/*
            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/Patient/{id}/*"), None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(bundle["resourceType"], "Bundle");
            assert_eq!(bundle["type"], "searchset");
            let self_url = bundle["link"][0]["url"]
                .as_str()
                .context("Bundle.link[0].url is string")?;
            assert!(
                self_url.ends_with(&format!("/fhir/Patient/{id}/*")),
                "unexpected self link: {self_url}"
            );
            let entries = bundle["entry"]
                .as_array()
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            // POST [base]/[Compartment]/[id]/_search
            let (status, _headers, body) = app
                .request_with_extra_headers(
                    Method::POST,
                    &format!("/fhir/Patient/{id}/_search"),
                    Some(Bytes::from_static(b"_count=1")),
                    &[("content-type", "application/x-www-form-urlencoded")],
                )
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let self_url = bundle["link"][0]["url"]
                .as_str()
                .context("Bundle.link[0].url is string")?;
            assert!(
                self_url.contains(&format!("/fhir/Patient/{id}/*")),
                "unexpected self link: {self_url}"
            );
            assert!(
                self_url.contains("_count=1"),
                "expected query string in self link: {self_url}"
            );

            // GET [base]/[Compartment]/[id]/[type]
            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/Patient/{id}/Patient"), None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let self_url = bundle["link"][0]["url"]
                .as_str()
                .context("Bundle.link[0].url is string")?;
            assert!(
                self_url.ends_with(&format!("/fhir/Patient/{id}/Patient")),
                "unexpected self link: {self_url}"
            );

            // POST [base]/[Compartment]/[id]/[type]/_search
            let (status, _headers, body) = app
                .request_with_extra_headers(
                    Method::POST,
                    &format!("/fhir/Patient/{id}/Patient/_search"),
                    Some(Bytes::from_static(b"_summary=count")),
                    &[("content-type", "application/x-www-form-urlencoded")],
                )
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let self_url = bundle["link"][0]["url"]
                .as_str()
                .context("Bundle.link[0].url is string")?;
            assert!(
                self_url.contains(&format!("/fhir/Patient/{id}/Patient")),
                "unexpected self link: {self_url}"
            );
            assert!(
                self_url.contains("_summary=count"),
                "expected query string in self link: {self_url}"
            );

            // Ensure plain instance read still works and isn't confused with compartment search.
            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/Patient/{id}"), None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let read: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(read["resourceType"], "Patient");
            assert_eq!(read["id"], id);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn compartment_search_without_memberships_is_empty() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let patient_a = json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "Alpha" }]
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Patient",
                    Some(to_json_body(&patient_a)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created: serde_json::Value = serde_json::from_slice(&body)?;
            let id_a = created["id"]
                .as_str()
                .context("created Patient A has id")?
                .to_string();

            let patient_b = json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "Beta" }]
            });
            let (status, _headers, _body) = app
                .request(
                    Method::POST,
                    "/fhir/Patient",
                    Some(to_json_body(&patient_b)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);

            // With no rows in compartment_memberships, compartment search should be empty rather
            // than behaving like an unscoped system/type search.
            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/Patient/{id_a}/*"), None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(bundle["resourceType"], "Bundle");
            assert_eq!(bundle["type"], "searchset");

            let entries_len = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            assert_eq!(entries_len, 0, "expected empty Bundle, got: {bundle}");

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn compartment_search_is_blocked_when_disabled() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.fhir.interactions.compartment.search = false;
        },
        |app| {
            Box::pin(async move {
                let patient = json!({
                    "resourceType": "Patient",
                    "active": true,
                    "name": [{ "family": "Smith", "given": ["Eve"] }]
                });

                let (status, _headers, body) = app
                    .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                    .await?;
                assert_eq!(status, StatusCode::CREATED);
                let created: serde_json::Value = serde_json::from_slice(&body)?;
                let id = created
                    .get("id")
                    .and_then(|v| v.as_str())
                    .context("created Patient has id")?
                    .to_string();

                // GET [base]/[Compartment]/[id]/* should now be blocked.
                let (status, _headers, _body) = app
                    .request(Method::GET, &format!("/fhir/Patient/{id}/*"), None)
                    .await?;
                assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

                Ok(())
            })
        },
    )
    .await
}
