//! FHIR Validator - Three-phase, pipeline-based validation architecture
//!
//! # Architecture
//!
//! The validator separates configuration, planning, and execution:
//!
//! ```text
//! ValidatorConfig (declarative) → ValidationPlan (executable) → Validator (reusable)
//! ```
//!
//! ## Phase 1: Declarative Configuration
//!
//! Define validation behavior via [`ValidatorConfig`]:
//! - What aspects to validate (schema, profiles, invariants, terminology, etc.)
//! - Strictness levels and policies
//! - Preset-based or fully custom
//! - Serializable (YAML/JSON)
//!
//! ## Phase 2: Compiled Validation Plan
//!
//! Configuration compiles into a [`ValidationPlan`]:
//! - Ordered list of stateless validation steps
//! - Validates configuration correctness
//! - Eliminates unused features
//! - Minimal, executable pipeline
//!
//! ## Phase 3: Reusable Validator & Stateless Execution
//!
//! [`Validator<C>`] owns the plan and FHIR context:
//! - Generic over context type (e.g., `DefaultFhirContext`)
//! - Reusable across many validations
//! - Each `validate()` call creates a short-lived `ValidationRun`
//! - Returns structured [`ValidationOutcome`]
//!
//! # Key Properties
//!
//! - **No combinatorial explosion**: Capabilities selected via configuration, not chained APIs
//! - **FHIR-context driven**: All profile/terminology resolution delegated to `fhir-context`
//! - **Extensible**: New validation steps added without breaking public API
//! - **Reusable & performant**: Heavy initialization once, cheap execution

use serde::{Deserialize, Serialize};
use std::time::Duration;

mod error;
mod plan;
mod steps;
pub mod terminology;
mod validator;

pub use error::ConfigError;
pub use plan::{
    BundlePlan, ConstraintsPlan, ProfilesPlan, ReferencesPlan, SchemaPlan, Step, TerminologyPlan,
    ValidationPlan,
};
pub use terminology::{CodeValidationResult, TerminologyProvider};
pub use validator::{IssueCode, IssueSeverity, ValidationIssue, ValidationOutcome, Validator};

// ============================================================================
// Core Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<Preset>,
    #[serde(default)]
    pub fhir: FhirConfig,
    #[serde(default)]
    pub report: ReportConfig,
    #[serde(default)]
    pub exec: ExecConfig,
    #[serde(default)]
    pub schema: SchemaConfig,
    #[serde(default)]
    pub constraints: ConstraintsConfig,
    #[serde(default)]
    pub profiles: ProfilesConfig,
    #[serde(default)]
    pub terminology: TerminologyConfig,
    #[serde(default)]
    pub references: ReferencesConfig,
    #[serde(default)]
    pub bundles: BundleConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    Ingestion,
    Authoring,
    Server,
    Publication,
}

// ============================================================================
// FHIR Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirConfig {
    #[serde(default = "default_fhir_version")]
    pub version: FhirVersion,
    #[serde(default)]
    pub allow_version_mismatch: bool,
}

fn default_fhir_version() -> FhirVersion {
    FhirVersion::R5
}

impl Default for FhirConfig {
    fn default() -> Self {
        Self {
            version: FhirVersion::R5,
            allow_version_mismatch: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FhirVersion {
    R4,
    R5,
}

// ============================================================================
// Execution Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecConfig {
    #[serde(default)]
    pub fail_fast: bool,
    #[serde(default = "default_max_issues")]
    pub max_issues: usize,
}

fn default_max_issues() -> usize {
    1000
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            fail_fast: false,
            max_issues: 1000,
        }
    }
}

// ============================================================================
// Report Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportConfig {
    #[serde(default = "default_include_warnings")]
    pub include_warnings: bool,
    #[serde(default)]
    pub include_information: bool,
}

fn default_include_warnings() -> bool {
    true
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            include_warnings: true,
            include_information: false,
        }
    }
}

// ============================================================================
// Schema Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaConfig {
    #[serde(default = "default_schema_mode")]
    pub mode: SchemaMode,
    #[serde(default)]
    pub allow_unknown_elements: bool,
    #[serde(default)]
    pub allow_modifier_extensions: bool,
}

