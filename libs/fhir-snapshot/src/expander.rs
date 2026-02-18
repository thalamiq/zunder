//! Snapshot expansion for StructureDefinitions
//!
//! Expands StructureDefinition snapshots to resolve:
//! - Complex types (recursively resolve children)
//! - Choice types (value[x] → valueQuantity, etc.)
//! - ContentReferences (copy referenced element's children)

use crate::error::{Error, Result};
use std::collections::{HashMap, HashSet};
use ferrum_context::FhirContext;
use ferrum_models::{ElementDefinition, Snapshot};

/// Context tracking for recursion prevention
#[derive(Debug, Clone)]
struct ResolutionContext {
    type_code: String,
    path: String,
    #[allow(dead_code)]
    depth: usize,
}

/// Snapshot expander for StructureDefinitions
pub struct SnapshotExpander {
    max_recursion_depth: HashMap<String, usize>,
    circular_prone_types: HashSet<String>,
    never_resolve_types: HashSet<String>,
    content_reference_max_depth: usize,
}

impl SnapshotExpander {
    /// Create a new snapshot expander with default settings
    pub fn new() -> Self {
        Self {
            max_recursion_depth: HashMap::from([
                ("Extension".into(), 1),
                ("Identifier".into(), 1),
                ("Reference".into(), 2),
                ("BackboneElement".into(), 1),
            ]),
            circular_prone_types: HashSet::from([
                "Extension".into(),
                "Identifier".into(),
                "Reference".into(),
                "BackboneElement".into(),
            ]),
            never_resolve_types: HashSet::from(["Unknown".into()]),
            content_reference_max_depth: 10,
        }
    }

    /// Expand a snapshot's elements
    pub fn expand_snapshot(
        &self,
        snapshot: &Snapshot,
        context: &dyn FhirContext,
    ) -> Result<Vec<ElementDefinition>> {
        let mut expanded = Vec::new();
        let mut seen = HashSet::new();
        let mut resolution_stack = Vec::new();
        let mut content_reference_stack = Vec::new();

        // Pass all elements for contentReference resolution
        let all_elements: Vec<&ElementDefinition> = snapshot.element.iter().collect();

        for element in &snapshot.element {
            self.expand_element_recursive(
                element,
                &mut expanded,
                &mut seen,
                &mut resolution_stack,
                &mut content_reference_stack,
                &all_elements,
                context,
            )?;
        }

        Ok(expanded)
    }

    /// Recursively expand an element
    #[allow(clippy::too_many_arguments)]
    fn expand_element_recursive(
        &self,
        element: &ElementDefinition,
        expanded: &mut Vec<ElementDefinition>,
        seen: &mut HashSet<String>,
        resolution_stack: &mut Vec<ResolutionContext>,
        content_reference_stack: &mut Vec<String>,
        all_elements: &[&ElementDefinition],
        context: &dyn FhirContext,
    ) -> Result<()> {
        let element_id = self.get_element_id(element)?;

        if seen.contains(&element_id) {
            return Ok(());
        }

        // Add current element
        expanded.push(element.clone());
        seen.insert(element_id.clone());

        // Collect children
        let mut children = Vec::new();

        // 1. Expand contentReference
        if let Some(content_ref) = &element.content_reference {
            children.extend(self.expand_content_reference(
                element,
                content_ref,
                seen,
                resolution_stack,
                content_reference_stack,
                all_elements,
                context,
            )?);
        }

        // 2. Expand choice types
        if element.is_choice_type() {
            children.extend(self.expand_choice_element(
                element,
                seen,
                resolution_stack,
                content_reference_stack,
                all_elements,
                context,
            )?);
        }

        // 3. Expand complex types
        if self.should_resolve_complex_element(element, resolution_stack) {
            children.extend(self.expand_complex_element(
                element,
                seen,
                resolution_stack,
                content_reference_stack,
                all_elements,
                context,
            )?);
        }

        // Add children after parent
        expanded.extend(children);

        Ok(())
    }

    /// Get element ID
    fn get_element_id(&self, element: &ElementDefinition) -> Result<String> {
        element
            .id
            .clone()
            .ok_or_else(|| Error::Expansion("Element missing id field".into()))
    }

    /// Get element path
    fn get_element_path(&self, element: &ElementDefinition) -> String {
        element.path.clone()
    }

