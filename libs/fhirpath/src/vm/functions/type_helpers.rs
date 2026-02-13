//! Type checking and type descriptor utilities for FHIRPath functions.
//!
//! This module provides functions for type inference, type matching, and type descriptor
//! generation used by type-related functions like `is()`, `as()`, `ofType()`, and `type()`.

use crate::context::Context;
use crate::error::{Error, Result};
use crate::value::{Collection, Value, ValueData};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use zunder_context::FhirContext;

fn is_numeric(col: Option<&Collection>) -> bool {
    col.and_then(|c| c.iter().next())
        .map(|v| matches!(v.data(), ValueData::Integer(_) | ValueData::Decimal(_)))
        .unwrap_or(false)
}

fn keys_within(obj: &HashMap<Arc<str>, Collection>, allowed: &[&str]) -> bool {
    obj.keys().all(|k| allowed.iter().any(|a| k.as_ref() == *a))
}

fn has_choice_value_key(obj: &HashMap<Arc<str>, Collection>) -> bool {
    obj.keys().any(|k| {
        let s = k.as_ref();
        s.starts_with("value") && s.len() > 5 && s.as_bytes()[5].is_ascii_uppercase()
    })
}

fn infer_structural_fhir_type_from_object(
    obj: &HashMap<Arc<str>, Collection>,
) -> Option<&'static str> {
    // The checks here are intentionally conservative and ordered from most-specific to least-specific
    // to reduce false positives in ambiguous JSON objects.

    // Meta
    if obj.contains_key("versionId")
        || obj.contains_key("lastUpdated")
        || obj.contains_key("profile")
        || obj.contains_key("security")
        || obj.contains_key("tag")
        || obj.contains_key("source")
    {
        return Some("Meta");
    }

    // Narrative
    if obj.contains_key("div") && obj.contains_key("status") {
        return Some("Narrative");
    }

    // Extension
    if obj.contains_key("url") && (obj.contains_key("extension") || has_choice_value_key(obj)) {
        return Some("Extension");
    }

    // Attachment
    if obj.contains_key("contentType")
        && (obj.contains_key("data")
            || obj.contains_key("url")
            || obj.contains_key("size")
            || obj.contains_key("hash")
            || obj.contains_key("title")
            || obj.contains_key("creation"))
    {
        return Some("Attachment");
    }

    // Signature
    if obj.contains_key("type")
        && obj.contains_key("when")
        && obj.contains_key("who")
        && (obj.contains_key("data") || obj.contains_key("sigFormat"))
    {
        return Some("Signature");
    }

    // SampledData
    if obj.contains_key("origin") && obj.contains_key("data") && obj.contains_key("dimensions") {
        return Some("SampledData");
    }

    // RatioRange
    if obj.contains_key("denominator")
        && (obj.contains_key("lowNumerator") || obj.contains_key("highNumerator"))
    {
        return Some("RatioRange");
    }

    // Ratio
    if obj.contains_key("numerator") && obj.contains_key("denominator") {
        return Some("Ratio");
    }

    // UsageContext
    if obj.contains_key("code")
        && (obj.contains_key("valueCodeableConcept")
            || obj.contains_key("valueQuantity")
            || obj.contains_key("valueRange")
            || obj.contains_key("valueReference"))
    {
        return Some("UsageContext");
    }

    // TriggerDefinition
    if obj.contains_key("type")
        && (obj.contains_key("condition")
            || obj.contains_key("data")
            || obj.contains_key("timingTiming")
            || obj.contains_key("timingReference")
            || obj.contains_key("timingDate")
            || obj.contains_key("timingDateTime"))
    {
        return Some("TriggerDefinition");
    }

    // RelatedArtifact
    if obj.contains_key("type")
        && (obj.contains_key("url")
            || obj.contains_key("resource")
            || obj.contains_key("citation")
            || obj.contains_key("document")
            || obj.contains_key("display"))
    {
        return Some("RelatedArtifact");
    }

    // Expression
    if obj.contains_key("language") && obj.contains_key("expression") {
        return Some("Expression");
    }

    // ParameterDefinition
    if obj.contains_key("use") && obj.contains_key("type") {
        return Some("ParameterDefinition");
    }

    // DataRequirement
    if obj.contains_key("type")
        && (obj.contains_key("codeFilter")
            || obj.contains_key("dateFilter")
            || obj.contains_key("mustSupport")
            || obj.contains_key("sort")
            || obj.contains_key("limit"))
    {
        return Some("DataRequirement");
    }

    // Availability
    if obj.contains_key("availableTime") || obj.contains_key("notAvailableTime") {
        return Some("Availability");
    }

    // ExtendedContactDetail (distinguish from ContactDetail)
    if obj.contains_key("telecom")
        && (obj.contains_key("address")
            || obj.contains_key("organization")
            || obj.contains_key("purpose"))
        && keys_within(
            obj,
            &[
                "id",
                "extension",
                "purpose",
                "name",
                "telecom",
                "address",
                "organization",
                "period",
            ],
        )
    {
        return Some("ExtendedContactDetail");
    }

    // ContactDetail
    if obj.contains_key("telecom") && keys_within(obj, &["id", "extension", "name", "telecom"]) {
        return Some("ContactDetail");
    }

    // CodeableReference (prefer over Reference)
    if obj.contains_key("concept") {
        return Some("CodeableReference");
    }

    // Annotation
    if obj.contains_key("text")
        && (obj.contains_key("authorString")
            || obj.contains_key("authorReference")
            || obj.contains_key("time"))
    {
        return Some("Annotation");
    }

    // Dosage
    if obj.contains_key("doseAndRate")
        || (obj.contains_key("timing")
            && (obj.contains_key("route")
                || obj.contains_key("site")
                || obj.contains_key("method")
                || obj.contains_key("asNeededBoolean")
                || obj.contains_key("asNeededCodeableConcept")))
    {
        return Some("Dosage");
    }

    // Timing
    if obj.contains_key("repeat") || obj.contains_key("event") {
        return Some("Timing");
    }

    // ContactPoint
    if obj.contains_key("system")
        && obj.contains_key("value")
        && !obj.contains_key("code")
        && (obj.contains_key("use") || obj.contains_key("rank") || obj.contains_key("period"))
    {
        return Some("ContactPoint");
    }

    // Address
    if obj.contains_key("line")
        || obj.contains_key("city")
        || obj.contains_key("state")
        || obj.contains_key("postalCode")
        || obj.contains_key("country")
    {
        return Some("Address");
    }

    // HumanName
    if obj.contains_key("family")
        || obj.contains_key("given")
        || obj.contains_key("prefix")
        || obj.contains_key("suffix")
    {
        return Some("HumanName");
    }

    // Money
    if is_numeric(obj.get("value")) && obj.contains_key("currency") {
        return Some("Money");
    }

    // Quantity: must have numeric value and typically has unit/system/code.
    if is_numeric(obj.get("value"))
        && (obj.contains_key("unit") || obj.contains_key("code") || obj.contains_key("system"))
    {
        return Some("Quantity");
    }

    // Period
    if obj.contains_key("start") || obj.contains_key("end") {
        return Some("Period");
    }

    // Range
    if obj.contains_key("low") || obj.contains_key("high") {
        return Some("Range");
    }

    // CodeableConcept
    if obj.contains_key("coding") || obj.contains_key("text") {
        return Some("CodeableConcept");
    }

    // Coding
    if (obj.contains_key("code") && obj.contains_key("system"))
        || (obj.contains_key("code") && obj.contains_key("display"))
    {
        return Some("Coding");
    }

    // Identifier
    if obj.contains_key("value") && !obj.contains_key("code") {
        return Some("Identifier");
    }

    // Reference
    if obj.contains_key("reference")
        || (obj.contains_key("identifier") && obj.contains_key("type"))
        || obj.contains_key("display")
    {
        return Some("Reference");
    }

    None
}

