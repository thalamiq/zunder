#[path = "../test_support/mod.rs"]
mod test_support;

#[test]
fn test_as_quantity() {
    use serde_json::json;
    use ferrum_fhirpath::{Context, Value};

    let obs = json!({
      "resourceType": "Observation",
      "valueQuantity": {
        "value": 185,
        "unit": "lbs",
        "system": "http://unitsofmeasure.org",
        "code": "[lb_av]"
      }
    });

    let engine = test_support::engine_r5();
    let resource = Value::from_json(obs);
    let ctx = Context::new(resource);

    // Test parts
    let r1 = engine
        .evaluate_expr("Observation.valueQuantity", &ctx, None)
        .unwrap();
    println!("valueQuantity: {} items", r1.len());

    let r2 = engine
        .evaluate_expr("Observation.value", &ctx, None)
        .unwrap();
    println!("value: {} items", r2.len());

    let r3 = engine
        .evaluate_expr("Observation.value is Quantity", &ctx, None)
        .unwrap();
    println!("value is Quantity: {:?}", r3.as_boolean().ok());

    let r4 = engine
        .evaluate_expr("Observation.value.as(Quantity)", &ctx, None)
        .unwrap();
    println!("value.as(Quantity): {} items", r4.len());

    if !r4.is_empty() {
        let r5 = engine
            .evaluate_expr("Observation.value.as(Quantity).unit", &ctx, None)
            .unwrap();
        if !r5.is_empty() {
            println!("unit: {:?}", r5.as_string().ok());
        } else {
            println!("unit: empty");
        }
    }

    assert!(!r4.is_empty(), "as(Quantity) should return the item");
}

#[test]
fn test_as_errors_on_multi_item_collection() {
    use serde_json::json;
    use ferrum_fhirpath::{Context, Value};

    // Per FHIRPath spec, as() requires a singleton collection.
    // Use ofType() for multi-item type filtering.
    let obs = json!({
        "resourceType": "Observation",
        "component": [
            {
                "code": {"text": "Systolic BP"},
                "valueQuantity": {
                    "value": 120,
                    "unit": "mmHg",
                    "system": "http://unitsofmeasure.org",
                    "code": "mm[Hg]"
                }
            },
            {
                "code": {"text": "Diastolic BP"},
                "valueQuantity": {
                    "value": 80,
                    "unit": "mmHg",
                    "system": "http://unitsofmeasure.org",
                    "code": "mm[Hg]"
                }
            },
            {
                "code": {"text": "Comment"},
                "valueString": "Normal reading"
            }
        ]
    });

    let engine = test_support::engine_r5();
    let resource = Value::from_json(obs);
    let ctx = Context::new(resource);

    // as() on multi-item collection returns empty (per FHIRPath spec)
    let result = engine
        .evaluate_expr("Observation.component.value.as(Quantity)", &ctx, None)
        .unwrap();
    assert!(
        result.is_empty(),
        "as() on multi-item collection should return empty"
    );

    // ofType() is the correct function for multi-item type filtering
    let result = engine
        .evaluate_expr("Observation.component.value.ofType(Quantity)", &ctx, None)
        .unwrap();
    assert_eq!(
        result.len(),
        2,
        "ofType(Quantity) should filter and return 2 Quantity items"
    );

    let units = engine
        .evaluate_expr(
            "Observation.component.value.ofType(Quantity).unit",
            &ctx,
            None,
        )
        .unwrap();
    assert_eq!(units.len(), 2, "Should get units from both quantities");
}

#[test]
fn test_ext1_choice_type_navigation() {
    use serde_json::json;
    use ferrum_fhirpath::{Context, Value};
    use std::sync::Arc;

    // Simulate ext-1 constraint evaluation on an Extension object
    // ext-1 expression: extension.exists() != value.exists()
    let resource = json!({
        "resourceType": "CodeSystem",
        "extension": [
            {"url": "http://example.com/ext", "valueCode": "trial-use"}
        ]
    });

    let engine = test_support::engine_r5();

    // Test 1: Navigate from root to extension[0].value via from_json_at (mimics constraint evaluator)
    let root = Arc::new(resource);
    let ext_value = Value::from_json_at(root.clone(), &["extension"], Some(0));
    let ctx = Context::new(ext_value);

    // "value.exists()" should find "valueCode" via choice type matching
    let result = engine.evaluate_expr("value.exists()", &ctx, None).unwrap();
    assert_eq!(
        result.as_boolean().ok(),
        Some(true),
        "value.exists() should find valueCode via choice type"
    );

    // "extension.exists()" should be false (no nested extensions)
    let result = engine.evaluate_expr("extension.exists()", &ctx, None).unwrap();
    assert_eq!(
        result.as_boolean().ok(),
        Some(false),
        "extension.exists() should be false"
    );

    // ext-1: extension.exists() != value.exists() → false != true → true (passes)
    let result = engine.evaluate_expr("extension.exists() != value.exists()", &ctx, None).unwrap();
    assert_eq!(
        result.as_boolean().ok(),
        Some(true),
        "ext-1 should pass for extension with value"
    );
}
