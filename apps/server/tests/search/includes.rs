//! _include and _revinclude tests
//!
//! FHIR Spec: 3.2.1.7.5 - Including other resources in search results

use crate::support::*;
use axum::http::{Method, StatusCode};
use serde_json::json;

// ============================================================================
// _revinclude
// ============================================================================

#[tokio::test]
async fn revinclude_basic() -> anyhow::Result<()> {
    // Patient?_revinclude=Condition:subject should return matched Patients
    // plus any Conditions that reference them via "subject".
    with_test_app(|app| {
        Box::pin(async move {
            let pool = &app.state.db_pool;

            register_search_parameter(pool, "subject", "Condition", "reference", "Condition.subject", &["Patient"]).await?;

            // Create Patient
            let patient = json!({"resourceType": "Patient", "name": [{"family": "Doe"}]});
            let (status, _, body) = app.request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?)).await?;
            assert_status(status, StatusCode::CREATED, "create patient");
            let patient_id = serde_json::from_slice::<serde_json::Value>(&body)?["id"].as_str().unwrap().to_string();

            // Create Condition referencing that Patient
            let condition = json!({
                "resourceType": "Condition",
                "subject": {"reference": format!("Patient/{}", patient_id)},
                "code": {"text": "Headache"}
            });
            let (status, _, body) = app.request(Method::POST, "/fhir/Condition", Some(to_json_body(&condition)?)).await?;
            assert_status(status, StatusCode::CREATED, "create condition");
            let cond_id = serde_json::from_slice::<serde_json::Value>(&body)?["id"].as_str().unwrap().to_string();

            // Search
            let (status, _, body) = app.request(Method::GET, "/fhir/Patient?_revinclude=Condition:subject", None).await?;
            assert_status(status, StatusCode::OK, "search");

            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            assert_bundle(&bundle)?;

            // The Patient should be a "match" entry
            let match_ids = extract_resource_ids_by_mode(&bundle, "Patient", "match")?;
            assert!(match_ids.contains(&patient_id), "Patient should be a match result");

            // The Condition should be an "include" entry
            let include_ids = extract_resource_ids_by_mode(&bundle, "Condition", "include")?;
            assert!(include_ids.contains(&cond_id), "Condition should be included via _revinclude");

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn revinclude_filters_by_source_type() -> anyhow::Result<()> {
    // _revinclude=Condition:subject should only return Conditions, not Observations
    // that also have a "subject" reference to the same Patient.
    with_test_app(|app| {
        Box::pin(async move {
            let pool = &app.state.db_pool;

            register_search_parameter(pool, "subject", "Condition", "reference", "Condition.subject", &["Patient"]).await?;
            register_search_parameter(pool, "subject", "Observation", "reference", "Observation.subject", &["Patient"]).await?;

            // Create Patient
            let patient = json!({"resourceType": "Patient", "name": [{"family": "Doe"}]});
            let (status, _, body) = app.request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?)).await?;
            assert_status(status, StatusCode::CREATED, "create patient");
            let patient_id = serde_json::from_slice::<serde_json::Value>(&body)?["id"].as_str().unwrap().to_string();

            // Create Condition referencing Patient
            let condition = json!({
                "resourceType": "Condition",
                "subject": {"reference": format!("Patient/{}", patient_id)},
                "code": {"text": "Headache"}
            });
            let (status, _, body) = app.request(Method::POST, "/fhir/Condition", Some(to_json_body(&condition)?)).await?;
            assert_status(status, StatusCode::CREATED, "create condition");
            let cond_id = serde_json::from_slice::<serde_json::Value>(&body)?["id"].as_str().unwrap().to_string();

            // Create Observation also referencing Patient
            let observation = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": {"text": "BP"},
                "subject": {"reference": format!("Patient/{}", patient_id)}
            });
            let (status, _, _body) = app.request(Method::POST, "/fhir/Observation", Some(to_json_body(&observation)?)).await?;
            assert_status(status, StatusCode::CREATED, "create observation");

            // _revinclude=Condition:subject — only Conditions, not Observations
            let (status, _, body) = app.request(Method::GET, "/fhir/Patient?_revinclude=Condition:subject", None).await?;
            assert_status(status, StatusCode::OK, "search");

            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            assert_bundle(&bundle)?;

            let include_cond_ids = extract_resource_ids_by_mode(&bundle, "Condition", "include")?;
            assert!(include_cond_ids.contains(&cond_id), "Condition should be included");

            let include_obs_ids = extract_resource_ids_by_mode(&bundle, "Observation", "include")?;
            assert!(include_obs_ids.is_empty(), "Observations should NOT be included when _revinclude specifies Condition");

            Ok(())
        })
    })
    .await
}

// ============================================================================
// _include
// ============================================================================

#[tokio::test]
async fn include_basic() -> anyhow::Result<()> {
    // Observation?_include=Observation:subject should return matched Observations
    // plus the referenced Patient.
    with_test_app(|app| {
        Box::pin(async move {
            let pool = &app.state.db_pool;

            register_search_parameter(pool, "subject", "Observation", "reference", "Observation.subject", &["Patient"]).await?;
            register_search_parameter(pool, "code", "Observation", "token", "Observation.code", &[]).await?;

            // Create Patient
            let patient = json!({"resourceType": "Patient", "name": [{"family": "Doe"}]});
            let (status, _, body) = app.request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?)).await?;
            assert_status(status, StatusCode::CREATED, "create patient");
            let patient_id = serde_json::from_slice::<serde_json::Value>(&body)?["id"].as_str().unwrap().to_string();

            // Create Observation referencing Patient
            let observation = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": {"coding": [{"system": "http://loinc.org", "code": "12345"}]},
                "subject": {"reference": format!("Patient/{}", patient_id)}
            });
            let (status, _, _body) = app.request(Method::POST, "/fhir/Observation", Some(to_json_body(&observation)?)).await?;
            assert_status(status, StatusCode::CREATED, "create observation");

            // Search
            let (status, _, body) = app.request(Method::GET, "/fhir/Observation?_include=Observation:subject", None).await?;
            assert_status(status, StatusCode::OK, "search");

            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            assert_bundle(&bundle)?;

            let match_obs = extract_resource_ids_by_mode(&bundle, "Observation", "match")?;
            assert!(!match_obs.is_empty(), "should have matched Observations");

            let include_patients = extract_resource_ids_by_mode(&bundle, "Patient", "include")?;
            assert!(include_patients.contains(&patient_id), "Patient should be included via _include");

            Ok(())
        })
    })
    .await
}
