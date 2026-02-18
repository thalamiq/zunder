//! Constraint validation (FHIRPath invariants)
//!
//! Validates resources against FHIRPath constraints (invariants) defined in:
//! - Base StructureDefinitions (element-level constraints)
//! - Profile StructureDefinitions (additional profile-specific constraints)
//!
//! Implements spec-compliant constraint evaluation per:
//! https://hl7.org/fhir/conformance-rules.html#constraints
//!
//! Key features:
//! - Evaluates constraints from both base definitions and profiles
//! - Supports error, warning, and best-practice guideline severities
//! - Allows constraint suppression via configuration
//! - Supports severity level overrides
//! - Handles constraints at all levels of the resource hierarchy

use crate::validator::{IssueCode, IssueSeverity, ValidationIssue};
use crate::{BestPracticeMode, ConstraintsPlan, IssueLevel};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use ferrum_context::FhirContext;
use ferrum_models::common::element_definition::{ConstraintSeverity, ElementDefinition};
use ferrum_fhirpath::{Context as FhirPathContext, Engine as FhirPathEngine, Value as FhirPathValue};

/// Validates constraints (FHIRPath invariants) on a resource
pub fn validate_constraints<C: FhirContext>(
    resource: &Value,
    plan: &ConstraintsPlan,
    context: &C,
    fhirpath_engine: &Arc<FhirPathEngine>,
    issues: &mut Vec<ValidationIssue>,
) {
    // Extract resourceType
    let resource_type = match get_resource_type(resource) {
        Some(rt) => rt,
        None => {
            // Schema validation should have caught this
            return;
        }
    };

    // Build suppression and override maps
    let suppressed_keys: HashSet<String> = plan.suppress.iter().map(|id| id.0.clone()).collect();

    let level_overrides: HashMap<String, IssueLevel> = plan
        .level_overrides
        .iter()
        .map(|override_| (override_.id.0.clone(), override_.level))
        .collect();

    // Get base StructureDefinition for this resource type
    let base_url = format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type);
    if let Ok(Some(structure_def)) = context.get_structure_definition(&base_url) {
        if let Some(snapshot) = structure_def.snapshot.as_ref() {
            validate_constraints_from_elements(
                resource,
                &resource_type,
                &snapshot.element,
                plan,
                &suppressed_keys,
                &level_overrides,
                fhirpath_engine,
                issues,
            );
        }
    }

    // If profiles are present in meta.profile, also validate their constraints
    // This ensures profile-specific constraints are checked
    if let Some(profile_urls) = extract_profile_urls(resource) {
        for profile_url in profile_urls {
            if let Ok(Some(profile_def)) = context.get_structure_definition(&profile_url) {
                if let Some(snapshot) = profile_def.snapshot.as_ref() {
                    validate_constraints_from_elements(
                        resource,
                        &resource_type,
                        &snapshot.element,
                        plan,
                        &suppressed_keys,
                        &level_overrides,
                        fhirpath_engine,
                        issues,
                    );
                }
            }
        }
    }
}

/// Validates constraints from a set of ElementDefinitions
#[allow(clippy::too_many_arguments)]
fn validate_constraints_from_elements(
    resource: &Value,
    resource_type: &str,
    elements: &[ElementDefinition],
    plan: &ConstraintsPlan,
    suppressed_keys: &HashSet<String>,
    level_overrides: &HashMap<String, IssueLevel>,
    fhirpath_engine: &Arc<FhirPathEngine>,
    issues: &mut Vec<ValidationIssue>,
) {
    // Collect all constraints from all elements
    let mut all_constraints = Vec::new();

    for element in elements {
        if let Some(constraints) = &element.constraint {
            for constraint in constraints {
                // Check if suppressed
                if suppressed_keys.contains(&constraint.key) {
                    continue;
                }

                // Check if this is a best practice guideline
                // Best practice guidelines are marked with the elementdefinition-bestpractice extension
                let is_best_practice =
                    is_best_practice_constraint(&element.extensions, &constraint.key);

                // Determine effective severity based on constraint severity, best practice mode, and overrides
                let effective_severity = determine_effective_severity(
                    &constraint.severity,
                    is_best_practice,
                    plan.best_practice,
                    level_overrides.get(&constraint.key),
                );

                // Skip if no effective severity (e.g., best practice ignored)
                let Some(severity) = effective_severity else {
                    continue;
                };

                all_constraints.push(ConstraintToEvaluate {
                    key: constraint.key.clone(),
                    expression: constraint.expression.clone(),
                    human: constraint.human.clone(),
                    source: constraint.source.clone(),
                    element_path: element.path.clone(),
                    severity,
                    is_best_practice,
                });
            }
        }
    }

    // Evaluate each constraint
    for constraint in all_constraints {
        evaluate_constraint(
            resource,
            resource_type,
            &constraint,
            fhirpath_engine,
            issues,
        );
    }
}

