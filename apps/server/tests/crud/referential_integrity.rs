//! Referential Integrity Tests
//!
//! These tests verify that the configurable referential integrity modes work:
//! - "lenient" (default): no reference checking, dangling refs allowed
//! - "strict": rejects writes with broken references, blocks deletes of referenced resources

use crate::support::{
    assert_status, minimal_patient, register_search_parameter, to_json_body, with_test_app,
    with_test_app_with_config, ObservationBuilder,
};
use axum::http::{Method, StatusCode};
use serde_json::json;

// ============================================================================
// Lenient Mode (default)
// ============================================================================

#[tokio::test]
async fn lenient_allows_dangling_reference() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Create an Observation referencing a Patient that does not exist.
            let obs = ObservationBuilder::new()
                .code_text("Weight")
                .subject("Patient/nonexistent-999")
                .build();

            let (status, _headers, _body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&obs)?),
                )
                .await?;

            assert_status(status, StatusCode::CREATED, "lenient allows dangling ref");
            Ok(())
        })
    })
    .await
}

// ============================================================================
// Strict Mode — Create
// ============================================================================

#[tokio::test]
async fn strict_rejects_dangling_reference_on_create() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.fhir.referential_integrity.mode = "strict".to_string();
        },
        |app| {
            Box::pin(async move {
                let obs = ObservationBuilder::new()
                    .code_text("Weight")
                    .subject("Patient/nonexistent-999")
                    .build();

                let (status, _headers, body) = app
                    .request(
                        Method::POST,
                        "/fhir/Observation",
                        Some(to_json_body(&obs)?),
                    )
                    .await?;

                assert_status(
                    status,
                    StatusCode::CONFLICT,
                    "strict rejects dangling ref on create",
                );

                let outcome: serde_json::Value = serde_json::from_slice(&body)?;
                assert_eq!(outcome["resourceType"], "OperationOutcome");

                Ok(())
            })
        },
    )
    .await
}

#[tokio::test]
async fn strict_allows_valid_reference_on_create() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.fhir.referential_integrity.mode = "strict".to_string();
        },
        |app| {
            Box::pin(async move {
                // Create the Patient first.
                let patient = minimal_patient();
                let (status, _headers, body) = app
                    .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                    .await?;
                assert_status(status, StatusCode::CREATED, "create patient");

                let created: serde_json::Value = serde_json::from_slice(&body)?;
                let patient_id = created["id"].as_str().unwrap();

                // Now create Observation referencing the existing Patient.
                let obs = ObservationBuilder::new()
                    .code_text("Weight")
                    .subject(format!("Patient/{}", patient_id))
                    .build();

                let (status, _headers, _body) = app
                    .request(
                        Method::POST,
                        "/fhir/Observation",
                        Some(to_json_body(&obs)?),
                    )
                    .await?;

                assert_status(
                    status,
                    StatusCode::CREATED,
                    "strict allows valid ref on create",
                );

                Ok(())
            })
        },
    )
    .await
}

// ============================================================================
// Strict Mode — Update
// ============================================================================

#[tokio::test]
async fn strict_rejects_dangling_reference_on_update() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.fhir.referential_integrity.mode = "strict".to_string();
        },
        |app| {
            Box::pin(async move {
                // Create a valid Patient.
                let patient = minimal_patient();
                let (status, _headers, body) = app
                    .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                    .await?;
                assert_status(status, StatusCode::CREATED, "create patient");

                let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
                let patient_id = created_patient["id"].as_str().unwrap();

                // Create a valid Observation.
                let obs = ObservationBuilder::new()
                    .code_text("Weight")
                    .subject(format!("Patient/{}", patient_id))
                    .build();
                let (status, _headers, body) = app
                    .request(
                        Method::POST,
                        "/fhir/Observation",
                        Some(to_json_body(&obs)?),
                    )
                    .await?;
                assert_status(status, StatusCode::CREATED, "create observation");

                let created_obs: serde_json::Value = serde_json::from_slice(&body)?;
                let obs_id = created_obs["id"].as_str().unwrap();

                // Update the Observation with a dangling reference.
                let mut updated_obs = created_obs.clone();
                updated_obs["subject"] = json!({
                    "reference": "Patient/nonexistent-999"
                });

                let (status, _headers, _body) = app
                    .request(
                        Method::PUT,
                        &format!("/fhir/Observation/{}", obs_id),
                        Some(to_json_body(&updated_obs)?),
                    )
                    .await?;

                assert_status(
                    status,
                    StatusCode::CONFLICT,
                    "strict rejects dangling ref on update",
                );

                Ok(())
            })
        },
    )
    .await
}

// ============================================================================
// Strict Mode — Delete
// ============================================================================

