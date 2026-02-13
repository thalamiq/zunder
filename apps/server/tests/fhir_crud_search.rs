#![allow(unused)]
#[allow(unused)]
mod support;

use anyhow::Context as _;
use axum::http::{Method, StatusCode};
use serde_json::json;
use support::*;

#[tokio::test]
async fn standard_meta_search_params_work_when_indexed() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal definitions for standard meta search parameters.
            // (Tests run without loading the full core SearchParameter set.)
            register_search_parameter(
                &app.state.db_pool,
                "_language",
                "Patient",
                "token",
                "Patient.language",
                &[],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "_source",
                "Patient",
                "uri",
                "Patient.meta.source",
                &[],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "_security",
                "Patient",
                "token",
                "Patient.meta.security",
                &[],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "_tag",
                "Patient",
                "token",
                "Patient.meta.tag",
                &[],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "_profile",
                "Patient",
                "reference",
                "Patient.meta.profile",
                &[],
            )
            .await?;

            let patient = json!({
                "resourceType": "Patient",
                "language": "es",
                "meta": {
                    "profile": ["http://hl7.org/fhir/StructureDefinition/bp"],
                    "source": "http://example.com/Organization/123",
                    "security": [{
                        "system": "http://terminology.hl7.org/CodeSystem/v3-Confidentiality",
                        "code": "R"
                    }],
                    "tag": [{
                        "system": "http://terminology.hl7.org/ValueSet/v3-SeverityObservation",
                        "code": "H"
                    }]
                }
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

            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    "/fhir/Patient?_language=es&_source=http://example.com/Organization/123&_security=http://terminology.hl7.org/CodeSystem/v3-Confidentiality|R&_tag=http://terminology.hl7.org/ValueSet/v3-SeverityObservation|H&_profile=http://hl7.org/fhir/StructureDefinition/bp",
                    None,
                )
                .await?;
            if status != StatusCode::OK {
                eprintln!("{}", String::from_utf8_lossy(&body));
            }
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn patient_create_read_update_and_search_by_id() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let patient = json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "Smith", "given": ["Eve"] }]
            });

            let (status, headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            assert!(headers.get("location").is_some());
            let created: serde_json::Value = serde_json::from_slice(&body)?;

            let id = created
                .get("id")
                .and_then(|v| v.as_str())
                .context("created Patient has id")?
                .to_string();
            assert_eq!(created["resourceType"], "Patient");
            assert_eq!(created["meta"]["versionId"], "1");

            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/Patient/{id}"), None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let read: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(read["id"], id);

            let updated_patient = json!({
                "resourceType": "Patient",
                "id": id,
                "active": false,
                "name": [{ "family": "Smith", "given": ["Eve"] }]
            });

            let (status, _headers, body) = app
                .request(
                    Method::PUT,
                    &format!("/fhir/Patient/{id}"),
                    Some(to_json_body(&updated_patient)?),
                )
                .await?;
            assert_eq!(status, StatusCode::OK);
            let updated: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(updated["meta"]["versionId"], "2");

            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/Patient?_id={id}"), None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(bundle["resourceType"], "Bundle");
            assert_eq!(bundle["type"], "searchset");
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn system_search_requires_type_parameter() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let (status, _headers, _body) = app.request(Method::GET, "/fhir?_id=123", None).await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn system_search_applies_common_meta_parameters() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            register_search_parameter(
                &app.state.db_pool,
                "_language",
                "Patient",
                "token",
                "Patient.language",
                &[],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "_source",
                "Patient",
                "uri",
                "Patient.meta.source",
                &[],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "_security",
                "Patient",
                "token",
                "Patient.meta.security",
                &[],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "_tag",
                "Patient",
                "token",
                "Patient.meta.tag",
                &[],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "_profile",
                "Patient",
                "reference",
                "Patient.meta.profile",
                &[],
            )
            .await?;

            let patient = json!({
                "resourceType": "Patient",
                "language": "es",
                "meta": {
                    "profile": ["http://hl7.org/fhir/StructureDefinition/bp"],
                    "source": "http://example.com/Organization/123",
                    "security": [{
                        "system": "http://terminology.hl7.org/CodeSystem/v3-Confidentiality",
                        "code": "R"
                    }],
                    "tag": [{
                        "system": "http://terminology.hl7.org/ValueSet/v3-SeverityObservation",
                        "code": "H"
                    }]
                }
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

            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    "/fhir?_type=Patient&_language=es&_source=http://example.com/Organization/123&_security=http://terminology.hl7.org/CodeSystem/v3-Confidentiality|R&_tag=http://terminology.hl7.org/ValueSet/v3-SeverityObservation|H&_profile=http://hl7.org/fhir/StructureDefinition/bp",
                    None,
                )
                .await?;
            if status != StatusCode::OK {
                eprintln!("{}", String::from_utf8_lossy(&body));
            }
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn unsupported_special_parameters_fail_closed() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            for param in ["_filter", "_query"] {
                let (status, _headers, _body) = app
                    .request(Method::GET, &format!("/fhir/Patient?{param}=x"), None)
                    .await?;
                assert_eq!(status, StatusCode::BAD_REQUEST, "param: {param}");
            }
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn in_parameter_matches_active_membership() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let patient_in = json!({ "resourceType": "Patient", "active": true });
            let patient_out = json!({ "resourceType": "Patient", "active": true });

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Patient",
                    Some(to_json_body(&patient_in)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_in: serde_json::Value = serde_json::from_slice(&body)?;
            let id_in = created_in["id"].as_str().context("id")?.to_string();

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Patient",
                    Some(to_json_body(&patient_out)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_out: serde_json::Value = serde_json::from_slice(&body)?;
            let id_out = created_out["id"].as_str().context("id")?.to_string();

            // Create a real Group to drive `_in` membership via server indexing.
            let group = json!({
                "resourceType": "Group",
                "id": "104",
                "type": "person",
                "actual": true,
                "member": [{
                    "entity": { "reference": format!("Patient/{id_in}") }
                }]
            });
            let (status, _headers, _body) = app
                .request(Method::PUT, "/fhir/Group/104", Some(to_json_body(&group)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);

            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Patient?_in=Group/104", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle["entry"]
                .as_array()
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id_in);

            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Patient?_in:not=Group/104", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle["entry"]
                .as_array()
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id_out);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn in_parameter_supports_membership_chaining() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Add a minimal Encounter.patient definition so `patient._in=...` is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "patient",
                "Encounter",
                "reference",
                "Encounter.subject",
                &["missing"],
            )
            .await?;

            let patient = json!({ "resourceType": "Patient", "active": true });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"].as_str().context("id")?.to_string();

            let encounter = json!({
                "resourceType": "Encounter",
                "status": "finished",
                "class": { "system": "http://terminology.hl7.org/CodeSystem/v3-ActCode", "code": "AMB" },
                "subject": { "reference": format!("Patient/{patient_id}") }
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Encounter", Some(to_json_body(&encounter)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_encounter: serde_json::Value = serde_json::from_slice(&body)?;
            let encounter_id = created_encounter["id"].as_str().context("id")?.to_string();

            // Create a real Group to drive `_in` membership via server indexing.
            let group = json!({
                "resourceType": "Group",
                "id": "104",
                "type": "person",
                "actual": true,
                "member": [{
                    "entity": { "reference": format!("Patient/{patient_id}") }
                }]
            });
            let (status, _headers, _body) = app
                .request(
                    Method::PUT,
                    "/fhir/Group/104",
                    Some(to_json_body(&group)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);

            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Encounter?patient._in=Group/104", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle["entry"]
                .as_array()
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], encounter_id);

            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Encounter?patient._in:not=Group/104", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            assert_eq!(entries.len(), 0);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn list_parameter_matches_list_membership_and_functional_literal() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let condition = json!({
                "resourceType": "Condition",
                "clinicalStatus": {
                    "coding": [{
                        "system": "http://terminology.hl7.org/CodeSystem/condition-clinical",
                        "code": "active"
                    }]
                },
                "verificationStatus": {
                    "coding": [{
                        "system": "http://terminology.hl7.org/CodeSystem/condition-ver-status",
                        "code": "confirmed"
                    }]
                },
                "code": { "text": "test" },
                "subject": { "reference": "Patient/example" }
            });

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Condition",
                    Some(to_json_body(&condition)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_condition: serde_json::Value = serde_json::from_slice(&body)?;
            let condition_id = created_condition["id"].as_str().context("id")?.to_string();

            let list_102 = json!({
                "resourceType": "List",
                "id": "102",
                "status": "current",
                "mode": "working",
                "entry": [{
                    "item": { "reference": format!("Condition/{condition_id}") }
                }]
            });
            let (status, _headers, _body) = app
                .request(
                    Method::PUT,
                    "/fhir/List/102",
                    Some(to_json_body(&list_102)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);

            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Condition?_list=102", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle["entry"]
                .as_array()
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], condition_id);

            // Functional literal values are treated as List.id when the List is materialized.
            let list_allergies = json!({
                "resourceType": "List",
                "id": "current-allergies",
                "status": "current",
                "mode": "working",
                "entry": [{
                    "item": { "reference": format!("Condition/{condition_id}") }
                }]
            });
            let (status, _headers, _body) = app
                .request(
                    Method::PUT,
                    "/fhir/List/current-allergies",
                    Some(to_json_body(&list_allergies)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    "/fhir/Condition?_list=$current-allergies",
                    None,
                )
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle["entry"]
                .as_array()
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn id_parameter_rejects_modifiers_and_pipes() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            let (status, _headers, _body) = app
                .request(Method::GET, "/fhir/Patient?_id:missing=false", None)
                .await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            let (status, _headers, _body) = app
                .request(Method::GET, "/fhir/Patient?_id=system|code", None)
                .await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn reference_identifier_modifier_matches_reference_identifier_only() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Add a minimal SearchParameter definition so Observation?subject:identifier=... is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "subject",
                "Observation",
                "reference",
                "Observation.subject",
                &["identifier", "missing"],
            )
            .await?;
            app.state
                .indexing_service
                .invalidate_cache(Some("Observation"));

            let mrn_system = "http://example.org/fhir/mrn";
            let mrn_value = "12345";

            let patient = json!({
                "resourceType": "Patient",
                "identifier": [{
                    "system": mrn_system,
                    "value": mrn_value
                }],
                "name": [{ "family": "Smith" }]
            });

            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"]
                .as_str()
                .context("created Patient has id")?
                .to_string();

            // Observation A: has subject.identifier matching the query.
            let obs_with_subject_identifier = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": { "text": "test" },
                "subject": {
                    "reference": format!("Patient/{patient_id}"),
                    "identifier": {
                        "system": mrn_system,
                        "value": mrn_value
                    }
                }
            });

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&obs_with_subject_identifier)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_obs_a: serde_json::Value = serde_json::from_slice(&body)?;
            let obs_a_id = created_obs_a["id"]
                .as_str()
                .context("created Observation A has id")?
                .to_string();

            let token_rows: Vec<(Option<String>, String)> = sqlx::query_as(
                r#"
                SELECT system, code
                FROM search_token
                WHERE resource_type = 'Observation'
                    AND resource_id = $1
                    AND parameter_name = 'subject'
                "#,
            )
            .bind(&obs_a_id)
            .fetch_all(&app.state.db_pool)
            .await?;
            assert!(
                token_rows.iter().any(
                    |(system, code)| system.as_deref() == Some(mrn_system) && code == mrn_value
                ),
                "expected Observation.subject identifier token to be indexed"
            );

            // Observation B: has a subject.reference to the Patient, but *no* subject.identifier.
            // Per spec, :identifier must NOT match via the referenced Patient.identifier.
            let obs_with_reference_only = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": { "text": "test" },
                "subject": {
                    "reference": format!("Patient/{patient_id}")
                }
            });

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&obs_with_reference_only)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_obs_b: serde_json::Value = serde_json::from_slice(&body)?;
            let obs_b_id = created_obs_b["id"]
                .as_str()
                .context("created Observation B has id")?
                .to_string();

            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!("/fhir/Observation?subject:identifier={mrn_system}|{mrn_value}"),
                    None,
                )
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], obs_a_id);
            assert_ne!(obs_a_id, obs_b_id);

            // Invalid token value: '|' (empty system and code) must be rejected.
            let (status, _headers, _body) = app
                .request(Method::GET, "/fhir/Observation?subject:identifier=|", None)
                .await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn include_iterate_applies_to_included_resources() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definitions so `_include` can resolve paths.
            register_search_parameter(
                &app.state.db_pool,
                "subject",
                "Observation",
                "reference",
                "Observation.subject",
                &["Patient"],
            )
            .await?;
            register_search_parameter(
                &app.state.db_pool,
                "link",
                "Patient",
                "reference",
                "Patient.link.other",
                &["Patient"],
            )
            .await?;

            let patient_1 = json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "Iterate" }]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient_1)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_p1: serde_json::Value = serde_json::from_slice(&body)?;
            let p1_id = created_p1["id"]
                .as_str()
                .context("created Patient 1 has id")?
                .to_string();

            let patient_2 = json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "Iterated" }]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient_2)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_p2: serde_json::Value = serde_json::from_slice(&body)?;
            let p2_id = created_p2["id"]
                .as_str()
                .context("created Patient 2 has id")?
                .to_string();

            // Update Patient/1 to link to Patient/2 (this is what `_include:iterate` will traverse).
            let mut updated_p1 = created_p1.clone();
            updated_p1["link"] = json!([{
                "other": { "reference": format!("Patient/{p2_id}") }
            }]);
            let (status, _headers, _body) = app
                .request(
                    Method::PUT,
                    &format!("/fhir/Patient/{p1_id}"),
                    Some(to_json_body(&updated_p1)?),
                )
                .await?;
            assert_eq!(status, StatusCode::OK);

            let observation = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": { "text": "iterate-test" },
                "subject": { "reference": format!("Patient/{p1_id}") }
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Observation", Some(to_json_body(&observation)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_obs: serde_json::Value = serde_json::from_slice(&body)?;
            let obs_id = created_obs["id"]
                .as_str()
                .context("created Observation has id")?
                .to_string();

            // Without `:iterate`, Patient:link is not applied to included Patient resources.
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/Observation?_id={obs_id}&_include=Observation:subject&_include=Patient:link"
                    ),
                    None,
                )
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let included_ids = entries
                .iter()
                .filter(|e| e.get("search").and_then(|s| s.get("mode")).and_then(|m| m.as_str()) == Some("include"))
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Patient"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(included_ids.len(), 1);
            assert_eq!(included_ids[0], p1_id);

            // With `_include:iterate=Patient:link`, Patient.link is applied to included Patients too.
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/Observation?_id={obs_id}&_include=Observation:subject&_include:iterate=Patient:link"
                    ),
                    None,
                )
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let mut included_ids = entries
                .iter()
                .filter(|e| e.get("search").and_then(|s| s.get("mode")).and_then(|m| m.as_str()) == Some("include"))
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Patient"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            included_ids.sort();
            let mut expected = vec![p1_id, p2_id];
            expected.sort();
            assert_eq!(included_ids, expected);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn missing_modifier_requires_single_boolean_value() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definition so Patient?name:missing=... is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "name",
                "Patient",
                "string",
                "Patient.name",
                &["missing"],
            )
            .await?;

            let (status, _headers, _body) = app
                .request(Method::GET, "/fhir/Patient?name:missing=true,false", None)
                .await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            let (status, _headers, _body) = app
                .request(Method::GET, "/fhir/Patient?name:missing=foo", None)
                .await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn token_not_modifier_uses_set_semantics_and_includes_missing() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definition so Patient?gender:not=... is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "gender",
                "Patient",
                "token",
                "Patient.gender",
                &["not"],
            )
            .await?;

            let male = json!({ "resourceType": "Patient", "gender": "male" });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&male)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let male_created: serde_json::Value = serde_json::from_slice(&body)?;
            let male_id = male_created["id"]
                .as_str()
                .context("created male Patient has id")?
                .to_string();

            let female = json!({ "resourceType": "Patient", "gender": "female" });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&female)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let female_created: serde_json::Value = serde_json::from_slice(&body)?;
            let female_id = female_created["id"]
                .as_str()
                .context("created female Patient has id")?
                .to_string();

            let missing = json!({ "resourceType": "Patient" });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&missing)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let missing_created: serde_json::Value = serde_json::from_slice(&body)?;
            let missing_id = missing_created["id"]
                .as_str()
                .context("created missing-gender Patient has id")?
                .to_string();

            // gender:not=male should include female and missing.
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Patient?gender:not=male", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let mut ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Patient"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            ids.sort();
            let mut expected = vec![female_id.clone(), missing_id.clone()];
            expected.sort();
            assert_eq!(ids, expected);

            // gender:not=male,female should include only missing (set semantics).
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Patient?gender:not=male,female", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let mut ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Patient"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            ids.sort();
            assert_eq!(ids, vec![missing_id]);

            // Avoid unused variable warning and provide a sanity check.
            assert_ne!(male_id, female_id);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn token_of_type_modifier_matches_correlated_identifier_type_and_value() -> anyhow::Result<()>
{
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definitions so Patient?identifier:of-type=... is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "identifier",
                "Patient",
                "token",
                "Patient.identifier",
                &["of-type"],
            )
            .await?;

            // Also define a non-Identifier token param to ensure :of-type is rejected there.
            register_search_parameter(
                &app.state.db_pool,
                "gender",
                "Patient",
                "token",
                "Patient.gender",
                &["of-type"],
            )
            .await?;

            let type_system = "http://terminology.hl7.org/CodeSystem/v2-0203";
            let mr_code = "MR";
            let mrt_code = "MRT";
            let value_12345 = "12345";

            // Patient 1: has MR|12345 and MRT|12345.
            let patient_1 = json!({
                "resourceType": "Patient",
                "identifier": [{
                    "type": { "coding": [{ "system": type_system, "code": mr_code }] },
                    "system": "http://example.org/ehr-primary/",
                    "value": value_12345
                },{
                    "type": { "coding": [{ "system": type_system, "code": mrt_code }] },
                    "system": "http://example.org/ehr-er",
                    "value": value_12345
                }]
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Patient",
                    Some(to_json_body(&patient_1)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_p1: serde_json::Value = serde_json::from_slice(&body)?;
            let p1_id = created_p1["id"]
                .as_str()
                .context("created Patient 1 has id")?
                .to_string();

            // Patient 2: has MRT|12345 only (should not match MR query).
            let patient_2 = json!({
                "resourceType": "Patient",
                "identifier": [{
                    "type": { "coding": [{ "system": type_system, "code": mrt_code }] },
                    "system": "http://example.org/ehr-er",
                    "value": value_12345
                }]
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Patient",
                    Some(to_json_body(&patient_2)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_p2: serde_json::Value = serde_json::from_slice(&body)?;
            let p2_id = created_p2["id"]
                .as_str()
                .context("created Patient 2 has id")?
                .to_string();

            // Patient 3: has MR|99999 (should not match MR|12345 query).
            let patient_3 = json!({
                "resourceType": "Patient",
                "identifier": [{
                    "type": { "coding": [{ "system": type_system, "code": mr_code }] },
                    "system": "http://example.org/ehr-primary/",
                    "value": "99999"
                }]
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Patient",
                    Some(to_json_body(&patient_3)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_p3: serde_json::Value = serde_json::from_slice(&body)?;
            let p3_id = created_p3["id"]
                .as_str()
                .context("created Patient 3 has id")?
                .to_string();

            // Patient 4: has MR|12345 but coding.system is missing (should not match, since :of-type requires system).
            let patient_4 = json!({
                "resourceType": "Patient",
                "identifier": [{
                    "type": { "coding": [{ "code": mr_code }] },
                    "system": "http://example.org/ehr-primary/",
                    "value": value_12345
                }]
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Patient",
                    Some(to_json_body(&patient_4)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_p4: serde_json::Value = serde_json::from_slice(&body)?;
            let p4_id = created_p4["id"]
                .as_str()
                .context("created Patient 4 has id")?
                .to_string();

            // MR query should return only Patient 1.
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/Patient?identifier:of-type={type_system}|{mr_code}|{value_12345}"
                    ),
                    None,
                )
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Patient"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![p1_id]);

            // Invalid :of-type formats (all 3 non-empty parts required).
            for bad in [
                format!("/fhir/Patient?identifier:of-type={type_system}|{mr_code}|"),
                format!("/fhir/Patient?identifier:of-type=|{mr_code}|{value_12345}"),
                format!("/fhir/Patient?identifier:of-type={type_system}||{value_12345}"),
            ] {
                let (status, _headers, _body) = app.request(Method::GET, &bad, None).await?;
                assert_eq!(status, StatusCode::BAD_REQUEST);
            }

            // :of-type must be rejected on non-Identifier token params.
            let (status, _headers, _body) = app
                .request(
                    Method::GET,
                    &format!("/fhir/Patient?gender:of-type={type_system}|{mr_code}|{value_12345}"),
                    None,
                )
                .await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn token_text_modifier_is_prefix_case_insensitive_and_literal() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definition so Condition?code:text=... is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "code",
                "Condition",
                "token",
                "Condition.code",
                &["text"],
            )
            .await?;

            let patient = json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "TextToken" }]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"]
                .as_str()
                .context("created Patient has id")?
                .to_string();

            let mk_condition = |display: &str| {
                json!({
                    "resourceType": "Condition",
                    "subject": { "reference": format!("Patient/{patient_id}") },
                    "code": { "coding": [{ "system": "http://snomed.info/sct", "code": "X", "display": display }] }
                })
            };

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Condition",
                    Some(to_json_body(&mk_condition("Headache finding"))?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_a: serde_json::Value = serde_json::from_slice(&body)?;
            let c_a_id = created_a["id"]
                .as_str()
                .context("created Condition A has id")?
                .to_string();

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Condition",
                    Some(to_json_body(&mk_condition("Acute headache"))?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_b: serde_json::Value = serde_json::from_slice(&body)?;
            let c_b_id = created_b["id"]
                .as_str()
                .context("created Condition B has id")?
                .to_string();

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Condition",
                    Some(to_json_body(&mk_condition("head_ache finding"))?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_c: serde_json::Value = serde_json::from_slice(&body)?;
            let c_c_id = created_c["id"]
                .as_str()
                .context("created Condition C has id")?
                .to_string();

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Condition",
                    Some(to_json_body(&mk_condition("headXache finding"))?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_d: serde_json::Value = serde_json::from_slice(&body)?;
            let c_d_id = created_d["id"]
                .as_str()
                .context("created Condition D has id")?
                .to_string();

            // Prefix, case-insensitive: "headache" matches "Headache …" but not "Acute headache".
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Condition?code:text=headache", None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Condition"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![c_a_id]);

            // Literal matching (escape LIKE meta-chars): "head_" matches "head_…" but not "headX…".
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Condition?code:text=head_", None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Condition"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![c_c_id]);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn token_code_text_and_text_advanced_modifiers_are_parsed_and_applied() -> anyhow::Result<()>
{
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definition so Condition?code:code-text=... is resolvable.
            // Use a CodeableConcept token (with display) so both modifiers are meaningful.
            register_search_parameter(
                &app.state.db_pool,
                "code",
                "Condition",
                "token",
                "Condition.code",
                &["code-text", "text-advanced"],
            )
            .await?;

            let male = json!({
                "resourceType": "Condition",
                "code": { "coding": [{ "system": "http://example.org/codes", "code": "male", "display": "Male Person" }] }
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Condition", Some(to_json_body(&male)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_male: serde_json::Value = serde_json::from_slice(&body)?;
            let male_id = created_male["id"]
                .as_str()
                .context("created male Condition has id")?
                .to_string();

            let female = json!({
                "resourceType": "Condition",
                "code": { "coding": [{ "system": "http://example.org/codes", "code": "female", "display": "Female Person" }] }
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Condition", Some(to_json_body(&female)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_female: serde_json::Value = serde_json::from_slice(&body)?;
            let female_id = created_female["id"]
                .as_str()
                .context("created female Condition has id")?
                .to_string();

            // :code-text matches on the code value (starts-with, case-insensitive).
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Condition?code:code-text=MA", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Condition"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![male_id.clone()]);

            // :text-advanced matches full text on display.
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Condition?code:text-advanced=Female", None)
                .await?;
            assert_eq!(status, StatusCode::OK);
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Condition"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![female_id]);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn reference_contains_is_rejected_for_non_hierarchical_reference_params() -> anyhow::Result<()>
{
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definition so Observation?subject:contains=... is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "subject",
                "Observation",
                "reference",
                "Observation.subject",
                &["contains"],
            )
            .await?;

            let (status, _headers, _body) = app
                .request(
                    Method::GET,
                    "/fhir/Observation?subject:contains=Patient/123",
                    None,
                )
                .await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn reference_text_modifier_is_prefix_case_insensitive_and_literal() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definition so Observation?subject:text=... is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "subject",
                "Observation",
                "reference",
                "Observation.subject",
                &["text"],
            )
            .await?;

            let patient = json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "TextRef" }]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&patient)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_patient: serde_json::Value = serde_json::from_slice(&body)?;
            let patient_id = created_patient["id"]
                .as_str()
                .context("created Patient has id")?
                .to_string();

            let mk_obs = |display: &str| {
                json!({
                    "resourceType": "Observation",
                    "status": "final",
                    "code": { "text": "text-ref-test" },
                    "subject": { "reference": format!("Patient/{patient_id}"), "display": display }
                })
            };

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&mk_obs("Bob"))?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_a: serde_json::Value = serde_json::from_slice(&body)?;
            let o_a_id = created_a["id"]
                .as_str()
                .context("created Observation A has id")?
                .to_string();

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&mk_obs("Alice Bob"))?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_b: serde_json::Value = serde_json::from_slice(&body)?;
            let o_b_id = created_b["id"]
                .as_str()
                .context("created Observation B has id")?
                .to_string();

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&mk_obs("bob_thing"))?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_c: serde_json::Value = serde_json::from_slice(&body)?;
            let o_c_id = created_c["id"]
                .as_str()
                .context("created Observation C has id")?
                .to_string();

            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&mk_obs("bobXthing"))?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_d: serde_json::Value = serde_json::from_slice(&body)?;
            let o_d_id = created_d["id"]
                .as_str()
                .context("created Observation D has id")?
                .to_string();

            // Prefix, case-insensitive: "bob" matches "Bob…" but not "Alice Bob".
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Observation?subject:text=bob", None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let mut ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Observation"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            ids.sort();
            let mut expected = vec![o_a_id.clone(), o_c_id.clone(), o_d_id.clone()];
            expected.sort();
            assert_eq!(ids, expected);
            assert_ne!(o_a_id, o_b_id);

            // Literal matching (escape LIKE meta-chars): "bob_" matches "bob_…" but not "bobX…".
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Observation?subject:text=bob_", None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Observation"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![o_c_id]);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn reference_type_modifier_requires_id_only_and_filters_target_type() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal SearchParameter definition so Observation?subject:Patient=... is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "subject",
                "Observation",
                "reference",
                "Observation.subject",
                &["missing", "Patient"],
            )
            .await?;

            // Create a Patient with a known id via update-as-create.
            let patient = json!({
                "resourceType": "Patient",
                "id": "23",
                "active": true,
                "name": [{ "family": "TypeModifier" }]
            });
            let (status, _headers, _body) = app
                .request(
                    Method::PUT,
                    "/fhir/Patient/23",
                    Some(to_json_body(&patient)?),
                )
                .await?;
            assert!(
                status == StatusCode::OK || status == StatusCode::CREATED,
                "expected PUT-as-create to succeed, got {status}"
            );

            let observation = json!({
                "resourceType": "Observation",
                "status": "final",
                "code": { "text": "type-modifier-test" },
                "subject": { "reference": "Patient/23" }
            });
            let (status, _headers, body) = app
                .request(
                    Method::POST,
                    "/fhir/Observation",
                    Some(to_json_body(&observation)?),
                )
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_obs: serde_json::Value = serde_json::from_slice(&body)?;
            let obs_id = created_obs["id"]
                .as_str()
                .context("created Observation has id")?
                .to_string();

            // subject:Patient=23 matches (equivalent to subject=Patient/23).
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Observation?subject:Patient=23", None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Observation"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![obs_id.clone()]);

            // Value must be id-only (reject typed or absolute reference formats).
            for bad in [
                "/fhir/Observation?subject:Patient=Patient/23",
                "/fhir/Observation?subject:Patient=http://example.org/fhir/Patient/23",
                "/fhir/Observation?subject:Patient=%2323",
            ] {
                let (status, _headers, _body) = app.request(Method::GET, bad, None).await?;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
            }

            // Type modifier filters by target_type.
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Observation?subject:Encounter=23", None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            assert_eq!(entries.len(), 0);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn sort_parameter_is_singleton_and_sorts_strings_case_insensitive() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Repeated _sort is an error (spec says behavior undefined; we treat as error).
            let (status, _headers, _body) = app
                .request(
                    Method::GET,
                    "/fhir/Patient?_sort=_id&_sort=_lastUpdated",
                    None,
                )
                .await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            // Minimal SearchParameter definition so Patient?_sort=name is resolvable.
            register_search_parameter(
                &app.state.db_pool,
                "name",
                "Patient",
                "string",
                "Patient.name",
                &["missing"],
            )
            .await?;

            let p_a = serde_json::json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "zulu" }]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&p_a)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_a: serde_json::Value = serde_json::from_slice(&body)?;
            let p_a_id = created_a["id"]
                .as_str()
                .context("created Patient A has id")?
                .to_string();

            let p_b = serde_json::json!({
                "resourceType": "Patient",
                "active": true,
                "name": [{ "family": "Alpha" }]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/Patient", Some(to_json_body(&p_b)?))
                .await?;
            assert_eq!(status, StatusCode::CREATED);
            let created_b: serde_json::Value = serde_json::from_slice(&body)?;
            let p_b_id = created_b["id"]
                .as_str()
                .context("created Patient B has id")?
                .to_string();

            // Case-insensitive ascending sort on a string param.
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Patient?_sort=name", None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let bundle: serde_json::Value = serde_json::from_slice(&body)?;
            let entries = bundle
                .get("entry")
                .and_then(|v| v.as_array())
                .context("Bundle.entry is array")?;
            let ids = entries
                .iter()
                .filter_map(|e| e.get("resource"))
                .filter(|r| r.get("resourceType").and_then(|v| v.as_str()) == Some("Patient"))
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect::<Vec<_>>();

            let pos_a = ids.iter().position(|id| id == &p_a_id).unwrap();
            let pos_b = ids.iter().position(|id| id == &p_b_id).unwrap();
            assert!(pos_b < pos_a, "expected Alpha before zulu");

            Ok(())
        })
    })
    .await
}