/// Type descriptor for reflection helpers
#[derive(Debug, Clone)]
pub struct TypeDescriptor {
    pub namespace: &'static str,
    pub name: String,
    pub base_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimitiveStringKind {
    Uri,
    Uuid,
    Oid,
    Code,
    PlainString,
}

fn is_uuid_like(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split('-').collect();
    if parts.len() == 5 {
        return parts[0].len() == 8
            && parts[1].len() == 4
            && parts[2].len() == 4
            && parts[3].len() == 4
            && parts[4].len() == 12
            && parts
                .iter()
                .all(|part| part.chars().all(|c| c.is_ascii_hexdigit()));
    }
    lower.starts_with("urn:uuid:")
        && lower[9..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn is_oid_like(value: &str) -> bool {
    value.starts_with("urn:oid:") && value[8..].chars().all(|c| c.is_ascii_digit() || c == '.')
}

fn is_uri_like(value: &str) -> bool {
    value.contains("://") || value.starts_with("urn:")
}

fn is_code_like(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && value.chars().any(|c| c.is_ascii_alphabetic())
}

fn classify_fhir_string(value: &str) -> PrimitiveStringKind {
    if is_uuid_like(value) {
        PrimitiveStringKind::Uuid
    } else if is_oid_like(value) {
        PrimitiveStringKind::Oid
    } else if is_uri_like(value) {
        PrimitiveStringKind::Uri
    } else if is_code_like(value) {
        PrimitiveStringKind::Code
    } else {
        PrimitiveStringKind::PlainString
    }
}

fn normalize_type_specifier(spec: &str) -> (Option<String>, String) {
    let trimmed = spec.trim().trim_start_matches('@');
    if let Some((ns, name)) = trimmed.split_once('.') {
        (Some(ns.to_string()), name.to_string())
    } else {
        (None, trimmed.to_string())
    }
}

fn fhir_type_exists(fc: &dyn FhirContext, type_name: &str) -> bool {
    if fc
        .get_core_structure_definition_by_type(type_name)
        .ok()
        .flatten()
        .is_some()
    {
        return true;
    }

    // Try a simple ASCII title-case variant (e.g., "patient" -> "Patient").
    let mut chars = type_name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let candidate = format!("{}{}", first.to_ascii_uppercase(), chars.as_str());
    fc.get_core_structure_definition_by_type(&candidate)
        .ok()
        .flatten()
        .is_some()
}

/// Validate that a type specifier is recognized in the active model(s).
///
/// Used by `is()`, `as()`, and `ofType()` so unknown identifiers surface as execution errors.
pub fn validate_type_specifier(
    type_spec: &str,
    fhir_context: Option<&dyn FhirContext>,
) -> Result<()> {
    let (ns, name_raw) = normalize_type_specifier(type_spec);
    let name = name_raw.trim_matches('`');
    if name.is_empty() {
        return Err(Error::InvalidOperation("Empty type specifier".into()));
    }

    let ns_lower = ns.as_deref().map(|s| s.to_ascii_lowercase());
    let lower = name.to_ascii_lowercase();
    let is_system_type = matches!(
        lower.as_str(),
        "boolean" | "string" | "integer" | "decimal" | "datetime" | "date" | "time" | "quantity"
    );
    match ns_lower.as_deref() {
        Some("system") => {
            // Per HL7 test suite notes, unknown System types should not raise an execution error for `is/as/ofType`;
            // they simply won't match.
            let _ = is_system_type;
            Ok(())
        }
        Some("fhir") => {
            if let Some(fc) = fhir_context {
                if fhir_type_exists(fc, name) {
                    Ok(())
                } else {
                    Err(Error::InvalidOperation(format!(
                        "Unknown FHIR type '{}'",
                        name
                    )))
                }
            } else {
                Ok(())
            }
        }
        None => {
            // Unqualified type names may refer to System types (e.g. Boolean, Integer, DateTime) or model types.
            if is_system_type {
                return Ok(());
            }
            if let Some(fc) = fhir_context {
                if fhir_type_exists(fc, name) {
                    Ok(())
                } else {
                    Err(Error::InvalidOperation(format!("Unknown type '{}'", name)))
                }
            } else {
                Ok(())
            }
        }
        Some(other) => Err(Error::InvalidOperation(format!(
            "Unknown namespace '{}'",
            other
        ))),
    }
}

pub(super) fn normalize_type_code(code: &str) -> String {
    if let Some(stripped) = code
        .strip_prefix("http://hl7.org/fhirpath/System.")
        .or_else(|| code.strip_prefix("System."))
    {
        return stripped.to_ascii_lowercase();
    }
    if let Some(stripped) = code
        .strip_prefix("http://hl7.org/fhir/StructureDefinition/")
        .or_else(|| code.strip_prefix("FHIR."))
    {
        return stripped.to_ascii_lowercase();
    }
    code.to_ascii_lowercase()
}

fn fhir_namespace_hint(path_hint: Option<&str>, value: &ValueData) -> bool {
    match value {
        ValueData::Object(_) | ValueData::Quantity { .. } => true,
        _ => path_hint.is_some(),
    }
}

fn is_system_primitive_identifier(spec: &str) -> bool {
    match spec.to_ascii_lowercase().as_str() {
        // Note: "quantity" is intentionally excluded - in FHIR context, Quantity refers to
        // the FHIR Quantity datatype, not System.Quantity
        "boolean" | "string" | "integer" | "decimal" | "datetime" | "date" | "time" => true,
        _ => false,
    }
}

fn type_hint_from_path(path_hint: Option<&str>) -> Option<String> {
    let raw_path = path_hint?.trim_matches('.');
    let segment = raw_path.rsplit('.').find(|s| !s.is_empty())?;
    if segment.is_empty() {
        return None;
    }

    // If the segment contains a capitalized suffix (choice types), prefer that
    if let Some((idx, _)) = segment.char_indices().find(|(_, c)| c.is_uppercase()) {
        let suffix = &segment[idx..];
        if !suffix.is_empty() {
            return Some(suffix.to_string());
        }
    }

    Some(segment.to_string())
}

pub(super) fn choose_declared_type_for_value(
    item: &Value,
    declared_types: &[String],
    path_hint: Option<&str>,
) -> Option<String> {
    if declared_types.is_empty() {
        return None;
    }
    if declared_types.len() == 1 {
        return Some(declared_types[0].to_ascii_lowercase());
    }

    // Prefer explicit choice variant derived from the path (e.g., valueString → string).
    if let Some(type_hint) = type_hint_from_path(path_hint) {
        if hint_looks_like_type(&type_hint) {
            let hint_lower = type_hint.to_ascii_lowercase();
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&hint_lower))
            {
                return Some(hint_lower);
            }
        }
    }

    // Otherwise, select a declared type compatible with the runtime kind.
    let inferred = infer_type_descriptor(item, path_hint)
        .name
        .to_ascii_lowercase();
    if declared_types
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&inferred))
    {
        return Some(inferred);
    }

    match item.data() {
        ValueData::String(_) => {
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case("string"))
            {
                return Some("string".to_string());
            }
        }
        ValueData::Boolean(_) => {
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case("boolean"))
            {
                return Some("boolean".to_string());
            }
        }
        ValueData::Integer(_) => {
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case("integer"))
            {
                return Some("integer".to_string());
            }
        }
        ValueData::Decimal(_) => {
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case("decimal"))
            {
                return Some("decimal".to_string());
            }
        }
        ValueData::Date { .. } => {
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case("date"))
            {
                return Some("date".to_string());
            }
        }
        ValueData::DateTime { .. } => {
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case("datetime"))
            {
                return Some("datetime".to_string());
            }
        }
        ValueData::Time { .. } => {
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case("time"))
            {
                return Some("time".to_string());
            }
        }
        ValueData::Quantity { .. } => {
            if declared_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case("quantity"))
            {
                return Some("quantity".to_string());
            }
        }
        ValueData::Object(obj) => {
            if let Some(inferred) = infer_structural_fhir_type_from_object(obj) {
                let inferred_lower = inferred.to_ascii_lowercase();
                if declared_types
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(&inferred_lower))
                {
                    return Some(inferred_lower);
                }
            }
        }
        ValueData::LazyJson { .. } => {
            // Materialize lazy JSON first and recursively match
            let materialized = item.materialize();
            return choose_declared_type_for_value(&materialized, declared_types, path_hint);
        }
        ValueData::Empty => {}
    }

    None
}

