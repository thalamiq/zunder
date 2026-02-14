//! FHIR-specific merge semantics for snapshot generation
//!
//! This module implements the FHIR rules for merging differential elements
//! onto base snapshot elements.

use serde_json::Value;
use zunder_context::FhirContext;
use zunder_models::{ElementDefinition, ElementDefinitionBinding, ElementDefinitionType};

/// Merge a differential element onto a base element according to FHIR rules
///
/// # Parameters
/// - `base`: The base element to build upon
/// - `diff`: The differential element with changes to apply
/// - `context`: FHIR context for resolving type definitions (currently unused but needed for future enhancements)
pub fn merge_element(
    base: &ElementDefinition,
    diff: &ElementDefinition,
    _context: &dyn FhirContext,
) -> ElementDefinition {
    let mut merged = base.clone();

    // Always use the diff's path (it reflects the correct context, e.g.
    // Location.address.city even when the base comes from Address.city)
    merged.path = diff.path.clone();

    // Override simple fields from differential
    if diff.id.is_some() {
        merged.id = diff.id.clone();
    }
    if diff.slice_name.is_some() {
        merged.slice_name = diff.slice_name.clone();
    }
    if diff.short.is_some() {
        merged.short = diff.short.clone();
    }
    if diff.definition.is_some() {
        merged.definition = diff.definition.clone();
    }
    if diff.comment.is_some() {
        merged.comment = diff.comment.clone();
    }
    if diff.requirements.is_some() {
        merged.requirements = diff.requirements.clone();
    }
    if diff.must_support.is_some() {
        merged.must_support = diff.must_support;
    }
    if diff.is_modifier.is_some() {
        merged.is_modifier = diff.is_modifier;
    }
    if diff.is_summary.is_some() {
        merged.is_summary = diff.is_summary;
    }
    if diff.fixed.is_some() && !is_empty_object(&diff.fixed) {
        merged.fixed = diff.fixed.clone();
    }
    if diff.pattern.is_some() && !is_empty_object(&diff.pattern) {
        merged.pattern = diff.pattern.clone();
    }
    if diff.default_value.is_some() && !is_empty_object(&diff.default_value) {
        merged.default_value = diff.default_value.clone();
    }
    if diff.content_reference.is_some() {
        merged.content_reference = diff.content_reference.clone();
    }

    // Merge cardinality
    if let Some(result) = merge_cardinality(base, diff) {
        merged.min = result.min;
        merged.max = result.max;
    }

    // Merge types
    if diff.types.is_some() {
        merged.types = merge_types(base.types.as_ref(), diff.types.as_ref());
    }

    // Merge binding
    if diff.binding.is_some() {
        merged.binding = merge_binding(base.binding.as_ref(), diff.binding.as_ref());
    }

    // Merge slicing (differential always wins)
    // Note: Slicing definitions should only appear on the base element,
    // not on individual slice instances
    if diff.slicing.is_some() {
        merged.slicing = diff.slicing.clone();
    } else if diff.is_slice() {
        // Slices should not inherit slicing definitions from their base element
        merged.slicing = None;
    }

    // Merge aliases (append)
    if let Some(ref diff_aliases) = diff.alias {
        let mut all_aliases = base.alias.clone().unwrap_or_default();
        for alias in diff_aliases {
            if !all_aliases.contains(alias) {
                all_aliases.push(alias.clone());
            }
        }
        merged.alias = Some(all_aliases);
    }

    // Merge constraints (append)
    if let Some(ref diff_constraints) = diff.constraint {
        let mut all_constraints = base.constraint.clone().unwrap_or_default();
        for constraint in diff_constraints {
            // Don't duplicate constraints with same key
            if !all_constraints.iter().any(|c| c.key == constraint.key) {
                all_constraints.push(constraint.clone());
            }
        }
        merged.constraint = Some(all_constraints);
    }

    // Merge mappings (append)
    if let Some(ref diff_mappings) = diff.mapping {
        let mut all_mappings = base.mapping.clone().unwrap_or_default();
        for mapping in diff_mappings {
            // Don't duplicate mappings with same identity
            if !all_mappings.iter().any(|m| m.identity == mapping.identity) {
                all_mappings.push(mapping.clone());
            }
        }
        merged.mapping = Some(all_mappings);
    }

    // Merge extensions (differential wins for conflicts)
    for (key, value) in &diff.extensions {
        merged.extensions.insert(key.clone(), value.clone());
    }

    // Clean up: move any extension data incorrectly captured in fixed to extensions
    cleanup_fixed_field(&mut merged);

    merged
}

