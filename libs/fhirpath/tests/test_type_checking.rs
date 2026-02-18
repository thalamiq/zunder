//! Comprehensive type checking tests for is(), as(), and ofType()
//!
//! Tests focus on what was actually fixed:
//! - Polymorphic field type matching (value[x], onset[x])
//! - Structural heuristics for complex datatypes
//! - Type checking priority: Runtime → Path hints → Declared types

use serde_json::json;
use ferrum_fhirpath::{Context, Value};

mod test_support;

#[test]
fn test_polymorphic_field_quantity_vs_period() {
    // This was the main bug: Observation.value.is(Period) returned true even when value was Quantity
    let engine = test_support::engine_r5();

    let obs_json = json!({
        "resourceType": "Observation",
        "status": "final",
        "code": {
            "coding": [{
                "system": "http://loinc.org",
                "code": "15074-8"
            }]
        },
        "valueQuantity": {
            "value": 140,
            "unit": "mg/dL",
            "system": "http://unitsofmeasure.org",
            "code": "mg/dL"
        }
    });
    let obs = Value::from_json(obs_json);
    let ctx = Context::new(obs);

    // Should match Quantity (runtime type)
    let result = engine
        .evaluate_expr("Observation.value.is(Quantity)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "valueQuantity should match Quantity"
    );

    // Should NOT match Period (even though Period is a declared possible type)
    let result = engine
        .evaluate_expr("Observation.value.is(Period)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "valueQuantity should not match Period"
    );

    // Should NOT match string
    let result = engine
        .evaluate_expr("Observation.value.is(string)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "valueQuantity should not match string"
    );
}

#[test]
fn test_polymorphic_field_period_vs_quantity() {
    let engine = test_support::engine_r5();

    let condition_json = json!({
        "resourceType": "Condition",
        "clinicalStatus": {
            "coding": [{
                "system": "http://terminology.hl7.org/CodeSystem/condition-clinical",
                "code": "active"
            }]
        },
        "code": {
            "coding": [{
                "system": "http://snomed.info/sct",
                "code": "386661006"
            }]
        },
        "subject": {
            "reference": "Patient/example"
        },
        "onsetPeriod": {
            "start": "2020-01-01",
            "end": "2020-12-31"
        }
    });
    let condition = Value::from_json(condition_json);
    let ctx = Context::new(condition);

    // Should match Period (runtime type)
    let result = engine
        .evaluate_expr("Condition.onset.is(Period)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "onsetPeriod should match Period"
    );

    // Should NOT match Quantity
    let result = engine
        .evaluate_expr("Condition.onset.is(Quantity)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "onsetPeriod should not match Quantity"
    );

    // Should NOT match string or dateTime
    let result = engine
        .evaluate_expr("Condition.onset.is(string)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "onsetPeriod should not match string"
    );
}

#[test]
fn test_polymorphic_field_string() {
    let engine = test_support::engine_r5();

    let obs_json = json!({
        "resourceType": "Observation",
        "status": "final",
        "code": {
            "coding": [{
                "system": "http://loinc.org",
                "code": "8867-4"
            }]
        },
        "valueString": "Positive"
    });
    let obs = Value::from_json(obs_json);
    let ctx = Context::new(obs);

    let result = engine
        .evaluate_expr("Observation.value.is(string)", &ctx, None)
        .unwrap();
    assert!(result.as_boolean().unwrap());

    let result = engine
        .evaluate_expr("Observation.value.is(FHIR.string)", &ctx, None)
        .unwrap();
    assert!(result.as_boolean().unwrap());

    let result = engine
        .evaluate_expr("Observation.value.is(String)", &ctx, None)
        .unwrap();
    assert!(!result.as_boolean().unwrap());

    let result = engine
        .evaluate_expr("Observation.value.is(System.String)", &ctx, None)
        .unwrap();
    assert!(!result.as_boolean().unwrap());

    // Should NOT match complex types
    let result = engine
        .evaluate_expr("Observation.value.is(Quantity)", &ctx, None)
        .unwrap();
    assert!(!result.as_boolean().unwrap());

    let result = engine
        .evaluate_expr("Observation.value.is(FHIR.Quantity)", &ctx, None)
        .unwrap();
    assert!(!result.as_boolean().unwrap());

    let result = engine
        .evaluate_expr("Observation.value.is(Period)", &ctx, None)
        .unwrap();
    assert!(!result.as_boolean().unwrap());

    let result = engine
        .evaluate_expr("Observation.value.type().name", &ctx, None)
        .unwrap();
    assert_eq!(result.as_string().unwrap().as_ref(), "string");
}

