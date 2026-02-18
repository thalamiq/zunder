use crate::terminology::{InMemoryTerminologyProvider, TerminologyProvider};
use crate::{ConfigError, TerminologyMode, ValidationPlan};
use ferrum_context::FhirContext;
use ferrum_fhirpath::Engine as FhirPathEngine;
use ferrum_snapshot::ExpandedFhirContext;
use serde_json::Value;
use std::sync::Arc;

/// Reusable validator - owns plan, context, FHIRPath engine, and optional terminology provider
pub struct Validator<C: FhirContext> {
    plan: ValidationPlan,
    context: Arc<C>,
    fhirpath_engine: Arc<FhirPathEngine>,
    terminology: Option<Arc<dyn TerminologyProvider>>,
}

impl<C: FhirContext + 'static> Validator<C> {
    pub fn new(plan: ValidationPlan, context: C) -> Self {
        let context = Arc::new(context);

        // Create FHIRPath engine sharing the same context for discriminator evaluation
        let fhirpath_engine = Arc::new(FhirPathEngine::new(
            context.clone() as Arc<dyn FhirContext>,
            None,
        ));

        // Create terminology provider based on plan
        let terminology = Self::create_terminology_provider(&plan, &context);

        Self {
            plan,
            context,
            fhirpath_engine,
            terminology,
        }
    }

    pub fn from_config(config: &crate::ValidatorConfig, context: C) -> Result<Self, ConfigError> {
        let plan = config.compile()?;
        Ok(Self::new(plan, context))
    }

    /// Wrap the current context with an [`ExpandedFhirContext`], which:
    /// - materializes snapshots from differentials (via `baseDefinition`)
    /// - deep-expands snapshots for nested type validation
    /// - caches expanded StructureDefinitions across validation runs
    pub fn with_expanded_snapshots(self) -> Validator<ExpandedFhirContext<C>>
    where
        C: Clone,
    {
        // Extract inner context from Arc
        let inner_context = Arc::try_unwrap(self.context).unwrap_or_else(|arc| (*arc).clone());
        let expanded_context = ExpandedFhirContext::new(inner_context);
        let expanded_arc = Arc::new(expanded_context);

        // Create new engine for the expanded context
        let fhirpath_engine = Arc::new(FhirPathEngine::new(
            expanded_arc.clone() as Arc<dyn FhirContext>,
            None,
        ));

        // Create terminology provider for expanded context
        let terminology = Validator::<ExpandedFhirContext<C>>::create_terminology_provider_from_plan_and_context(
            &self.plan,
            &expanded_arc,
        );

        Validator {
            plan: self.plan,
            context: expanded_arc,
            fhirpath_engine,
            terminology,
        }
    }

    pub fn validate(&self, resource: &Value) -> ValidationOutcome {
        ValidationRun::new(
            &self.plan,
            &self.context,
            &self.fhirpath_engine,
            self.terminology.as_deref(),
            resource,
        )
        .execute()
    }

    pub fn validate_batch(&self, resources: &[Value]) -> Vec<ValidationOutcome> {
        resources.iter().map(|r| self.validate(r)).collect()
    }

    pub fn plan(&self) -> &ValidationPlan {
        &self.plan
    }

    pub fn context(&self) -> &Arc<C> {
        &self.context
    }

    fn create_terminology_provider(
        plan: &ValidationPlan,
        context: &Arc<C>,
    ) -> Option<Arc<dyn TerminologyProvider>> {
        Self::create_terminology_provider_from_plan_and_context(plan, context)
    }

    fn create_terminology_provider_from_plan_and_context(
        plan: &ValidationPlan,
        context: &Arc<C>,
    ) -> Option<Arc<dyn TerminologyProvider>> {
        // Check if any step requires terminology
        let has_terminology_step = plan.steps.iter().any(|s| {
            matches!(s, crate::Step::Terminology(t) if t.mode != TerminologyMode::Off)
        });

        if !has_terminology_step {
            return None;
        }

        // For Local mode, create an InMemoryTerminologyProvider
        Some(Arc::new(InMemoryTerminologyProvider::new(context.clone())))
    }
}