/// Clean up the fixed field by moving extension data to extensions HashMap
///
/// This fixes an issue where the `extension` field gets incorrectly captured
/// by the `fixed` field during deserialization due to both using `#[serde(flatten)]`.
pub(crate) fn cleanup_fixed_field(elem: &mut ElementDefinition) {
    // Move pattern* fields from extensions HashMap to pattern field
    // This handles cases where pattern* fields were incorrectly captured by extensions during deserialization
    move_pattern_from_extensions_to_pattern(&mut elem.extensions, &mut elem.pattern);

    // Move pattern* fields from fixed to pattern
    move_pattern_fields(&mut elem.fixed, &mut elem.pattern);

    // Clean up extension fields
    cleanup_value_field(&mut elem.fixed, &mut elem.extensions);
    cleanup_value_field(&mut elem.pattern, &mut elem.extensions);
    cleanup_value_field(&mut elem.default_value, &mut elem.extensions);
}

/// Move pattern* fields from extensions HashMap to pattern field
///
/// During deserialization with multiple #[serde(flatten)] fields, pattern* fields
/// can be incorrectly captured by the extensions HashMap instead of the pattern field.
/// This function moves them to the correct location.
fn move_pattern_from_extensions_to_pattern(
    extensions: &mut std::collections::HashMap<String, Value>,
    pattern: &mut Option<Value>,
) {
    // Find all pattern* fields in extensions
    let pattern_keys: Vec<String> = extensions
        .keys()
        .filter(|k| k.starts_with("pattern"))
        .cloned()
        .collect();

    if !pattern_keys.is_empty() {
        // Create or update pattern object
        let mut pattern_obj = if let Some(Value::Object(existing_pattern)) = pattern.as_ref() {
            existing_pattern.clone()
        } else {
            serde_json::Map::new()
        };

        // Move pattern* fields from extensions to pattern
        for key in &pattern_keys {
            if let Some(value) = extensions.remove(key) {
                pattern_obj.insert(key.clone(), value);
            }
        }

        // Update pattern
        if !pattern_obj.is_empty() {
            *pattern = Some(Value::Object(pattern_obj));
        }
    }
}

/// Move pattern* fields from fixed to pattern
///
/// In FHIR, fields like patternCoding, patternString, etc. should be in pattern, not fixed.
/// Due to #[serde(flatten)] ordering, they can get captured by fixed first.
fn move_pattern_fields(fixed: &mut Option<Value>, pattern: &mut Option<Value>) {
    if let Some(Value::Object(obj)) = fixed.as_ref() {
        // Find all pattern* fields
        let pattern_keys: Vec<String> = obj
            .keys()
            .filter(|k| k.starts_with("pattern"))
            .cloned()
            .collect();

        if !pattern_keys.is_empty() {
            // Create or update pattern object
            let mut pattern_obj = if let Some(Value::Object(existing_pattern)) = pattern.as_ref() {
                existing_pattern.clone()
            } else {
                serde_json::Map::new()
            };

            // Move pattern* fields from fixed to pattern
            let mut new_fixed_obj = obj.clone();
            for key in &pattern_keys {
                if let Some(value) = obj.get(key) {
                    pattern_obj.insert(key.clone(), value.clone());
                    new_fixed_obj.remove(key);
                }
            }

            // Update fixed (remove pattern fields)
            if new_fixed_obj.is_empty() {
                *fixed = None;
            } else {
                *fixed = Some(Value::Object(new_fixed_obj));
            }

            // Update pattern (add pattern fields)
            if !pattern_obj.is_empty() {
                *pattern = Some(Value::Object(pattern_obj));
            }
        }
    }
}

