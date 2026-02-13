#![allow(unused)]
#[allow(unused)]
mod support;

use axum::http::{Method, StatusCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;
use support::{
    assert_resource_id, assert_status, assert_version_id, constants, patient_with_mrn,
    register_search_parameter, to_json_body, with_test_app,
};

fn status_code_prefix(status: &str) -> &str {
    status.split_whitespace().next().unwrap_or("")
}

#[tokio::test]
async fn batch_conditional_create_one_match_returns_existing() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            register_search_parameter(
                &app.state.db_pool,
                "identifier",
                "Patient",
                "token",
                "Patient.identifier",
                &[],
            )
            .await?;

            let patient = patient_with_mrn("Doe", "123");
            let (_status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            let created: serde_json::Value = serde_json::from_slice(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "batch",
                "entry": [{
                    "request": {
                        "method": "POST",
                        "url": "Patient",
                        "ifNoneExist": "identifier=http://example.org/fhir/mrn|123"
                    },
                    "resource": patient_with_mrn("Other", "123")
                }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_status(status, StatusCode::OK, "batch");

            let response: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(response["type"], "batch-response");
            let entry = &response["entry"][0];
            assert_eq!(
                status_code_prefix(entry["response"]["status"].as_str().unwrap()),
                "200"
            );

            let matched = entry["resource"].clone();
            assert_resource_id(&matched, &id)?;
            assert_version_id(&matched, "1")?;

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn batch_conditional_update_put_query_updates() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            register_search_parameter(
                &app.state.db_pool,
                "identifier",
                "Patient",
                "token",
                "Patient.identifier",
                &[],
            )
            .await?;

            let patient = patient_with_mrn("Doe", "123");
            let (_status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            let created: serde_json::Value = serde_json::from_slice(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "batch",
                "entry": [{
                    "request": {
                        "method": "PUT",
                        "url": "Patient?identifier=http://example.org/fhir/mrn|123"
                    },
                    "resource": {
                        "resourceType": "Patient",
                        "active": false
                    }
                }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_status(status, StatusCode::OK, "batch");

            let response: serde_json::Value = serde_json::from_slice(&body)?;
            let entry = &response["entry"][0];
            assert_eq!(
                status_code_prefix(entry["response"]["status"].as_str().unwrap()),
                "200"
            );

            let updated = entry["resource"].clone();
            assert_resource_id(&updated, &id)?;
            assert_version_id(&updated, "2")?;
            assert_eq!(updated["active"], false);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn batch_conditional_delete_delete_query_deletes() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            register_search_parameter(
                &app.state.db_pool,
                "identifier",
                "Patient",
                "token",
                "Patient.identifier",
                &[],
            )
            .await?;

            let patient = patient_with_mrn("Doe", "123");
            let (_status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            let created: serde_json::Value = serde_json::from_slice(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "batch",
                "entry": [{
                    "request": {
                        "method": "DELETE",
                        "url": "Patient?identifier=http://example.org/fhir/mrn|123"
                    }
                }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_status(status, StatusCode::OK, "batch");

            let response: serde_json::Value = serde_json::from_slice(&body)?;
            let entry = &response["entry"][0];
            assert_eq!(
                status_code_prefix(entry["response"]["status"].as_str().unwrap()),
                "204"
            );

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn transaction_conditional_create_rewrites_references() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            register_search_parameter(
                &app.state.db_pool,
                "identifier",
                "Patient",
                "token",
                "Patient.identifier",
                &[],
            )
            .await?;

            let patient = patient_with_mrn("Doe", "123");
            let (_status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            let created: serde_json::Value = serde_json::from_slice(&body)?;
            let existing_id = created["id"].as_str().unwrap().to_string();

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "transaction",
                "entry": [
                    {
                        "fullUrl": "urn:uuid:pt1",
                        "request": {
                            "method": "POST",
                            "url": "Patient",
                            "ifNoneExist": "identifier=http://example.org/fhir/mrn|123"
                        },
                        "resource": patient_with_mrn("Other", "123")
                    },
                    {
                        "request": {
                            "method": "POST",
                            "url": "Observation"
                        },
                        "resource": {
                            "resourceType": "Observation",
                            "status": "final",
                            "code": { "text": "test" },
                            "subject": { "reference": "urn:uuid:pt1" }
                        }
                    }
                ]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_status(status, StatusCode::OK, "transaction");

            let response: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(response["type"], "transaction-response");

            let patient_entry = &response["entry"][0];
            assert_eq!(
                status_code_prefix(patient_entry["response"]["status"].as_str().unwrap()),
                "200"
            );
            let matched_patient = patient_entry["resource"].clone();
            assert_resource_id(&matched_patient, &existing_id)?;

            let obs_entry = &response["entry"][1];
            assert_eq!(
                status_code_prefix(obs_entry["response"]["status"].as_str().unwrap()),
                "201"
            );
            let observation = obs_entry["resource"].clone();
            assert_eq!(
                observation["subject"]["reference"].as_str().unwrap(),
                format!("Patient/{}", existing_id)
            );

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn batch_conditional_patch_updates() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            register_search_parameter(
                &app.state.db_pool,
                "identifier",
                "Patient",
                "token",
                "Patient.identifier",
                &[],
            )
            .await?;

            let patient = patient_with_mrn("Doe", "123");
            let (_status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            let created: serde_json::Value = serde_json::from_slice(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            let patch_doc = json!([
                { "op": "add", "path": "/active", "value": false }
            ]);
            let patch_bytes = serde_json::to_vec(&patch_doc)?;
            let patch_b64 = STANDARD.encode(patch_bytes);

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "batch",
                "entry": [{
                    "request": {
                        "method": "PATCH",
                        "url": "Patient?identifier=http://example.org/fhir/mrn|123"
                    },
                    "resource": {
                        "resourceType": "Binary",
                        "contentType": "application/json-patch+json",
                        "data": patch_b64
                    }
                }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_status(status, StatusCode::OK, "batch");

            let response: serde_json::Value = serde_json::from_slice(&body)?;
            let entry = &response["entry"][0];
            assert_eq!(
                status_code_prefix(entry["response"]["status"].as_str().unwrap()),
                "200"
            );

            let updated = entry["resource"].clone();
            assert_resource_id(&updated, &id)?;
            assert_version_id(&updated, "2")?;
            assert_eq!(updated["active"], false);

            Ok(())
        })
    })
    .await
}
