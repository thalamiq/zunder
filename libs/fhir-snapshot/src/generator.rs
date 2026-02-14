//! Snapshot and differential generation
//!
//! Provides:
//! - Simple snapshot generation: merge a differential onto a base snapshot
//! - Differential generation: compute the delta between a snapshot and its base
//! - Deep snapshot generation: run expansion (contentReference, choice, complex) on a snapshot

use crate::error::{Error, Result};
use crate::expander::SnapshotExpander;
use crate::inheritance::{
    propagate_constraints, propagate_slice_names, validate_cardinality_inheritance,
};
use crate::merge::{cleanup_fixed_field, merge_element};
use crate::normalization::{normalize_differential, normalize_snapshot};
use crate::slicing::SlicingContext;
use crate::validation::{validate_differential, validate_snapshot};
use std::collections::HashMap;
use zunder_context::FhirContext;
use zunder_models::StructureDefinition;
use zunder_models::{Differential, ElementDefinition, ElementDefinitionBase, Snapshot};

/// Find the correct insertion position for a new non-slice element in the snapshot.
///
/// Strategy: find the best ancestor of the new element among existing elements,
/// then insert after that ancestor and all of its descendants. This handles both
/// normal parent paths and choice-type expansions (e.g., `effectivePeriod` under `effective[x]`).
fn find_insertion_position(elements: &[ElementDefinition], new_elem: &ElementDefinition) -> usize {
    let path = &new_elem.path;

    // Find the best (most specific) ancestor in the existing element list.
    let mut best_ancestor_idx = None;
    let mut best_ancestor_depth = 0;

    for (i, elem) in elements.iter().enumerate() {
        if is_ancestor_of(&elem.path, path) {
            let depth = elem.path.matches('.').count();
            if best_ancestor_idx.is_none() || depth > best_ancestor_depth {
                best_ancestor_idx = Some(i);
                best_ancestor_depth = depth;
            }
        }
    }

    let anchor = match best_ancestor_idx {
        Some(idx) => idx,
        None => return elements.len(),
    };

    let ancestor_path = &elements[anchor].path;

    // Find the last element that is a descendant of this ancestor
    let mut last_descendant = anchor;
    for (i, elem) in elements.iter().enumerate().skip(anchor + 1) {
        if is_ancestor_of(ancestor_path, &elem.path) || elem.path == *ancestor_path {
            last_descendant = i;
        }
    }

    last_descendant + 1
}

/// Check if `ancestor` is an ancestor of `descendant` in FHIR path terms.
/// Handles both normal paths and choice-type expansions.
fn is_ancestor_of(ancestor: &str, descendant: &str) -> bool {
    // Direct parent: "Patient.name" is ancestor of "Patient.name.family"
    if descendant.starts_with(ancestor)
        && descendant.len() > ancestor.len()
        && descendant.as_bytes()[ancestor.len()] == b'.'
    {
        return true;
    }

    // Choice-type: "Observation.effective[x]" is ancestor of "Observation.effectivePeriod"
    // and also "Observation.effectivePeriod.start"
    if let Some(base) = ancestor.strip_suffix("[x]") {
        if descendant.starts_with(base)
            && descendant.len() > base.len()
            && descendant.as_bytes()[base.len()].is_ascii_uppercase()
        {
            return true;
        }
    }

    false
}