/// Clean up a value field (fixed, pattern, or defaultValue) by moving extension data to extensions HashMap
///
/// This removes:
/// - Top-level "extension" fields (should be in extensions HashMap)
/// - Fields starting with "_" (FHIR extension fields, should be in extensions HashMap)
fn cleanup_value_field(
    value: &mut Option<Value>,
    extensions: &mut std::collections::HashMap<String, Value>,
) {
    if let Some(Value::Object(obj)) = value.as_ref() {
        // Check if value contains extension data (should be in extensions, not in value fields)
        let mut has_extensions = false;
        let mut new_value_obj = obj.clone();

        // Move "extension" field to extensions HashMap
        if obj.contains_key("extension") {
            if let Some(extension_value) = obj.get("extension") {
                extensions.insert("extension".to_string(), extension_value.clone());
                new_value_obj.remove("extension");
                has_extensions = true;
            }
        }

        // Move fields starting with "_" (FHIR extension fields) to extensions HashMap
        let underscore_keys: Vec<String> =
            obj.keys().filter(|k| k.starts_with('_')).cloned().collect();
        for key in &underscore_keys {
            if let Some(extension_value) = obj.get(key) {
                extensions.insert(key.clone(), extension_value.clone());
                new_value_obj.remove(key);
                has_extensions = true;
            }
        }

        // If we removed extensions, update the value
        if has_extensions {
            // If the remaining object is empty, set value to None
            if new_value_obj.is_empty() {
                *value = None;
            } else {
                *value = Some(Value::Object(new_value_obj));
            }
        } else if obj.is_empty() {
            // Empty object should be treated as None
            *value = None;
        }
    }
}

/// Check if a Value is an empty JSON object
fn is_empty_object(value: &Option<Value>) -> bool {
    match value {
        Some(Value::Object(obj)) => obj.is_empty(),
        _ => false,
    }
}

/// Result of cardinality merge
struct CardinalityResult {
    min: Option<u32>,
    max: Option<String>,
}

/// Merge cardinality according to FHIR rules
///
/// Rules:
/// - For non-slice elements: Differential can only make cardinality more restrictive
///   (min can only increase, max can only decrease)
/// - For slice elements: Cardinality is set independently of the base element
///   (slices define their own occurrence constraints)
fn merge_cardinality(
    base: &ElementDefinition,
    diff: &ElementDefinition,
) -> Option<CardinalityResult> {
    // For slices, cardinality represents the minimum for THAT slice
    // and can be set independently of the base element's cardinality
    let is_slice = diff.is_slice();

    let min = match (base.min, diff.min) {
        (Some(base_min), Some(diff_min)) => {
            if is_slice {
                // Slices can specify their own min independent of base
                Some(diff_min)
            } else {
                // Non-slices: min can only increase (more restrictive)
                Some(diff_min.max(base_min))
            }
        }
        (Some(base_min), None) => Some(base_min),
        (None, Some(diff_min)) => Some(diff_min),
        (None, None) => None,
    };

    let max = match (&base.max, &diff.max) {
        (Some(base_max), Some(diff_max)) => {
            if is_slice {
                // Slices can specify their own max independent of base
                Some(diff_max.clone())
            } else {
                // Non-slices: max can only decrease (more restrictive)
                Some(more_restrictive_max(base_max, diff_max).to_string())
            }
        }
        (Some(base_max), None) => Some(base_max.clone()),
        (None, Some(diff_max)) => Some(diff_max.clone()),
        (None, None) => None,
    };

    // Only return if at least one changed
    if min.is_some() || max.is_some() {
        Some(CardinalityResult { min, max })
    } else {
        None
    }
}

/// Determine the more restrictive max cardinality
fn more_restrictive_max<'a>(base: &'a str, diff: &'a str) -> &'a str {
    match (base, diff) {
        ("*", _) => diff,
        (_, "*") => base,
        (b, d) => {
            let b_num: u32 = b.parse().unwrap_or(u32::MAX);
            let d_num: u32 = d.parse().unwrap_or(u32::MAX);
            if d_num < b_num {
                diff
            } else {
                base
            }
        }
    }
}