/// Internal representation of a constraint to evaluate
struct ConstraintToEvaluate {
    key: String,
    expression: Option<String>,
    human: String,
    source: Option<String>,
    element_path: String,
    severity: IssueSeverity,
    is_best_practice: bool,
}

/// Determines the effective severity for a constraint
fn determine_effective_severity(
    constraint_severity: &ConstraintSeverity,
    is_best_practice: bool,
    best_practice_mode: BestPracticeMode,
    level_override: Option<&IssueLevel>,
) -> Option<IssueSeverity> {
    // Apply level override first if present
    if let Some(override_level) = level_override {
        return Some(match override_level {
            IssueLevel::Error => IssueSeverity::Error,
            IssueLevel::Warning => IssueSeverity::Warning,
            IssueLevel::Information => IssueSeverity::Information,
        });
    }

    // Handle best practice guidelines
    if is_best_practice {
        return match best_practice_mode {
            BestPracticeMode::Ignore => None,
            BestPracticeMode::Warn => Some(IssueSeverity::Warning),
            BestPracticeMode::Error => Some(IssueSeverity::Error),
        };
    }

    // Use constraint's declared severity
    Some(match constraint_severity {
        ConstraintSeverity::Error => IssueSeverity::Error,
        ConstraintSeverity::Warning => IssueSeverity::Warning,
    })
}

/// Checks if a constraint is marked as a best practice guideline
fn is_best_practice_constraint(extensions: &HashMap<String, Value>, _key: &str) -> bool {
    // Check for elementdefinition-bestpractice extension
    // This extension can be on the constraint itself or inherited
    if let Some(ext_value) =
        extensions.get("http://hl7.org/fhir/StructureDefinition/elementdefinition-bestpractice")
    {
        if let Some(b) = ext_value.as_bool() {
            return b;
        }
    }

    // Could also check constraint extensions if they were parsed separately
    // For now, default to false
    false
}

/// Evaluates a single constraint against the resource
fn evaluate_constraint(
    resource: &Value,
    _resource_type: &str,
    constraint: &ConstraintToEvaluate,
    fhirpath_engine: &Arc<FhirPathEngine>,
    issues: &mut Vec<ValidationIssue>,
) {
    // Skip if no expression
    let Some(expression) = &constraint.expression else {
        return;
    };

    // Find the context node for this constraint
    // Constraints are evaluated on the element they're defined on
    let context_path = &constraint.element_path;
    let context_node = resolve_context_node(resource, context_path);

    // If context node doesn't exist, the constraint doesn't apply
    // (e.g., optional element not present)
    if context_node.is_null() {
        return;
    }

    // Build the path segments for sub-path navigation (skip resource type prefix)
    let path_parts: Vec<&str> = context_path.split('.').collect();
    let sub_keys: Vec<&str> = if path_parts.len() > 1 {
        path_parts[1..].to_vec()
    } else {
        Vec::new()
    };

    // Share the resource root across all evaluations
    let root = Arc::new(resource.clone());

    // If context node is an array, evaluate the constraint on each item individually.
    // FHIR constraints are defined per-element, so for repeating elements like Patient.name
    // the constraint must be evaluated on each name entry separately.
    if let Some(arr) = context_node.as_array() {
        for (i, _item) in arr.iter().enumerate() {
            let fhirpath_value =
                FhirPathValue::from_json_at(root.clone(), &sub_keys, Some(i));
            let location = format!("{}[{}]", context_path, i);
            evaluate_constraint_on_node(
                fhirpath_value,
                expression,
                constraint,
                fhirpath_engine,
                &location,
                issues,
            );
        }
    } else {
        // Scalar element — evaluate directly
        let fhirpath_value = if sub_keys.is_empty() {
            FhirPathValue::from_json_root(root)
        } else {
            FhirPathValue::from_json_at(root, &sub_keys, None)
        };
        evaluate_constraint_on_node(
            fhirpath_value,
            expression,
            constraint,
            fhirpath_engine,
            context_path,
            issues,
        );
    }
}