    /// Normalize type code (remove namespace prefixes, matching Python implementation)
    fn normalize_type_code(&self, type_code: &str) -> String {
        // Map System types to primitive names (matching Python implementation)
        match type_code {
            "http://hl7.org/fhirpath/System.String" => "string".to_string(),
            "http://hl7.org/fhirpath/System.Boolean" => "boolean".to_string(),
            "http://hl7.org/fhirpath/System.Date" => "date".to_string(),
            "http://hl7.org/fhirpath/System.DateTime" => "dateTime".to_string(),
            "http://hl7.org/fhirpath/System.Decimal" => "decimal".to_string(),
            "http://hl7.org/fhirpath/System.Integer" => "integer".to_string(),
            "http://hl7.org/fhirpath/System.Time" => "time".to_string(),
            _ => {
                // Remove System prefix if present, otherwise return as-is
                if let Some(stripped) = type_code.strip_prefix("http://hl7.org/fhirpath/System.") {
                    stripped.to_string()
                } else {
                    type_code.to_string()
                }
            }
        }
    }

    /// Capitalize first letter of a string
    fn capitalize_first_letter(&self, s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        }
    }

    /// Check if complex element should be resolved
    fn should_resolve_complex_element(
        &self,
        element: &ElementDefinition,
        resolution_stack: &[ResolutionContext],
    ) -> bool {
        // Get type code
        let type_code = element
            .types
            .as_ref()
            .and_then(|types| types.first())
            .map(|t| t.code.as_str());

        let Some(type_code) = type_code else {
            return false;
        };

        let normalized_type = self.normalize_type_code(type_code);
        let element_path = self.get_element_path(element);

        // Never resolve primitive types (case-insensitive check)
        let primitive_types = [
            "boolean",
            "integer",
            "integer64",
            "string",
            "decimal",
            "date",
            "datetime",
            "time",
            "base64binary",
            "code",
            "id",
            "oid",
            "uri",
            "url",
            "canonical",
            "unsignedint",
            "positiveint",
            "instant",
            "markdown",
            "xhtml",
            "uuid",
        ];
        if primitive_types
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&normalized_type))
        {
            return false;
        }

        // Never resolve explicitly excluded types
        if self.never_resolve_types.contains(&normalized_type) {
            return false;
        }

        // Don't resolve root-level elements
        if element_path.split('.').count() <= 1 {
            return false;
        }

        // Check recursion limits for circular-prone types
        if self.circular_prone_types.contains(&normalized_type) {
            let current_depth = resolution_stack
                .iter()
                .filter(|ctx| ctx.type_code == normalized_type)
                .count();

            let max_depth = self
                .max_recursion_depth
                .get(&normalized_type)
                .copied()
                .unwrap_or(5);

            if current_depth >= max_depth {
                return false;
            }

            // Check for exact path cycles
            for ctx in resolution_stack {
                if ctx.type_code == normalized_type && ctx.path == element_path {
                    return false;
                }
            }
        }

        true
    }

    /// Expand contentReference element
    #[allow(clippy::too_many_arguments)]
    fn expand_content_reference(
        &self,
        element: &ElementDefinition,
        content_ref: &str,
        seen: &mut HashSet<String>,
        resolution_stack: &mut Vec<ResolutionContext>,
        stack: &mut Vec<String>,
        all_elements: &[&ElementDefinition],
        context: &dyn FhirContext,
    ) -> Result<Vec<ElementDefinition>> {
        // Support canonical or local refs; take last segment after '#'
        let ref_id = content_ref
            .rsplit('#')
            .next()
            .unwrap_or(content_ref)
            .to_string();

        // Check for circular reference
        if stack.contains(&ref_id) {
            return Ok(Vec::new());
        }

        // Check depth
        if stack.len() >= self.content_reference_max_depth {
            return Ok(Vec::new());
        }

        // Find the referenced element
        let referenced_element = all_elements
            .iter()
            .find(|e| e.id.as_ref().map(|id| id == &ref_id).unwrap_or(false));

        let Some(referenced_element) = referenced_element else {
            return Ok(Vec::new());
        };

        stack.push(ref_id.clone());

        let result = (|| -> Result<Vec<ElementDefinition>> {
            let current_path = self.get_element_path(element);
            let current_id = self.get_element_id(element)?;
            let referenced_path = self.get_element_path(referenced_element);
            let referenced_id = self.get_element_id(referenced_element)?;

            if current_path.is_empty() || referenced_path.is_empty() {
                return Ok(Vec::new());
            }

            // Find all child elements of the referenced element
            let referenced_children: Vec<&ElementDefinition> = all_elements
                .iter()
                .filter(|e| {
                    let elem_id = e.id.as_deref().unwrap_or("");
                    let elem_path = self.get_element_path(e);
                    elem_id.starts_with(&format!("{}.", referenced_id))
                        || elem_path.starts_with(&format!("{}.", referenced_path))
                })
                .copied()
                .collect();

            let mut content_reference_children = Vec::new();

            for child_elem in referenced_children {
                let child_path = self.get_element_path(child_elem);
                let child_id = self.get_element_id(child_elem)?;

                // Replace the referenced element's path with current element's path
                let new_path = child_path.replacen(&referenced_path, &current_path, 1);
                let new_id = child_id.replacen(&referenced_id, &current_id, 1);

                if seen.contains(&new_id) {
                    continue;
                }

                let mut new_child = child_elem.clone();
                new_child.path = new_path.clone();
                new_child.id = Some(new_id.clone());

                // Update base.path if present
                if let Some(ref mut base) = new_child.base {
                    base.path = new_path.clone();
                }

                content_reference_children.push(new_child.clone());
                seen.insert(new_id.clone());

                // Recursively expand this child
                let mut grandchildren = Vec::new();

                if let Some(child_content_ref) = &new_child.content_reference {
                    grandchildren.extend(self.expand_content_reference(
                        &new_child,
                        child_content_ref,
                        seen,
                        resolution_stack,
                        stack,
                        all_elements,
                        context,
                    )?);
                } else {
                    if new_child.is_choice_type() {
                        grandchildren.extend(self.expand_choice_element(
                            &new_child,
                            seen,
                            resolution_stack,
                            stack,
                            all_elements,
                            context,
                        )?);
                    }

                    if self.should_resolve_complex_element(&new_child, resolution_stack) {
                        grandchildren.extend(self.expand_complex_element(
                            &new_child,
                            seen,
                            resolution_stack,
                            stack,
                            all_elements,
                            context,
                        )?);
                    }
                }

                content_reference_children.extend(grandchildren);
            }

            Ok(content_reference_children)
        })();

        stack.pop();

        result
    }

    /// Expand choice element (value[x] → valueQuantity, etc.)
    fn expand_choice_element(
        &self,
        element: &ElementDefinition,
        seen: &mut HashSet<String>,
        resolution_stack: &mut Vec<ResolutionContext>,
        content_reference_stack: &mut Vec<String>,
        all_elements: &[&ElementDefinition],
        context: &dyn FhirContext,
    ) -> Result<Vec<ElementDefinition>> {
        let base_path = self.get_element_path(element);

        let mut choice_elements = Vec::new();

        if let Some(types) = &element.types {
            for type_info in types {
                let type_code = &type_info.code;
                let choice_name = self.capitalize_first_letter(type_code);
                // Replace [x] in the full path (e.g., "Patient.value[x]" -> "Patient.valueQuantity")
                let choice_path = base_path.replace("[x]", &choice_name);
                let choice_id = if let Some(parent_id) = &element.id {
                    format!("{}:{}", parent_id, choice_name)
                } else {
                    choice_path.clone()
                };

                if seen.contains(&choice_id) {
                    continue;
                }

                let mut choice_element = element.clone();
                choice_element.id = Some(choice_id.clone());
                choice_element.path = choice_path.clone();
                choice_element.types = Some(vec![ferrum_models::ElementDefinitionType {
                    code: type_code.clone(),
                    profile: type_info.profile.clone(),
                    target_profile: type_info.target_profile.clone(),
                    aggregation: type_info.aggregation.clone(),
                    versioning: type_info.versioning.clone(),
                }]);

                choice_elements.push(choice_element.clone());
                seen.insert(choice_id.clone());

                // Recursively expand if complex
                if self.should_resolve_complex_element(&choice_element, resolution_stack) {
                    let children = self.expand_complex_element(
                        &choice_element,
                        seen,
                        resolution_stack,
                        content_reference_stack,
                        all_elements,
                        context,
                    )?;
                    choice_elements.extend(children);
                }
            }
        }

        Ok(choice_elements)
    }

    /// Extract profile URL from element type, if present
    fn get_type_profile(&self, element: &ElementDefinition) -> Option<String> {
        element
            .types
            .as_ref()
            .and_then(|types| types.first())
            .and_then(|t| t.profile.as_ref())
            .and_then(|profiles| profiles.first())
            .cloned()
    }

    /// Expand complex element
    fn expand_complex_element(
        &self,
        element: &ElementDefinition,
        seen: &mut HashSet<String>,
        resolution_stack: &mut Vec<ResolutionContext>,
        _content_reference_stack: &mut Vec<String>,
        _all_elements: &[&ElementDefinition],
        context: &dyn FhirContext,
    ) -> Result<Vec<ElementDefinition>> {
        let type_info = element
            .types
            .as_ref()
            .and_then(|types| types.first())
            .ok_or_else(|| Error::Expansion("Complex element missing type".into()))?;

        let type_code = &type_info.code;
        let normalized_type = self.normalize_type_code(type_code);
        let element_path = self.get_element_path(element);
        let element_id = self.get_element_id(element)?;

        // Check recursion limits
        let current_depth = resolution_stack
            .iter()
            .filter(|ctx| ctx.type_code == normalized_type)
            .count();

        let max_depth = self
            .max_recursion_depth
            .get(&normalized_type)
            .copied()
            .unwrap_or(5);

        if current_depth >= max_depth {
            return Ok(Vec::new());
        }

        // Get StructureDefinition - prefer profile if specified
        let canonical_url = if let Some(profile_url) = self.get_type_profile(element) {
            // Use the profile URL directly
            profile_url
        } else {
            // Fall back to base type
            format!(
                "http://hl7.org/fhir/StructureDefinition/{}",
                normalized_type
            )
        };

        // If the profile/base type cannot be resolved, skip expanding further instead of failing
        let structure_def = match context.get_structure_definition(&canonical_url) {
            Ok(Some(sd)) => sd,
            Ok(None) => {
                eprintln!(
                    "warn: StructureDefinition not found for {}, skipping expansion",
                    canonical_url
                );
                return Ok(Vec::new());
            }
            Err(e) => {
                eprintln!(
                    "warn: failed to resolve StructureDefinition {}: {}, skipping expansion",
                    canonical_url, e
                );
                return Ok(Vec::new());
            }
        };

        // Get snapshot elements
        let snapshot = match structure_def.snapshot.as_ref() {
            Some(snap) => snap,
            None => {
                eprintln!(
                    "warn: StructureDefinition {} missing snapshot, skipping expansion",
                    canonical_url
                );
                return Ok(Vec::new());
            }
        };

        let struct_type = &structure_def.type_;

        let mut complex_elements = Vec::new();
        // Local view of imported elements for contentReference resolution within this type
        let imported_all_elements: Vec<&ElementDefinition> = snapshot.element.iter().collect();

        resolution_stack.push(ResolutionContext {
            type_code: normalized_type.clone(),
            path: element_path.clone(),
            depth: current_depth,
        });

        for child_element in &snapshot.element {
            let child_path = &child_element.path;

            if child_path.is_empty() {
                continue;
            }

            // Replace struct type with element path (only first occurrence)
            let new_path = child_path.replacen(struct_type, &element_path, 1);
            let child_id = child_element.id.as_deref().unwrap_or(child_path);
            let new_id = child_id.replacen(struct_type, &element_id, 1);

            if seen.contains(&new_id) {
                continue;
            }

            let mut resolved_child = child_element.clone();
            resolved_child.path = new_path.clone();
            resolved_child.id = Some(new_id.clone());

            // Optionally rewrite base.path to keep alignment with new path
            if let Some(ref mut base) = resolved_child.base {
                base.path = new_path.clone();
            }

            complex_elements.push(resolved_child.clone());
            seen.insert(new_id.clone());

            // Recursively expand complex type children
            if self.should_resolve_complex_element(&resolved_child, resolution_stack) {
                let grandchildren = self.expand_complex_element(
                    &resolved_child,
                    seen,
                    resolution_stack,
                    _content_reference_stack,
                    &imported_all_elements,
                    context,
                )?;
                complex_elements.extend(grandchildren);
            }
        }

        resolution_stack.pop();

        Ok(complex_elements)
    }
}

impl Default for SnapshotExpander {
    fn default() -> Self {
        Self::new()
    }
}