#[test]
fn test_structural_detection_quantity() {
    // Quantity should be detected by structure (value + unit/code/system)
    let engine = test_support::engine_r5();

    let quantity_json = json!({
        "value": 25,
        "unit": "years",
        "system": "http://unitsofmeasure.org",
        "code": "a"
    });
    let quantity = Value::from_json(quantity_json);
    let ctx = Context::new(quantity);

    let result = engine
        .evaluate_expr("is(Quantity)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "Object with value+unit should match Quantity"
    );
}

#[test]
fn test_structural_detection_period() {
    // Period should be detected by structure (start and/or end)
    let engine = test_support::engine_r5();

    let period_json = json!({
        "start": "2020-01-01",
        "end": "2020-12-31"
    });
    let period = Value::from_json(period_json);
    let ctx = Context::new(period);

    let result = engine
        .evaluate_expr("is(Period)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "Object with start/end should match Period"
    );
}

#[test]
fn test_structural_detection_coding() {
    // Coding should be detected by structure (system + code)
    let engine = test_support::engine_r5();

    let coding_json = json!({
        "system": "http://loinc.org",
        "code": "15074-8",
        "display": "Glucose"
    });
    let coding = Value::from_json(coding_json);
    let ctx = Context::new(coding);

    let result = engine
        .evaluate_expr("is(Coding)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "Object with system+code should match Coding"
    );
}

#[test]
fn test_structural_detection_identifier_vs_coding() {
    // Identifier has value but NOT code (to distinguish from Coding)
    let engine = test_support::engine_r5();

    let identifier_json = json!({
        "system": "http://example.org/ids",
        "value": "12345"
    });
    let identifier = Value::from_json(identifier_json);
    let ctx = Context::new(identifier);

    let result = engine
        .evaluate_expr("is(Identifier)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "Object with value (no code) should match Identifier"
    );

    let result = engine
        .evaluate_expr("identifier.is(Coding)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "Identifier should not match Coding (missing code field)"
    );
}

#[test]
fn test_oftype_filters_polymorphic_collection() {
    // ofType() should filter based on runtime type, not declared types
    let engine = test_support::engine_r5();

    let bundle_json = json!({
        "resourceType": "Bundle",
        "type": "collection",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": "p1"
                }
            },
            {
                "resource": {
                    "resourceType": "Observation",
                    "id": "o1",
                    "status": "final",
                    "code": {
                        "coding": [{
                            "system": "http://loinc.org",
                            "code": "15074-8"
                        }]
                    }
                }
            },
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": "p2"
                }
            }
        ]
    });
    let bundle = Value::from_json(bundle_json);
    let ctx = Context::new(bundle);

    // Filter for just Patient resources
    let result = engine
        .evaluate_expr("Bundle.entry.resource.ofType(Patient).count()", &ctx, None)
        .unwrap();
    assert_eq!(
        result.as_integer().unwrap(),
        2,
        "Should find 2 Patient resources"
    );

    // Filter for just Observation resources
    let result = engine
        .evaluate_expr(
            "Bundle.entry.resource.ofType(Observation).count()",
            &ctx,
            None,
        )
        .unwrap();
    assert_eq!(
        result.as_integer().unwrap(),
        1,
        "Should find 1 Observation resource"
    );
}