/// Short-lived validation execution
struct ValidationRun<'a, C: FhirContext> {
    plan: &'a ValidationPlan,
    context: &'a Arc<C>,
    fhirpath_engine: &'a Arc<FhirPathEngine>,
    terminology: Option<&'a dyn TerminologyProvider>,
    resource: &'a Value,
    issues: Vec<ValidationIssue>,
}

impl<'a, C: FhirContext> ValidationRun<'a, C> {
    fn new(
        plan: &'a ValidationPlan,
        context: &'a Arc<C>,
        fhirpath_engine: &'a Arc<FhirPathEngine>,
        terminology: Option<&'a dyn TerminologyProvider>,
        resource: &'a Value,
    ) -> Self {
        Self {
            plan,
            context,
            fhirpath_engine,
            terminology,
            resource,
            issues: Vec::new(),
        }
    }

    fn execute(mut self) -> ValidationOutcome {
        for step in &self.plan.steps {
            if self.plan.fail_fast && self.has_errors() {
                break;
            }

            if self.issues.len() >= self.plan.max_issues {
                break;
            }

            self.execute_step(step);
        }

        ValidationOutcome {
            resource_type: self.get_resource_type(),
            valid: !self.has_errors(),
            issues: self.issues,
        }
    }

    fn execute_step(&mut self, step: &crate::Step) {
        use crate::Step;

        match step {
            Step::Schema(plan) => self.validate_schema(plan),
            Step::Profiles(plan) => self.validate_profiles(plan),
            Step::Constraints(plan) => self.validate_constraints(plan),
            Step::Terminology(plan) => self.validate_terminology(plan),
            Step::References(plan) => self.validate_references(plan),
            Step::Bundles(plan) => self.validate_bundles(plan),
        }
    }

    fn validate_schema(&mut self, plan: &crate::SchemaPlan) {
        crate::steps::schema::validate_schema(
            self.resource,
            plan,
            self.context.as_ref(),
            &mut self.issues,
        );
    }

    fn validate_profiles(&mut self, plan: &crate::ProfilesPlan) {
        crate::steps::profiles::validate_profiles(
            self.resource,
            plan,
            self.context.as_ref(),
            self.fhirpath_engine,
            &mut self.issues,
        );
    }

    fn validate_constraints(&mut self, plan: &crate::ConstraintsPlan) {
        crate::steps::constraints::validate_constraints(
            self.resource,
            plan,
            self.context.as_ref(),
            self.fhirpath_engine,
            &mut self.issues,
        );
    }

    fn validate_terminology(&mut self, plan: &crate::TerminologyPlan) {
        if let Some(terminology) = self.terminology {
            crate::steps::terminology::validate_terminology(
                self.resource,
                plan,
                self.context.as_ref(),
                terminology,
                &mut self.issues,
            );
        }
    }

    fn validate_references(&mut self, _plan: &crate::ReferencesPlan) {
        // TODO: Implement reference validation
    }

    fn validate_bundles(&mut self, _plan: &crate::BundlePlan) {
        // TODO: Implement bundle validation
    }

    fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error || i.severity == IssueSeverity::Fatal)
    }

    fn get_resource_type(&self) -> Option<String> {
        self.resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

/// Validation result for a single resource
#[derive(Debug, Clone)]
pub struct ValidationOutcome {
    pub resource_type: Option<String>,
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationOutcome {
    pub fn success(resource_type: Option<String>) -> Self {
        Self {
            resource_type,
            valid: true,
            issues: Vec::new(),
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.valid
    }

    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error || i.severity == IssueSeverity::Fatal)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count()
    }

    pub fn to_operation_outcome(&self) -> Value {
        serde_json::json!({
            "resourceType": "OperationOutcome",
            "issue": self.issues.iter().map(|i| i.to_json()).collect::<Vec<_>>()
        })
    }
}

/// Individual validation issue
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub code: IssueCode,
    pub diagnostics: String,
    pub location: Option<String>,
    pub expression: Option<Vec<String>>,
}

impl ValidationIssue {
    pub fn error(code: IssueCode, diagnostics: String) -> Self {
        Self {
            severity: IssueSeverity::Error,
            code,
            diagnostics,
            location: None,
            expression: None,
        }
    }