pub(super) fn infer_type_descriptor(item: &Value, path_hint: Option<&str>) -> TypeDescriptor {
    let value = item.data();
    let is_fhir = fhir_namespace_hint(path_hint, value);

    match value {
        ValueData::Boolean(_) => TypeDescriptor {
            namespace: if is_fhir { "FHIR" } else { "System" },
            name: if is_fhir {
                "boolean".to_string()
            } else {
                "Boolean".to_string()
            },
            base_type: None,
        },
        ValueData::Integer(_) => TypeDescriptor {
            namespace: if is_fhir { "FHIR" } else { "System" },
            name: if is_fhir {
                "integer".to_string()
            } else {
                "Integer".to_string()
            },
            base_type: None,
        },
        ValueData::Decimal(_) => TypeDescriptor {
            namespace: if is_fhir { "FHIR" } else { "System" },
            name: if is_fhir {
                "decimal".to_string()
            } else {
                "Decimal".to_string()
            },
            base_type: None,
        },
        ValueData::String(s) => {
            if is_fhir {
                // FIRST: Try to use path hint if available (more definitive than heuristics)
                if let Some(type_from_hint) = type_hint_from_path(path_hint) {
                    let hint_lower = type_from_hint.to_ascii_lowercase();
                    // Map common path hints to FHIR primitive types
                    match hint_lower.as_str() {
                        "string" => {
                            return TypeDescriptor {
                                namespace: "FHIR",
                                name: "string".to_string(),
                                base_type: None,
                            };
                        }
                        "uuid" => {
                            return TypeDescriptor {
                                namespace: "FHIR",
                                name: "uuid".to_string(),
                                base_type: Some("uri".to_string()),
                            };
                        }
                        "uri" | "url" | "canonical" => {
                            return TypeDescriptor {
                                namespace: "FHIR",
                                name: hint_lower,
                                base_type: Some("string".to_string()),
                            };
                        }
                        "oid" => {
                            return TypeDescriptor {
                                namespace: "FHIR",
                                name: "oid".to_string(),
                                base_type: Some("uri".to_string()),
                            };
                        }
                        "code" | "id" => {
                            return TypeDescriptor {
                                namespace: "FHIR",
                                name: hint_lower,
                                base_type: Some("string".to_string()),
                            };
                        }
                        "markdown" => {
                            return TypeDescriptor {
                                namespace: "FHIR",
                                name: "markdown".to_string(),
                                base_type: Some("string".to_string()),
                            };
                        }
                        _ => {
                            // Path hint didn't give us a primitive type, fall through to classification
                        }
                    }
                }

                // SECOND: Fall back to heuristic classification if no path hint
                match classify_fhir_string(s.as_ref()) {
                    PrimitiveStringKind::Uri => TypeDescriptor {
                        namespace: "FHIR",
                        name: "uri".to_string(),
                        base_type: Some("string".to_string()),
                    },
                    PrimitiveStringKind::Uuid => TypeDescriptor {
                        namespace: "FHIR",
                        name: "uuid".to_string(),
                        base_type: Some("uri".to_string()),
                    },
                    PrimitiveStringKind::Oid => TypeDescriptor {
                        namespace: "FHIR",
                        name: "oid".to_string(),
                        base_type: Some("uri".to_string()),
                    },
                    PrimitiveStringKind::Code => TypeDescriptor {
                        namespace: "FHIR",
                        name: "code".to_string(),
                        base_type: Some("string".to_string()),
                    },
                    PrimitiveStringKind::PlainString => TypeDescriptor {
                        namespace: "FHIR",
                        name: "string".to_string(),
                        base_type: None,
                    },
                }
            } else {
                TypeDescriptor {
                    namespace: "System",
                    name: "String".to_string(),
                    base_type: None,
                }
            }
        }
        ValueData::Date { .. } => TypeDescriptor {
            namespace: if is_fhir { "FHIR" } else { "System" },
            name: if is_fhir {
                "date".to_string()
            } else {
                "Date".to_string()
            },
            base_type: None,
        },
        ValueData::DateTime {
            value: _,
            precision: _,
            timezone_offset: _,
        } => TypeDescriptor {
            namespace: if is_fhir { "FHIR" } else { "System" },
            name: if is_fhir {
                "dateTime".to_string()
            } else {
                "DateTime".to_string()
            },
            base_type: None,
        },
        ValueData::Time {
            value: _,
            precision: _,
        } => TypeDescriptor {
            namespace: if is_fhir { "FHIR" } else { "System" },
            name: if is_fhir {
                "time".to_string()
            } else {
                "Time".to_string()
            },
            base_type: None,
        },
        ValueData::Quantity { .. } => TypeDescriptor {
            namespace: "FHIR",
            name: "Quantity".to_string(),
            base_type: None,
        },
        ValueData::Object(obj) => {
            let resource_type = obj.get("resourceType").and_then(|col| {
                col.iter().next().and_then(|v| match v.data() {
                    ValueData::String(s) => Some(s.as_ref().to_string()),
                    _ => None,
                })
            });

            let name = if let Some(rt) = resource_type {
                rt
            } else if let Some(type_hint) = type_hint_from_path(path_hint) {
                if hint_looks_like_type(&type_hint) {
                    type_hint
                } else if let Some(structural) = infer_structural_fhir_type_from_object(obj) {
                    structural.to_string()
                } else {
                    "Element".to_string()
                }
            } else if let Some(structural) = infer_structural_fhir_type_from_object(obj) {
                structural.to_string()
            } else {
                "Element".to_string()
            };

            TypeDescriptor {
                namespace: "FHIR",
                name,
                base_type: None,
            }
        }
        ValueData::LazyJson { .. } => {
            // Materialize lazy JSON first and recursively infer type descriptor
            let materialized = item.materialize();
            infer_type_descriptor(&materialized, path_hint)
        }
        ValueData::Empty => TypeDescriptor {
            namespace: "System",
            name: "Empty".to_string(),
            base_type: None,
        },
    }
}