/// Evaluate a FHIRPath constraint expression on a single context node.
fn evaluate_constraint_on_node(
    fhirpath_value: FhirPathValue,
    expression: &str,
    constraint: &ConstraintToEvaluate,
    fhirpath_engine: &Arc<FhirPathEngine>,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let ctx = FhirPathContext::new(fhirpath_value);

    // Evaluate FHIRPath expression
    let result = match fhirpath_engine.evaluate_expr(expression, &ctx, None) {
        Ok(result) => result,
        Err(e) => {
            // FHIRPath evaluation error - report as a processing issue
            issues.push(
                ValidationIssue::error(
                    IssueCode::Processing,
                    format!(
                        "Failed to evaluate constraint '{}': FHIRPath error: {}",
                        constraint.key, e
                    ),
                )
                .with_location(location.to_string()),
            );
            return;
        }
    };

    // Check if constraint is satisfied
    // Per FHIR spec: constraint passes if expression evaluates to true
    // Empty collection or false means constraint failed
    let passes = result.as_boolean().unwrap_or(false);

    if !passes {
        // Constraint failed - create issue
        let mut issue = match constraint.severity {
            IssueSeverity::Error | IssueSeverity::Fatal => {
                ValidationIssue::error(IssueCode::Invariant, format_constraint_message(constraint))
            }
            IssueSeverity::Warning => ValidationIssue::warning(
                IssueCode::Invariant,
                format_constraint_message(constraint),
            ),
            IssueSeverity::Information => ValidationIssue::information(
                IssueCode::Informational,
                format_constraint_message(constraint),
            ),
        };

        // Add location and expression
        issue = issue
            .with_location(location.to_string())
            .with_expression(vec![constraint.element_path.to_string()]);

        issues.push(issue);
    }
}

/// Formats a constraint failure message
fn format_constraint_message(constraint: &ConstraintToEvaluate) -> String {
    let prefix = if constraint.is_best_practice {
        "[Best Practice]"
    } else {
        ""
    };

    let source_info = if let Some(ref source) = constraint.source {
        format!(" (from {})", source)
    } else {
        String::new()
    };

    format!(
        "{} Constraint '{}' failed: {}{}",
        prefix, constraint.key, constraint.human, source_info
    )
}

/// Resolves the context node for a constraint evaluation
///
/// For constraints on "Patient.name", this returns the value at resource["name"]
/// For constraints on "Patient", this returns the entire resource
fn resolve_context_node<'a>(resource: &'a Value, element_path: &str) -> &'a Value {
    // Split path into parts (e.g., "Patient.name.given" -> ["Patient", "name", "given"])
    let parts: Vec<&str> = element_path.split('.').collect();

    // Skip the first part (resource type)
    if parts.len() == 1 {
        // Constraint on root resource
        return resource;
    }

    // Navigate to the context node
    let mut current = resource;
    for part in parts.iter().skip(1) {
        // Handle array indices if present (though typically not in element paths)
        current = match current.get(part) {
            Some(value) => value,
            None => return &Value::Null,
        };
    }

    current
}