/// Merge type definitions
///
/// FHIR rules:
/// - Differential can restrict types to a subset of base types
/// - Can add profiles to types
/// - Can add targetProfiles for Reference types
fn merge_types(
    base: Option<&Vec<ElementDefinitionType>>,
    diff: Option<&Vec<ElementDefinitionType>>,
) -> Option<Vec<ElementDefinitionType>> {
    match (base, diff) {
        (None, None) => None,
        (None, Some(d)) => Some(d.clone()),
        (Some(b), None) => Some(b.clone()),
        (Some(base_types), Some(diff_types)) => {
            let mut merged_types = Vec::new();

            for diff_type in diff_types {
                // Find matching base type
                if let Some(base_type) = base_types.iter().find(|bt| bt.code == diff_type.code) {
                    let mut merged_type = base_type.clone();

                    // Merge profiles (differential adds/restricts)
                    if let Some(ref diff_profiles) = diff_type.profile {
                        merged_type.profile = Some(diff_profiles.clone());
                    }

                    // Merge target profiles (differential adds/restricts)
                    if let Some(ref diff_targets) = diff_type.target_profile {
                        merged_type.target_profile = Some(diff_targets.clone());
                    }

                    // Merge aggregation (differential replaces)
                    if diff_type.aggregation.is_some() {
                        merged_type.aggregation = diff_type.aggregation.clone();
                    }

                    // Merge versioning (differential replaces)
                    if diff_type.versioning.is_some() {
                        merged_type.versioning = diff_type.versioning.clone();
                    }

                    merged_types.push(merged_type);
                } else {
                    // Type not in base - shouldn't happen in valid differentials
                    // but include it anyway
                    merged_types.push(diff_type.clone());
                }
            }

            Some(merged_types)
        }
    }
}

/// Merge binding definitions
///
/// FHIR rules:
/// - Differential can make binding more restrictive
/// - Can change from example -> preferred -> extensible -> required
/// - Can change the ValueSet
fn merge_binding(
    base: Option<&ElementDefinitionBinding>,
    diff: Option<&ElementDefinitionBinding>,
) -> Option<ElementDefinitionBinding> {
    match (base, diff) {
        (None, None) => None,
        (None, Some(d)) => Some(d.clone()),
        (Some(b), None) => Some(b.clone()),
        (Some(base_binding), Some(diff_binding)) => {
            let mut merged = base_binding.clone();

            // Strength can only become more restrictive
            let new_strength =
                more_restrictive_binding_strength(&base_binding.strength, &diff_binding.strength);
            merged.strength = new_strength;

            // Description and ValueSet from differential if present
            if diff_binding.description.is_some() {
                merged.description = diff_binding.description.clone();
            }
            if diff_binding.value_set.is_some() {
                merged.value_set = diff_binding.value_set.clone();
            }

            Some(merged)
        }
    }
}

