use crate::validator::IssueSeverity;

/// Result of validating a code against a ValueSet or CodeSystem.
#[derive(Debug, Clone)]
pub struct CodeValidationResult {
    /// Whether the code is valid in the given context.
    pub valid: bool,
    /// The correct display for the concept (if known).
    pub display: Option<String>,
    /// Human-readable message (error or warning detail).
    pub message: Option<String>,
    /// Override the default severity (e.g., for fragment CodeSystems: Warning instead of Error).
    pub severity_override: Option<IssueSeverity>,
}

/// Provides terminology validation capabilities to the validator.
///
/// Implementations range from in-memory (package-based, using FhirContext) to
/// remote (HTTP terminology server). The validator calls this trait during
/// the terminology validation step.
pub trait TerminologyProvider: Send + Sync {
    /// Validate a code against a ValueSet binding.
    ///
    /// Returns `Ok(None)` if the ValueSet cannot be resolved (provider doesn't know it).
    /// Returns `Ok(Some(result))` with validation outcome if the ValueSet is known.
    fn validate_code(
        &self,
        system: &str,
        code: &str,
        display: Option<&str>,
        value_set_url: &str,
    ) -> Result<Option<CodeValidationResult>, Box<dyn std::error::Error>>;

    /// Check if a code exists in a CodeSystem (without ValueSet context).
    ///
    /// Returns `Ok(None)` if the CodeSystem is not known.
    fn validate_code_in_system(
        &self,
        system: &str,
        code: &str,
    ) -> Result<Option<CodeValidationResult>, Box<dyn std::error::Error>>;
}
