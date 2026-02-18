//! FHIR complex types and shared data structures
//!
//! This module contains enums and structs that are reused across FHIR resources.
//! No validation - just data representation.

use serde::{Deserialize, Serialize};

/// Publication status of a conformance resource
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PublicationStatus {
    #[default]
    Draft,
    Active,
    Retired,
    Unknown,
}

/// Binding strength for terminology bindings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindingStrength {
    Required,
    Extensible,
    Preferred,
    Example,
}

/// FHIR Extension
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extension {
    pub url: String,

    #[serde(flatten)]
    pub value: serde_json::Value,
}

/// Contact detail for a resource
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactDetail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub telecom: Option<Vec<ContactPoint>>,
}

/// Contact point (phone, email, etc.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactPoint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>, // phone | fax | email | pager | url | sms | other

    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_: Option<String>, // home | work | temp | old | mobile

    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
}

/// Coding - a reference to a code defined by a terminology system
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Coding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,

    #[serde(rename = "userSelected", skip_serializing_if = "Option::is_none")]
    pub user_selected: Option<bool>,
}

/// UsageContext - usage context for a conformance resource
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageContext {
    pub code: Coding,

    #[serde(flatten)]
    pub value: serde_json::Value, // Can be CodeableConcept, Quantity, Range, or Reference
}
