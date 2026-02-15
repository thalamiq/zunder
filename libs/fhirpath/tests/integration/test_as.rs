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

    // as() on multi-item collection should error
    let result = engine.evaluate_expr("Observation.component.value.as(Quantity)", &ctx, None);
    assert!(result.is_err(), "as() should error on multi-item collection");

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
