#![allow(unused)]
#[allow(unused)]
mod support;

use axum::http::{Method, StatusCode};
use serde_json::json;
use support::{
    assert_status, minimal_patient, register_search_parameter, to_json_body, with_test_app,
};

async fn assert_reference_indexed(
    pool: &sqlx::PgPool,
    obs_id: &str,
    param_code: &str,
    expected_target_type: &str,
    expected_target_id: &str,
) -> anyhow::Result<()> {
    let row: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT target_type, target_id
        FROM search_reference
        WHERE resource_type = 'Observation'
          AND resource_id = $1
          AND parameter_name = $2
        "#,
    )
    .bind(obs_id)
    .bind(param_code)
    .fetch_optional(pool)
    .await?;

    let (target_type, target_id) = row.expect("missing expected search_reference row");
    assert_eq!(target_type, expected_target_type);
    assert_eq!(target_id, expected_target_id);
    Ok(())
}

async fn assert_string_indexed(
    pool: &sqlx::PgPool,
    obs_id: &str,
    param_code: &str,
    expected_value: &str,
) -> anyhow::Result<()> {
    let value: Option<String> = sqlx::query_scalar(
        r#"
        SELECT value
        FROM search_string
        WHERE resource_type = 'Observation'
          AND resource_id = $1
          AND parameter_name = $2
        "#,
    )
    .bind(obs_id)
    .bind(param_code)
    .fetch_optional(pool)
    .await?;

    assert_eq!(
        value.as_deref(),
        Some(expected_value),
        "missing/incorrect search_string row"
    );
    Ok(())
}

