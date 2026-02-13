#![allow(unused)]
#[allow(unused)]
mod support;

use axum::http::{Method, StatusCode};
use serde_json::json;
use support::{
    assert_status, patient_with_mrn, register_search_parameter, to_json_body, with_test_app,
};

#[tokio::test]
async fn transaction_conditional_reference_resolves_single_match() -> anyhow::Result<()> {
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
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"].as_str().unwrap().to_string();

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "transaction",
                "entry": [{
                    "request": { "method": "POST", "url": "Observation" },
                    "resource": {
                        "resourceType": "Observation",
                        "status": "final",
                        "code": { "text": "test" },
                        "subject": { "reference": "Patient?identifier=http://example.org/fhir/mrn|123" }
                    }
                }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_status(status, StatusCode::OK, "transaction");

            let response: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(response["resourceType"], "Bundle");
            assert_eq!(response["type"], "transaction-response");
            let observation = response["entry"][0]["resource"].clone();
            assert_eq!(
                observation["subject"]["reference"].as_str().unwrap(),
                format!("Patient/{}", patient_id)
            );

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn transaction_conditional_reference_no_match_fails_and_rolls_back() -> anyhow::Result<()> {
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

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "transaction",
                "entry": [{
                    "request": { "method": "POST", "url": "Observation" },
                    "resource": {
                        "resourceType": "Observation",
                        "status": "final",
                        "code": { "text": "test" },
                        "subject": { "reference": "Patient?identifier=http://example.org/fhir/mrn|does-not-exist" }
                    }
                }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_eq!(status, StatusCode::PRECONDITION_FAILED);

            let outcome: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(outcome["resourceType"], "OperationOutcome");

            let obs_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM resources WHERE resource_type = 'Observation' AND is_current = true AND deleted = false",
            )
            .fetch_one(&app.state.db_pool)
            .await?;
            assert_eq!(obs_count, 0);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn transaction_conditional_reference_multiple_matches_fails_and_rolls_back(
) -> anyhow::Result<()> {
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

            let patient_a = patient_with_mrn("Doe", "dup");
            let patient_b = patient_with_mrn("Roe", "dup");
            let (_status, _headers, _body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient_a)?))
                .await?;
            let (_status, _headers, _body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient_b)?))
                .await?;

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "transaction",
                "entry": [{
                    "request": { "method": "POST", "url": "Observation" },
                    "resource": {
                        "resourceType": "Observation",
                        "status": "final",
                        "code": { "text": "test" },
                        "subject": { "reference": "Patient?identifier=http://example.org/fhir/mrn|dup" }
                    }
                }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_eq!(status, StatusCode::PRECONDITION_FAILED);

            let outcome: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(outcome["resourceType"], "OperationOutcome");

            let obs_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM resources WHERE resource_type = 'Observation' AND is_current = true AND deleted = false",
            )
            .fetch_one(&app.state.db_pool)
            .await?;
            assert_eq!(obs_count, 0);

            Ok(())
        })
    })
    .await
}
