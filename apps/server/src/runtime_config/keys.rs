//! Configuration keys and their metadata
//!
//! Defines all runtime-configurable settings with their types, defaults, and descriptions.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Categories for organizing configuration settings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigCategory {
    Logging,
    Search,
    Interactions,
    Format,
    Behavior,
    Audit,
}

impl fmt::Display for ConfigCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigCategory::Logging => write!(f, "logging"),
            ConfigCategory::Search => write!(f, "search"),
            ConfigCategory::Interactions => write!(f, "interactions"),
            ConfigCategory::Format => write!(f, "format"),
            ConfigCategory::Behavior => write!(f, "behavior"),
            ConfigCategory::Audit => write!(f, "audit"),
        }
    }
}

impl ConfigCategory {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "logging" => Some(ConfigCategory::Logging),
            "search" => Some(ConfigCategory::Search),
            "interactions" => Some(ConfigCategory::Interactions),
            "format" => Some(ConfigCategory::Format),
            "behavior" => Some(ConfigCategory::Behavior),
            "audit" => Some(ConfigCategory::Audit),
            _ => None,
        }
    }

    pub fn all() -> Vec<ConfigCategory> {
        vec![
            ConfigCategory::Logging,
            ConfigCategory::Search,
            ConfigCategory::Interactions,
            ConfigCategory::Format,
            ConfigCategory::Behavior,
            ConfigCategory::Audit,
        ]
    }
}

/// Value types for configuration settings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigValueType {
    Boolean,
    Integer,
    String,
    StringEnum,
}

impl fmt::Display for ConfigValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigValueType::Boolean => write!(f, "boolean"),
            ConfigValueType::Integer => write!(f, "integer"),
            ConfigValueType::String => write!(f, "string"),
            ConfigValueType::StringEnum => write!(f, "string_enum"),
        }
    }
}

/// All configurable keys with their metadata
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    // Logging
    LoggingLevel,

    // Search
    SearchDefaultCount,
    SearchMaxCount,
    SearchMaxTotalResults,
    SearchMaxIncludeDepth,
    SearchMaxIncludes,

    // Interactions - Instance
    InteractionsInstanceRead,
    InteractionsInstanceVread,
    InteractionsInstanceUpdate,
    InteractionsInstancePatch,
    InteractionsInstanceDelete,
    InteractionsInstanceHistory,
    InteractionsInstanceDeleteHistory,
    InteractionsInstanceDeleteHistoryVersion,

    // Interactions - Type
    InteractionsTypeCreate,
    InteractionsTypeConditionalCreate,
    InteractionsTypeSearch,
    InteractionsTypeHistory,
    InteractionsTypeConditionalUpdate,
    InteractionsTypeConditionalPatch,
    InteractionsTypeConditionalDelete,

    // Interactions - System
    InteractionsSystemCapabilities,
    InteractionsSystemSearch,
    InteractionsSystemHistory,
    InteractionsSystemDelete,
    InteractionsSystemBatch,
    InteractionsSystemTransaction,
    InteractionsSystemHistoryBundle,

    // Interactions - Compartment
    InteractionsCompartmentSearch,

    // Interactions - Operations
    InteractionsOperationsSystem,
    InteractionsOperationsTypeLevel,
    InteractionsOperationsInstance,

    // Format
    FormatDefault,
    FormatDefaultPreferReturn,

    // Behavior
    BehaviorAllowUpdateCreate,
    BehaviorHardDelete,

    // Audit
    AuditEnabled,
    AuditIncludeSuccess,
    AuditIncludeAuthzFailure,
    AuditIncludeProcessingFailure,
    AuditCaptureSearchQuery,
    AuditCaptureOperationOutcome,
    AuditPerPatientEventsForSearch,
    AuditInteractionsRead,
    AuditInteractionsVread,
    AuditInteractionsHistory,
    AuditInteractionsSearch,
    AuditInteractionsCreate,
    AuditInteractionsUpdate,
    AuditInteractionsPatch,
    AuditInteractionsDelete,
    AuditInteractionsCapabilities,
    AuditInteractionsOperation,
    AuditInteractionsBatch,
    AuditInteractionsTransaction,
    AuditInteractionsExport,
}

