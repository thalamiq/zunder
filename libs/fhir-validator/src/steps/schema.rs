//! Schema validation against base FHIR StructureDefinitions
//!
//! Validates resource structure against the **base** resource type definition (e.g., core "Patient"):
//! - Correct resourceType
//! - Required elements present
//! - Cardinality constraints (min/max)
//! - Data type correctness
//! - Unknown elements (if disallowed)
//! - Modifier extensions (if disallowed)
//!
//! **Note**: This step validates against base StructureDefinitions only, not profiles.
//! Profile validation (including slicing) is handled by the Profiles step.

use crate::validator::{IssueCode, ValidationIssue};
use crate::SchemaPlan;
use serde_json::Value;
use std::collections::HashMap;
use ferrum_context::FhirContext;
use ferrum_snapshot::{ElementDefinition, ExpandedFhirContext};

/// Validates a resource against its base StructureDefinition (core FHIR resource type)
pub fn validate_schema<C: FhirContext>(
    resource: &Value,
    plan: &SchemaPlan,
    context: &C,
    issues: &mut Vec<ValidationIssue>,
) {
    // Extract resourceType
    let resource_type = match get_resource_type(resource) {
        Some(rt) => rt,
        None => {
            issues.push(
                ValidationIssue::error(
                    IssueCode::Required,
                    "Resource must have a 'resourceType' field".to_string(),
                )
                .with_location("Resource".to_string()),
            );
            return;
        }
    };

    // Construct base StructureDefinition URL (e.g., http://hl7.org/fhir/StructureDefinition/Patient)
    let base_profile_url = format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type);

    // Get base StructureDefinition
    let structure_def = match context.get_structure_definition(&base_profile_url) {
        Ok(Some(sd)) => sd,
        Ok(None) => {
            issues.push(
                ValidationIssue::error(
                    IssueCode::NotFound,
                    format!(
                        "Base StructureDefinition not found for resource type '{}'",
                        resource_type
                    ),
                )
                .with_location(format!("{}.resourceType", resource_type)),
            );
            return;
        }
        Err(e) => {
            issues.push(ValidationIssue::error(
                IssueCode::Exception,
                format!(
                    "Error loading base StructureDefinition '{}': {}",
                    base_profile_url, e
                ),
            ));
            return;
        }
    };

    // Ensure StructureDefinition matches resourceType
    if structure_def.type_ != resource_type {
        issues.push(
            ValidationIssue::error(
                IssueCode::Invalid,
                format!(
                    "Base StructureDefinition '{}' is for type '{}' but resourceType is '{}'",
                    base_profile_url, structure_def.type_, resource_type
                ),
            )
            .with_location(format!("{}.resourceType", resource_type)),
        );
        return;
    }

    // Prefer the provided context if it already serves expanded snapshots. If it doesn't (e.g. choice
    // variants missing), fall back to on-the-fly expansion via ExpandedFhirContext.
    let structure_def = {
        let needs_expansion = match structure_def.snapshot.as_ref() {
            None => true,
            Some(snapshot) => {
                let index = ElementIndex::new(&snapshot.element);
                snapshot_needs_expansion(resource, &resource_type, &index)
            }
        };

        if needs_expansion {
            let expanded_context = ExpandedFhirContext::borrowed(context);
            match expanded_context.get_structure_definition(&base_profile_url) {
                Ok(Some(sd)) => sd,
                Ok(None) => {
                    issues.push(
                        ValidationIssue::error(
                            IssueCode::NotFound,
                            format!(
                                "Base StructureDefinition not found for resource type '{}'",
                                resource_type
                            ),
                        )
                        .with_location(format!("{}.resourceType", resource_type)),
                    );
                    return;
                }
                Err(e) => {
                    issues.push(ValidationIssue::error(
                        IssueCode::Exception,
                        format!(
                            "Error expanding base StructureDefinition '{}': {}",
                            base_profile_url, e
                        ),
                    ));
                    return;
                }
            }
        } else {
            structure_def
        }
    };

    let Some(snapshot) = structure_def.snapshot.as_ref() else {
        issues.push(
            ValidationIssue::error(
                IssueCode::Exception,
                format!(
                    "Base StructureDefinition '{}' has no snapshot",
                    base_profile_url
                ),
            )
            .with_location(format!("{}.resourceType", resource_type)),
        );
        return;
    };

    let index = ElementIndex::new(&snapshot.element);
    validate_object(resource, &resource_type, &index, plan, issues);
}

struct ChoiceBase<'a> {
    base_name: &'a str,
    element: &'a ElementDefinition,
}