fn default_schema_mode() -> SchemaMode {
    SchemaMode::On
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaMode {
    Off,
    On,
}

impl Default for SchemaConfig {
    fn default() -> Self {
        Self {
            mode: SchemaMode::On,
            allow_unknown_elements: false,
            allow_modifier_extensions: false,
        }
    }
}

// ============================================================================
// Constraints Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintsConfig {
    #[serde(default)]
    pub mode: ConstraintsMode,
    #[serde(default)]
    pub best_practice: BestPracticeMode,
    #[serde(default)]
    pub suppress: Vec<ConstraintId>,
    #[serde(default)]
    pub level_overrides: Vec<ConstraintLevelOverride>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConstraintsMode {
    Off,
    InvariantsOnly,
    #[default]
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BestPracticeMode {
    #[default]
    Ignore,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConstraintId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintLevelOverride {
    pub id: ConstraintId,
    pub level: IssueLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueLevel {
    Error,
    Warning,
    Information,
}

impl Default for ConstraintsConfig {
    fn default() -> Self {
        Self {
            mode: ConstraintsMode::Full,
            best_practice: BestPracticeMode::Ignore,
            suppress: vec![],
            level_overrides: vec![],
        }
    }
}

// ============================================================================
// Profiles Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilesConfig {
    #[serde(default)]
    pub mode: ProfilesMode,
    /// Explicit list of profile URLs to validate against.
    /// If provided, validates against these profiles instead of (or in addition to) meta.profile.
    /// If not provided, validates against profiles declared in resource.meta.profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explicit_profiles: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProfilesMode {
    #[default]
    Off,
    On,
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        Self {
            mode: ProfilesMode::Off,
            explicit_profiles: None,
        }
    }
}

// ============================================================================
// Terminology Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminologyConfig {
    #[serde(default)]
    pub mode: TerminologyMode,
    #[serde(default)]
    pub extensible_handling: ExtensibleHandling,
    #[serde(with = "duration_millis", default = "default_terminology_timeout")]
    pub timeout: Duration,
    #[serde(default)]
    pub on_timeout: TimeoutPolicy,
    #[serde(default)]
    pub cache: CachePolicy,
}

fn default_terminology_timeout() -> Duration {
    Duration::from_millis(1500)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TerminologyMode {
    #[default]
    Off,
    Local,
    Remote,
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExtensibleHandling {
    Ignore,
    #[default]
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeoutPolicy {
    Skip,
    #[default]
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CachePolicy {
    None,
    #[default]
    Memory,
}

impl Default for TerminologyConfig {
    fn default() -> Self {
        Self {
            mode: TerminologyMode::Off,
            extensible_handling: ExtensibleHandling::Warn,
            timeout: Duration::from_millis(1500),
            on_timeout: TimeoutPolicy::Warn,
            cache: CachePolicy::Memory,
        }
    }
}

// Duration serialization helper
mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

// ============================================================================
// References Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferencesConfig {
    #[serde(default)]
    pub mode: ReferenceMode,
    #[serde(default = "default_allow_external")]
    pub allow_external: bool,
}

fn default_allow_external() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReferenceMode {
    #[default]
    Off,
    TypeOnly,
    Existence,
    Full,
}

impl Default for ReferencesConfig {
    fn default() -> Self {
        Self {
            mode: ReferenceMode::Off,
            allow_external: true,
        }
    }
}

// ============================================================================
// Bundle Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleConfig {
    #[serde(default)]
    pub mode: BundleMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BundleMode {
    #[default]
    Off,
    On,
}

impl Default for BundleConfig {
    fn default() -> Self {
        Self {
            mode: BundleMode::Off,
        }
    }
}

// ============================================================================
// ValidatorConfig Implementation
// ============================================================================

impl ValidatorConfig {
    pub fn preset(p: Preset) -> Self {
        let mut cfg = Self::defaults();
        cfg.preset = Some(p);

        match p {
            Preset::Ingestion => {
                cfg.schema.mode = SchemaMode::On;
                cfg.constraints.mode = ConstraintsMode::Off;
                cfg.profiles.mode = ProfilesMode::Off;
                cfg.terminology.mode = TerminologyMode::Off;
                cfg.references.mode = ReferenceMode::Off;
            }
            Preset::Authoring => {
                cfg.schema.mode = SchemaMode::On;
                cfg.profiles.mode = ProfilesMode::On;
                cfg.constraints.mode = ConstraintsMode::Full;
                cfg.terminology.mode = TerminologyMode::Local;
            }
            Preset::Server => {
                cfg.schema.mode = SchemaMode::On;
                cfg.profiles.mode = ProfilesMode::On;
                cfg.constraints.mode = ConstraintsMode::Full;
                cfg.terminology.mode = TerminologyMode::Hybrid;
                cfg.references.mode = ReferenceMode::Existence;
            }
            Preset::Publication => {
                cfg.schema.mode = SchemaMode::On;
                cfg.profiles.mode = ProfilesMode::On;
                cfg.constraints.mode = ConstraintsMode::Full;
                cfg.constraints.best_practice = BestPracticeMode::Warn;
                cfg.terminology.mode = TerminologyMode::Remote;
                cfg.references.mode = ReferenceMode::Full;
            }
        }

        cfg
    }

    pub fn defaults() -> Self {
        Self {
            preset: None,
            fhir: FhirConfig {
                version: FhirVersion::R5,
                allow_version_mismatch: false,
            },
            report: ReportConfig::default(),
            exec: ExecConfig::default(),
            schema: SchemaConfig::default(),
            constraints: ConstraintsConfig::default(),
            profiles: ProfilesConfig::default(),
            terminology: TerminologyConfig::default(),
            references: ReferencesConfig::default(),
            bundles: BundleConfig::default(),
        }
    }

    pub fn compile(&self) -> Result<ValidationPlan, ConfigError> {
        // Validate incompatible combinations
        if self.references.mode == ReferenceMode::Full
            && self.terminology.mode == TerminologyMode::Off
        {
            return Err(ConfigError::TerminologyRequiredForFullRef);
        }

        let mut steps = Vec::new();

        if self.schema.mode == SchemaMode::On {
            steps.push(Step::Schema(SchemaPlan::from(&self.schema)));
        }
        if self.profiles.mode == ProfilesMode::On {
            steps.push(Step::Profiles(ProfilesPlan::from(&self.profiles)));
        }
        if self.constraints.mode != ConstraintsMode::Off {
            steps.push(Step::Constraints(ConstraintsPlan::from(&self.constraints)));
        }
        if self.terminology.mode != TerminologyMode::Off {
            steps.push(Step::Terminology(TerminologyPlan::from(&self.terminology)));
        }
        if self.references.mode != ReferenceMode::Off {
            steps.push(Step::References(ReferencesPlan::from(&self.references)));
        }
        if self.bundles.mode == BundleMode::On {
            steps.push(Step::Bundles(BundlePlan::from(&self.bundles)));
        }

        Ok(ValidationPlan {
            steps,
            fail_fast: self.exec.fail_fast,
            max_issues: self.exec.max_issues,
        })
    }

    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }

    pub fn builder() -> ValidatorConfigBuilder {
        ValidatorConfigBuilder::default()
    }
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

// ============================================================================
// Builder Pattern
// ============================================================================

#[derive(Debug, Default, Clone)]
pub struct ValidatorConfigBuilder {
    cfg: Option<ValidatorConfig>,
}

impl ValidatorConfigBuilder {
    pub fn preset(mut self, p: Preset) -> Self {
        self.cfg = Some(ValidatorConfig::preset(p));
        self
    }

    pub fn fhir_version(mut self, version: FhirVersion) -> Self {
        self.cfg().fhir.version = version;
        self
    }

    pub fn schema_mode(mut self, mode: SchemaMode) -> Self {
        self.cfg().schema.mode = mode;
        self
    }

    pub fn constraints_mode(mut self, mode: ConstraintsMode) -> Self {
        self.cfg().constraints.mode = mode;
        self
    }

    pub fn profiles_mode(mut self, mode: ProfilesMode) -> Self {
        self.cfg().profiles.mode = mode;
        self
    }

    pub fn terminology_mode(mut self, mode: TerminologyMode) -> Self {
        self.cfg().terminology.mode = mode;
        self
    }

    pub fn reference_mode(mut self, mode: ReferenceMode) -> Self {
        self.cfg().references.mode = mode;
        self
    }

    pub fn fail_fast(mut self, fail_fast: bool) -> Self {
        self.cfg().exec.fail_fast = fail_fast;
        self
    }

    pub fn max_issues(mut self, max: usize) -> Self {
        self.cfg().exec.max_issues = max;
        self
    }

    pub fn build(self) -> ValidatorConfig {
        self.cfg.unwrap_or_default()
    }

    fn cfg(&mut self) -> &mut ValidatorConfig {
        self.cfg.get_or_insert_with(ValidatorConfig::defaults)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_ingestion() {
        let cfg = ValidatorConfig::preset(Preset::Ingestion);
        assert_eq!(cfg.schema.mode, SchemaMode::On);
        assert_eq!(cfg.constraints.mode, ConstraintsMode::Off);
        assert_eq!(cfg.terminology.mode, TerminologyMode::Off);
    }

    #[test]
    fn test_builder() {
        let cfg = ValidatorConfig::builder()
            .preset(Preset::Server)
            .terminology_mode(TerminologyMode::Local)
            .fail_fast(true)
            .build();

        assert_eq!(cfg.preset, Some(Preset::Server));
        assert_eq!(cfg.terminology.mode, TerminologyMode::Local);
        assert!(cfg.exec.fail_fast);
    }

    #[test]
    fn test_yaml_roundtrip() {
        let cfg = ValidatorConfig::preset(Preset::Authoring);
        let yaml = cfg.to_yaml().unwrap();
        let parsed = ValidatorConfig::from_yaml(&yaml).unwrap();
        assert_eq!(cfg.preset, parsed.preset);
        assert_eq!(cfg.schema.mode, parsed.schema.mode);
    }

    #[test]
    fn test_compile_validation() {
        let cfg = ValidatorConfig::builder()
            .reference_mode(ReferenceMode::Full)
            .terminology_mode(TerminologyMode::Off)
            .build();

        let result = cfg.compile();
        assert!(matches!(
            result,
            Err(ConfigError::TerminologyRequiredForFullRef)
        ));
    }
}