    pub fn warning(code: IssueCode, diagnostics: String) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            code,
            diagnostics,
            location: None,
            expression: None,
        }
    }

    pub fn information(code: IssueCode, diagnostics: String) -> Self {
        Self {
            severity: IssueSeverity::Information,
            code,
            diagnostics,
            location: None,
            expression: None,
        }
    }

    pub fn with_location(mut self, location: String) -> Self {
        self.location = Some(location);
        self
    }

    pub fn with_expression(mut self, expression: Vec<String>) -> Self {
        self.expression = Some(expression);
        self
    }

    fn to_json(&self) -> Value {
        let mut issue = serde_json::json!({
            "severity": self.severity.to_string().to_lowercase(),
            "code": self.code.to_string(),
            "diagnostics": self.diagnostics,
        });

        if let Some(ref loc) = self.location {
            issue["location"] = serde_json::json!([loc]);
        }

        if let Some(ref expr) = self.expression {
            issue["expression"] = serde_json::json!(expr);
        }

        issue
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Fatal,
    Error,
    Warning,
    Information,
}

impl std::fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fatal => write!(f, "Fatal"),
            Self::Error => write!(f, "Error"),
            Self::Warning => write!(f, "Warning"),
            Self::Information => write!(f, "Information"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueCode {
    Invalid,
    Structure,
    Required,
    Value,
    Invariant,
    Security,
    Login,
    Unknown,
    Expired,
    Forbidden,
    Suppressed,
    Processing,
    NotSupported,
    Duplicate,
    MultipleMatches,
    NotFound,
    Deleted,
    TooLong,
    CodeInvalid,
    Extension,
    TooCostly,
    BusinessRule,
    Conflict,
    Transient,
    LockError,
    NoStore,
    Exception,
    Timeout,
    Incomplete,
    Throttled,
    Informational,
}

impl std::fmt::Display for IssueCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Invalid => "invalid",
            Self::Structure => "structure",
            Self::Required => "required",
            Self::Value => "value",
            Self::Invariant => "invariant",
            Self::Security => "security",
            Self::Login => "login",
            Self::Unknown => "unknown",
            Self::Expired => "expired",
            Self::Forbidden => "forbidden",
            Self::Suppressed => "suppressed",
            Self::Processing => "processing",
            Self::NotSupported => "not-supported",
            Self::Duplicate => "duplicate",
            Self::MultipleMatches => "multiple-matches",
            Self::NotFound => "not-found",
            Self::Deleted => "deleted",
            Self::TooLong => "too-long",
            Self::CodeInvalid => "code-invalid",
            Self::Extension => "extension",
            Self::TooCostly => "too-costly",
            Self::BusinessRule => "business-rule",
            Self::Conflict => "conflict",
            Self::Transient => "transient",
            Self::LockError => "lock-error",
            Self::NoStore => "no-store",
            Self::Exception => "exception",
            Self::Timeout => "timeout",
            Self::Incomplete => "incomplete",
            Self::Throttled => "throttled",
            Self::Informational => "informational",
        };
        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_outcome_operations() {
        let outcome = ValidationOutcome {
            resource_type: Some("Patient".to_string()),
            valid: false,
            issues: vec![
                ValidationIssue::error(IssueCode::Required, "Missing required field".to_string()),
                ValidationIssue::warning(IssueCode::Value, "Deprecated code".to_string()),
            ],
        };

        assert!(!outcome.valid);
        assert!(outcome.has_errors());
        assert_eq!(outcome.error_count(), 1);
        assert_eq!(outcome.warning_count(), 1);
    }

    #[test]
    fn test_operation_outcome_conversion() {
        let outcome = ValidationOutcome {
            resource_type: Some("Patient".to_string()),
            valid: false,
            issues: vec![ValidationIssue::error(
                IssueCode::Required,
                "name is required".to_string(),
            )
            .with_location("Patient.name".to_string())
            .with_expression(vec!["Patient.name".to_string()])],
        };

        let op_outcome = outcome.to_operation_outcome();
        assert_eq!(op_outcome["resourceType"], "OperationOutcome");
        assert_eq!(op_outcome["issue"][0]["severity"], "error");
        assert_eq!(op_outcome["issue"][0]["code"], "required");
    }
}