/// Extracts profile URLs from resource meta.profile
fn extract_profile_urls(resource: &Value) -> Option<Vec<String>> {
    let profiles = resource.get("meta")?.get("profile")?.as_array()?;

    Some(
        profiles
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
    )
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

    #[test]
    fn test_resolve_context_node_root() {
        let resource = serde_json::json!({
            "resourceType": "Patient",
            "id": "123"
        });

        let context = resolve_context_node(&resource, "Patient");
        assert_eq!(context, &resource);
    }

    #[test]
    fn test_resolve_context_node_nested() {
        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{
                "family": "Smith",
                "given": ["John"]
            }]
        });

        let context = resolve_context_node(&resource, "Patient.name");
        assert_eq!(
            context,
            &serde_json::json!([{
                "family": "Smith",
                "given": ["John"]
            }])
        );
    }

    #[test]
    fn test_resolve_context_node_missing() {
        let resource = serde_json::json!({
            "resourceType": "Patient",
            "id": "123"
        });

        let context = resolve_context_node(&resource, "Patient.name");
        assert!(context.is_null());
    }

    #[test]
    fn test_format_constraint_message() {
        let constraint = ConstraintToEvaluate {
            key: "pat-1".to_string(),
            expression: Some("name.exists()".to_string()),
            human: "Patient must have a name".to_string(),
            source: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
            element_path: "Patient".to_string(),
            severity: IssueSeverity::Error,
            is_best_practice: false,
        };

        let message = format_constraint_message(&constraint);
        assert!(message.contains("pat-1"));
        assert!(message.contains("Patient must have a name"));
        assert!(message.contains("http://hl7.org/fhir/StructureDefinition/Patient"));
    }

    #[test]
    fn test_format_constraint_message_best_practice() {
        let constraint = ConstraintToEvaluate {
            key: "bp-1".to_string(),
            expression: Some("telecom.exists()".to_string()),
            human: "Patient should have contact information".to_string(),
            source: None,
            element_path: "Patient".to_string(),
            severity: IssueSeverity::Warning,
            is_best_practice: true,
        };

        let message = format_constraint_message(&constraint);
        assert!(message.contains("[Best Practice]"));
        assert!(message.contains("bp-1"));
    }

    #[test]
    fn test_determine_effective_severity_with_override() {
        let severity = determine_effective_severity(
            &ConstraintSeverity::Error,
            false,
            BestPracticeMode::Ignore,
            Some(&IssueLevel::Warning),
        );
        assert_eq!(severity, Some(IssueSeverity::Warning));
    }

    #[test]
    fn test_determine_effective_severity_best_practice_ignore() {
        let severity = determine_effective_severity(
            &ConstraintSeverity::Warning,
            true,
            BestPracticeMode::Ignore,
            None,
        );
        assert_eq!(severity, None);
    }

    #[test]
    fn test_determine_effective_severity_best_practice_warn() {
        let severity = determine_effective_severity(
            &ConstraintSeverity::Warning,
            true,
            BestPracticeMode::Warn,
            None,
        );
        assert_eq!(severity, Some(IssueSeverity::Warning));
    }

    #[test]
    fn test_determine_effective_severity_best_practice_error() {
        let severity = determine_effective_severity(
            &ConstraintSeverity::Warning,
            true,
            BestPracticeMode::Error,
            None,
        );
        assert_eq!(severity, Some(IssueSeverity::Error));
    }

    #[test]
    fn test_determine_effective_severity_standard_error() {
        let severity = determine_effective_severity(
            &ConstraintSeverity::Error,
            false,
            BestPracticeMode::Ignore,
            None,
        );
        assert_eq!(severity, Some(IssueSeverity::Error));
    }

    #[test]
    fn test_determine_effective_severity_standard_warning() {
        let severity = determine_effective_severity(
            &ConstraintSeverity::Warning,
            false,
            BestPracticeMode::Ignore,
            None,
        );
        assert_eq!(severity, Some(IssueSeverity::Warning));
    }
}
