//! FHIR StructureDefinition model
//!
//! Version-agnostic model for StructureDefinitions that works across R4, R4B, and R5.

use super::complex::*;
use super::element_definition::{Differential, ElementDefinition, Snapshot};
use super::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// FHIR StructureDefinition resource
///
/// Defines the structure, constraints, and terminology bindings for FHIR resources and data types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructureDefinition {
    /// Resource type - always "StructureDefinition"
    #[serde(default = "default_resource_type")]
    pub resource_type: String,

    // --- Metadata fields ---
    /// Logical id of this artifact
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Canonical identifier for this structure definition (unique globally)
    pub url: String,

    /// Business version of the structure definition
    ///
    /// This is a business versionId, not a resource version id.
    /// The version can be appended to the url in a reference to allow a reference
    /// to a particular business version of the structure definition with the format [url]|[version].
    /// Note that there may be multiple resource versions of the structure that have this same identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Indicates the mechanism used to compare versions to determine which is more current
    ///
    /// Supported algorithms: semver, integer, alpha, date, natural
    /// If not specified, versions cannot be reliably compared lexicographically.
    #[serde(
        rename = "versionAlgorithmString",
        skip_serializing_if = "Option::is_none"
    )]
    pub version_algorithm_string: Option<String>,

    /// Indicates the mechanism used to compare versions (as Coding)
    ///
    /// Supported algorithms: semver, integer, alpha, date, natural
    /// The code should be one of: semver, integer, alpha, date, natural
    /// The system should be: http://hl7.org/fhir/version-algorithm
    #[serde(
        rename = "versionAlgorithmCoding",
        skip_serializing_if = "Option::is_none"
    )]
    pub version_algorithm_coding: Option<Coding>,

    /// Name for this structure definition (computer friendly)
    #[serde(default)]
    pub name: String,

    /// Name for this structure definition (human friendly)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Publication status (draft | active | retired | unknown)
    pub status: PublicationStatus,

    /// For testing purposes, not real usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<bool>,

    /// Date last changed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,

    /// Name of the publisher (organization or individual)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,

    /// Contact details for the publisher
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<Vec<ContactDetail>>,

    /// Natural language description of the structure definition
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The context that the content is intended to support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_context: Option<Vec<UsageContext>>,

    /// Intended jurisdiction for structure definition (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jurisdiction: Option<Vec<Value>>, // CodeableConcept

    /// Why this structure definition is defined
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,

    /// Use and/or publishing restrictions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,

    // --- Core definition fields ---
    /// FHIR Version this StructureDefinition targets
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_version: Option<String>,

    /// External specifications that this structure conforms to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mapping: Option<Vec<StructureDefinitionMapping>>,

    /// Kind of structure (primitive-type | complex-type | resource | logical)
    pub kind: StructureDefinitionKind,

    /// Whether this is an abstract type
    #[serde(rename = "abstract")]
    pub is_abstract: bool,

    /// If an extension, where it can be used (FHIRPath)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<StructureDefinitionContext>>,

    /// Type defined or constrained by this structure
    #[serde(rename = "type")]
    pub type_: String,

    /// Definition that this type is constrained/specialized from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_definition: Option<String>,

    /// Derivation type (specialization | constraint)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivation: Option<TypeDerivationRule>,

    // --- Snapshot and Differential ---
    /// Snapshot view of the structure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,

    /// Differential view of the structure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub differential: Option<Differential>,

    // --- Additional fields ---
    /// Keywords to assist with search
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword: Option<Vec<Coding>>,

    /// Additional content beyond core fields (extensions, version-specific fields)
    #[serde(flatten)]
    pub extensions: HashMap<String, Value>,
}

fn default_resource_type() -> String {
    "StructureDefinition".to_string()
}

/// Kind of structure this definition describes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureDefinitionKind {
    /// A primitive data type
    PrimitiveType,
    /// A complex data type
    ComplexType,
    /// A resource
    Resource,
    /// A logical model (not directly implementable)
    Logical,
}

/// How the type relates to its baseDefinition
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TypeDerivationRule {
    /// This definition defines a new type based on the baseDefinition
    Specialization,
    /// This definition constrains the baseDefinition
    Constraint,
}

/// Mapping to another standard/specification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructureDefinitionMapping {
    /// Internal id when this mapping is used
    pub identity: String,

    /// Identifies what this mapping refers to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Names what this mapping refers to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Versions, issues, scope limitations, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Context where an extension can be used
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructureDefinitionContext {
    /// Type of context (fhirpath | element | extension)
    #[serde(rename = "type")]
    pub context_type: String,

    /// FHIRPath expression or element id
    pub expression: String,
}

impl StructureDefinition {
    /// Create a new StructureDefinition with minimal required fields
    pub fn new(
        url: impl Into<String>,
        name: impl Into<String>,
        kind: StructureDefinitionKind,
        type_: impl Into<String>,
    ) -> Self {
        Self {
            resource_type: "StructureDefinition".to_string(),
            id: None,
            url: url.into(),
            version: None,
            version_algorithm_string: None,
            version_algorithm_coding: None,
            name: name.into(),
            title: None,
            status: PublicationStatus::Draft,
            experimental: None,
            date: None,
            publisher: None,
            contact: None,
            description: None,
            use_context: None,
            jurisdiction: None,
            purpose: None,
            copyright: None,
            fhir_version: None,
            mapping: None,
            kind,
            is_abstract: false,
            context: None,
            type_: type_.into(),
            base_definition: None,
            derivation: None,
            snapshot: None,
            differential: None,
            keyword: None,
            extensions: HashMap::new(),
        }
    }

