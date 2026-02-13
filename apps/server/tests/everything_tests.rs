#![allow(unused)]
#[allow(unused)]
mod support;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
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

/// Register the OperationDefinition for $everything and install compartment memberships.
async fn setup_everything(app: &TestApp) -> anyhow::Result<()> {
    // Register search parameters needed for compartment membership indexing.
    // Without these, the indexing service won't populate search_reference for subject/patient.
    register_search_parameter(
        &app.state.db_pool,
        "subject",
        "Observation",
        "reference",
        "Observation.subject",
        &[],
    )
    .await?;
    register_search_parameter(
        &app.state.db_pool,
        "subject",
        "Condition",
        "reference",
        "Condition.subject",
        &[],
    )
    .await?;

    // Invalidate the search param cache so the engine picks up the new parameters.
    app.state.search_engine.invalidate_param_cache();

    // Register the OperationDefinition
    let op_def = json!({
        "resourceType": "OperationDefinition",
        "id": "everything",
        "url": "http://hl7.org/fhir/OperationDefinition/Patient-everything",
        "status": "active",
        "kind": "operation",
        "code": "everything",
        "resource": ["Patient"],
        "system": false,
        "type": false,
        "instance": true,
        "affectsState": false
    });
    let (status, _headers, _body) = app
        .request(
            Method::POST,
            "/fhir/OperationDefinition",
            Some(to_json_body(&op_def)?),
        )
        .await?;
    assert_status(status, StatusCode::CREATED, "create OperationDefinition");

    // Reload operation definitions
    app.state.operation_registry.load_definitions().await?;

    // Insert compartment memberships for Patient compartment
    sqlx::query(
        "INSERT INTO compartment_memberships (compartment_type, resource_type, parameter_names) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind("Patient")
    .bind("Patient")
    .bind(&vec!["{def}".to_string()])
    .execute(&app.state.db_pool)
    .await?;

    sqlx::query(
        "INSERT INTO compartment_memberships (compartment_type, resource_type, parameter_names) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind("Patient")
    .bind("Observation")
    .bind(&vec!["subject".to_string(), "patient".to_string()])
    .execute(&app.state.db_pool)
    .await?;

    sqlx::query(
        "INSERT INTO compartment_memberships (compartment_type, resource_type, parameter_names) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind("Patient")
    .bind("Condition")
    .bind(&vec!["subject".to_string(), "patient".to_string()])
    .execute(&app.state.db_pool)
    .await?;

    Ok(())
}

#[tokio::test]
async fn everything_returns_patient_and_compartment_resources() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            setup_everything(app).await?;

            // Create a Patient
            let patient = json!({
                "resourceType": "Patient",
                "name": [{"family": "Everything", "given": ["Test"]}]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let patient_id = created["id"].as_str().unwrap().to_string();

            // Create Observations referencing the Patient
            let obs1 = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": {"coding": [{"system": "http://loinc.org", "code": "1234-5"}]},
                "subject": {"reference": format!("Patient/{}", patient_id)}
            });
            let (status, _headers, _body) = app
                .request(Method::POST, "/fhir/Observation", Some(to_json_body(&obs1)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Observation 1");

            let obs2 = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": {"coding": [{"system": "http://loinc.org", "code": "5678-9"}]},
                "subject": {"reference": format!("Patient/{}", patient_id)}
            });
            let (status, _headers, _body) = app
                .request(Method::POST, "/fhir/Observation", Some(to_json_body(&obs2)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Observation 2");

            // Call $everything
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!("/fhir/Patient/{}/$everything", patient_id),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "$everything");
            let bundle = parse_json(&body)?;
            assert_eq!(bundle["resourceType"], "Bundle");
            assert_eq!(bundle["type"], "searchset");

            let es = entries(&bundle);
            // Should have at least the Patient + 2 Observations
            assert!(
                es.len() >= 3,
                "expected at least 3 entries (Patient + 2 Obs), got {}",
                es.len()
            );

            // Verify Patient is included
            let has_patient = es
                .iter()
                .any(|e| e["resource"]["resourceType"] == "Patient");
            assert!(has_patient, "expected Patient in $everything bundle");

            // Verify Observations are included
            let obs_count = es
                .iter()
                .filter(|e| e["resource"]["resourceType"] == "Observation")
                .count();
            assert!(
                obs_count >= 2,
                "expected at least 2 Observations, got {}",
                obs_count
            );

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn everything_with_type_filter() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            setup_everything(app).await?;

            // Create Patient
            let patient = json!({
                "resourceType": "Patient",
                "name": [{"family": "TypeFilter"}]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let patient_id = created["id"].as_str().unwrap().to_string();

            // Create Observation
            let obs = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": {"coding": [{"system": "http://loinc.org", "code": "1234-5"}]},
                "subject": {"reference": format!("Patient/{}", patient_id)}
            });
            let (status, _headers, _body) = app
                .request(Method::POST, "/fhir/Observation", Some(to_json_body(&obs)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Observation");

            // Create Condition
            let cond = json!({
                "resourceType": "Condition",
                "clinicalStatus": {
                    "coding": [{"system": "http://terminology.hl7.org/CodeSystem/condition-clinical", "code": "active"}]
                },
                "subject": {"reference": format!("Patient/{}", patient_id)},
                "code": {"coding": [{"system": "http://snomed.info/sct", "code": "38341003"}]}
            });
            let (status, _headers, _body) = app
                .request(Method::POST, "/fhir/Condition", Some(to_json_body(&cond)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Condition");

            // Call $everything with _type=Observation — should NOT include Condition
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/Patient/{}/$everything?_type=Observation",
                        patient_id
                    ),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "$everything _type=Observation");
            let bundle = parse_json(&body)?;
            let es = entries(&bundle);

            let has_obs = es
                .iter()
                .any(|e| e["resource"]["resourceType"] == "Observation");
            assert!(has_obs, "expected Observation in filtered bundle");

            let has_condition = es
                .iter()
                .any(|e| e["resource"]["resourceType"] == "Condition");
            assert!(
                !has_condition,
                "Condition should NOT be in _type=Observation bundle"
            );

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn everything_nonexistent_patient_returns_404() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            setup_everything(app).await?;

            let (status, _headers, _body) = app
                .request(
                    Method::GET,
                    "/fhir/Patient/nonexistent-id/$everything",
                    None,
                )
                .await?;
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "expected 404 for nonexistent Patient"
            );

            Ok(())
        })
    })
    .await
}