#[tokio::test]
async fn crud_resolve_typecheck_indexes_reference_param() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let sp_code = "subject-resolve-patient";
            register_search_parameter(
                &app.state.db_pool,
                sp_code,
                "Observation",
                "reference",
                "subject.where(resolve() is Patient)",
                &[],
            )
            .await?;

            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"].as_str().unwrap().to_string();

            let observation = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": { "text": "test" },
                "subject": { "reference": format!("Patient/{}", patient_id) }
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&observation)?),
                )
                .await?;
            assert_status(status, StatusCode::CREATED, "create Observation");
            let created_obs: serde_json::Value = serde_json::from_slice(&body)?;
            let obs_id = created_obs["id"].as_str().unwrap().to_string();

            assert_reference_indexed(&app.state.db_pool, &obs_id, sp_code, "Patient", &patient_id)
                .await?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn crud_resolve_dereference_indexes_string_param() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let sp_code = "subject-patient-family";
            register_search_parameter(
                &app.state.db_pool,
                sp_code,
                "Observation",
                "string",
                "subject.resolve().name.family",
                &[],
            )
            .await?;

            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"].as_str().unwrap().to_string();

            let observation = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": { "text": "test" },
                "subject": { "reference": format!("Patient/{}", patient_id) }
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&observation)?),
                )
                .await?;
            assert_status(status, StatusCode::CREATED, "create Observation");
            let created_obs: serde_json::Value = serde_json::from_slice(&body)?;
            let obs_id = created_obs["id"].as_str().unwrap().to_string();

            assert_string_indexed(&app.state.db_pool, &obs_id, sp_code, "Doe").await?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn transaction_bundle_fullurl_resolve_typecheck_is_indexed() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let sp_code = "subject-resolve-patient";
            register_search_parameter(
                &app.state.db_pool,
                sp_code,
                "Observation",
                "reference",
                "subject.where(resolve() is Patient)",
                &[],
            )
            .await?;

            let patient_full_url = "urn:uuid:patient-1";
            let observation_full_url = "urn:uuid:observation-1";

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "transaction",
                "entry": [
                    {
                        "fullUrl": patient_full_url,
                        "request": { "method": "POST", "url": "Patient" },
                        "resource": minimal_patient()
                    },
                    {
                        "fullUrl": observation_full_url,
                        "request": { "method": "POST", "url": "Observation" },
                        "resource": {
                            "resourceType": "Observation",
                            "status": "final",
                            "code": { "text": "test" },
                            "subject": { "reference": patient_full_url }
                        }
                    }
                ]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_status(status, StatusCode::OK, "transaction");

            let response: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(response["resourceType"], "Bundle");
            assert_eq!(response["type"], "transaction-response");

            let patient = response["entry"][0]["resource"].clone();
            let observation = response["entry"][1]["resource"].clone();
            let patient_id = patient["id"].as_str().unwrap().to_string();
            let obs_id = observation["id"].as_str().unwrap().to_string();

            assert_eq!(
                observation["subject"]["reference"].as_str().unwrap(),
                format!("Patient/{}", patient_id)
            );

            assert_reference_indexed(&app.state.db_pool, &obs_id, sp_code, "Patient", &patient_id)
                .await?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn batch_bundle_resolve_typecheck_is_indexed() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let sp_code = "subject-resolve-patient";
            register_search_parameter(
                &app.state.db_pool,
                sp_code,
                "Observation",
                "reference",
                "subject.where(resolve() is Patient)",
                &[],
            )
            .await?;

            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"].as_str().unwrap().to_string();

            let bundle = json!({
                "resourceType": "Bundle",
                "type": "batch",
                "entry": [{
                    "request": { "method": "POST", "url": "Observation" },
                    "resource": {
                        "resourceType": "Observation",
                        "status": "final",
                        "code": { "text": "test" },
                        "subject": { "reference": format!("Patient/{}", patient_id) }
                    }
                }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir", Some(to_json_body(&bundle)?))
                .await?;
            assert_status(status, StatusCode::OK, "batch");

            let response: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(response["resourceType"], "Bundle");
            assert_eq!(response["type"], "batch-response");

            let observation = response["entry"][0]["resource"].clone();
            let obs_id = observation["id"].as_str().unwrap().to_string();

            assert_reference_indexed(&app.state.db_pool, &obs_id, sp_code, "Patient", &patient_id)
                .await?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn indexing_service_batch_api_reindexes_resolve_expressions() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let sp_code = "subject-resolve-patient";
            register_search_parameter(
                &app.state.db_pool,
                sp_code,
                "Observation",
                "reference",
                "subject.where(resolve() is Patient)",
                &[],
            )
            .await?;

            let patient = minimal_patient();
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"].as_str().unwrap().to_string();

            let observation = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": { "text": "test" },
                "subject": { "reference": format!("Patient/{}", patient_id) }
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&observation)?),
                )
                .await?;
            assert_status(status, StatusCode::CREATED, "create Observation");
            let created_obs: serde_json::Value = serde_json::from_slice(&body)?;
            let obs_id = created_obs["id"].as_str().unwrap().to_string();

            sqlx::query(
                r#"
                DELETE FROM search_reference
                WHERE resource_type = 'Observation'
                  AND resource_id = $1
                  AND parameter_name = $2
                "#,
            )
            .bind(&obs_id)
            .bind(sp_code)
            .execute(&app.state.db_pool)
            .await?;

            let count: i64 = sqlx::query_scalar(
                r#"
                SELECT COUNT(*)
                FROM search_reference
                WHERE resource_type = 'Observation'
                  AND resource_id = $1
                  AND parameter_name = $2
                "#,
            )
            .bind(&obs_id)
            .bind(sp_code)
            .fetch_one(&app.state.db_pool)
            .await?;
            assert_eq!(count, 0);

            let store = zunder::db::PostgresResourceStore::new(app.state.db_pool.clone());
            let resources = store
                .load_resources_batch("Observation", &[obs_id.clone()])
                .await?;
            app.state
                .indexing_service
                .index_resources_batch(&resources)
                .await?;

            assert_reference_indexed(&app.state.db_pool, &obs_id, sp_code, "Patient", &patient_id)
                .await?;
            Ok(())
        })
    })
    .await
}
