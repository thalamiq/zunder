//! Terminology validation step
//!
//! Walks the resource's StructureDefinition snapshot, finds elements with ValueSet bindings,
//! extracts coded values from the resource, and validates them via a TerminologyProvider.

use ferrum_context::FhirContext;
use ferrum_models::BindingStrength;
use serde_json::Value;

use crate::terminology::TerminologyProvider;
use crate::validator::{IssueCode, IssueSeverity, ValidationIssue};
use crate::{ExtensibleHandling, TerminologyPlan};

/// Run terminology validation on a resource.
pub fn validate_terminology(
    resource: &Value,
    plan: &TerminologyPlan,
    context: &dyn FhirContext,
    terminology: &dyn TerminologyProvider,
    issues: &mut Vec<ValidationIssue>,
) {
    let resource_type = match resource.get("resourceType").and_then(|v| v.as_str()) {
        Some(rt) => rt,
        None => return,
    };

    // Get the base StructureDefinition
    let sd = match context.get_core_structure_definition_by_type(resource_type) {
        Ok(Some(sd)) => sd,
        _ => return,
    };

    let snapshot = match sd.snapshot.as_ref() {
        Some(s) => s,
        None => return,
    };

    // Walk elements looking for bindings
    for element in &snapshot.element {
        let binding = match element.binding.as_ref() {
            Some(b) => b,
            None => continue,
        };

        // Skip Example bindings — never validated
        if binding.strength == BindingStrength::Example {
            continue;
        }

        // Skip Preferred if extensible_handling is Ignore
        if binding.strength == BindingStrength::Preferred
            && plan.extensible_handling == ExtensibleHandling::Ignore
        {
            continue;
        }

        let value_set_url = match binding.value_set.as_deref() {
            Some(url) => {
                // Strip version suffix (e.g., "http://...ValueSet/foo|4.0.1" → "http://...ValueSet/foo")
                url.split('|').next().unwrap_or(url)
            }
            None => continue,
        };

        // Determine the element type
        let type_code = element
            .types
            .as_ref()
            .and_then(|types| types.first())
            .map(|t| t.code.as_str())
            .unwrap_or("");

        // Navigate to the value in the resource at this element's path
        let element_path = &element.path;
        let relative_path = strip_resource_type(element_path, resource_type);

        let values = extract_values_at_path(resource, relative_path);
        if values.is_empty() {
            continue;
        }

        for (value, location) in values {
            validate_coded_value(
                value,
                type_code,
                value_set_url,
                binding.strength,
                plan,
                terminology,
                &location,
                issues,
            );
        }
    }
}

/// Strip the resource type prefix from an element path.
/// "Patient.name" → "name", "Patient" → ""
fn strip_resource_type<'a>(path: &'a str, resource_type: &str) -> &'a str {
    if path == resource_type {
        return "";
    }
    path.strip_prefix(resource_type)
        .and_then(|s| s.strip_prefix('.'))
        .unwrap_or(path)
}

/// Extract all values at a dot-separated path from a JSON resource.
/// Returns (value, fhirpath_location) pairs.
/// Handles arrays: "Patient.name" could have multiple entries.
fn extract_values_at_path<'a>(resource: &'a Value, path: &str) -> Vec<(&'a Value, String)> {
    if path.is_empty() {
        return vec![(resource, String::new())];
    }

    let resource_type = resource
        .get("resourceType")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let segments: Vec<&str> = path.split('.').collect();
    let mut results = Vec::new();
    collect_at_path(
        resource,
        &segments,
        0,
        resource_type.to_string(),
        &mut results,
    );
    results
}

fn collect_at_path<'a>(
    value: &'a Value,
    segments: &[&str],
    index: usize,
    current_path: String,
    results: &mut Vec<(&'a Value, String)>,
) {
    if index >= segments.len() {
        results.push((value, current_path));
        return;
    }

    let segment = segments[index];

    // Handle choice types: if segment is "value[x]", look for valueCode, valueCoding, etc.
    if segment.ends_with("[x]") {
        let prefix = segment.strip_suffix("[x]").unwrap_or(segment);
        if let Some(obj) = value.as_object() {
            for (key, val) in obj {
                if key.starts_with(prefix) && key.len() > prefix.len() {
                    let path = format!("{}.{}", current_path, key);
                    if val.is_array() {
                        if let Some(arr) = val.as_array() {
                            for (i, item) in arr.iter().enumerate() {
                                let item_path = format!("{}[{}]", path, i);
                                collect_at_path(item, segments, index + 1, item_path, results);
                            }
                        }
                    } else {
                        collect_at_path(val, segments, index + 1, path, results);
                    }
                }
            }
        }
        return;
    }

    match value.get(segment) {
        Some(child) if child.is_array() => {
            if let Some(arr) = child.as_array() {
                for (i, item) in arr.iter().enumerate() {
                    let path = format!("{}[{}]", format_path(&current_path, segment), i);
                    collect_at_path(item, segments, index + 1, path, results);
                }
            }
        }
        Some(child) => {
            let path = format_path(&current_path, segment);
            collect_at_path(child, segments, index + 1, path, results);
        }
        None => {}
    }
}

fn format_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{}.{}", prefix, segment)
    }
}

