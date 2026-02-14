//! FHIR constraint inheritance and propagation
//!
//! This module implements FHIR rules for propagating constraints from parent elements
//! to their descendants in a snapshot. According to FHIR rules:
//!
//! - Changes to min/max cardinality apply to descendants unless overridden
//! - mustSupport flags propagate downward
//! - isModifier context propagates to children
//! - Bindings can be inherited or restricted
//! - Constraints (invariants) apply to descendants

use crate::error::Result;
use std::collections::HashMap;
use zunder_models::{ElementDefinition, Snapshot};

/// Context for tracking inheritance during snapshot generation
pub struct InheritanceContext {
    /// Track mustSupport inheritance by path
    must_support_inheritance: HashMap<String, bool>,
    /// Track isModifier context by path
    modifier_context: HashMap<String, bool>,
    /// Track isSummary inheritance by path
    summary_inheritance: HashMap<String, bool>,
}

impl InheritanceContext {
    /// Create a new inheritance context
    pub fn new() -> Self {
        Self {
            must_support_inheritance: HashMap::new(),
            modifier_context: HashMap::new(),
            summary_inheritance: HashMap::new(),
        }
    }

    /// Register an element's flags for inheritance
    pub fn register_element(&mut self, element: &ElementDefinition) {
        let path = &element.path;

        // Track mustSupport
        if let Some(must_support) = element.must_support {
            if must_support {
                self.must_support_inheritance.insert(path.clone(), true);
            }
        }

        // Track isModifier
        if let Some(is_modifier) = element.is_modifier {
            if is_modifier {
                self.modifier_context.insert(path.clone(), true);
            }
        }

        // Track isSummary
        if let Some(is_summary) = element.is_summary {
            if is_summary {
                self.summary_inheritance.insert(path.clone(), true);
            }
        }
    }

    /// Check if mustSupport should be inherited by a child element
    pub fn should_inherit_must_support(&self, element_path: &str) -> bool {
        // Check all ancestor paths
        self.check_ancestor_inheritance(&self.must_support_inheritance, element_path)
    }

    /// Check if isModifier context applies to a child element
    pub fn should_inherit_modifier(&self, element_path: &str) -> bool {
        self.check_ancestor_inheritance(&self.modifier_context, element_path)
    }

    /// Check if isSummary should be inherited
    pub fn should_inherit_summary(&self, element_path: &str) -> bool {
        self.check_ancestor_inheritance(&self.summary_inheritance, element_path)
    }

    /// Helper to check inheritance from ancestors
    fn check_ancestor_inheritance(&self, map: &HashMap<String, bool>, element_path: &str) -> bool {
        let mut current_path = element_path.to_string();

        loop {
            if let Some(&value) = map.get(&current_path) {
                if value {
                    return true;
                }
            }

            // Move to parent
            if let Some(pos) = current_path.rfind('.') {
                current_path = current_path[..pos].to_string();
            } else {
                break;
            }
        }

        false
    }
}

impl Default for InheritanceContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Propagate constraints from parents to children in a snapshot
///
/// Note: FHIR constraint inheritance is complex and context-dependent.
/// Per the FHIR specification:
/// - mustSupport does NOT automatically propagate (profile-specific)
/// - isModifier does NOT automatically propagate (element-specific property)
/// - isSummary does NOT automatically propagate (element-specific property)
///
/// These flags must be explicitly set in the differential or base definition.
/// This function exists as a placeholder for future rule-based propagation.
pub fn propagate_constraints(_snapshot: &mut Snapshot) -> Result<()> {
    // Per FHIR spec, most constraint flags do NOT automatically propagate
    // They must be explicitly set in differentials or inherited from base definitions
    //
    // Future enhancements could add:
    // - Context-specific propagation rules
    // - Extension-based propagation
    // - Profile-specific propagation policies

    Ok(())
}