#[tokio::test]
async fn strict_blocks_delete_when_referenced() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.fhir.referential_integrity.mode = "strict".to_string();
        },
        |app| {
            Box::pin(async move {
                // Register the "subject" search parameter so that the inline indexer
                // populates search_reference when the Observation is created.
                register_search_parameter(
                    &app.state.db_pool,
                    "subject",
                    "Observation",
                    "reference",
                    "Observation.subject",
                    &[],
                )
                .await?;

                // Create Patient.
                let patient = minimal_patient();
                let (status, _headers, body) = app
                    .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                    .await?;
                assert_status(status, StatusCode::CREATED, "create patient");

                let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
                let patient_id = created_patient["id"].as_str().unwrap();

                // Create Observation referencing the Patient.
                let obs = ObservationBuilder::new()
                    .code_text("Weight")
                    .subject(format!("Patient/{}", patient_id))
                    .build();
                let (status, _headers, _body) = app
                    .request(
                        Method::POST,
                        "/fhir/Observation",
                        Some(to_json_body(&obs)?),
                    )
                    .await?;
                assert_status(status, StatusCode::CREATED, "create observation");

                // Try to delete the Patient — should fail because it's referenced.
                let (status, _headers, body) = app
                    .request(
                        Method::DELETE,
                        &format!("/fhir/Patient/{}", patient_id),
                        None,
                    )
                    .await?;

                assert_status(
                    status,
                    StatusCode::CONFLICT,
                    "strict blocks delete when referenced",
                );

                let outcome: serde_json::Value = serde_json::from_slice(&body)?;
                assert_eq!(outcome["resourceType"], "OperationOutcome");

                Ok(())
            })
        },
    )
    .await
}

#[tokio::test]
async fn strict_allows_delete_when_unreferenced() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.fhir.referential_integrity.mode = "strict".to_string();
        },
        |app| {
            Box::pin(async move {
                // Create Patient with no references to it.
                let patient = minimal_patient();
                let (status, _headers, body) = app
                    .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                    .await?;
                assert_status(status, StatusCode::CREATED, "create patient");

                let created: serde_json::Value = serde_json::from_slice(&body)?;
                let patient_id = created["id"].as_str().unwrap();

                // Delete should succeed.
                let (status, _headers, _body) = app
                    .request(
                        Method::DELETE,
                        &format!("/fhir/Patient/{}", patient_id),
                        None,
                    )
                    .await?;

                assert_status(
                    status,
                    StatusCode::NO_CONTENT,
                    "strict allows delete when unreferenced",
                );

                Ok(())
            })
        },
    )
    .await
}

// ============================================================================
// Strict Mode — Reference Types
// ============================================================================

#[tokio::test]
async fn strict_ignores_fragment_and_absolute_refs() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.fhir.referential_integrity.mode = "strict".to_string();
        },
        |app| {
            Box::pin(async move {
                // Resource with a contained reference (#) and an absolute external URL.
                // Neither should be checked for referential integrity.
                let resource = json!({
                    "resourceType": "Observation",
                    "status": "final",
                    "code": { "text": "test" },
                    "contained": [{
                        "resourceType": "Patient",
                        "id": "p1",
                        "name": [{"family": "Contained"}]
                    }],
                    "subject": { "reference": "#p1" },
                    "performer": [{ "reference": "http://external.example.com/Practitioner/1" }]
                });

                let (status, _headers, _body) = app
                    .request(
                        Method::POST,
                        "/fhir/Observation",
                        Some(to_json_body(&resource)?),
                    )
                    .await?;

                assert_status(
                    status,
                    StatusCode::CREATED,
                    "strict ignores fragment and absolute refs",
                );

                Ok(())
            })
        },
    )
    .await
}

#[tokio::test]
async fn strict_allows_self_reference() -> anyhow::Result<()> {
    with_test_app_with_config(
        |config| {
            config.fhir.referential_integrity.mode = "strict".to_string();
        },
        |app| {
            Box::pin(async move {
                // Create a List that references itself (e.g., List.entry.item pointing to own id).
                // First create it, then update to self-reference.
                let list = json!({
                    "resourceType": "List",
                    "status": "current",
                    "mode": "working"
                });

                let (status, _headers, body) = app
                    .request(Method::POST, "/fhir/List", Some(to_json_body(&list)?))
                    .await?;
                assert_status(status, StatusCode::CREATED, "create list");

                let created: serde_json::Value = serde_json::from_slice(&body)?;
                let list_id = created["id"].as_str().unwrap();

                // Update the List to contain a self-reference.
                let mut self_ref_list = created.clone();
                self_ref_list["entry"] = json!([{
                    "item": { "reference": format!("List/{}", list_id) }
                }]);

                let (status, _headers, _body) = app
                    .request(
                        Method::PUT,
                        &format!("/fhir/List/{}", list_id),
                        Some(to_json_body(&self_ref_list)?),
                    )
                    .await?;

                assert_status(
                    status,
                    StatusCode::OK,
                    "strict allows self-reference",
                );

                Ok(())
            })
        },
    )
    .await
}
