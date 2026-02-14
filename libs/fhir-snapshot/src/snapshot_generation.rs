//! Snapshot and differential generation for StructureDefinition resources
//!
//! This module provides high-level functions for generating snapshots and differentials
//! from StructureDefinition resources (defined in fhir-models).
//!
//! It handles merging metadata and elements according to FHIR profiling rules.

use crate::error::{Error, Result};
use crate::generator::{
    generate_differential as generate_diff_elements, generate_snapshot_internal,
    post_process_snapshot,
};
// Types are used directly from fhir_models
use zunder_context::FhirContext;
use zunder_models::common::structure_definition::TypeDerivationRule;
use zunder_models::StructureDefinition;

// No conversion needed - we're using fhir_models types directly

/// Generate a complete StructureDefinition with snapshot from base + differential
///
/// This function:
/// 1. Takes base StructureDefinition and differential StructureDefinition
/// 2. Merges metadata from both (url, name, title, status, etc.)
/// 3. Generates snapshot from base.snapshot + derived.differential
/// 4. Returns complete StructureDefinition with both snapshot and differential
///
/// # Parameters
/// - `base_sd`: The base StructureDefinition resource (optional, will be resolved from baseDefinition if None)
/// - `derived_sd`: The derived StructureDefinition resource (with differential)
/// - `context`: FHIR context for resolving base type definitions
pub fn generate_structure_definition_snapshot(
    base_sd: Option<&StructureDefinition>,
    derived_sd: &StructureDefinition,
    context: &dyn FhirContext,
) -> Result<StructureDefinition> {
    // Validate derived has differential
    let derived_diff_models = derived_sd.differential.as_ref().ok_or_else(|| {
        Error::Differential("Derived StructureDefinition missing differential".into())
    })?;

    // Resolve base StructureDefinition (either provided or via baseDefinition URL)
    let resolved_base_sd = resolve_base_structure_definition(base_sd, derived_sd, context)?;

    // If the base SD only has a differential (is itself a profile), recursively generate its snapshot
    let resolved_base_sd = if resolved_base_sd.snapshot.is_none()
        && resolved_base_sd.differential.is_some()
    {
        generate_structure_definition_snapshot(None, &resolved_base_sd, context)?
    } else {
        resolved_base_sd
    };

    // Extract base snapshot
    let base_snapshot_models = resolved_base_sd
        .snapshot
        .as_ref()
        .ok_or_else(|| Error::Snapshot("Base StructureDefinition missing snapshot".into()))?;

    // use zunder_models types directly
    let base_snapshot = base_snapshot_models;
    let derived_diff = derived_diff_models;

    // Generate new snapshot - pass base SD for better element lookup
    let mut new_snapshot = generate_snapshot_internal(
        base_snapshot,
        derived_diff,
        Some(&resolved_base_sd),
        context,
    )?;

    // Post-process: expand fragment contentReferences, default slicing.ordered
    post_process_snapshot(&mut new_snapshot, context);

    // Merge StructureDefinition metadata
    let mut result_sd = merge_structure_definition_metadata(&resolved_base_sd, derived_sd);

    // Use snapshot directly (already fhir_models type)
    result_sd.snapshot = Some(new_snapshot);

    // Keep the original differential from derived
    result_sd.differential = derived_sd.differential.clone();

    Ok(result_sd)
}

/// Generate a differential StructureDefinition by comparing derived to base
///
/// This function:
/// 1. Takes base StructureDefinition and derived StructureDefinition
/// 2. Computes differential from derived.snapshot vs base.snapshot
/// 3. Returns StructureDefinition with differential (no snapshot)
pub fn generate_structure_definition_differential(
    base_sd: &StructureDefinition,
    derived_sd: &StructureDefinition,
) -> Result<StructureDefinition> {
    // Extract snapshots
    let base_snapshot_models = base_sd
        .snapshot
        .as_ref()
        .ok_or_else(|| Error::Snapshot("Base StructureDefinition missing snapshot".into()))?;

    let derived_snapshot_models = derived_sd
        .snapshot
        .as_ref()
        .ok_or_else(|| Error::Snapshot("Derived StructureDefinition missing snapshot".into()))?;

    // use zunder_models types directly
    let base_snapshot = base_snapshot_models;
    let derived_snapshot = derived_snapshot_models;

    // Generate differential
    let differential = generate_diff_elements(base_snapshot, derived_snapshot)?;

    // Create result StructureDefinition with metadata from derived
    let mut result_sd = merge_structure_definition_metadata(base_sd, derived_sd);

    // Use differential directly (already fhir_models type)
    result_sd.differential = Some(differential);

    // Remove snapshot (differential-only profile)
    result_sd.snapshot = None;

    Ok(result_sd)
}