struct ElementIndex<'a> {
    by_path: HashMap<&'a str, &'a ElementDefinition>,
    children_by_parent: HashMap<&'a str, Vec<&'a ElementDefinition>>,
    choice_bases_by_parent: HashMap<&'a str, Vec<ChoiceBase<'a>>>,
    root_path: String,
}

impl<'a> ElementIndex<'a> {
    fn new(elements: &'a [ElementDefinition]) -> Self {
        let mut by_path = HashMap::new();
        let mut children_by_parent: HashMap<&'a str, Vec<&'a ElementDefinition>> = HashMap::new();
        let mut choice_bases_by_parent: HashMap<&'a str, Vec<ChoiceBase<'a>>> = HashMap::new();

        for element in elements {
            if element.path.contains(':') {
                continue; // ignore slices for now
            }
            by_path.insert(element.path.as_str(), element);

            let Some((parent, name)) = element.path.rsplit_once('.') else {
                continue;
            };
            children_by_parent.entry(parent).or_default().push(element);

            if name.ends_with("[x]") {
                choice_bases_by_parent
                    .entry(parent)
                    .or_default()
                    .push(ChoiceBase {
                        base_name: name.trim_end_matches("[x]"),
                        element,
                    });
            }
        }

        // Root path is the first element's path (e.g., "Patient", "Bundle")
        let root_path = elements
            .first()
            .map(|e| e.path.clone())
            .unwrap_or_default();

        Self {
            by_path,
            children_by_parent,
            choice_bases_by_parent,
            root_path,
        }
    }

    fn root_path(&self) -> &str {
        &self.root_path
    }

    fn has_path(&self, path: &str) -> bool {
        self.by_path.contains_key(path)
    }