#[test]
fn test_as_function_with_type_conversion() {
    // as() should return the value if it matches the type, empty otherwise
    let engine = test_support::engine_r5();

    let obs_json = json!({
        "resourceType": "Observation",
        "status": "final",
        "code": {
            "coding": [{
                "system": "http://loinc.org",
                "code": "15074-8"
            }]
        },
        "valueQuantity": {
            "value": 140,
            "unit": "mg/dL"
        }
    });
    let obs = Value::from_json(obs_json);
    let ctx = Context::new(obs);

    // as() should return the value if it matches the type
    let result = engine
        .evaluate_expr("Observation.value.as(Quantity)", &ctx, None)
        .unwrap();
    assert_eq!(
        result.len(),
        1,
        "as(Quantity) should return the Quantity value"
    );

    // as() should return empty if it doesn't match
    let result = engine
        .evaluate_expr("Observation.value.as(Period)", &ctx, None)
        .unwrap();
    assert_eq!(
        result.len(),
        0,
        "as(Period) should return empty for Quantity value"
    );
}

#[test]
fn test_resource_type_checking() {
    // Resources should be identifiable by resourceType field
    let engine = test_support::engine_r5();

    let patient_json = json!({
        "resourceType": "Patient",
        "id": "example",
        "active": true
    });
    let patient = Value::from_json(patient_json);
    let ctx = Context::new(patient);

    let result = engine
        .evaluate_expr("Patient.is(Patient)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "Patient resource should match Patient type"
    );

    let result = engine
        .evaluate_expr("Patient.is(Observation)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "Patient should not match Observation type"
    );
}

#[test]
fn test_nested_complex_type() {
    // Complex types nested in resources should be type-checked correctly
    let engine = test_support::engine_r5();

    let patient_json = json!({
        "resourceType": "Patient",
        "id": "example",
        "name": [{
            "family": "Smith",
            "given": ["John"]
        }]
    });
    let patient = Value::from_json(patient_json);
    let ctx = Context::new(patient);

    let result = engine
        .evaluate_expr("Patient.name.is(HumanName)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "Patient.name should match HumanName"
    );

    let result = engine
        .evaluate_expr("Patient.name.is(Coding)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "HumanName should not match Coding"
    );
}

#[test]
fn test_quantity_specialization_age() {
    // Age inherits from Quantity
    let engine = test_support::engine_r5();

    let patient_json = json!({
        "resourceType": "Patient",
        "id": "example",
        "deceasedAge": {
            "value": 85,
            "unit": "years",
            "system": "http://unitsofmeasure.org",
            "code": "a"
        }
    });
    let patient = Value::from_json(patient_json);
    let ctx = Context::new(patient);

    // Age should match Quantity (inheritance)
    let result = engine
        .evaluate_expr("Patient.deceased.is(Quantity)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "Age should inherit from Quantity"
    );
}

#[test]
fn test_priority_runtime_over_declared() {
    // Runtime type should take priority over declared possible types
    // This is the core fix: value[x] has many declared types, but we should check actual runtime type first
    let engine = test_support::engine_r5();

    let obs_json = json!({
        "resourceType": "Observation",
        "status": "final",
        "code": {
            "coding": [{
                "system": "http://loinc.org",
                "code": "15074-8"
            }]
        },
        "valueCodeableConcept": {
            "coding": [{
                "system": "http://snomed.info/sct",
                "code": "168800009",
                "display": "Positive"
            }]
        }
    });
    let obs = Value::from_json(obs_json);
    let ctx = Context::new(obs);

    // Should match CodeableConcept (actual runtime type)
    let result = engine
        .evaluate_expr("Observation.value.is(CodeableConcept)", &ctx, None)
        .unwrap();
    assert!(
        result.as_boolean().unwrap(),
        "Should match actual runtime type"
    );

    // Should NOT match other declared types like Quantity, Period, string, etc.
    let result = engine
        .evaluate_expr("Observation.value.is(Quantity)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "Should not match other declared types"
    );

    let result = engine
        .evaluate_expr("Observation.value.is(string)", &ctx, None)
        .unwrap();
    assert!(
        !result.as_boolean().unwrap(),
        "Should not match other declared types"
    );
}