/// Determine the more restrictive binding strength
fn more_restrictive_binding_strength(
    base: &zunder_models::BindingStrength,
    diff: &zunder_models::BindingStrength,
) -> zunder_models::BindingStrength {
    use zunder_models::BindingStrength;
    // Order: Example < Preferred < Extensible < Required
    match (base, diff) {
        (BindingStrength::Example, _) => diff.clone(),
        (_, BindingStrength::Required) => BindingStrength::Required,
        (BindingStrength::Preferred, BindingStrength::Extensible) => BindingStrength::Extensible,
        (BindingStrength::Preferred, BindingStrength::Preferred) => BindingStrength::Preferred,
        (BindingStrength::Extensible, BindingStrength::Extensible) => BindingStrength::Extensible,
        _ => base.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zunder_context::DefaultFhirContext;

    /// Create an R4 context for testing
    async fn create_test_context() -> DefaultFhirContext {
        DefaultFhirContext::from_fhir_version_async(None, "R4")
            .await
            .expect("Failed to create R4 context")
    }

    fn make_element(path: &str, min: Option<u32>, max: Option<&str>) -> ElementDefinition {
        ElementDefinition {
            id: None,
            path: path.to_string(),
            representation: None,
            slice_name: None,
            slice_is_constraining: None,
            short: None,
            definition: None,
            comment: None,
            requirements: None,
            alias: None,
            min,
            max: max.map(|s| s.to_string()),
            base: None,
            content_reference: None,
            types: None,
            default_value: None,
            meaning_when_missing: None,
            order_meaning: None,
            fixed: None,
            pattern: None,
            example: None,
            min_value: None,
            max_value: None,
            max_length: None,
            condition: None,
            constraint: None,
            is_modifier: None,
            is_modifier_reason: None,
            is_summary: None,
            binding: None,
            mapping: None,
            slicing: None,
            must_support: None,
            extensions: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn merges_cardinality_restrictively() {
        let ctx = create_test_context().await;
        let base = make_element("Patient.name", Some(0), Some("*"));
        let diff = make_element("Patient.name", Some(1), Some("5"));

        let merged = merge_element(&base, &diff, &ctx);

        assert_eq!(merged.min, Some(1));
        assert_eq!(merged.max, Some("5".to_string()));
    }

    #[tokio::test]
    async fn min_cannot_decrease() {
        let ctx = create_test_context().await;
        let base = make_element("Patient.name", Some(1), Some("*"));
        let diff = make_element("Patient.name", Some(0), Some("*"));

        let merged = merge_element(&base, &diff, &ctx);

        // min should be max of base and diff
        assert_eq!(merged.min, Some(1));
    }

    #[tokio::test]
    async fn max_cannot_increase() {
        let ctx = create_test_context().await;
        let base = make_element("Patient.name", Some(0), Some("5"));
        let diff = make_element("Patient.name", Some(0), Some("10"));

        let merged = merge_element(&base, &diff, &ctx);

        // max should be more restrictive
        assert_eq!(merged.max, Some("5".to_string()));
    }

    #[test]
    fn more_restrictive_max_handles_star() {
        // This test doesn't need async context
        assert_eq!(more_restrictive_max("*", "1"), "1");
        assert_eq!(more_restrictive_max("1", "*"), "1");
        assert_eq!(more_restrictive_max("5", "3"), "3");
        assert_eq!(more_restrictive_max("3", "5"), "3");
    }

    #[test]
    fn binding_strength_becomes_more_restrictive() {
        // This test doesn't need async context
        use zunder_models::BindingStrength;
        assert_eq!(
            more_restrictive_binding_strength(
                &BindingStrength::Example,
                &BindingStrength::Required
            ),
            BindingStrength::Required
        );
        assert_eq!(
            more_restrictive_binding_strength(
                &BindingStrength::Required,
                &BindingStrength::Example
            ),
            BindingStrength::Required
        );
        assert_eq!(
            more_restrictive_binding_strength(
                &BindingStrength::Preferred,
                &BindingStrength::Extensible
            ),
            BindingStrength::Extensible
        );
    }

    #[tokio::test]
    async fn merges_types_with_profiles() {
        let ctx = create_test_context().await;
        let mut base = make_element("Patient.identifier", None, None);
        base.types = Some(vec![ElementDefinitionType {
            code: "Identifier".to_string(),
            profile: None,
            target_profile: None,
            aggregation: None,
            versioning: None,
        }]);

        let mut diff = make_element("Patient.identifier", None, None);
        diff.types = Some(vec![ElementDefinitionType {
            code: "Identifier".to_string(),
            profile: Some(vec![
                "http://example.org/fhir/StructureDefinition/MyIdentifier".to_string(),
            ]),
            target_profile: None,
            aggregation: None,
            versioning: None,
        }]);

        let merged = merge_element(&base, &diff, &ctx);

        assert!(merged.types.is_some());
        let types = merged.types.unwrap();
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].code, "Identifier");
        assert!(types[0].profile.is_some());
    }
}
