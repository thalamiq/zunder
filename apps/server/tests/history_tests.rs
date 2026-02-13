#![allow(unused)]
#[allow(unused)]
mod support;

use axum::http::{Method, StatusCode};
use chrono::Utc;
use serde_json::Value;
use support::*;

fn parse_json(body: &[u8]) -> anyhow::Result<Value> {
    Ok(serde_json::from_slice(body)?)
}

fn entries(bundle: &Value) -> Vec<Value> {
    bundle
        .get("entry")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

#[tokio::test]
async fn history_instance_orders_newest_first_and_includes_deletes() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Create
            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            // Update
            let mut updated_body = minimal_patient();
            updated_body["id"] = Value::String(id.clone());
            let (status, _headers, body) = app
                .request(
                    Method::PUT,
                    &format!("/fhir/Patient/{}", id),
                    Some(to_json_body(&updated_body)?),
                )
                .await?;
            assert_status(status, StatusCode::OK, "update Patient");
            let updated = parse_json(&body)?;
            let update_last_updated = updated["meta"]["lastUpdated"].as_str().unwrap().to_string();

            // Delete
            let (status, _headers, _body) = app
                .request(Method::DELETE, &format!("/fhir/Patient/{}", id), None)
                .await?;
            assert_status(status, StatusCode::NO_CONTENT, "delete Patient");

            // Instance history
            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/Patient/{}/_history", id), None)
                .await?;
            assert_status(status, StatusCode::OK, "instance history");
            let bundle = parse_json(&body)?;
            assert_eq!(bundle["resourceType"], "Bundle");
            assert_eq!(bundle["type"], "history");

            let es = entries(&bundle);
            assert_eq!(
                es.len(),
                3,
                "expected 3 history entries (POST, PUT, DELETE)"
            );

            assert_eq!(es[0]["request"]["method"], "DELETE");
            assert_eq!(es[1]["request"]["method"], "PUT");
            assert_eq!(es[2]["request"]["method"], "POST");

            // PUT/POST entries must contain a resource.
            assert!(
                es[1].get("resource").is_some(),
                "PUT entry must have resource"
            );
            assert!(
                es[2].get("resource").is_some(),
                "POST entry must have resource"
            );
            // DELETE may omit resource.
            assert!(
                es[0].get("resource").is_none(),
                "DELETE entry may omit resource"
            );

            // _since is inclusive (at or after given instant)
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/Patient/{}/_history?_since={}",
                        id,
                        urlencoding::encode(&update_last_updated)
                    ),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "instance history _since");
            let bundle = parse_json(&body)?;
            let es = entries(&bundle);
            assert_eq!(es.len(), 2, "expected PUT + DELETE since update");
            assert_eq!(es[0]["request"]["method"], "DELETE");
            assert_eq!(es[1]["request"]["method"], "PUT");

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn history_instance_sort_lastupdated_ascending() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            let mut updated_body = minimal_patient();
            updated_body["id"] = Value::String(id.clone());
            let (status, _headers, _body) = app
                .request(
                    Method::PUT,
                    &format!("/fhir/Patient/{}", id),
                    Some(to_json_body(&updated_body)?),
                )
                .await?;
            assert_status(status, StatusCode::OK, "update Patient");

            let (status, _headers, _body) = app
                .request(Method::DELETE, &format!("/fhir/Patient/{}", id), None)
                .await?;
            assert_status(status, StatusCode::NO_CONTENT, "delete Patient");

            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!("/fhir/Patient/{}/_history?_sort=_lastUpdated", id),
                    None,
                )
                .await?;
            assert_status(
                status,
                StatusCode::OK,
                "instance history _sort=_lastUpdated",
            );
            let bundle = parse_json(&body)?;
            let es = entries(&bundle);
            assert_eq!(es.len(), 3);
            // Oldest first: POST -> PUT -> DELETE
            assert_eq!(es[0]["request"]["method"], "POST");
            assert_eq!(es[1]["request"]["method"], "PUT");
            assert_eq!(es[2]["request"]["method"], "DELETE");

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn history_rejects_duplicate_parameters() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            let since = created["meta"]["lastUpdated"].as_str().unwrap();
            let path = format!(
                "/fhir/Patient/{}/_history?_since={}&_since={}",
                id,
                urlencoding::encode(since),
                urlencoding::encode(since)
            );
            let (status, _headers, _body) = app.request(Method::GET, &path, None).await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn history_type_and_system_endpoints_work() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Patient
            let p1 = example_patient("Doe", "Jane");
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&p1)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let p1_created = parse_json(&body)?;
            let p1_id = p1_created["id"].as_str().unwrap().to_string();

            // Observation referencing Patient
            let obs = minimal_observation(&p1_id);
            let (status, _headers, _body) = app
                .request(Method::POST, "/fhir/Observation", Some(to_json_body(&obs)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Observation");

            // Type history: Patient
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Patient/_history?_count=50", None)
                .await?;
            assert_status(status, StatusCode::OK, "type history");
            let bundle = parse_json(&body)?;
            assert_eq!(bundle["type"], "history");
            let es = entries(&bundle);
            assert!(
                es.iter()
                    .any(|e| e["request"]["url"] == format!("Patient/{}", p1_id)),
                "expected Patient in type history"
            );

            // System history: includes Patient + Observation
            let (status, _headers, body) = app.request(Method::GET, "/fhir/_history", None).await?;
            assert_status(status, StatusCode::OK, "system history");
            let bundle = parse_json(&body)?;
            let es = entries(&bundle);
            assert!(
                es.iter().any(|e| e["request"]["url"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("Patient/")),
                "expected Patient in system history"
            );
            assert!(
                es.iter().any(|e| e["request"]["url"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("Observation/")),
                "expected Observation in system history"
            );

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn history_supports_xml_response_format() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            let (status, headers, body) = app
                .request_with_extra_headers(
                    Method::GET,
                    &format!("/fhir/Patient/{}/_history?_format=application/fhir+xml", id),
                    None,
                    &[("accept", "application/fhir+xml")],
                )
                .await?;
            assert_status(status, StatusCode::OK, "history xml");

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
async fn history_at_returns_version_current_at_instant() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Create v1
            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient v1");
            let v1 = parse_json(&body)?;
            let id = v1["id"].as_str().unwrap().to_string();
            let v1_updated = v1["meta"]["lastUpdated"].as_str().unwrap().to_string();

            // Record a timestamp after v1 but before v2
            let between_v1_v2 = Utc::now().to_rfc3339();

            // Update to v2
            let mut v2_body = minimal_patient();
            v2_body["id"] = Value::String(id.clone());
            let (status, _headers, body) = app
                .request(
                    Method::PUT,
                    &format!("/fhir/Patient/{}", id),
                    Some(to_json_body(&v2_body)?),
                )
                .await?;
            assert_status(status, StatusCode::OK, "update Patient v2");
            let v2 = parse_json(&body)?;
            let _v2_updated = v2["meta"]["lastUpdated"].as_str().unwrap().to_string();

            // Record a timestamp after v2 but before v3
            let between_v2_v3 = Utc::now().to_rfc3339();

            // Update to v3
            let mut v3_body = minimal_patient();
            v3_body["id"] = Value::String(id.clone());
            let (status, _headers, body) = app
                .request(
                    Method::PUT,
                    &format!("/fhir/Patient/{}", id),
                    Some(to_json_body(&v3_body)?),
                )
                .await?;
            assert_status(status, StatusCode::OK, "update Patient v3");
            let _v3 = parse_json(&body)?;

            // _at for between_v1_v2 — should return v1
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/Patient/{}/_history?_at={}",
                        id,
                        urlencoding::encode(&between_v1_v2)
                    ),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "history _at between v1 and v2");
            let bundle = parse_json(&body)?;
            let es = entries(&bundle);
            assert_eq!(es.len(), 1, "expected exactly 1 entry for _at");
            assert_eq!(
                es[0]["resource"]["meta"]["versionId"], "1",
                "expected v1 at the _at instant"
            );

            // _at for between_v2_v3 — should return v2
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/Patient/{}/_history?_at={}",
                        id,
                        urlencoding::encode(&between_v2_v3)
                    ),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "history _at between v2 and v3");
            let bundle = parse_json(&body)?;
            let es = entries(&bundle);
            assert_eq!(es.len(), 1, "expected exactly 1 entry for _at");
            assert_eq!(
                es[0]["resource"]["meta"]["versionId"], "2",
                "expected v2 at the _at instant"
            );

            // _at for a time before v1 — should return empty bundle
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/Patient/{}/_history?_at={}",
                        id,
                        urlencoding::encode(&v1_updated.replace("2026", "2020"))
                    ),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "history _at before creation");
            let bundle = parse_json(&body)?;
            let es = entries(&bundle);
            assert_eq!(es.len(), 0, "expected empty bundle for _at before creation");

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn history_at_rejects_combined_with_since() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let id = created["id"].as_str().unwrap().to_string();
            let ts = created["meta"]["lastUpdated"].as_str().unwrap();

            let path = format!(
                "/fhir/Patient/{}/_history?_since={}&_at={}",
                id,
                urlencoding::encode(ts),
                urlencoding::encode(ts)
            );
            let (status, _headers, _body) = app.request(Method::GET, &path, None).await?;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "_since and _at together should be rejected"
            );

            Ok(())
        })
    })
    .await
}