/// Merge StructureDefinition metadata
///
/// Takes metadata from derived, using base as fallback for missing fields
fn merge_structure_definition_metadata(
    base_sd: &StructureDefinition,
    derived_sd: &StructureDefinition,
) -> StructureDefinition {
    let mut result = derived_sd.clone();

    // Fill missing optional fields from base
    if result.experimental.is_none() {
        result.experimental = base_sd.experimental;
    }
    if result.date.is_none() {
        result.date = base_sd.date.clone();
    }
    if result.publisher.is_none() {
        result.publisher = base_sd.publisher.clone();
    }
    if result.contact.is_none() {
        result.contact = base_sd.contact.clone();
    }
    if result.description.is_none() {
        result.description = base_sd.description.clone();
    }
    if result.use_context.is_none() {
        result.use_context = base_sd.use_context.clone();
    }
    if result.jurisdiction.is_none() {
        result.jurisdiction = base_sd.jurisdiction.clone();
    }
    if result.purpose.is_none() {
        result.purpose = base_sd.purpose.clone();
    }
    if result.copyright.is_none() {
        result.copyright = base_sd.copyright.clone();
    }

    // Merge extensions - derived extensions override base extensions by URL
    let mut merged_extensions = base_sd.extensions.clone();
    for (key, value) in &derived_sd.extensions {
        merged_extensions.insert(key.clone(), value.clone());
    }
    result.extensions = merged_extensions;

    // Structural fields with fallback
    if result.fhir_version.is_none() {
        result.fhir_version = base_sd.fhir_version.clone();
    }
    if result.mapping.is_none() {
        result.mapping = base_sd.mapping.clone();
    }

    // Type should be preserved from derived (already set)
    // baseDefinition only from derived (profile identity), so keep existing

    // Derivation default
    if result.derivation.is_none() {
        result.derivation = Some(TypeDerivationRule::Constraint);
    }

    result
}

/// Resolve the base StructureDefinition either from the provided value or by fetching from context using baseDefinition.
fn resolve_base_structure_definition(
    base_sd: Option<&StructureDefinition>,
    derived_sd: &StructureDefinition,
    context: &dyn FhirContext,
) -> Result<StructureDefinition> {
    if let Some(sd) = base_sd {
        return Ok(sd.clone());
    }

    let base_url = derived_sd.base_definition.as_ref().ok_or_else(|| {
        Error::Snapshot("Derived StructureDefinition missing baseDefinition".into())
    })?;

    let base_sd = context
        .get_structure_definition(base_url)
        .map_err(|e| Error::Snapshot(e.to_string()))?
        .ok_or_else(|| {
            Error::Snapshot(format!("Base StructureDefinition not found: {}", base_url))
        })?;

    Ok((*base_sd).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zunder_models::common::complex::PublicationStatus;
    use zunder_models::common::structure_definition::StructureDefinitionKind;

    #[test]
    fn merges_metadata_correctly() {
        let mut base = StructureDefinition::new(
            "http://example.org/base",
            "BaseProfile",
            StructureDefinitionKind::Resource,
            "Patient",
        );
        base.version = Some("1.0.0".to_string());
        base.status = PublicationStatus::Active;
        base.publisher = Some("Base Publisher".to_string());
        base.description = Some("Base description".to_string());

        let mut derived = StructureDefinition::new(
            "http://example.org/derived",
            "DerivedProfile",
            StructureDefinitionKind::Resource,
            "Patient",
        );
        derived.version = Some("2.0.0".to_string());
        derived.title = Some("Derived Profile Title".to_string());
        derived.status = PublicationStatus::Draft;
        derived.description = Some("Derived description".to_string());
        derived.base_definition = Some("http://example.org/base".to_string());

        let result = merge_structure_definition_metadata(&base, &derived);

        // Derived values should win
        assert_eq!(result.url, "http://example.org/derived");
        assert_eq!(result.version, Some("2.0.0".to_string()));
        assert_eq!(result.name, "DerivedProfile");
        assert_eq!(result.status, PublicationStatus::Draft);
        assert_eq!(result.description, Some("Derived description".to_string()));

        // Publisher falls back to base
        assert_eq!(result.publisher, Some("Base Publisher".to_string()));

        // Title from derived
        assert_eq!(result.title, Some("Derived Profile Title".to_string()));
    }

    #[test]
    fn merges_extensions() {
        let mut base = StructureDefinition::new(
            "http://example.org/base",
            "Base",
            StructureDefinitionKind::Resource,
            "Patient",
        );
        base.extensions
            .insert("http://example.org/ext1".to_string(), json!("base-value1"));
        base.extensions
            .insert("http://example.org/ext2".to_string(), json!("base-value2"));

        let mut derived = StructureDefinition::new(
            "http://example.org/derived",
            "Derived",
            StructureDefinitionKind::Resource,
            "Patient",
        );
        derived.extensions.insert(
            "http://example.org/ext1".to_string(),
            json!("derived-value1"),
        );
        derived.extensions.insert(
            "http://example.org/ext3".to_string(),
            json!("derived-value3"),
        );

        let result = merge_structure_definition_metadata(&base, &derived);

        // Should have 3 extensions: ext2 from base, ext1 and ext3 from derived
        assert_eq!(result.extensions.len(), 3);

        // ext1 should be from derived (override)
        assert_eq!(
            result.extensions.get("http://example.org/ext1"),
            Some(&json!("derived-value1"))
        );

        // ext2 should be from base
        assert_eq!(
            result.extensions.get("http://example.org/ext2"),
            Some(&json!("base-value2"))
        );

        // ext3 should be from derived
        assert_eq!(
            result.extensions.get("http://example.org/ext3"),
            Some(&json!("derived-value3"))
        );
    }
}