    /// Parse from JSON Value
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(Error::from)
    }

    /// Convert to JSON Value
    pub fn to_value(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(Error::from)
    }

    /// Get the root element from snapshot
    pub fn get_root_element(&self) -> Option<&ElementDefinition> {
        self.snapshot.as_ref().and_then(|s| s.element.first())
    }

    /// Get element by path from snapshot
    pub fn get_element_by_path(&self, path: &str) -> Option<&ElementDefinition> {
        self.snapshot
            .as_ref()
            .and_then(|s| s.element.iter().find(|e| e.path == path))
    }

    /// Get all elements from snapshot
    pub fn get_elements(&self) -> Option<&[ElementDefinition]> {
        self.snapshot.as_ref().map(|s| s.element.as_slice())
    }

    /// Check if this is a resource definition
    pub fn is_resource(&self) -> bool {
        self.kind == StructureDefinitionKind::Resource
    }

    /// Check if this is an extension definition
    pub fn is_extension(&self) -> bool {
        self.type_ == "Extension"
    }

    /// Check if this is a profile (constraint on another definition)
    pub fn is_profile(&self) -> bool {
        self.derivation == Some(TypeDerivationRule::Constraint)
    }

    /// Check if this has a snapshot
    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }

    /// Check if this has a differential
    pub fn has_differential(&self) -> bool {
        self.differential.is_some()
    }

    /// Get the version with the URL (canonical|version format)
    pub fn get_versioned_url(&self) -> String {
        match &self.version {
            Some(v) => format!("{}|{}", self.url, v),
            None => self.url.clone(),
        }
    }

    /// Get base type name (strips canonical URL)
    pub fn get_base_type_name(&self) -> Option<String> {
        self.base_definition
            .as_ref()
            .and_then(|url| url.rsplit('/').next().map(|s| s.to_string()))
    }

    /// Get the version algorithm code
    ///
    /// Returns the algorithm code from either versionAlgorithmString or versionAlgorithmCoding.
    /// Returns None if neither is set.
    pub fn get_version_algorithm(&self) -> Option<String> {
        if let Some(ref alg_str) = self.version_algorithm_string {
            return Some(alg_str.clone());
        }
        if let Some(ref alg_coding) = self.version_algorithm_coding {
            if let Some(ref code) = alg_coding.code {
                return Some(code.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deserialize_structure_definition() {
        let json = json!({
            "resourceType": "StructureDefinition",
            "id": "Patient",
            "url": "http://hl7.org/fhir/StructureDefinition/Patient",
            "version": "4.0.1",
            "name": "Patient",
            "status": "active",
            "kind": "resource",
            "abstract": false,
            "type": "Patient",
            "baseDefinition": "http://hl7.org/fhir/StructureDefinition/DomainResource",
            "derivation": "specialization"
        });

        let sd: StructureDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(sd.name, "Patient");
        assert_eq!(sd.url, "http://hl7.org/fhir/StructureDefinition/Patient");
        assert_eq!(sd.kind, StructureDefinitionKind::Resource);
        assert_eq!(sd.derivation, Some(TypeDerivationRule::Specialization));
        assert!(!sd.is_abstract);
    }

    #[test]
    fn test_serialize_structure_definition() {
        let sd = StructureDefinition::new(
            "http://example.org/fhir/StructureDefinition/MyPatient",
            "MyPatient",
            StructureDefinitionKind::Resource,
            "Patient",
        );

        let json = serde_json::to_value(&sd).unwrap();
        assert_eq!(json["resourceType"], "StructureDefinition");
        assert_eq!(json["name"], "MyPatient");
        assert_eq!(json["kind"], "resource");
    }

    #[test]
    fn test_is_resource() {
        let mut sd = StructureDefinition::new(
            "http://example.org/StructureDefinition/Test",
            "Test",
            StructureDefinitionKind::Resource,
            "Patient",
        );
        assert!(sd.is_resource());

        sd.kind = StructureDefinitionKind::ComplexType;
        assert!(!sd.is_resource());
    }

    #[test]
    fn test_is_extension() {
        let mut sd = StructureDefinition::new(
            "http://example.org/StructureDefinition/MyExt",
            "MyExt",
            StructureDefinitionKind::ComplexType,
            "Extension",
        );
        assert!(sd.is_extension());

        sd.type_ = "Patient".to_string();
        assert!(!sd.is_extension());
    }

    #[test]
    fn test_is_profile() {
        let mut sd = StructureDefinition::new(
            "http://example.org/StructureDefinition/MyProfile",
            "MyProfile",
            StructureDefinitionKind::Resource,
            "Patient",
        );
        assert!(!sd.is_profile());

        sd.derivation = Some(TypeDerivationRule::Constraint);
        assert!(sd.is_profile());
    }

    #[test]
    fn test_get_versioned_url() {
        let mut sd = StructureDefinition::new(
            "http://example.org/StructureDefinition/Test",
            "Test",
            StructureDefinitionKind::Resource,
            "Patient",
        );
        assert_eq!(
            sd.get_versioned_url(),
            "http://example.org/StructureDefinition/Test"
        );

        sd.version = Some("1.0.0".to_string());
        assert_eq!(
            sd.get_versioned_url(),
            "http://example.org/StructureDefinition/Test|1.0.0"
        );
    }

    #[test]
    fn test_get_base_type_name() {
        let mut sd = StructureDefinition::new(
            "http://example.org/StructureDefinition/MyPatient",
            "MyPatient",
            StructureDefinitionKind::Resource,
            "Patient",
        );
        assert_eq!(sd.get_base_type_name(), None);

        sd.base_definition =
            Some("http://hl7.org/fhir/StructureDefinition/DomainResource".to_string());
        assert_eq!(sd.get_base_type_name(), Some("DomainResource".to_string()));
    }
}
