#![allow(unused)]
#[allow(unused)]
mod support;

use anyhow::Context as _;
use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use support::*;

fn parse_json(body: &[u8]) -> anyhow::Result<Value> {
    Ok(serde_json::from_slice(body)?)
}

async fn create_search_parameter(app: &TestApp, sp: Value) -> anyhow::Result<()> {
    let (status, _headers, body) = app
        .request(
            Method::POST,
            "/fhir/SearchParameter",
            Some(to_json_body(&sp)?),
        )
        .await?;
    if status != StatusCode::CREATED {
        eprintln!("{}", String::from_utf8_lossy(&body));
    }
    assert_status(status, StatusCode::CREATED, "create SearchParameter");
    Ok(())
}

/// Ensures common meta parameters work when defined at base Resource level, matching the
/// typical core package model (base = Resource/DomainResource, expression begins with that base).
#[tokio::test]
async fn common_meta_params_work_from_resource_base_definitions() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Define standard meta params at Resource base, using expressions with Resource prefix.
            // SearchParameterHook should simplify these to be evaluable during indexing.
            create_search_parameter(
                app,
                json!({
                    "resourceType": "SearchParameter",
                    "status": "active",
                    "code": "_language",
                    "base": ["Resource"],
                    "type": "token",
                    "expression": "Resource.language"
                }),
            )
            .await?;
            create_search_parameter(
                app,
                json!({
                    "resourceType": "SearchParameter",
                    "status": "active",
                    "code": "_source",
                    "base": ["Resource"],
                    "type": "uri",
                    "expression": "Resource.meta.source"
                }),
            )
            .await?;
            create_search_parameter(
                app,
                json!({
                    "resourceType": "SearchParameter",
                    "status": "active",
                    "code": "_security",
                    "base": ["Resource"],
                    "type": "token",
                    "expression": "Resource.meta.security"
                }),
            )
            .await?;
            create_search_parameter(
                app,
                json!({
                    "resourceType": "SearchParameter",
                    "status": "active",
                    "code": "_tag",
                    "base": ["Resource"],
                    "type": "token",
                    "expression": "Resource.meta.tag"
                }),
            )
            .await?;
            create_search_parameter(
                app,
                json!({
                    "resourceType": "SearchParameter",
                    "status": "active",
                    "code": "_profile",
                    "base": ["Resource"],
                    "type": "reference",
                    "expression": "Resource.meta.profile"
                }),
            )
            .await?;

            // Patient with meta fields.
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
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let id = created["id"]
                .as_str()
                .context("created Patient has id")?
                .to_string();

            // Combined search should match.
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
            assert_status(status, StatusCode::OK, "combined search");
            let bundle = parse_json(&body)?;
            let entries = bundle["entry"].as_array().context("Bundle.entry array")?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn tag_and_security_token_semantics_match_system_only_and_code_only() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            create_search_parameter(
                app,
                json!({
                    "resourceType": "SearchParameter",
                    "status": "active",
                    "code": "_tag",
                    "base": ["Resource"],
                    "type": "token",
                    "expression": "Resource.meta.tag"
                }),
            )
            .await?;
            create_search_parameter(
                app,
                json!({
                    "resourceType": "SearchParameter",
                    "status": "active",
                    "code": "_security",
                    "base": ["Resource"],
                    "type": "token",
                    "expression": "Resource.meta.security"
                }),
            )
            .await?;

            let patient = json!({
                "resourceType": "Patient",
                "meta": {
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
            assert_status(status, StatusCode::CREATED, "create Patient");
            let created = parse_json(&body)?;
            let id = created["id"].as_str().unwrap().to_string();

            // Code-only token: matches any system with the code.
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Patient?_tag=H", None)
                .await?;
            assert_status(status, StatusCode::OK, "_tag code-only");
            let bundle = parse_json(&body)?;
            let entries = bundle["entry"].as_array().unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            // System-only token: matches any code within that system.
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    "/fhir/Patient?_tag=http://terminology.hl7.org/ValueSet/v3-SeverityObservation|",
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "_tag system-only");
            let bundle = parse_json(&body)?;
            let entries = bundle["entry"].as_array().unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            // _security code-only + system-only
            let (status, _headers, body) = app
                .request(Method::GET, "/fhir/Patient?_security=R", None)
                .await?;
            assert_status(status, StatusCode::OK, "_security code-only");
            let bundle = parse_json(&body)?;
            let entries = bundle["entry"].as_array().unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    "/fhir/Patient?_security=http://terminology.hl7.org/CodeSystem/v3-Confidentiality|",
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "_security system-only");
            let bundle = parse_json(&body)?;
            let entries = bundle["entry"].as_array().unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["resource"]["id"], id);

            Ok(())
        })
    })
    .await
}
