#![allow(unused)]
#[allow(unused)]
mod support;

use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use support::*;

fn parse_json(body: &[u8]) -> anyhow::Result<Value> {
    Ok(serde_json::from_slice(body)?)
}

async fn create_operation_definition(app: &TestApp, op: Value) -> anyhow::Result<()> {
    let (status, _headers, _body) = app
        .request(
            Method::POST,
            "/fhir/OperationDefinition",
            Some(to_json_body(&op)?),
        )
        .await?;
    assert_status(status, StatusCode::CREATED, "create OperationDefinition");
    Ok(())
}

#[tokio::test]
async fn batch_bundle_respects_format_parameter() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Create a Patient to read in the batch.
            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let id = created["id"].as_str().unwrap();

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "batch",
                "entry": [{
                    "request": { "method": "GET", "url": format!("Patient/{}", id) }
                }]
            });

            let (status, headers, body) = app
                .request_with_extra_headers(
                    Method::POST,
                    "/fhir?_format=application/fhir+xml",
                    Some(to_json_body(&bundle)?),
                    &[("accept", "application/fhir+xml")],
                )
                .await?;
            assert_status(status, StatusCode::OK, "batch");

            let ct = headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(
                ct.starts_with("application/fhir+xml"),
                "expected fhir+xml content-type, got '{}'",
                ct
            );
            assert!(!body.is_empty());
            assert_eq!(body[0], b'<');

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn operations_respect_format_parameter() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal OperationDefinition required by the operation router/registry.
            create_operation_definition(
                app,
                json!({
                    "resourceType": "OperationDefinition",
                    "status": "active",
                    "kind": "operation",
                    "code": "lookup",
                    "resource": ["CodeSystem"],
                    "system": false,
                    "type": true,
                    "instance": false,
                    "affectsState": false
                }),
            )
            .await?;

            // Create a small CodeSystem.
            let cs = json!({
                "resourceType": "CodeSystem",
                "url": "http://example.org/CodeSystem/test",
                "status": "active",
                "content": "complete",
                "concept": [{ "code": "a", "display": "A" }]
            });
            let (status, _headers, _body) = app
                .request(Method::POST, "/fhir/CodeSystem", Some(to_json_body(&cs)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create CodeSystem");

            // Load operation definitions into the registry.
            app.state.operation_registry.load_definitions().await?;

            let (status, headers, body) = app
                .request_with_extra_headers(
                    Method::GET,
                    "/fhir/CodeSystem/$lookup?system=http://example.org/CodeSystem/test&code=a&_format=application/fhir+xml",
                    None,
                    &[("accept", "application/fhir+xml")],
                )
                .await?;
            assert_status(status, StatusCode::OK, "$lookup");

            let ct = headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(
                ct.starts_with("application/fhir+xml"),
                "expected fhir+xml content-type, got '{}'",
                ct
            );
            assert!(!body.is_empty());
            assert_eq!(body[0], b'<');

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn create_patient_as_json_get_as_xml() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // POST Patient as JSON
            let patient = json!({
                "resourceType": "Patient",
                "name": [{"family": "Smith", "given": ["John"]}]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created: Value = serde_json::from_slice(&body)?;
            let id = created["id"].as_str().unwrap();

            // GET as XML
            let (status, headers, body) = app
                .request_with_extra_headers(
                    Method::GET,
                    &format!("/fhir/Patient/{}?_format=xml", id),
                    None,
                    &[("accept", "application/fhir+xml")],
                )
                .await?;
            assert_status(status, StatusCode::OK, "read Patient as XML");

            let ct = headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(
                ct.starts_with("application/fhir+xml"),
                "expected fhir+xml content-type, got '{}'",
                ct
            );
            let body_str = String::from_utf8_lossy(&body);
            assert!(body_str.contains("<Patient"), "expected <Patient in XML body");
            assert!(body_str.contains("Smith"), "expected Smith in XML body");

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn create_patient_as_xml_get_as_json() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Build XML body.
            // Note: The XML→JSON converter does structural mapping — single elements
            // are NOT auto-wrapped in arrays. FHIR spec requires array knowledge from
            // StructureDefinitions, so `name` appears as a single object, not an array.
            // This test verifies the round-trip works (POST XML → stored → GET JSON).
            let xml_body = r#"<?xml version="1.0" encoding="UTF-8"?>
<Patient xmlns="http://hl7.org/fhir">
  <name>
    <family value="Doe"/>
    <given value="Jane"/>
  </name>
</Patient>"#;

            // POST Patient as XML
            let (status, _headers, body) = app
                .request_with_extra_headers(
                    Method::POST,
                    "/fhir/Patient",
                    Some(Bytes::from(xml_body)),
                    &[
                        ("content-type", "application/fhir+xml"),
                        ("accept", "application/fhir+json"),
                    ],
                )
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient from XML");
            let created: Value = serde_json::from_slice(&body)?;
            assert_eq!(created["resourceType"], "Patient");
            // Structural conversion: single `<name>` becomes an object, not an array
            let name = &created["name"];
            let family = if name.is_array() {
                name[0]["family"].as_str()
            } else {
                name["family"].as_str()
            };
            assert_eq!(family, Some("Doe"), "expected family=Doe");
            let id = created["id"].as_str().unwrap();

            // GET as JSON (default) — verify resource was persisted
            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/Patient/{}", id), None)
                .await?;
            assert_status(status, StatusCode::OK, "read Patient as JSON");
            let read: Value = serde_json::from_slice(&body)?;
            assert_eq!(read["resourceType"], "Patient");
            let read_name = &read["name"];
            let read_family = if read_name.is_array() {
                read_name[0]["family"].as_str()
            } else {
                read_name["family"].as_str()
            };
            assert_eq!(read_family, Some("Doe"), "persisted family=Doe");

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn search_result_as_xml() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Create a Patient
            let patient = json!({
                "resourceType": "Patient",
                "name": [{"family": "XmlTest"}]
            });
            let (status, _headers, _body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");

            // Search and get XML format
            let (status, headers, body) = app
                .request_with_extra_headers(
                    Method::GET,
                    "/fhir/Patient?_format=xml",
                    None,
                    &[("accept", "application/fhir+xml")],
                )
                .await?;
            assert_status(status, StatusCode::OK, "search as XML");

            let ct = headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(
                ct.starts_with("application/fhir+xml"),
                "expected fhir+xml content-type, got '{}'",
                ct
            );
            let body_str = String::from_utf8_lossy(&body);
            assert!(body_str.contains("<Bundle"), "expected <Bundle in XML body");

            Ok(())
        })
    })
    .await
}