impl ConfigKey {
    /// Get the string key used in the database
    pub fn as_str(&self) -> &'static str {
        match self {
            // Logging
            ConfigKey::LoggingLevel => "logging.level",

            // Search
            ConfigKey::SearchDefaultCount => "fhir.search.default_count",
            ConfigKey::SearchMaxCount => "fhir.search.max_count",
            ConfigKey::SearchMaxTotalResults => "fhir.search.max_total_results",
            ConfigKey::SearchMaxIncludeDepth => "fhir.search.max_include_depth",
            ConfigKey::SearchMaxIncludes => "fhir.search.max_includes",

            // Interactions - Instance
            ConfigKey::InteractionsInstanceRead => "fhir.interactions.instance.read",
            ConfigKey::InteractionsInstanceVread => "fhir.interactions.instance.vread",
            ConfigKey::InteractionsInstanceUpdate => "fhir.interactions.instance.update",
            ConfigKey::InteractionsInstancePatch => "fhir.interactions.instance.patch",
            ConfigKey::InteractionsInstanceDelete => "fhir.interactions.instance.delete",
            ConfigKey::InteractionsInstanceHistory => "fhir.interactions.instance.history",
            ConfigKey::InteractionsInstanceDeleteHistory => {
                "fhir.interactions.instance.delete_history"
            }
            ConfigKey::InteractionsInstanceDeleteHistoryVersion => {
                "fhir.interactions.instance.delete_history_version"
            }

            // Interactions - Type
            ConfigKey::InteractionsTypeCreate => "fhir.interactions.type.create",
            ConfigKey::InteractionsTypeConditionalCreate => {
                "fhir.interactions.type.conditional_create"
            }
            ConfigKey::InteractionsTypeSearch => "fhir.interactions.type.search",
            ConfigKey::InteractionsTypeHistory => "fhir.interactions.type.history",
            ConfigKey::InteractionsTypeConditionalUpdate => {
                "fhir.interactions.type.conditional_update"
            }
            ConfigKey::InteractionsTypeConditionalPatch => {
                "fhir.interactions.type.conditional_patch"
            }
            ConfigKey::InteractionsTypeConditionalDelete => {
                "fhir.interactions.type.conditional_delete"
            }

            // Interactions - System
            ConfigKey::InteractionsSystemCapabilities => "fhir.interactions.system.capabilities",
            ConfigKey::InteractionsSystemSearch => "fhir.interactions.system.search",
            ConfigKey::InteractionsSystemHistory => "fhir.interactions.system.history",
            ConfigKey::InteractionsSystemDelete => "fhir.interactions.system.delete",
            ConfigKey::InteractionsSystemBatch => "fhir.interactions.system.batch",
            ConfigKey::InteractionsSystemTransaction => "fhir.interactions.system.transaction",
            ConfigKey::InteractionsSystemHistoryBundle => "fhir.interactions.system.history_bundle",

            // Interactions - Compartment
            ConfigKey::InteractionsCompartmentSearch => "fhir.interactions.compartment.search",

            // Interactions - Operations
            ConfigKey::InteractionsOperationsSystem => "fhir.interactions.operations.system",
            ConfigKey::InteractionsOperationsTypeLevel => "fhir.interactions.operations.type_level",
            ConfigKey::InteractionsOperationsInstance => "fhir.interactions.operations.instance",

            // Format
            ConfigKey::FormatDefault => "fhir.default_format",
            ConfigKey::FormatDefaultPreferReturn => "fhir.default_prefer_return",

            // Behavior
            ConfigKey::BehaviorAllowUpdateCreate => "fhir.allow_update_create",
            ConfigKey::BehaviorHardDelete => "fhir.hard_delete",

            // Audit
            ConfigKey::AuditEnabled => "logging.audit.enabled",
            ConfigKey::AuditIncludeSuccess => "logging.audit.include_success",
            ConfigKey::AuditIncludeAuthzFailure => "logging.audit.include_authz_failure",
            ConfigKey::AuditIncludeProcessingFailure => "logging.audit.include_processing_failure",
            ConfigKey::AuditCaptureSearchQuery => "logging.audit.capture_search_query",
            ConfigKey::AuditCaptureOperationOutcome => "logging.audit.capture_operation_outcome",
            ConfigKey::AuditPerPatientEventsForSearch => {
                "logging.audit.per_patient_events_for_search"
            }
            ConfigKey::AuditInteractionsRead => "logging.audit.interactions.read",
            ConfigKey::AuditInteractionsVread => "logging.audit.interactions.vread",
            ConfigKey::AuditInteractionsHistory => "logging.audit.interactions.history",
            ConfigKey::AuditInteractionsSearch => "logging.audit.interactions.search",
            ConfigKey::AuditInteractionsCreate => "logging.audit.interactions.create",
            ConfigKey::AuditInteractionsUpdate => "logging.audit.interactions.update",
            ConfigKey::AuditInteractionsPatch => "logging.audit.interactions.patch",
            ConfigKey::AuditInteractionsDelete => "logging.audit.interactions.delete",
            ConfigKey::AuditInteractionsCapabilities => "logging.audit.interactions.capabilities",
            ConfigKey::AuditInteractionsOperation => "logging.audit.interactions.operation",
            ConfigKey::AuditInteractionsBatch => "logging.audit.interactions.batch",
            ConfigKey::AuditInteractionsTransaction => "logging.audit.interactions.transaction",
            ConfigKey::AuditInteractionsExport => "logging.audit.interactions.export",
        }
    }

    /// Get the category for this key
    pub fn category(&self) -> ConfigCategory {
        match self {
            ConfigKey::LoggingLevel => ConfigCategory::Logging,

            ConfigKey::SearchDefaultCount
            | ConfigKey::SearchMaxCount
            | ConfigKey::SearchMaxTotalResults
            | ConfigKey::SearchMaxIncludeDepth
            | ConfigKey::SearchMaxIncludes => ConfigCategory::Search,

            ConfigKey::InteractionsInstanceRead
            | ConfigKey::InteractionsInstanceVread
            | ConfigKey::InteractionsInstanceUpdate
            | ConfigKey::InteractionsInstancePatch
            | ConfigKey::InteractionsInstanceDelete
            | ConfigKey::InteractionsInstanceHistory
            | ConfigKey::InteractionsInstanceDeleteHistory
            | ConfigKey::InteractionsInstanceDeleteHistoryVersion
            | ConfigKey::InteractionsTypeCreate
            | ConfigKey::InteractionsTypeConditionalCreate
            | ConfigKey::InteractionsTypeSearch
            | ConfigKey::InteractionsTypeHistory
            | ConfigKey::InteractionsTypeConditionalUpdate
            | ConfigKey::InteractionsTypeConditionalPatch
            | ConfigKey::InteractionsTypeConditionalDelete
            | ConfigKey::InteractionsSystemCapabilities
            | ConfigKey::InteractionsSystemSearch
            | ConfigKey::InteractionsSystemHistory
            | ConfigKey::InteractionsSystemDelete
            | ConfigKey::InteractionsSystemBatch
            | ConfigKey::InteractionsSystemTransaction
            | ConfigKey::InteractionsSystemHistoryBundle
            | ConfigKey::InteractionsCompartmentSearch
            | ConfigKey::InteractionsOperationsSystem
            | ConfigKey::InteractionsOperationsTypeLevel
            | ConfigKey::InteractionsOperationsInstance => ConfigCategory::Interactions,

            ConfigKey::FormatDefault | ConfigKey::FormatDefaultPreferReturn => {
                ConfigCategory::Format
            }

            ConfigKey::BehaviorAllowUpdateCreate | ConfigKey::BehaviorHardDelete => {
                ConfigCategory::Behavior
            }

            ConfigKey::AuditEnabled
            | ConfigKey::AuditIncludeSuccess
            | ConfigKey::AuditIncludeAuthzFailure
            | ConfigKey::AuditIncludeProcessingFailure
            | ConfigKey::AuditCaptureSearchQuery
            | ConfigKey::AuditCaptureOperationOutcome
            | ConfigKey::AuditPerPatientEventsForSearch
            | ConfigKey::AuditInteractionsRead
            | ConfigKey::AuditInteractionsVread
            | ConfigKey::AuditInteractionsHistory
            | ConfigKey::AuditInteractionsSearch
            | ConfigKey::AuditInteractionsCreate
            | ConfigKey::AuditInteractionsUpdate
            | ConfigKey::AuditInteractionsPatch
            | ConfigKey::AuditInteractionsDelete
            | ConfigKey::AuditInteractionsCapabilities
            | ConfigKey::AuditInteractionsOperation
            | ConfigKey::AuditInteractionsBatch
            | ConfigKey::AuditInteractionsTransaction
            | ConfigKey::AuditInteractionsExport => ConfigCategory::Audit,
        }
    }

    /// Get the value type for this key
    pub fn value_type(&self) -> ConfigValueType {
        match self {
            ConfigKey::LoggingLevel => ConfigValueType::StringEnum,

            ConfigKey::SearchDefaultCount
            | ConfigKey::SearchMaxCount
            | ConfigKey::SearchMaxTotalResults
            | ConfigKey::SearchMaxIncludeDepth
            | ConfigKey::SearchMaxIncludes => ConfigValueType::Integer,

            ConfigKey::FormatDefault | ConfigKey::FormatDefaultPreferReturn => {
                ConfigValueType::StringEnum
            }

            // All interaction flags and audit flags are booleans
            _ => ConfigValueType::Boolean,
        }
    }

    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            // Logging
            ConfigKey::LoggingLevel => "Log level (trace, debug, info, warn, error)",

            // Search
            ConfigKey::SearchDefaultCount => "Default page size when _count is not specified",
            ConfigKey::SearchMaxCount => "Maximum allowed _count value",
            ConfigKey::SearchMaxTotalResults => "Maximum total results across all pages",
            ConfigKey::SearchMaxIncludeDepth => {
                "Maximum depth for _include:iterate and _revinclude:iterate"
            }
            ConfigKey::SearchMaxIncludes => {
                "Maximum number of _include/_revinclude parameters allowed"
            }

            // Interactions - Instance
            ConfigKey::InteractionsInstanceRead => "Enable GET /{type}/{id}",
            ConfigKey::InteractionsInstanceVread => "Enable GET /{type}/{id}/_history/{vid}",
            ConfigKey::InteractionsInstanceUpdate => "Enable PUT /{type}/{id}",
            ConfigKey::InteractionsInstancePatch => "Enable PATCH /{type}/{id}",
            ConfigKey::InteractionsInstanceDelete => "Enable DELETE /{type}/{id}",
            ConfigKey::InteractionsInstanceHistory => "Enable GET /{type}/{id}/_history",
            ConfigKey::InteractionsInstanceDeleteHistory => "Enable DELETE /{type}/{id}/_history",
            ConfigKey::InteractionsInstanceDeleteHistoryVersion => {
                "Enable DELETE /{type}/{id}/_history/{vid}"
            }

            // Interactions - Type
            ConfigKey::InteractionsTypeCreate => "Enable POST /{type}",
            ConfigKey::InteractionsTypeConditionalCreate => {
                "Enable conditional create via If-None-Exist"
            }
            ConfigKey::InteractionsTypeSearch => "Enable GET /{type} and POST /{type}/_search",
            ConfigKey::InteractionsTypeHistory => "Enable GET /{type}/_history",
            ConfigKey::InteractionsTypeConditionalUpdate => {
                "Enable PUT /{type} (conditional update)"
            }
            ConfigKey::InteractionsTypeConditionalPatch => {
                "Enable PATCH /{type} (conditional patch)"
            }
            ConfigKey::InteractionsTypeConditionalDelete => {
                "Enable DELETE /{type} (conditional delete)"
            }

            // Interactions - System
            ConfigKey::InteractionsSystemCapabilities => "Enable GET /metadata",
            ConfigKey::InteractionsSystemSearch => "Enable GET / and POST /_search",
            ConfigKey::InteractionsSystemHistory => "Enable GET /_history",
            ConfigKey::InteractionsSystemDelete => {
                "Enable DELETE / (conditional delete across all types)"
            }
            ConfigKey::InteractionsSystemBatch => "Enable POST / with Bundle.type=batch",
            ConfigKey::InteractionsSystemTransaction => {
                "Enable POST / with Bundle.type=transaction"
            }
            ConfigKey::InteractionsSystemHistoryBundle => "Enable POST / with Bundle.type=history",

            // Interactions - Compartment
            ConfigKey::InteractionsCompartmentSearch => "Enable compartment search endpoints",

            // Interactions - Operations
            ConfigKey::InteractionsOperationsSystem => "Enable system-level $operations",
            ConfigKey::InteractionsOperationsTypeLevel => "Enable type-level $operations",
            ConfigKey::InteractionsOperationsInstance => "Enable instance-level $operations",

            // Format
            ConfigKey::FormatDefault => "Default response format (json or xml)",
            ConfigKey::FormatDefaultPreferReturn => {
                "Default Prefer header return behavior (minimal, representation, operationoutcome)"
            }

            // Behavior
            ConfigKey::BehaviorAllowUpdateCreate => {
                "Allow clients to create resources via PUT with client-defined IDs"
            }
            ConfigKey::BehaviorHardDelete => {
                "When true, DELETE physically removes the resource and its history"
            }

            // Audit
            ConfigKey::AuditEnabled => "Master switch for audit logging",
            ConfigKey::AuditIncludeSuccess => "Record successful interactions (HTTP < 400)",
            ConfigKey::AuditIncludeAuthzFailure => "Record authorization failures (HTTP 401/403)",
            ConfigKey::AuditIncludeProcessingFailure => {
                "Record processing failures (HTTP >= 400 excluding 401/403)"
            }
            ConfigKey::AuditCaptureSearchQuery => {
                "Capture full raw HTTP request for search interactions"
            }
            ConfigKey::AuditCaptureOperationOutcome => {
                "Try to capture OperationOutcome from failure responses"
            }
            ConfigKey::AuditPerPatientEventsForSearch => {
                "Emit one AuditEvent per resolved patient for search"
            }
            ConfigKey::AuditInteractionsRead => "Audit read interactions",
            ConfigKey::AuditInteractionsVread => "Audit vread interactions",
            ConfigKey::AuditInteractionsHistory => "Audit history interactions",
            ConfigKey::AuditInteractionsSearch => "Audit search interactions",
            ConfigKey::AuditInteractionsCreate => "Audit create interactions",
            ConfigKey::AuditInteractionsUpdate => "Audit update interactions",
            ConfigKey::AuditInteractionsPatch => "Audit patch interactions",
            ConfigKey::AuditInteractionsDelete => "Audit delete interactions",
            ConfigKey::AuditInteractionsCapabilities => "Audit /metadata requests",
            ConfigKey::AuditInteractionsOperation => "Audit $operation interactions",
            ConfigKey::AuditInteractionsBatch => "Audit batch interactions",
            ConfigKey::AuditInteractionsTransaction => "Audit transaction interactions",
            ConfigKey::AuditInteractionsExport => "Audit export interactions",
        }
    }

    /// Get enum values for StringEnum types
    pub fn enum_values(&self) -> Option<Vec<&'static str>> {
        match self {
            ConfigKey::LoggingLevel => Some(vec!["trace", "debug", "info", "warn", "error"]),
            ConfigKey::FormatDefault => Some(vec!["json", "xml"]),
            ConfigKey::FormatDefaultPreferReturn => {
                Some(vec!["minimal", "representation", "operationoutcome"])
            }
            _ => None,
        }
    }

    /// Get min/max values for Integer types
    pub fn integer_bounds(&self) -> Option<(i64, i64)> {
        match self {
            ConfigKey::SearchDefaultCount => Some((1, 1000)),
            ConfigKey::SearchMaxCount => Some((1, 10000)),
            ConfigKey::SearchMaxTotalResults => Some((1, 100000)),
            ConfigKey::SearchMaxIncludeDepth => Some((0, 10)),
            ConfigKey::SearchMaxIncludes => Some((0, 50)),
            _ => None,
        }
    }

    /// Parse a string key back to ConfigKey
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<ConfigKey> {
        match s {
            "logging.level" => Some(ConfigKey::LoggingLevel),

            "fhir.search.default_count" => Some(ConfigKey::SearchDefaultCount),
            "fhir.search.max_count" => Some(ConfigKey::SearchMaxCount),
            "fhir.search.max_total_results" => Some(ConfigKey::SearchMaxTotalResults),
            "fhir.search.max_include_depth" => Some(ConfigKey::SearchMaxIncludeDepth),
            "fhir.search.max_includes" => Some(ConfigKey::SearchMaxIncludes),

            "fhir.interactions.instance.read" => Some(ConfigKey::InteractionsInstanceRead),
            "fhir.interactions.instance.vread" => Some(ConfigKey::InteractionsInstanceVread),
            "fhir.interactions.instance.update" => Some(ConfigKey::InteractionsInstanceUpdate),
            "fhir.interactions.instance.patch" => Some(ConfigKey::InteractionsInstancePatch),
            "fhir.interactions.instance.delete" => Some(ConfigKey::InteractionsInstanceDelete),
            "fhir.interactions.instance.history" => Some(ConfigKey::InteractionsInstanceHistory),
            "fhir.interactions.instance.delete_history" => {
                Some(ConfigKey::InteractionsInstanceDeleteHistory)
            }
            "fhir.interactions.instance.delete_history_version" => {
                Some(ConfigKey::InteractionsInstanceDeleteHistoryVersion)
            }

            "fhir.interactions.type.create" => Some(ConfigKey::InteractionsTypeCreate),
            "fhir.interactions.type.conditional_create" => {
                Some(ConfigKey::InteractionsTypeConditionalCreate)
            }
            "fhir.interactions.type.search" => Some(ConfigKey::InteractionsTypeSearch),
            "fhir.interactions.type.history" => Some(ConfigKey::InteractionsTypeHistory),
            "fhir.interactions.type.conditional_update" => {
                Some(ConfigKey::InteractionsTypeConditionalUpdate)
            }
            "fhir.interactions.type.conditional_patch" => {
                Some(ConfigKey::InteractionsTypeConditionalPatch)
            }
            "fhir.interactions.type.conditional_delete" => {
                Some(ConfigKey::InteractionsTypeConditionalDelete)
            }

            "fhir.interactions.system.capabilities" => {
                Some(ConfigKey::InteractionsSystemCapabilities)
            }
            "fhir.interactions.system.search" => Some(ConfigKey::InteractionsSystemSearch),
            "fhir.interactions.system.history" => Some(ConfigKey::InteractionsSystemHistory),
            "fhir.interactions.system.delete" => Some(ConfigKey::InteractionsSystemDelete),
            "fhir.interactions.system.batch" => Some(ConfigKey::InteractionsSystemBatch),
            "fhir.interactions.system.transaction" => {
                Some(ConfigKey::InteractionsSystemTransaction)
            }
            "fhir.interactions.system.history_bundle" => {
                Some(ConfigKey::InteractionsSystemHistoryBundle)
            }

            "fhir.interactions.compartment.search" => {
                Some(ConfigKey::InteractionsCompartmentSearch)
            }

            "fhir.interactions.operations.system" => Some(ConfigKey::InteractionsOperationsSystem),
            "fhir.interactions.operations.type_level" => {
                Some(ConfigKey::InteractionsOperationsTypeLevel)
            }
            "fhir.interactions.operations.instance" => {
                Some(ConfigKey::InteractionsOperationsInstance)
            }

            "fhir.default_format" => Some(ConfigKey::FormatDefault),
            "fhir.default_prefer_return" => Some(ConfigKey::FormatDefaultPreferReturn),

            "fhir.allow_update_create" => Some(ConfigKey::BehaviorAllowUpdateCreate),
            "fhir.hard_delete" => Some(ConfigKey::BehaviorHardDelete),

            "logging.audit.enabled" => Some(ConfigKey::AuditEnabled),
            "logging.audit.include_success" => Some(ConfigKey::AuditIncludeSuccess),
            "logging.audit.include_authz_failure" => Some(ConfigKey::AuditIncludeAuthzFailure),
            "logging.audit.include_processing_failure" => {
                Some(ConfigKey::AuditIncludeProcessingFailure)
            }
            "logging.audit.capture_search_query" => Some(ConfigKey::AuditCaptureSearchQuery),
            "logging.audit.capture_operation_outcome" => {
                Some(ConfigKey::AuditCaptureOperationOutcome)
            }
            "logging.audit.per_patient_events_for_search" => {
                Some(ConfigKey::AuditPerPatientEventsForSearch)
            }
            "logging.audit.interactions.read" => Some(ConfigKey::AuditInteractionsRead),
            "logging.audit.interactions.vread" => Some(ConfigKey::AuditInteractionsVread),
            "logging.audit.interactions.history" => Some(ConfigKey::AuditInteractionsHistory),
            "logging.audit.interactions.search" => Some(ConfigKey::AuditInteractionsSearch),
            "logging.audit.interactions.create" => Some(ConfigKey::AuditInteractionsCreate),
            "logging.audit.interactions.update" => Some(ConfigKey::AuditInteractionsUpdate),
            "logging.audit.interactions.patch" => Some(ConfigKey::AuditInteractionsPatch),
            "logging.audit.interactions.delete" => Some(ConfigKey::AuditInteractionsDelete),
            "logging.audit.interactions.capabilities" => {
                Some(ConfigKey::AuditInteractionsCapabilities)
            }
            "logging.audit.interactions.operation" => Some(ConfigKey::AuditInteractionsOperation),
            "logging.audit.interactions.batch" => Some(ConfigKey::AuditInteractionsBatch),
            "logging.audit.interactions.transaction" => {
                Some(ConfigKey::AuditInteractionsTransaction)
            }
            "logging.audit.interactions.export" => Some(ConfigKey::AuditInteractionsExport),

            _ => None,
        }
    }

    /// Get all configuration keys
    pub fn all() -> Vec<ConfigKey> {
        vec![
            // Logging
            ConfigKey::LoggingLevel,
            // Search
            ConfigKey::SearchDefaultCount,
            ConfigKey::SearchMaxCount,
            ConfigKey::SearchMaxTotalResults,
            ConfigKey::SearchMaxIncludeDepth,
            ConfigKey::SearchMaxIncludes,
            // Interactions - Instance
            ConfigKey::InteractionsInstanceRead,
            ConfigKey::InteractionsInstanceVread,
            ConfigKey::InteractionsInstanceUpdate,
            ConfigKey::InteractionsInstancePatch,
            ConfigKey::InteractionsInstanceDelete,
            ConfigKey::InteractionsInstanceHistory,
            ConfigKey::InteractionsInstanceDeleteHistory,
            ConfigKey::InteractionsInstanceDeleteHistoryVersion,
            // Interactions - Type
            ConfigKey::InteractionsTypeCreate,
            ConfigKey::InteractionsTypeConditionalCreate,
            ConfigKey::InteractionsTypeSearch,
            ConfigKey::InteractionsTypeHistory,
            ConfigKey::InteractionsTypeConditionalUpdate,
            ConfigKey::InteractionsTypeConditionalPatch,
            ConfigKey::InteractionsTypeConditionalDelete,
            // Interactions - System
            ConfigKey::InteractionsSystemCapabilities,
            ConfigKey::InteractionsSystemSearch,
            ConfigKey::InteractionsSystemHistory,
            ConfigKey::InteractionsSystemDelete,
            ConfigKey::InteractionsSystemBatch,
            ConfigKey::InteractionsSystemTransaction,
            ConfigKey::InteractionsSystemHistoryBundle,
            // Interactions - Compartment
            ConfigKey::InteractionsCompartmentSearch,
            // Interactions - Operations
            ConfigKey::InteractionsOperationsSystem,
            ConfigKey::InteractionsOperationsTypeLevel,
            ConfigKey::InteractionsOperationsInstance,
            // Format
            ConfigKey::FormatDefault,
            ConfigKey::FormatDefaultPreferReturn,
            // Behavior
            ConfigKey::BehaviorAllowUpdateCreate,
            ConfigKey::BehaviorHardDelete,
            // Audit
            ConfigKey::AuditEnabled,
            ConfigKey::AuditIncludeSuccess,
            ConfigKey::AuditIncludeAuthzFailure,
            ConfigKey::AuditIncludeProcessingFailure,
            ConfigKey::AuditCaptureSearchQuery,
            ConfigKey::AuditCaptureOperationOutcome,
            ConfigKey::AuditPerPatientEventsForSearch,
            ConfigKey::AuditInteractionsRead,
            ConfigKey::AuditInteractionsVread,
            ConfigKey::AuditInteractionsHistory,
            ConfigKey::AuditInteractionsSearch,
            ConfigKey::AuditInteractionsCreate,
            ConfigKey::AuditInteractionsUpdate,
            ConfigKey::AuditInteractionsPatch,
            ConfigKey::AuditInteractionsDelete,
            ConfigKey::AuditInteractionsCapabilities,
            ConfigKey::AuditInteractionsOperation,
            ConfigKey::AuditInteractionsBatch,
            ConfigKey::AuditInteractionsTransaction,
            ConfigKey::AuditInteractionsExport,
        ]
    }
}

impl fmt::Display for ConfigKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