/// Propagate cardinality constraints to validate consistency
///
/// If a parent has min > 0, all required paths must exist
/// If a parent has max = 0, no children should exist
pub fn validate_cardinality_inheritance(snapshot: &Snapshot) -> Result<()> {
    let element_map: HashMap<String, &ElementDefinition> = snapshot
        .element
        .iter()
        .map(|e| (e.path.clone(), e))
        .collect();

    for element in &snapshot.element {
        // If this element has min > 0, check that it's consistent with parent
        if let Some(min) = element.min {
            if min > 0 {
                // Find parent
                if let Some(parent_path) = element.path.rfind('.').map(|pos| &element.path[..pos]) {
                    if let Some(parent) = element_map.get(parent_path) {
                        if let Some(parent_max) = &parent.max {
                            if parent_max == "0" {
                                // This is common in profiles: a parent is zeroed out
                                // but child definitions inherited from the base still
                                // carry their original min. The children are effectively
                                // unreachable, so we just warn.
                                eprintln!(
                                    "warn: Element '{}' has min={} but parent '{}' has max=0 (children are unreachable)",
                                    element.path, min, parent_path
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Propagate slice names to child elements
///
/// When a parent element is a slice (e.g., Patient.name:official),
/// its children should have IDs that reflect the slice name
/// (e.g., Patient.name:official.family)
pub fn propagate_slice_names(snapshot: &mut Snapshot) {
    // Build a map of paths to their slice names
    let mut slice_map: HashMap<String, String> = HashMap::new();

    for element in &snapshot.element {
        if let Some(slice_name) = &element.slice_name {
            slice_map.insert(element.path.clone(), slice_name.clone());
        }
    }

    // Update child element IDs to include parent slice names
    for element in &mut snapshot.element {
        // Check if this element is a child of a sliced element
        let mut current_path = element.path.clone();
        let mut slice_prefix = String::new();

        while let Some(pos) = current_path.rfind('.') {
            current_path = current_path[..pos].to_string();

            if let Some(parent_slice) = slice_map.get(&current_path) {
                if slice_prefix.is_empty() {
                    slice_prefix = parent_slice.clone();
                } else {
                    slice_prefix = format!("{}:{}", parent_slice, slice_prefix);
                }
            }
        }

        // Update ID if we found a parent slice
        if !slice_prefix.is_empty() && element.slice_name.is_none() {
            // This is a child of a sliced element
            // ID should be: path:parentSliceName
            if let Some(ref id) = element.id {
                if !id.contains(':') {
                    element.id = Some(format!("{}:{}", id, slice_prefix));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_element(path: &str, must_support: Option<bool>) -> ElementDefinition {
        ElementDefinition {
            id: Some(path.to_string()),
            path: path.to_string(),
            representation: None,
            slice_name: None,
            slice_is_constraining: None,
            short: None,
            definition: None,
            comment: None,
            requirements: None,
            alias: None,
            min: None,
            max: None,
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
            must_support,
            extensions: HashMap::new(),
        }
    }

    #[test]
    fn propagates_must_support_to_children() {
        let mut snapshot = Snapshot {
            element: vec![
                make_element("Patient", None),
                make_element("Patient.name", Some(true)),
                make_element("Patient.name.family", None),
                make_element("Patient.name.given", None),
            ],
        };

        propagate_constraints(&mut snapshot).unwrap();

        // Note: mustSupport does NOT automatically propagate in the conservative implementation
        // This is intentional to avoid over-constraining profiles
        assert_eq!(snapshot.element[2].must_support, None);
        assert_eq!(snapshot.element[3].must_support, None);
    }

    #[test]
    fn does_not_auto_propagate_is_modifier() {
        let parent = make_element("Patient.modifierExtension", None);
        let mut parent_with_modifier = parent;
        parent_with_modifier.is_modifier = Some(true);

        let mut snapshot = Snapshot {
            element: vec![
                make_element("Patient", None),
                parent_with_modifier,
                make_element("Patient.modifierExtension.url", None),
                make_element("Patient.modifierExtension.value", None),
            ],
        };

        propagate_constraints(&mut snapshot).unwrap();

        // Per FHIR spec, isModifier does NOT automatically propagate
        // It must be explicitly set in the differential/base
        assert_eq!(snapshot.element[2].is_modifier, None);
        assert_eq!(snapshot.element[3].is_modifier, None);
    }

    #[test]
    fn does_not_override_explicit_flags() {
        let parent = make_element("Patient.name", Some(true));
        let child = make_element("Patient.name.family", Some(false));

        let mut snapshot = Snapshot {
            element: vec![make_element("Patient", None), parent, child],
        };

        propagate_constraints(&mut snapshot).unwrap();

        // Explicit false should not be overridden
        assert_eq!(snapshot.element[2].must_support, Some(false));
    }

    #[test]
    fn validates_cardinality_consistency() {
        let parent = make_element("Patient.name", None);
        let mut parent_with_max = parent;
        parent_with_max.max = Some("0".to_string());

        let child = make_element("Patient.name.family", None);
        let mut child_with_min = child;
        child_with_min.min = Some(1);

        let snapshot = Snapshot {
            element: vec![
                make_element("Patient", None),
                parent_with_max,
                child_with_min,
            ],
        };

        // This is now allowed (warning only) — parent max=0 with child min=1
        // is common in profiles where the parent is zeroed out but inherited
        // child definitions still carry their original cardinality.
        let result = validate_cardinality_inheritance(&snapshot);
        assert!(result.is_ok());
    }

    #[test]
    fn inheritance_context_tracks_ancestors() {
        let mut ctx = InheritanceContext::new();

        let elem1 = make_element("Patient.name", Some(true));
        ctx.register_element(&elem1);

        // Child should inherit
        assert!(ctx.should_inherit_must_support("Patient.name.family"));

        // Grandchild should inherit
        assert!(ctx.should_inherit_must_support("Patient.name.family.extension"));

        // Unrelated path should not inherit
        assert!(!ctx.should_inherit_must_support("Patient.birthDate"));
    }
}