/// Check if a value matches a System type
fn check_system_type(item: &Value, type_name: &str) -> bool {
    let type_name_lower = type_name.to_ascii_lowercase();
    match item.data() {
        ValueData::Boolean(_) => type_name_lower == "boolean",
        ValueData::String(_) => type_name_lower == "string",
        ValueData::Integer(_) => type_name_lower == "integer",
        ValueData::Decimal(_) => type_name_lower == "decimal",
        ValueData::Date { .. } => type_name_lower == "date",
        ValueData::DateTime {
            value: _,
            precision: _,
            timezone_offset: _,
        } => type_name_lower == "datetime",
        ValueData::Time {
            value: _,
            precision: _,
        } => type_name_lower == "time",
        ValueData::Quantity { .. } => type_name_lower == "quantity",
        _ => false,
    }
}

/// Check if a value has a definitive runtime type that can be determined
/// Returns true if the value has clear type indicators (like resourceType or specific fields)
///
/// Note: Primitives (String, Integer, Boolean) return FALSE because they may be subtypes
/// (e.g., String could be uri, uuid, code, etc.) and need path hints for disambiguation
fn has_definitive_runtime_type(item: &Value) -> bool {
    match item.data() {
        ValueData::LazyJson { .. } => match item.data().resolved_json() {
            Some(JsonValue::Object(obj)) => obj.contains_key("resourceType"),
            _ => false,
        },
        ValueData::Object(obj) => {
            // Has resourceType - definitive FHIR resource
            if obj.contains_key("resourceType") {
                return true;
            }
            // Many FHIR datatypes can only be disambiguated with path hints (e.g., Age vs Quantity).
            // Treat generic Quantity-shaped objects as non-definitive so path hints can refine them.
            match infer_structural_fhir_type_from_object(obj) {
                Some("Quantity") => false,
                Some(_) => true,
                None => false,
            }
        }
        // Primitives do NOT have definitive types for FHIR purposes
        // A String could be uri, uuid, code, id, etc. - need path hints to disambiguate
        _ => false,
    }
}

