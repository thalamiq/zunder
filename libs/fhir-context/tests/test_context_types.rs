//! Verifies that the FhirContext resolves all core FHIR types.
//! Catches incomplete package caches or broken type indexing.

use ferrum_context::{DefaultFhirContext, FhirContext};
use std::sync::{Arc, OnceLock};
use tokio::runtime::Runtime;

fn context_r5() -> &'static Arc<DefaultFhirContext> {
    static CTX: OnceLock<Arc<DefaultFhirContext>> = OnceLock::new();
    CTX.get_or_init(|| {
        let rt = Runtime::new().unwrap();
        Arc::new(
            rt.block_on(DefaultFhirContext::from_fhir_version_async(None, "R5"))
                .expect("Failed to create R5 context"),
        )
    })
}

fn assert_type_resolves(ctx: &dyn FhirContext, name: &str) {
    let sd = ctx
        .get_core_structure_definition_by_type(name)
        .unwrap_or_else(|e| panic!("Error resolving '{}': {}", name, e));
    assert!(sd.is_some(), "Type '{}' not found in context", name);
}

#[test]
#[ignore] // requires cached R5 package
fn context_resolves_primitive_types() {
    let ctx = context_r5();
    for name in [
        "boolean", "integer", "string", "decimal", "uri", "url", "canonical", "base64Binary",
        "instant", "date", "dateTime", "time", "code", "oid", "id", "markdown", "unsignedInt",
        "positiveInt", "uuid",
    ] {
        assert_type_resolves(ctx.as_ref(), name);
    }
}

#[test]
#[ignore] // requires cached R5 package
fn context_resolves_complex_types() {
    let ctx = context_r5();
    for name in [
        "Address", "Age", "Annotation", "Attachment", "CodeableConcept", "Coding",
        "ContactPoint", "Count", "Distance", "Dosage", "Duration", "HumanName", "Identifier",
        "Money", "Period", "Quantity", "Range", "Ratio", "Reference", "SampledData", "Signature",
        "Timing", "Meta", "Narrative", "Extension",
    ] {
        assert_type_resolves(ctx.as_ref(), name);
    }
}

#[test]
#[ignore] // requires cached R5 package
fn context_resolves_resource_types() {
    let ctx = context_r5();
    for name in [
        "Patient", "Observation", "Encounter", "Condition", "Procedure", "DiagnosticReport",
        "MedicationRequest", "Bundle", "Questionnaire", "ValueSet", "CodeSystem",
        "StructureDefinition", "OperationDefinition",
    ] {
        assert_type_resolves(ctx.as_ref(), name);
    }
}
