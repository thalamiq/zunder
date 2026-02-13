#![allow(unused)]
#[allow(unused)]
mod support;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use support::*;

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
async fn terminology_operations_smoke_test() -> anyhow::Result<()> {
    with_test_app(|app| {
        Box::pin(async move {
            // Minimal OperationDefinitions required by the operation router/registry.
            create_operation_definition(
                &app,
                json!({
                    "resourceType": "OperationDefinition",
                    "status": "active",
                    "kind": "operation",
                    "code": "expand",
                    "resource": ["ValueSet"],
                    "system": false,
                    "type": true,
                    "instance": true,
                    "affectsState": false
                }),
            )
            .await?;

            create_operation_definition(
                &app,
                json!({
                    "resourceType": "OperationDefinition",
                    "status": "active",
                    "kind": "operation",
                    "code": "validate-code",
                    "resource": ["ValueSet"],
                    "system": false,
                    "type": true,
                    "instance": true,
                    "affectsState": false
                }),
            )
            .await?;

            create_operation_definition(
                &app,
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

            create_operation_definition(
                &app,
                json!({
                    "resourceType": "OperationDefinition",
                    "status": "active",
                    "kind": "operation",
                    "code": "subsumes",
                    "resource": ["CodeSystem"],
                    "system": false,
                    "type": true,
                    "instance": true,
                    "affectsState": false
                }),
            )
            .await?;

            create_operation_definition(
                &app,
                json!({
                    "resourceType": "OperationDefinition",
                    "status": "active",
                    "kind": "operation",
                    "code": "translate",
                    "resource": ["ConceptMap"],
                    "system": false,
                    "type": true,
                    "instance": true,
                    "affectsState": false
                }),
            )
            .await?;

            create_operation_definition(
                &app,
                json!({
                    "resourceType": "OperationDefinition",
                    "status": "active",
                    "kind": "operation",
                    "code": "closure",
                    "resource": ["ConceptMap"],
                    "system": true,
                    "type": false,
                    "instance": false,
                    "affectsState": true,
                    "parameter": [{
                        "name": "name",
                        "use": "in",
                        "min": 1,
                        "max": "1",
                        "type": "string"
                    }]
                }),
            )
            .await?;

            // Create small CodeSystem + ValueSet + ConceptMap.
            let cs = json!({
                "resourceType": "CodeSystem",
                "url": "http://example.org/CodeSystem/test",
                "status": "active",
                "content": "complete",
                "concept": [
                    { "code": "parent", "display": "Parent", "concept": [ { "code": "child", "display": "Child" } ] },
                    { "code": "a", "display": "A" }
                ]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/CodeSystem", Some(to_json_body(&cs)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create CodeSystem");
            let cs_created: Value = serde_json::from_slice(&body)?;
            let cs_id = cs_created["id"].as_str().unwrap().to_string();

            let vs = json!({
                "resourceType": "ValueSet",
                "url": "http://example.org/ValueSet/test",
                "status": "active",
                "compose": {
                    "include": [{
                        "system": "http://example.org/CodeSystem/test",
                        "concept": [
                            { "code": "a", "display": "A" },
                            { "code": "child", "display": "Child" }
                        ]
                    }]
                }
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/ValueSet", Some(to_json_body(&vs)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create ValueSet");
            let vs_created: Value = serde_json::from_slice(&body)?;
            let vs_id = vs_created["id"].as_str().unwrap().to_string();

            let cm = json!({
                "resourceType": "ConceptMap",
                "url": "http://example.org/ConceptMap/test",
                "status": "active",
                "group": [{
                    "source": "http://example.org/CodeSystem/test",
                    "target": "http://example.org/CodeSystem/target",
                    "element": [{
                        "code": "a",
                        "target": [{ "code": "x", "equivalence": "equivalent" }]
                    }]
                }]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/ConceptMap", Some(to_json_body(&cm)?))
                .await?;
            assert_status(status, StatusCode::CREATED, "create ConceptMap");
            let cm_created: Value = serde_json::from_slice(&body)?;
            let cm_id = cm_created["id"].as_str().unwrap().to_string();

            // Load operation definitions into the registry.
            app.state.operation_registry.load_definitions().await?;

            // $expand
            let (status, _headers, body) = app
                .request(Method::GET, &format!("/fhir/ValueSet/{}/$expand?count=10", vs_id), None)
                .await?;
            assert_status(status, StatusCode::OK, "$expand");
            let expanded: Value = serde_json::from_slice(&body)?;
            assert_eq!(expanded["resourceType"], "ValueSet");
            assert!(expanded["expansion"]["contains"].as_array().unwrap().len() >= 2);

            // $validate-code
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/ValueSet/{}/$validate-code?system=http://example.org/CodeSystem/test&code=a",
                        vs_id
                    ),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "$validate-code");
            let validated: Value = serde_json::from_slice(&body)?;
            assert_eq!(validated["resourceType"], "Parameters");
            assert!(
                validated["parameter"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["name"] == "result" && p.get("valueBoolean") == Some(&Value::Bool(true))),
                "expected result=true"
            );

            // $lookup
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    "/fhir/CodeSystem/$lookup?system=http://example.org/CodeSystem/test&code=a",
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "$lookup");
            let looked_up: Value = serde_json::from_slice(&body)?;
            assert_eq!(looked_up["resourceType"], "Parameters");

            // $subsumes (instance-level)
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!("/fhir/CodeSystem/{}/$subsumes?codeA=parent&codeB=child", cs_id),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "$subsumes");
            let subsumed: Value = serde_json::from_slice(&body)?;
            assert!(
                subsumed["parameter"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["name"] == "outcome" && p.get("valueCode") == Some(&Value::String("subsumes".to_string()))),
                "expected outcome=subsumes"
            );

            // $translate (instance-level)
            let (status, _headers, body) = app
                .request(
                    Method::GET,
                    &format!(
                        "/fhir/ConceptMap/{}/$translate?system=http://example.org/CodeSystem/test&code=a",
                        cm_id
                    ),
                    None,
                )
                .await?;
            assert_status(status, StatusCode::OK, "$translate");
            let translated: Value = serde_json::from_slice(&body)?;
            assert_eq!(translated["resourceType"], "Parameters");

            // $closure (system-level, POST only)
            let closure_req = json!({
                "resourceType": "Parameters",
                "parameter": [
                    { "name": "name", "valueString": "t1" },
                    { "name": "concept", "valueCoding": { "system": "http://example.org/CodeSystem/test", "code": "parent" } }
                ]
            });
            let (status, _headers, body) = app
                .request(Method::POST, "/fhir/$closure", Some(to_json_body(&closure_req)?))
                .await?;
            assert_status(status, StatusCode::OK, "$closure");
            let closure_cm: Value = serde_json::from_slice(&body)?;
            assert_eq!(closure_cm["resourceType"], "ConceptMap");

            Ok(())
        })
    })
    .await
}