/// Check if a value matches a FHIR type (with inheritance support)
fn check_fhir_type(
    item: &Value,
    type_name: &str,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
    exact_match: bool,
) -> bool {
    let target_type_lower = type_name.to_ascii_lowercase();

    // Materialize LazyJson before type checking
    let item = item.materialize();

    if let ValueData::Object(obj) = item.data() {
        // FIRST: Structural inference for common complex datatypes when `resourceType` is absent.
        if let Some(inferred) = infer_structural_fhir_type_from_object(obj) {
            if inferred.eq_ignore_ascii_case(&target_type_lower) {
                return true;
            }
        }
    }

    let descriptor = infer_type_descriptor(&item, path_hint);

    // SECOND: If item is a FHIR resource/object with resourceType, check it
    if let ValueData::Object(obj) = item.data() {
        if let Some(rt_col) = obj.get("resourceType") {
            if let Some(rt_val) = rt_col.iter().next() {
                if let ValueData::String(rt) = rt_val.data() {
                    let actual_type = rt.as_ref().to_ascii_lowercase();
                    let target_type = type_name.to_ascii_lowercase();

                    if exact_match {
                        // For 'as', require exact match
                        return actual_type == target_type;
                    } else {
                        // For 'is', check inheritance: actual_type == target_type or any base type matches target_type.
                        if actual_type == target_type {
                            return true;
                        }

                        if let Some(fc) = fhir_context {
                            return fhir_type_is_a(fc, rt.as_ref(), &target_type);
                        }
                    }
                }
            }
        }
    }

    // For primitive string types, prefer declared types from the FHIR context when available.
    let mut actual_name = descriptor.name.to_ascii_lowercase();
    if matches!(item.data(), ValueData::String(_)) {
        if let (Some(fc), Some(path)) = (fhir_context, path_hint) {
            let resource_type = match ctx.resource.data() {
                ValueData::Object(root_obj) => root_obj
                    .get("resourceType")
                    .and_then(|col| col.iter().next())
                    .and_then(|val| match val.data() {
                        ValueData::String(s) => Some(s.as_ref().to_string()),
                        _ => None,
                    }),
                ValueData::LazyJson { .. } => ctx
                    .resource
                    .data()
                    .resolved_json()
                    .and_then(|v| v.get("resourceType"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                _ => None,
            };

            if let Some(rt) = resource_type {
                if let Ok(Some(elem)) = fc.resolve_path_type(&rt, path) {
                    let declared: Vec<String> = elem
                        .type_codes
                        .iter()
                        .map(|s| normalize_type_code(s))
                        .collect();
                    if let Some(chosen) =
                        choose_declared_type_for_value(&item, &declared, Some(path))
                    {
                        actual_name = chosen;
                    }
                }
            }
        }
    }

    let target_name = type_name.to_ascii_lowercase();

    if exact_match {
        actual_name == target_name
    } else {
        check_type_inheritance(&actual_name, &target_name)
    }
}

/// Check whether `actual_type` is the same as, or a subtype of, `target_type_lower`.
///
/// Uses StructureDefinition.baseDefinition chain (core model only).
fn fhir_type_is_a(fc: &dyn FhirContext, actual_type: &str, target_type_lower: &str) -> bool {
    let mut current = actual_type.to_string();
    loop {
        if current.to_ascii_lowercase() == target_type_lower {
            return true;
        }

        let sd = match fc.get_core_structure_definition_by_type(&current) {
            Ok(Some(sd)) => sd,
            _ => return false,
        };

        let Some(base_def) = sd.base_definition.as_deref() else {
            return false;
        };

        let Some(base_type) = base_def.strip_prefix("http://hl7.org/fhir/StructureDefinition/")
        else {
            return false;
        };

        current = base_type.to_string();
    }
}

/// Centralized FHIR primitive type inheritance rules
/// Returns true if actual_type is-a target_type (including exact match)
fn check_type_inheritance(actual_type: &str, target_type: &str) -> bool {
    if actual_type == target_type {
        return true;
    }

    // FHIR primitive type inheritance hierarchy
    // Format: (child_type, Vec<parent_types>)
    let inheritance_table: &[(&str, &[&str])] = &[
        // String hierarchy
        ("code", &["string"]),
        ("id", &["string"]),
        ("markdown", &["string"]),
        ("uri", &["string"]),
        ("url", &["uri", "string"]),
        ("canonical", &["uri", "string"]),
        ("uuid", &["uri", "string"]),
        ("oid", &["uri", "string"]),
        // Integer hierarchy
        ("positiveint", &["integer"]),
        ("unsignedint", &["integer"]),
        ("integer64", &["integer"]),
        // Decimal hierarchy
        ("decimal", &[]),
        // Quantity hierarchy (complex type, but has inheritance)
        ("age", &["quantity"]),
        ("distance", &["quantity"]),
        ("duration", &["quantity"]),
        ("count", &["quantity"]),
        ("simplequantity", &["quantity"]),
        ("moneyquantity", &["quantity"]),
        // Date/Time hierarchy
        ("datetime", &[]),
        ("date", &[]),
        ("time", &[]),
        ("instant", &[]),
        // Boolean
        ("boolean", &[]),
        // Base64
        ("base64binary", &[]),
    ];

    // Check if actual_type inherits from target_type
    if let Some((_, parents)) = inheritance_table
        .iter()
        .find(|(child, _)| *child == actual_type)
    {
        parents
            .iter()
            .any(|parent| *parent == target_type || check_type_inheritance(parent, target_type))
    } else {
        false
    }
}

/// Check if a value matches a type specifier (used by is/as/ofType/type())
///
/// When no namespace is specified, follows FHIRPath spec:
/// 1. Check FHIR model first (context-specific model)
/// 2. Fallback to System model if not found in FHIR
pub fn matches_type_specifier(
    item: &Value,
    type_spec: &str,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
) -> bool {
    matches_type_specifier_internal(item, type_spec, path_hint, fhir_context, ctx, false)
}

/// Check if a value matches a type specifier with exact matching (for 'as' operator)
pub fn matches_type_specifier_exact(
    item: &Value,
    type_spec: &str,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
) -> bool {
    matches_type_specifier_internal(item, type_spec, path_hint, fhir_context, ctx, true)
}

/// Internal type matching logic with clear priority order
fn matches_type_specifier_internal(
    item: &Value,
    type_spec: &str,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
    exact_match: bool,
) -> bool {
    // Parse type specifier
    let (spec_namespace, spec_name_raw) = normalize_type_specifier(type_spec);
    let spec_name = spec_name_raw.trim_matches('`').to_ascii_lowercase();

    // Infer the runtime type of the value
    let descriptor = infer_type_descriptor(item, path_hint);
    let is_fhir_value = descriptor.namespace.eq_ignore_ascii_case("FHIR");

    // Determine effective namespace (handling implicit System namespace for capitalized primitives)
    let effective_namespace = resolve_effective_namespace(&spec_namespace, &spec_name_raw);

    // STRATEGY: Match based on namespace presence and type
    match effective_namespace.as_deref() {
        Some("system") => {
            // Explicit System namespace: FHIR values never match System types
            !is_fhir_value && check_system_type(item, &spec_name)
        }
        Some("fhir") => {
            // Explicit FHIR namespace: check FHIR type
            // For primitive types (integer, decimal, boolean, string), allow match regardless of inferred namespace
            // For complex types (objects, Quantity), require that it was inferred as a FHIR value
            if has_definitive_runtime_type(item) {
                is_fhir_value
                    && check_fhir_type(item, &spec_name, path_hint, fhir_context, ctx, exact_match)
            } else {
                check_fhir_type(item, &spec_name, path_hint, fhir_context, ctx, exact_match)
            }
        }
        Some(_) => {
            // Unknown namespace
            false
        }
        None => {
            // No namespace: follow FHIRPath precedence rules
            match_unqualified_type(
                item,
                &spec_name,
                &spec_name_raw,
                path_hint,
                fhir_context,
                ctx,
                is_fhir_value,
                exact_match,
            )
        }
    }
}

/// Resolve the effective namespace for a type specifier
fn resolve_effective_namespace(
    spec_namespace: &Option<String>,
    spec_name_raw: &str,
) -> Option<String> {
    if let Some(ns) = spec_namespace {
        return Some(ns.to_ascii_lowercase());
    }

    // Unqualified type starting with uppercase + is a System primitive → implicit System namespace
    if spec_name_raw
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
        && is_system_primitive_identifier(spec_name_raw)
    {
        return Some("system".to_string());
    }

    None
}

/// Match an unqualified type name (no namespace) using FHIRPath precedence rules
#[allow(clippy::too_many_arguments)]
fn match_unqualified_type(
    item: &Value,
    spec_name: &str,
    spec_name_raw: &str,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
    is_fhir_value: bool,
    exact_match: bool,
) -> bool {
    if is_fhir_value {
        // FHIR values: Try runtime type → path hints → declared types
        match_fhir_value_unqualified(
            item,
            spec_name,
            spec_name_raw,
            path_hint,
            fhir_context,
            ctx,
            exact_match,
        )
    } else {
        // System values: Check System type directly
        check_system_type(item, spec_name)
    }
}

/// Match FHIR value against unqualified type with fallback logic
fn match_fhir_value_unqualified(
    item: &Value,
    spec_name: &str,
    spec_name_raw: &str,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
    exact_match: bool,
) -> bool {
    // PRIORITY 1: Runtime type (definitive structures like Quantity, Period, resources)
    if has_definitive_runtime_type(item) {
        return check_fhir_type(item, spec_name, path_hint, fhir_context, ctx, exact_match);
    }

    // PRIORITY 2: Runtime type check (for ambiguous objects, but can match)
    if check_fhir_type(item, spec_name, path_hint, fhir_context, ctx, exact_match) {
        return true;
    }

    // PRIORITY 3: Path hint disambiguation (choice variants, e.g., valueQuantity → Quantity).
    //
    // If the hint clearly implies a concrete type and it doesn't match the requested spec,
    // do NOT fall back to declared types (that would re-introduce polymorphic false positives).
    if let Some(type_hint) = type_hint_from_path(path_hint) {
        if hint_looks_like_type(&type_hint) {
            let hint_lower = type_hint.to_ascii_lowercase();
            if exact_match {
                return hint_lower == spec_name;
            }

            if hint_lower == spec_name
                || hint_lower.ends_with(spec_name)
                || check_type_inheritance(&hint_lower, spec_name)
            {
                return true;
            }

            return false;
        }
    }

    // PRIORITY 4: Declared element types from StructureDefinition (last resort)
    match_declared_element_type(item, spec_name_raw, path_hint, fhir_context, ctx)
}

fn hint_looks_like_type(type_hint: &str) -> bool {
    type_hint
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
        || matches!(
            type_hint.to_ascii_lowercase().as_str(),
            "boolean"
                | "string"
                | "integer"
                | "decimal"
                | "datetime"
                | "date"
                | "time"
                | "quantity"
                | "period"
                | "coding"
                | "identifier"
                | "reference"
                | "codeableconcept"
        )
}

/// Check declared element type from StructureDefinition (for polymorphic fields)
fn match_declared_element_type(
    _item: &Value,
    spec_name_raw: &str,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
) -> bool {
    let (fc, path) = match (fhir_context, path_hint) {
        (Some(fc), Some(path)) => (fc, path),
        _ => return false,
    };

    // Get resource type from context
    let resource_type: Arc<str> = match ctx.resource.data() {
        ValueData::Object(root_obj) => match root_obj
            .get("resourceType")
            .and_then(|col| col.iter().next())
            .and_then(|val| match val.data() {
                ValueData::String(s) => Some(s.clone()),
                _ => None,
            }) {
            Some(rt) => rt,
            None => return false,
        },
        ValueData::LazyJson { .. } => match ctx
            .resource
            .data()
            .resolved_json()
            .and_then(|v| v.get("resourceType"))
            .and_then(|v| v.as_str())
        {
            Some(rt) => Arc::from(rt),
            None => return false,
        },
        _ => return false,
    };

    // Resolve path type from StructureDefinition
    let elem = match fc.resolve_path_type(resource_type.as_ref(), path) {
        Ok(Some(elem)) => elem,
        _ => return false,
    };

    let declared_types: Vec<String> = elem
        .type_codes
        .iter()
        .map(|s| normalize_type_code(s))
        .collect();

    let wanted = normalize_type_code(spec_name_raw);
    declared_types.contains(&wanted)
}

/// Create a Value representing type information from a TypeDescriptor
pub fn type_info_value(desc: &TypeDescriptor) -> Value {
    let mut map: HashMap<Arc<str>, Collection> = HashMap::new();
    let namespace_key: Arc<str> = Arc::from("namespace");
    let name_key: Arc<str> = Arc::from("name");

    map.insert(
        namespace_key.clone(),
        Collection::singleton(Value::string(desc.namespace)),
    );
    map.insert(
        name_key.clone(),
        Collection::singleton(Value::string(desc.name.clone())),
    );

    if let Some(base) = &desc.base_type {
        map.insert(
            Arc::from("baseType"),
            Collection::singleton(Value::string(base.clone())),
        );
    }

    Value::object(map)
}