    fn children_of(&self, parent_path: &str) -> &[&'a ElementDefinition] {
        self.children_by_parent
            .get(parent_path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn choice_bases_of(&self, parent_path: &str) -> &[ChoiceBase<'a>] {
        self.choice_bases_by_parent
            .get(parent_path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn is_choice_variant_name(&self, parent_path: &str, name: &str) -> bool {
        self.choice_bases_of(parent_path).iter().any(|b| {
            name.starts_with(b.base_name)
                && name.len() > b.base_name.len()
                && name[b.base_name.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase())
        })
    }
}

fn snapshot_needs_expansion(resource: &Value, root_path: &str, index: &ElementIndex<'_>) -> bool {
    fn has_non_special_keys(obj: &serde_json::Map<String, Value>) -> bool {
        obj.keys().any(|k| {
            !is_special_element_key(k)
                && !k.starts_with('_')
                && k.as_str() != "extension"
                && k.as_str() != "modifierExtension"
        })
    }

    fn visit(value: &Value, path: &str, index: &ElementIndex<'_>) -> bool {
        match value {
            Value::Object(obj) => {
                for (key, child) in obj {
                    if is_special_element_key(key) || key.starts_with('_') {
                        continue;
                    }

                    let child_path = format!("{}.{}", path, key);

                    // Choice variant present in instance, but missing in snapshot.
                    if !index.has_path(&child_path) && index.is_choice_variant_name(path, key) {
                        return true;
                    }

                    // Complex child present, but snapshot has no children (likely not deep-expanded).
                    if child.is_object() {
                        if index.has_path(&child_path)
                            && index.children_of(&child_path).is_empty()
                            && has_non_special_keys(child.as_object().unwrap())
                        {
                            return true;
                        }
                        if visit(child, &child_path, index) {
                            return true;
                        }
                    } else if let Some(arr) = child.as_array() {
                        let has_object_items = arr.iter().any(|v| v.is_object());
                        if has_object_items
                            && index.has_path(&child_path)
                            && index.children_of(&child_path).is_empty()
                        {
                            return true;
                        }
                        for item in arr {
                            if visit(item, &child_path, index) {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            Value::Array(arr) => arr.iter().any(|v| visit(v, path, index)),
            _ => false,
        }
    }

    visit(resource, root_path, index)
}

fn validate_object(
    value: &Value,
    path: &str,
    index: &ElementIndex<'_>,
    plan: &SchemaPlan,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(obj) = value.as_object() else {
        return;
    };

    // Cardinality for choice bases applies across variants.
    for choice_base in index.choice_bases_of(path) {
        let mut occurrences = 0_u64;
        for (key, v) in obj.iter() {
            if key.starts_with(choice_base.base_name)
                && key.len() > choice_base.base_name.len()
                && key[choice_base.base_name.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase())
            {
                occurrences += match v {
                    Value::Array(arr) => arr.len() as u64,
                    Value::Null => 0,
                    _ => 1,
                };
            }
        }

        let min = choice_base.element.min.unwrap_or(0) as u64;
        let max = choice_base.element.max.as_deref().unwrap_or("*");
        let base_path = format!("{}.{}", path, choice_base.base_name);
        validate_choice_cardinality(
            occurrences,
            choice_base.base_name,
            &base_path,
            min,
            max,
            issues,
        );
    }

    // Cardinality + type for direct children (skip cardinality for choice variants).
    for child_def in index.children_of(path) {
        let Some(name) = child_def.path.split('.').next_back() else {
            continue;
        };
        if name.ends_with("[x]") {
            continue;
        }

        let child_path = format!("{}.{}", path, name);
        let child_value = obj.get(name);

        if !index.is_choice_variant_name(path, name) {
            let min = child_def.min.unwrap_or(0) as u64;
            let max = child_def.max.as_deref().unwrap_or("*");
            validate_cardinality(child_value, name, &child_path, min, max, issues);
        }

        if let Some(v) = child_value {
            if !v.is_null() {
                validate_data_type(v, child_def, &child_path, issues);
            }
        }

        if let Some(v) = child_value {
            if v.is_object() {
                validate_object(v, &child_path, index, plan, issues);
            } else if let Some(arr) = v.as_array() {
                for item in arr {
                    if item.is_object() {
                        validate_object(item, &child_path, index, plan, issues);
                    }
                }
            }
        }
    }

    // Unknown element check.
    if !plan.allow_unknown_elements {
        // Skip unknown element checking for Extension objects — their children (url, value[x],
        // extension, id) are defined in the Extension SD, not the parent resource's snapshot.
        // We detect extension objects by checking if the parent path ends with ".extension"
        // or ".modifierExtension" and the object has a "url" key.
        let is_extension_object = (path.ends_with(".extension") || path.ends_with(".modifierExtension"))
            && obj.contains_key("url");

        // Skip unknown element checking for nested resources (contained, Bundle.entry.resource,
        // Parameters.parameter.resource). These have their own resourceType and should be
        // validated against their own StructureDefinition, not the parent's.
        let is_nested_resource = obj.contains_key("resourceType")
            && path != index.root_path();

        if !is_extension_object && !is_nested_resource {
            for key in obj.keys() {
                if is_special_element_key(key) {
                    continue;
                }

                if let Some(stripped) = key.strip_prefix('_') {
                    let candidate = format!("{}.{}", path, stripped);
                    if index.has_path(&candidate) {
                        continue;
                    }
                }

                let candidate = format!("{}.{}", path, key);
                if index.has_path(&candidate) {
                    continue;
                }

                // Check if this is a choice type variant (e.g., "boundsPeriod" for "bounds[x]")
                if index.is_choice_variant_name(path, key) {
                    continue;
                }

                issues.push(
                    ValidationIssue::error(
                        IssueCode::Structure,
                        format!("Unknown element '{}'", key),
                    )
                    .with_location(candidate.clone())
                    .with_expression(vec![candidate]),
                );
            }
        }
    }

    if !plan.allow_modifier_extensions {
        check_modifier_extensions(value, path, issues);
    }
}

fn is_special_element_key(key: &str) -> bool {
    matches!(
        key,
        "resourceType" | "id" | "meta" | "extension" | "modifierExtension" | "fhir_comments"
    )
}

fn validate_choice_cardinality(
    occurrences: u64,
    element_name: &str,
    element_path: &str,
    min: u64,
    max: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if occurrences < min {
        issues.push(
            ValidationIssue::error(
                IssueCode::Required,
                format!(
                    "Element '{}' has cardinality {}..{}, but found {} occurrence(s)",
                    element_name, min, max, occurrences
                ),
            )
            .with_location(element_path.to_string())
            .with_expression(vec![element_path.to_string()]),
        );
    }

    if max != "*" {
        if let Ok(max_num) = max.parse::<u64>() {
            if occurrences > max_num {
                issues.push(
                    ValidationIssue::error(
                        IssueCode::Structure,
                        format!(
                            "Element '{}' has cardinality {}..{}, but found {} occurrence(s)",
                            element_name, min, max, occurrences
                        ),
                    )
                    .with_location(element_path.to_string())
                    .with_expression(vec![element_path.to_string()]),
                );
            }
        }
    }
}

/// Validates cardinality constraints (min/max)
fn validate_cardinality(
    value: Option<&Value>,
    element_name: &str,
    element_path: &str,
    min: u64,
    max: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let count = match value {
        None => 0,
        Some(Value::Array(arr)) => arr.len() as u64,
        Some(_) => 1,
    };

    // Check minimum cardinality
    if count < min {
        issues.push(
            ValidationIssue::error(
                IssueCode::Required,
                format!(
                    "Element '{}' has cardinality {}..{}, but found {} occurrence(s)",
                    element_name, min, max, count
                ),
            )
            .with_location(element_path.to_string())
            .with_expression(vec![element_path.to_string()]),
        );
    }

    // Check maximum cardinality
    if max != "*" {
        if let Ok(max_num) = max.parse::<u64>() {
            if count > max_num {
                issues.push(
                    ValidationIssue::error(
                        IssueCode::Structure,
                        format!(
                            "Element '{}' has cardinality {}..{}, but found {} occurrence(s)",
                            element_name, min, max, count
                        ),
                    )
                    .with_location(element_path.to_string())
                    .with_expression(vec![element_path.to_string()]),
                );
            }
        }
    }
}

/// Validates data type of element value
fn validate_data_type(
    value: &Value,
    element_def: &ElementDefinition,
    element_path: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(types) = element_def.types.as_ref() else {
        return;
    };

    if types.is_empty() {
        return;
    }

    let values: Vec<&Value> = match value {
        Value::Array(arr) => arr.iter().collect(),
        _ => vec![value],
    };

    for val in values {
        if val.is_null() {
            continue;
        }

        let mut ok = false;
        for type_def in types {
            if validate_primitive_type(val, type_def.code.as_str()) {
                ok = true;
                break;
            }
        }

        if !ok {
            let expected_types: Vec<String> = types.iter().map(|t| t.code.clone()).collect();
            issues.push(
                ValidationIssue::error(
                    IssueCode::Value,
                    format!(
                        "Element has incorrect type. Expected one of: {}",
                        expected_types.join(", ")
                    ),
                )
                .with_location(element_path.to_string())
                .with_expression(vec![element_path.to_string()]),
            );
            return;
        }
    }
}

/// Validates primitive FHIR data types
fn validate_primitive_type(value: &Value, type_code: &str) -> bool {
    match type_code {
        "string" | "uri" | "url" | "canonical" | "code" | "oid" | "id" | "uuid" | "markdown"
        | "xhtml" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" | "unsignedInt" | "positiveInt" => value.is_number(),
        "decimal" => value.is_number() || value.is_string(), // Can be string for precision
        "date" | "dateTime" | "instant" | "time" => {
            // Basic check - should be string, detailed format validation can come later
            value.is_string()
        }
        "base64Binary" => value.is_string(),
        // Complex types - check for object
        "CodeableConcept" | "Coding" | "Identifier" | "Reference" | "Quantity" | "Period"
        | "Range" | "Ratio" | "HumanName" | "Address" | "ContactPoint" => value.is_object(),
        // BackboneElement and nested types
        "BackboneElement" | "Element" => value.is_object(),
        // Default: accept if it's an object or matches resourceType
        _ => value.is_object() || value.is_string(),
    }
}

/// Checks for modifier extensions if disallowed
fn check_modifier_extensions(resource: &Value, path: &str, issues: &mut Vec<ValidationIssue>) {
    if let Some(modifier_ext) = resource.get("modifierExtension") {
        if modifier_ext.is_array() && !modifier_ext.as_array().unwrap().is_empty() {
            issues.push(
                ValidationIssue::error(
                    IssueCode::Extension,
                    "Modifier extensions are not allowed by configuration".to_string(),
                )
                .with_location(format!("{}.modifierExtension", path))
                .with_expression(vec![format!("{}.modifierExtension", path)]),
            );
        }
    }

    // Recursively check nested objects
    if let Some(obj) = resource.as_object() {
        for (key, value) in obj {
            if value.is_object() {
                check_modifier_extensions(value, &format!("{}.{}", path, key), issues);
            } else if let Some(arr) = value.as_array() {
                for (idx, item) in arr.iter().enumerate() {
                    if item.is_object() {
                        check_modifier_extensions(
                            item,
                            &format!("{}.{}.[{}]", path, key, idx),
                            issues,
                        );
                    }
                }
            }
        }
    }
}

/// Helper to extract resourceType from resource
fn get_resource_type(resource: &Value) -> Option<String> {
    resource
        .get("resourceType")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use ferrum_context::Result as ContextResult;

    #[test]
    fn test_get_resource_type() {
        let resource = json!({"resourceType": "Patient"});
        assert_eq!(get_resource_type(&resource), Some("Patient".to_string()));

        let no_type = json!({"id": "123"});
        assert_eq!(get_resource_type(&no_type), None);
    }

    #[test]
    fn test_validate_primitive_type() {
        assert!(validate_primitive_type(&json!("test"), "string"));
        assert!(validate_primitive_type(&json!(true), "boolean"));
        assert!(validate_primitive_type(&json!(42), "integer"));
        assert!(validate_primitive_type(
            &json!({"system": "http://test"}),
            "CodeableConcept"
        ));

        assert!(!validate_primitive_type(&json!(42), "string"));
        assert!(!validate_primitive_type(&json!("test"), "boolean"));
    }

    #[test]
    fn test_validate_cardinality() {
        let mut issues = Vec::new();

        // Required field missing (min=1, found 0)
        validate_cardinality(None, "name", "Patient.name", 1, "1", &mut issues);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, IssueCode::Required);

        issues.clear();

        // Too many occurrences (max=1, found 2)
        let array_value = json!(["a", "b"]);
        validate_cardinality(
            Some(&array_value),
            "identifier",
            "Patient.identifier",
            0,
            "1",
            &mut issues,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, IssueCode::Structure);

        issues.clear();

        // Valid cardinality (0..*, found 2)
        validate_cardinality(
            Some(&array_value),
            "identifier",
            "Patient.identifier",
            0,
            "*",
            &mut issues,
        );
        assert_eq!(issues.len(), 0);
    }

    struct MockContext {
        by_url: HashMap<String, Arc<Value>>,
    }

    impl FhirContext for MockContext {
        fn get_resource_by_url(
            &self,
            canonical_url: &str,
            _version: Option<&str>,
        ) -> ContextResult<Option<Arc<Value>>> {
            Ok(self.by_url.get(canonical_url).cloned())
        }
    }

    #[test]
    fn schema_expands_snapshots_and_materializes_differentials() {
        let mut by_url = HashMap::new();

        by_url.insert(
            "http://hl7.org/fhir/StructureDefinition/HumanName".to_string(),
            Arc::new(json!({
                "resourceType": "StructureDefinition",
                "url": "http://hl7.org/fhir/StructureDefinition/HumanName",
                "name": "HumanName",
                "status": "active",
                "kind": "complex-type",
                "abstract": false,
                "type": "HumanName",
                "snapshot": { "element": [
                    { "id": "HumanName", "path": "HumanName" },
                    { "id": "HumanName.given", "path": "HumanName.given", "min": 0, "max": "*", "type": [{ "code": "string" }] }
                ]}
            })),
        );

        by_url.insert(
            "http://hl7.org/fhir/StructureDefinition/Patient".to_string(),
            Arc::new(json!({
                "resourceType": "StructureDefinition",
                "url": "http://hl7.org/fhir/StructureDefinition/Patient",
                "name": "Patient",
                "status": "active",
                "kind": "resource",
                "abstract": false,
                "type": "Patient",
                "snapshot": { "element": [
                    { "id": "Patient", "path": "Patient" },
                    { "id": "Patient.name", "path": "Patient.name", "min": 0, "max": "*", "type": [{ "code": "HumanName" }] }
                ]}
            })),
        );

        by_url.insert(
            "http://example.org/fhir/StructureDefinition/MyPatient".to_string(),
            Arc::new(json!({
                "resourceType": "StructureDefinition",
                "url": "http://example.org/fhir/StructureDefinition/MyPatient",
                "name": "MyPatient",
                "status": "active",
                "kind": "resource",
                "abstract": false,
                "type": "Patient",
                "baseDefinition": "http://hl7.org/fhir/StructureDefinition/Patient",
                "derivation": "constraint",
                "differential": { "element": [
                    { "id": "Patient.birthDate", "path": "Patient.birthDate", "min": 1, "max": "1", "type": [{ "code": "date" }] }
                ]}
            })),
        );

        let ctx = ferrum_snapshot::ExpandedFhirContext::new(MockContext { by_url });
        let plan = SchemaPlan {
            allow_unknown_elements: false,
            allow_modifier_extensions: true,
        };

        // Deep snapshot expansion should allow validating nested fields under Patient.name.*
        let resource = json!({
            "resourceType": "Patient",
            "name": [{ "given": ["a"], "unknown": "x" }]
        });

        let mut issues = Vec::new();
        validate_schema(&resource, &plan, &ctx, &mut issues);

        // Schema validation should catch unknown element
        assert!(issues
            .iter()
            .any(|i| i.location.as_deref() == Some("Patient.name.unknown")
                && i.code == IssueCode::Structure));
    }
}