/// Validate a coded value against a ValueSet binding.
fn validate_coded_value(
    value: &Value,
    type_code: &str,
    value_set_url: &str,
    binding_strength: BindingStrength,
    plan: &TerminologyPlan,
    terminology: &dyn TerminologyProvider,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    match type_code {
        "code" => {
            // A code is a bare string value. The system comes from the binding.
            if let Some(code) = value.as_str() {
                validate_single_code(
                    None,
                    code,
                    None,
                    value_set_url,
                    binding_strength,
                    plan,
                    terminology,
                    location,
                    issues,
                );
            }
        }
        "Coding" => {
            validate_coding(
                value,
                value_set_url,
                binding_strength,
                plan,
                terminology,
                location,
                issues,
            );
        }
        "CodeableConcept" => {
            validate_codeable_concept(
                value,
                value_set_url,
                binding_strength,
                plan,
                terminology,
                location,
                issues,
            );
        }
        "Quantity" => {
            // Quantity can have a system + code for units
            let system = value.get("system").and_then(|v| v.as_str());
            let code = value.get("code").and_then(|v| v.as_str());
            if let (Some(system), Some(code)) = (system, code) {
                validate_single_code(
                    Some(system),
                    code,
                    None,
                    value_set_url,
                    binding_strength,
                    plan,
                    terminology,
                    location,
                    issues,
                );
            }
        }
        "string" | "uri" => {
            // string/uri with a binding: treat like a code
            if let Some(code) = value.as_str() {
                validate_single_code(
                    None,
                    code,
                    None,
                    value_set_url,
                    binding_strength,
                    plan,
                    terminology,
                    location,
                    issues,
                );
            }
        }
        _ => {}
    }
}

fn validate_coding(
    coding: &Value,
    value_set_url: &str,
    binding_strength: BindingStrength,
    plan: &TerminologyPlan,
    terminology: &dyn TerminologyProvider,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let system = coding.get("system").and_then(|v| v.as_str());
    let code = coding.get("code").and_then(|v| v.as_str());
    let display = coding.get("display").and_then(|v| v.as_str());

    if let Some(code) = code {
        validate_single_code(
            system,
            code,
            display,
            value_set_url,
            binding_strength,
            plan,
            terminology,
            location,
            issues,
        );
    }
}

fn validate_codeable_concept(
    cc: &Value,
    value_set_url: &str,
    binding_strength: BindingStrength,
    plan: &TerminologyPlan,
    terminology: &dyn TerminologyProvider,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let codings = match cc.get("coding").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return,
    };

    if codings.is_empty() {
        return;
    }

    // For required bindings, at least one coding must be valid
    // For extensible/preferred, we validate each coding individually
    if binding_strength == BindingStrength::Required {
        let mut any_valid = false;
        let mut first_message = None;

        for coding in codings {
            let system = coding.get("system").and_then(|v| v.as_str()).unwrap_or("");
            let code = coding.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let display = coding.get("display").and_then(|v| v.as_str());

            match terminology.validate_code(system, code, display, value_set_url) {
                Ok(Some(result)) if result.valid => {
                    any_valid = true;
                    break;
                }
                Ok(Some(result)) => {
                    if first_message.is_none() {
                        first_message = result.message;
                    }
                }
                _ => {}
            }
        }

        if !any_valid {
            let msg = first_message.unwrap_or_else(|| {
                format!(
                    "None of the codings are in the required ValueSet '{}'",
                    value_set_url
                )
            });
            issues.push(
                ValidationIssue::error(IssueCode::CodeInvalid, msg)
                    .with_location(location.to_string()),
            );
        }
    } else {
        // For extensible/preferred: validate each coding individually
        for (i, coding) in codings.iter().enumerate() {
            let coding_location = format!("{}.coding[{}]", location, i);
            validate_coding(
                coding,
                value_set_url,
                binding_strength,
                plan,
                terminology,
                &coding_location,
                issues,
            );
        }
    }
}

/// Core validation: check a single system+code against a ValueSet.
fn validate_single_code(
    system: Option<&str>,
    code: &str,
    display: Option<&str>,
    value_set_url: &str,
    binding_strength: BindingStrength,
    plan: &TerminologyPlan,
    terminology: &dyn TerminologyProvider,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let system = system.unwrap_or("");

    let result = match terminology.validate_code(system, code, display, value_set_url) {
        Ok(Some(r)) => r,
        Ok(None) => return, // ValueSet not known — skip silently
        Err(_) => return,   // Provider error — skip silently
    };

    if result.valid {
        // If there's a warning message (e.g., display mismatch), emit it
        if let Some(ref msg) = result.message {
            let severity = result
                .severity_override
                .unwrap_or(IssueSeverity::Warning);
            issues.push(
                ValidationIssue {
                    severity,
                    code: IssueCode::CodeInvalid,
                    diagnostics: msg.clone(),
                    location: Some(location.to_string()),
                    expression: None,
                },
            );
        }
        return;
    }

    // Code is invalid — determine severity based on binding strength
    let severity = match result.severity_override {
        Some(s) => s,
        None => binding_strength_to_severity(binding_strength, plan),
    };

    // Skip if severity is below threshold
    if severity == IssueSeverity::Information {
        return;
    }

    let msg = result.message.unwrap_or_else(|| {
        format!(
            "Code '{}' from system '{}' is not in the ValueSet '{}'",
            code, system, value_set_url
        )
    });

    issues.push(
        ValidationIssue {
            severity,
            code: IssueCode::CodeInvalid,
            diagnostics: msg,
            location: Some(location.to_string()),
            expression: None,
        },
    );
}

/// Map binding strength to issue severity.
fn binding_strength_to_severity(
    strength: BindingStrength,
    plan: &TerminologyPlan,
) -> IssueSeverity {
    match strength {
        BindingStrength::Required => IssueSeverity::Error,
        BindingStrength::Extensible => match plan.extensible_handling {
            ExtensibleHandling::Error => IssueSeverity::Error,
            ExtensibleHandling::Warn => IssueSeverity::Warning,
            ExtensibleHandling::Ignore => IssueSeverity::Information,
        },
        BindingStrength::Preferred => IssueSeverity::Information,
        BindingStrength::Example => IssueSeverity::Information,
    }
}