/// Find base element for a differential element using FHIR's inheritance chain.
///
/// This implements a 4-step lookup:
/// 1. Try to find by ID in base snapshot
/// 2. Try to find by path in base snapshot
/// 3. Try to find in base's base recursively (follow baseDefinition chain)
/// 4. Try to find by type (use type's StructureDefinition)
fn find_base_element(
    diff_elem: &ElementDefinition,
    base_snapshot: &Snapshot,
    base_structure_definition: Option<&StructureDefinition>,
    context: &dyn FhirContext,
) -> Result<Option<ElementDefinition>> {
    // Step 1: Try to find by ID in base snapshot
    if let Some(ref diff_id) = diff_elem.id {
        if let Some(base_elem) = base_snapshot
            .element
            .iter()
            .find(|e| e.id.as_ref() == Some(diff_id))
        {
            return Ok(Some(base_elem.clone()));
        }
    }

    // Step 2: Try to find by path in base snapshot
    let base_elem = base_snapshot
        .element
        .iter()
        .find(|e| e.path == diff_elem.path && e.slice_name.is_none());

    if let Some(elem) = base_elem {
        return Ok(Some(elem.clone()));
    }

    // Step 2.5: Look up the parent element's type and find the child in that type's SD.
    // For example, `Location.address.postalCode` → parent is `Location.address` (type Address)
    // → look for `Address.postalCode` in the Address StructureDefinition.
    if let Some(dot_pos) = diff_elem.path.rfind('.') {
        let parent_path = &diff_elem.path[..dot_pos];
        let child_name = &diff_elem.path[dot_pos + 1..];

        // Find the parent in the base snapshot
        if let Some(parent_elem) = base_snapshot
            .element
            .iter()
            .find(|e| e.path == parent_path && e.slice_name.is_none())
        {
            if let Some(ref types) = parent_elem.types {
                if let Some(first_type) = types.first() {
                    let type_url = if first_type.code.starts_with("http://") {
                        first_type.code.clone()
                    } else {
                        format!(
                            "http://hl7.org/fhir/StructureDefinition/{}",
                            first_type.code
                        )
                    };

                    if let Some(type_sd) = context.get_structure_definition(&type_url)? {
                        if let Some(ref type_snapshot_def) = type_sd.snapshot {
                            let snapshot_value =
                                serde_json::to_value(type_snapshot_def).map_err(|e| {
                                    Error::Expansion(format!(
                                        "Failed to serialize snapshot: {}",
                                        e
                                    ))
                                })?;
                            if let Ok(type_snapshot) =
                                serde_json::from_value::<Snapshot>(snapshot_value)
                            {
                                let target_path =
                                    format!("{}.{}", first_type.code, child_name);
                                if let Some(type_elem) = type_snapshot
                                    .element
                                    .iter()
                                    .find(|e| e.path == target_path)
                                {
                                    return Ok(Some(type_elem.clone()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 3: Try to find in base's base recursively
    if let Some(base_sd) = base_structure_definition {
        // Get the baseDefinition URL
        if let Some(ref base_url) = base_sd.base_definition {
            // Fetch the base's base StructureDefinition
            if let Some(base_base_sd) = context.get_structure_definition(base_url)? {
                // Extract snapshot from base's base
                if let Some(ref base_base_snapshot) = base_base_sd.snapshot {
                    // Convert to our Snapshot type
                    let snapshot_value = serde_json::to_value(base_base_snapshot).map_err(|e| {
                        Error::Expansion(format!("Failed to serialize snapshot: {}", e))
                    })?;
                    if let Ok(base_base_snapshot) =
                        serde_json::from_value::<Snapshot>(snapshot_value)
                    {
                        // Recursively search in the base's base
                        return find_base_element(
                            diff_elem,
                            &base_base_snapshot,
                            Some(&base_base_sd),
                            context,
                        );
                    }
                }
            }
        }
    }

    // Step 4: Try to find by type
    // For elements like slices, look up the type's StructureDefinition
    if let Some(ref types) = diff_elem.types {
        if let Some(first_type) = types.first() {
            let type_url = if first_type.code.starts_with("http://") {
                first_type.code.clone()
            } else {
                format!(
                    "http://hl7.org/fhir/StructureDefinition/{}",
                    first_type.code
                )
            };

            if let Some(type_sd) = context.get_structure_definition(&type_url)? {
                if let Some(ref type_snapshot_def) = type_sd.snapshot {
                    // Convert to our Snapshot type
                    let snapshot_value = serde_json::to_value(type_snapshot_def).map_err(|e| {
                        Error::Expansion(format!("Failed to serialize snapshot: {}", e))
                    })?;
                    if let Ok(type_snapshot) = serde_json::from_value::<Snapshot>(snapshot_value) {
                        // Look for element with matching tail path
                        // For example, if diff_elem.path is "Condition.code.coding",
                        // look for "Coding" in the Coding StructureDefinition
                        let element_name = first_type.code.clone();

                        if let Some(type_elem) = type_snapshot
                            .element
                            .iter()
                            .find(|e| e.path == element_name && e.slice_name.is_none())
                        {
                            return Ok(Some(type_elem.clone()));
                        }
                    }
                }
            }
        }
    }

    // If all lookups fail, return None
    Ok(None)
}

/// Expand a fragment contentReference (e.g. `#Observation.referenceRange`) into a fully
/// qualified canonical form (`http://hl7.org/fhir/StructureDefinition/Observation#Observation.referenceRange`).
fn expand_content_reference(cr: &str, context: &dyn FhirContext) -> String {
    if let Some(fragment) = cr.strip_prefix('#') {
        let resource_type = fragment.split('.').next().unwrap_or(fragment);
        let canonical = format!(
            "http://hl7.org/fhir/StructureDefinition/{}",
            resource_type
        );
        if context
            .get_structure_definition(&canonical)
            .ok()
            .flatten()
            .is_some()
        {
            return format!("{}#{}", canonical, fragment);
        }
    }
    cr.to_string()
}

/// Post-process snapshot elements: expand fragment contentReferences and
/// default slicing.ordered to false.
pub(crate) fn post_process_snapshot(snapshot: &mut Snapshot, context: &dyn FhirContext) {
    for elem in &mut snapshot.element {
        if let Some(ref mut slicing) = elem.slicing {
            if slicing.ordered.is_none() {
                slicing.ordered = Some(false);
            }
        }

        if let Some(ref cr) = elem.content_reference {
            if cr.starts_with('#') {
                elem.content_reference = Some(expand_content_reference(cr, context));
            }
        }
    }
}

/// Sort differential elements into canonical FHIR order: by path hierarchy,
/// with children immediately after their parent and slices after the base element.
fn sort_differential(differential: &Differential, base: &Snapshot) -> Differential {
    let mut elements = differential.element.clone();

    // Build a position map from the base snapshot for ordering
    let base_positions: HashMap<&str, usize> = base
        .element
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.as_str(), i))
        .collect();

    elements.sort_by(|a, b| {
        // Primary: order by base snapshot position of the path
        let pos_a = base_positions.get(a.path.as_str()).copied().unwrap_or(usize::MAX);
        let pos_b = base_positions.get(b.path.as_str()).copied().unwrap_or(usize::MAX);
        pos_a
            .cmp(&pos_b)
            // Secondary: non-slices before slices on the same path
            .then_with(|| a.is_slice().cmp(&b.is_slice()))
    });

    Differential { element: elements }
}

/// Generate snapshot with optional base StructureDefinition for better lookups.
///
/// This is an advanced function that allows passing the base StructureDefinition
/// to enable recursive lookup through the base chain and type-based resolution.
/// For most uses, prefer `generate_snapshot` which has a simpler API.
pub(crate) fn generate_snapshot_internal(
    base: &Snapshot,
    differential: &Differential,
    base_structure_definition: Option<&StructureDefinition>,
    context: &dyn FhirContext,
) -> Result<Snapshot> {
    // Validate inputs; use base as-is (shallow) to preserve original snapshot ordering/structure
    validate_snapshot(base)?;
    validate_differential(differential, base)?;

    // Sort differential into canonical order before merging
    let sorted_diff = sort_differential(differential, base);
    let differential = &sorted_diff;

    let base_for_merge = base.clone();

    // Initialize slicing context to track slicing definitions and instances
    let mut slicing_ctx = SlicingContext::new();

    // Build index of base elements by path
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, elem) in base_for_merge.element.iter().enumerate() {
        index.insert(elem.key(), i);

        // Register slicing entries from base
        if let Some(ref slicing) = elem.slicing {
            slicing_ctx.register_slice_entry(&elem.path, slicing.clone(), i)?;
        }

        // Register slice instances from base
        if elem.is_slice() {
            slicing_ctx.register_slice_instance(elem)?;
        }
    }

    // Detect implicit slicing in differential
    slicing_ctx.detect_implicit_slicing(&differential.element);

    // Start with base elements, adding base metadata
    let mut merged_elements: Vec<ElementDefinition> = base_for_merge
        .element
        .iter()
        .map(|elem| {
            let mut new_elem = elem.clone();
            // Set base metadata to track inheritance
            if new_elem.base.is_none() {
                new_elem.base = Some(ElementDefinitionBase {
                    path: elem.path.clone(),
                    min: elem.min.unwrap_or(0),
                    max: elem.max.clone().unwrap_or_else(|| "*".to_string()),
                });
            }
            // Clean up: move any extension data incorrectly captured in fixed to extensions
            cleanup_fixed_field(&mut new_elem);
            new_elem
        })
        .collect();

    // Apply differential
    for diff_elem in &differential.element {
        let key = diff_elem.key();

        // Check if this element introduces slicing
        if let Some(ref slicing) = diff_elem.slicing {
            let entry_idx = merged_elements
                .iter()
                .position(|e| e.path == diff_elem.path && !e.is_slice())
                .unwrap_or(merged_elements.len());
            slicing_ctx.register_slice_entry(&diff_elem.path, slicing.clone(), entry_idx)?;
        }

        // Register slice instances from differential
        if diff_elem.is_slice() {
            slicing_ctx.register_slice_instance(diff_elem)?;
        }

        if let Some(&idx) = index.get(&key) {
            // Merge with existing element
            let base_elem = &merged_elements[idx];
            let mut merged = merge_element(base_elem, diff_elem, context);

            // Preserve or update base metadata
            if merged.base.is_none() && base_elem.base.is_some() {
                merged.base = base_elem.base.clone();
            }

            merged_elements[idx] = merged;
        } else {
            // New element from differential
            // Check if this is part of a slice
            let is_new_slice = diff_elem.is_slice();

            // Use the 4-step lookup to find the best base element
            let merged = match find_base_element(
                diff_elem,
                &base_for_merge,
                base_structure_definition,
                context,
            )? {
                Some(base_elem) => {
                    // Found a base element - merge the differential onto it
                    let mut m = merge_element(&base_elem, diff_elem, context);
                    // Set base metadata
                    m.base = Some(ElementDefinitionBase {
                        path: base_elem.path.clone(),
                        min: base_elem.min.unwrap_or(0),
                        max: base_elem.max.clone().unwrap_or_else(|| "*".to_string()),
                    });
                    m
                }
                None => {
                    // No base element found - use differential element as-is
                    let mut d = diff_elem.clone();
                    // For completely new elements, base path = current path
                    d.base = Some(ElementDefinitionBase {
                        path: d.path.clone(),
                        min: d.min.unwrap_or(0),
                        max: d.max.clone().unwrap_or_else(|| "*".to_string()),
                    });
                    d
                }
            };

            // If this is a slice, check slicing rules (warn only during generation —
            // "closed" means no *further* slices in derived profiles, but the defining
            // differential itself may introduce slices).
            if is_new_slice {
                let can_add = slicing_ctx
                    .can_add_slice(&diff_elem.path, diff_elem.slice_name.as_ref().unwrap())?;
                if !can_add {
                    eprintln!(
                        "warn: Slice '{}' added to closed slicing on '{}' (allowed during snapshot generation)",
                        diff_elem.slice_name.as_ref().unwrap(),
                        diff_elem.path
                    );
                }
            }

            // Determine insertion position for proper ordering
            let position = if is_new_slice {
                slicing_ctx.get_slice_position(&merged_elements, &merged)
            } else {
                find_insertion_position(&merged_elements, &merged)
            };

            merged_elements.insert(position, merged);

            // Update index with new position
            let key_clone = key.clone();
            index.insert(key, position);
            // Update all indices after insertion point
            for (k, v) in index.iter_mut() {
                if *v >= position && k != &key_clone {
                    *v += 1;
                }
            }
        }
    }

    // Inject implicit slicing entries
    for (path, _slice_names) in slicing_ctx.get_all_implicit_slicing().iter() {
        if let Some(elem_idx) = merged_elements
            .iter()
            .position(|e| &e.path == path && !e.is_slice())
        {
            if merged_elements[elem_idx].slicing.is_none() {
                // Add default slicing definition
                merged_elements[elem_idx].slicing =
                    Some(slicing_ctx.create_default_slicing_entry(path));
            }
        }
    }

    // Create snapshot
    let mut snapshot = Snapshot {
        element: merged_elements,
    };

    // Normalize IDs and slice names (keep original ordering)
    normalize_snapshot(&mut snapshot);

    // Propagate constraints from parents to children
    propagate_constraints(&mut snapshot)?;

    // Propagate slice names to child elements
    propagate_slice_names(&mut snapshot);

    // Validate slicing rules
    for (path, _) in slicing_ctx.get_all_slice_entries().iter() {
        slicing_ctx.validate_discriminators(path)?;
    }

    // Validate cardinality consistency
    validate_cardinality_inheritance(&snapshot)?;

    // Validate result
    validate_snapshot(&snapshot)?;

    Ok(snapshot)
}

/// Generate a snapshot by applying a differential onto a base snapshot
///
/// This function:
/// 1. Validates the differential against the base
/// 2. Merges differential elements onto base elements using FHIR merge semantics
/// 3. Normalizes IDs and slice names
/// 4. Sorts elements in canonical FHIR order
/// 5. Validates the resulting snapshot
///
/// # Parameters
/// - `base`: The base snapshot to build upon
/// - `differential`: The differential elements to apply
/// - `context`: FHIR context for resolving base type definitions when needed
pub fn generate_snapshot(
    base: &Snapshot,
    differential: &Differential,
    context: &dyn FhirContext,
) -> Result<Snapshot> {
    generate_snapshot_internal(base, differential, None, context)
}

/// Generate a differential by comparing a snapshot to its base snapshot
///
/// This function computes the minimal set of changes needed to transform
/// the base snapshot into the given snapshot.
pub fn generate_differential(base: &Snapshot, snapshot: &Snapshot) -> Result<Differential> {
    validate_snapshot(base)?;
    validate_snapshot(snapshot)?;

    // Build index of base elements by path
    let base_index: HashMap<String, &ElementDefinition> =
        base.element.iter().map(|e| (e.key(), e)).collect();

    let mut diff_elements = Vec::new();

    for elem in &snapshot.element {
        let key = elem.key();
        let base_elem = base_index.get(&key);

        match base_elem {
            None => {
                // Entire element is new - include all fields
                diff_elements.push(elem.clone());
            }
            Some(base) => {
                // Compute delta for this element
                if let Some(delta) = compute_element_delta(base, elem) {
                    diff_elements.push(delta);
                }
            }
        }
    }

    let mut differential = Differential {
        element: diff_elements,
    };

    // Normalize IDs and slice names
    normalize_differential(&mut differential);

    Ok(differential)
}

/// Compute the delta between a base element and a snapshot element
///
/// Returns None if there are no meaningful differences
fn compute_element_delta(
    base: &ElementDefinition,
    snapshot: &ElementDefinition,
) -> Option<ElementDefinition> {
    let mut has_changes = false;

    // Start with just path and id
    let mut delta = ElementDefinition {
        id: snapshot.id.clone(),
        path: snapshot.path.clone(),
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
        must_support: None,
        extensions: HashMap::new(),
    };

    // Check each field for changes
    if snapshot.slice_name != base.slice_name {
        delta.slice_name = snapshot.slice_name.clone();
        has_changes = true;
    }

    if snapshot.min != base.min {
        delta.min = snapshot.min;
        has_changes = true;
    }

    if snapshot.max != base.max {
        delta.max = snapshot.max.clone();
        has_changes = true;
    }

    if snapshot.types != base.types {
        delta.types = snapshot.types.clone();
        has_changes = true;
    }

    if snapshot.binding != base.binding {
        delta.binding = snapshot.binding.clone();
        has_changes = true;
    }

    if snapshot.slicing != base.slicing {
        delta.slicing = snapshot.slicing.clone();
        has_changes = true;
    }

    if snapshot.content_reference != base.content_reference {
        delta.content_reference = snapshot.content_reference.clone();
        has_changes = true;
    }

    if snapshot.short != base.short {
        delta.short = snapshot.short.clone();
        has_changes = true;
    }

    if snapshot.definition != base.definition {
        delta.definition = snapshot.definition.clone();
        has_changes = true;
    }

    if snapshot.comment != base.comment {
        delta.comment = snapshot.comment.clone();
        has_changes = true;
    }

    if snapshot.requirements != base.requirements {
        delta.requirements = snapshot.requirements.clone();
        has_changes = true;
    }

    if snapshot.alias != base.alias {
        delta.alias = snapshot.alias.clone();
        has_changes = true;
    }

    if snapshot.must_support != base.must_support {
        delta.must_support = snapshot.must_support;
        has_changes = true;
    }

    if snapshot.is_modifier != base.is_modifier {
        delta.is_modifier = snapshot.is_modifier;
        has_changes = true;
    }

    if snapshot.is_summary != base.is_summary {
        delta.is_summary = snapshot.is_summary;
        has_changes = true;
    }

    if snapshot.fixed != base.fixed {
        delta.fixed = snapshot.fixed.clone();
        has_changes = true;
    }

    if snapshot.pattern != base.pattern {
        delta.pattern = snapshot.pattern.clone();
        has_changes = true;
    }

    if snapshot.default_value != base.default_value {
        delta.default_value = snapshot.default_value.clone();
        has_changes = true;
    }

    if snapshot.constraint != base.constraint {
        delta.constraint = snapshot.constraint.clone();
        has_changes = true;
    }

    if snapshot.mapping != base.mapping {
        delta.mapping = snapshot.mapping.clone();
        has_changes = true;
    }

    // Check extensions
    for (key, value) in &snapshot.extensions {
        if base.extensions.get(key) != Some(value) {
            delta.extensions.insert(key.clone(), value.clone());
            has_changes = true;
        }
    }

    if has_changes {
        Some(delta)
    } else {
        None
    }
}

/// Generate a deep snapshot by expanding a simple snapshot
///
/// This applies expansion for:
/// - contentReference elements
/// - choice types (e.g., value[x])
/// - complex types
pub fn generate_deep_snapshot(snapshot: &Snapshot, context: &dyn FhirContext) -> Result<Snapshot> {
    validate_snapshot(snapshot)?;

    let expander = SnapshotExpander::new();

    // Use snapshot directly (it's already zunder_models::Snapshot)
    let expanded_elements = expander.expand_snapshot(snapshot, context)?;

    // Create expanded snapshot
    let mut expanded = Snapshot {
        element: expanded_elements,
    };

    // Normalize (keep original ordering from expansion)
    normalize_snapshot(&mut expanded);

    validate_snapshot(&expanded)?;

    Ok(expanded)
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
    async fn merges_differential_into_base_snapshot() {
        let ctx = create_test_context().await;
        let base = Snapshot {
            element: vec![
                make_element("Patient", None, None),
                make_element("Patient.name", Some(0), Some("*")),
            ],
        };

        let mut diff_name = make_element("Patient.name", Some(1), None);
        diff_name.short = Some("Patient name".to_string());

        let differential = Differential {
            element: vec![
                diff_name,
                make_element("Patient.name.family", Some(0), Some("1")),
            ],
        };

        let snapshot = generate_snapshot(&base, &differential, &ctx).unwrap();

        assert_eq!(snapshot.element.len(), 3);

        let name = snapshot
            .element
            .iter()
            .find(|e| e.path == "Patient.name")
            .unwrap();
        assert_eq!(name.min, Some(1));
        assert_eq!(name.short, Some("Patient name".to_string()));
    }

    #[tokio::test]
    async fn computes_differential_from_snapshot_and_base() {
        let base = Snapshot {
            element: vec![
                make_element("Patient", None, None),
                make_element("Patient.name", Some(0), Some("*")),
            ],
        };

        let snapshot = Snapshot {
            element: vec![
                make_element("Patient", None, None),
                make_element("Patient.name", Some(1), Some("*")),
                make_element("Patient.name.family", Some(0), Some("1")),
            ],
        };

        let diff = generate_differential(&base, &snapshot).unwrap();

        assert_eq!(diff.element.len(), 2);

        let name_diff = diff
            .element
            .iter()
            .find(|e| e.path == "Patient.name")
            .unwrap();
        assert_eq!(name_diff.min, Some(1));
    }

    #[tokio::test]
    async fn enforces_cardinality_restrictions() {
        let ctx = create_test_context().await;
        let base = Snapshot {
            element: vec![
                make_element("Patient", None, None),
                make_element("Patient.name", Some(0), Some("*")),
            ],
        };

        let diff_name = make_element("Patient.name", Some(1), Some("5"));

        let differential = Differential {
            element: vec![diff_name],
        };

        let snapshot = generate_snapshot(&base, &differential, &ctx).unwrap();

        let name = snapshot
            .element
            .iter()
            .find(|e| e.path == "Patient.name")
            .unwrap();

        assert_eq!(name.min, Some(1));
        assert_eq!(name.max, Some("5".to_string()));
    }

    #[tokio::test]
    async fn normalizes_element_ids() {
        let ctx = create_test_context().await;
        let base = Snapshot {
            element: vec![make_element("Patient", None, None)],
        };

        let mut slice_elem = make_element("Patient.name", None, None);
        slice_elem.slice_name = Some("official".to_string());
        slice_elem.id = None; // Will be normalized

        let differential = Differential {
            element: vec![slice_elem],
        };

        let snapshot = generate_snapshot(&base, &differential, &ctx).unwrap();

        let slice = snapshot
            .element
            .iter()
            .find(|e| e.slice_name.is_some())
            .unwrap();

        assert_eq!(slice.id, Some("Patient.name:official".to_string()));
    }

    #[tokio::test]
    async fn maintains_canonical_element_order() {
        let ctx = create_test_context().await;
        let base = Snapshot {
            element: vec![
                make_element("Patient", None, None),
                make_element("Patient.name", None, None),
            ],
        };

        let differential = Differential {
            element: vec![
                make_element("Patient.birthDate", None, None),
                make_element("Patient.name.family", None, None),
            ],
        };

        let snapshot = generate_snapshot(&base, &differential, &ctx).unwrap();

        // Canonical order: children are placed after their parent
        assert_eq!(snapshot.element[0].path, "Patient");
        assert_eq!(snapshot.element[1].path, "Patient.name");
        assert_eq!(snapshot.element[2].path, "Patient.name.family");
        assert_eq!(snapshot.element[3].path, "Patient.birthDate");
    }
}
