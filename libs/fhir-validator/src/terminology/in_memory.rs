use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use ferrum_context::FhirContext;
use serde_json::Value;

use super::provider::{CodeValidationResult, TerminologyProvider};
use crate::validator::IssueSeverity;

/// Expanded ValueSet: a flat set of (system, code) pairs for O(1) lookup.
#[derive(Debug)]
struct ExpandedValueSet {
    /// Set of (system, code) for fast membership check
    codes: HashSet<(String, String)>,
    /// Concepts with display names for display validation
    concepts: Vec<ExpandedConcept>,
}

#[derive(Debug, Clone)]
struct ExpandedConcept {
    system: String,
    code: String,
    display: Option<String>,
}

/// In-memory terminology provider that works with any FhirContext.
///
/// Expands ValueSets from the context's loaded packages and validates
/// codes against the expanded set. Caches expanded ValueSets for reuse.
pub struct InMemoryTerminologyProvider<C: FhirContext> {
    context: Arc<C>,
    expansion_cache: RwLock<HashMap<String, Arc<ExpandedValueSet>>>,
}

impl<C: FhirContext> InMemoryTerminologyProvider<C> {
    pub fn new(context: Arc<C>) -> Self {
        Self {
            context,
            expansion_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Expand a ValueSet by canonical URL. Returns None if the ValueSet is not found.
    fn expand_value_set(
        &self,
        url: &str,
    ) -> Result<Option<Arc<ExpandedValueSet>>, Box<dyn std::error::Error>> {
        // Check cache
        if let Some(cached) = self.expansion_cache.read().unwrap().get(url) {
            return Ok(Some(cached.clone()));
        }

        // Load ValueSet from context
        let vs_resource = match self.context.get_resource_by_url(url, None)? {
            Some(r) => r,
            None => return Ok(None),
        };

        // Only process ValueSet resources
        if vs_resource.get("resourceType").and_then(|v| v.as_str()) != Some("ValueSet") {
            return Ok(None);
        }

        let mut concepts = Vec::new();
        let mut visited = HashSet::new();
        visited.insert(url.to_string());

        self.expand_value_set_resource(&vs_resource, &mut concepts, &mut visited)?;

        let codes: HashSet<(String, String)> = concepts
            .iter()
            .map(|c| (c.system.clone(), c.code.clone()))
            .collect();

        let expanded = Arc::new(ExpandedValueSet { codes, concepts });

        // Cache the expansion
        self.expansion_cache
            .write()
            .unwrap()
            .insert(url.to_string(), expanded.clone());

        Ok(Some(expanded))
    }

    fn expand_value_set_resource(
        &self,
        vs: &Value,
        concepts: &mut Vec<ExpandedConcept>,
        visited: &mut HashSet<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 1. If the ValueSet has a pre-expanded expansion, use it directly
        if let Some(contains) = vs
            .get("expansion")
            .and_then(|e| e.get("contains"))
            .and_then(|c| c.as_array())
        {
            self.extract_expansion_contains(contains, concepts);
            return Ok(());
        }

        // 2. Process compose
        if let Some(compose) = vs.get("compose") {
            self.process_compose(compose, concepts, visited)?;
        }

        Ok(())
    }

    fn extract_expansion_contains(
        &self,
        contains: &[Value],
        concepts: &mut Vec<ExpandedConcept>,
    ) {
        for item in contains {
            let system = item.get("system").and_then(|v| v.as_str());
            let code = item.get("code").and_then(|v| v.as_str());
            let display = item
                .get("display")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Skip abstract entries (they have abstract: true and no code sometimes)
            let is_abstract = item
                .get("abstract")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if let (Some(system), Some(code)) = (system, code) {
                if !is_abstract {
                    concepts.push(ExpandedConcept {
                        system: system.to_string(),
                        code: code.to_string(),
                        display,
                    });
                }
            }

            // Recurse into nested contains
            if let Some(nested) = item.get("contains").and_then(|v| v.as_array()) {
                self.extract_expansion_contains(nested, concepts);
            }
        }
    }

    fn process_compose(
        &self,
        compose: &Value,
        concepts: &mut Vec<ExpandedConcept>,
        visited: &mut HashSet<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Process includes
        if let Some(includes) = compose.get("include").and_then(|v| v.as_array()) {
            for include in includes {
                self.process_include(include, concepts, visited)?;
            }
        }

        // Process excludes
        if let Some(excludes) = compose.get("exclude").and_then(|v| v.as_array()) {
            for exclude in excludes {
                self.process_exclude(exclude, concepts);
            }
        }

        Ok(())
    }

    fn process_include(
        &self,
        include: &Value,
        concepts: &mut Vec<ExpandedConcept>,
        visited: &mut HashSet<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let system = include.get("system").and_then(|v| v.as_str());

        // Explicit concept list
        if let Some(concept_list) = include.get("concept").and_then(|v| v.as_array()) {
            let system = system.unwrap_or("");
            for concept in concept_list {
                let code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");
                let display = concept
                    .get("display")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if !code.is_empty() {
                    concepts.push(ExpandedConcept {
                        system: system.to_string(),
                        code: code.to_string(),
                        display,
                    });
                }
            }
        } else if let Some(system) = system {
            // Include all codes from a CodeSystem (if available)
            // Try to load the CodeSystem and extract its concepts
            if let Ok(Some(cs)) = self.context.get_resource_by_url(system, None) {
                if cs.get("resourceType").and_then(|v| v.as_str()) == Some("CodeSystem") {
                    // Check for filters — skip filtered expansions in basic impl
                    if include.get("filter").is_some() {
                        // Filter-based expansion: too complex for basic in-memory provider
                        // Skip silently — the code might still match
                    } else {
                        self.extract_codesystem_concepts(&cs, system, concepts);
                    }
                }
            }
        }

        // Referenced ValueSets
        if let Some(vs_refs) = include.get("valueSet").and_then(|v| v.as_array()) {
            for vs_ref in vs_refs {
                if let Some(url) = vs_ref.as_str() {
                    if visited.insert(url.to_string()) {
                        if let Ok(Some(vs)) = self.context.get_resource_by_url(url, None) {
                            if vs.get("resourceType").and_then(|v| v.as_str()) == Some("ValueSet")
                            {
                                self.expand_value_set_resource(&vs, concepts, visited)?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn process_exclude(&self, exclude: &Value, concepts: &mut Vec<ExpandedConcept>) {
        let system = exclude.get("system").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(concept_list) = exclude.get("concept").and_then(|v| v.as_array()) {
            let exclude_set: HashSet<(&str, &str)> = concept_list
                .iter()
                .filter_map(|c| {
                    let code = c.get("code").and_then(|v| v.as_str())?;
                    Some((system, code))
                })
                .collect();
            concepts.retain(|c| !exclude_set.contains(&(c.system.as_str(), c.code.as_str())));
        }
    }

    fn extract_codesystem_concepts(
        &self,
        cs: &Value,
        system: &str,
        concepts: &mut Vec<ExpandedConcept>,
    ) {
        if let Some(concept_arr) = cs.get("concept").and_then(|v| v.as_array()) {
            self.extract_concepts_recursive(concept_arr, system, concepts);
        }
    }

    fn extract_concepts_recursive(
        &self,
        concept_arr: &[Value],
        system: &str,
        concepts: &mut Vec<ExpandedConcept>,
    ) {
        for concept in concept_arr {
            let code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let display = concept
                .get("display")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if !code.is_empty() {
                concepts.push(ExpandedConcept {
                    system: system.to_string(),
                    code: code.to_string(),
                    display,
                });
            }
            // Recurse into nested concepts
            if let Some(nested) = concept.get("concept").and_then(|v| v.as_array()) {
                self.extract_concepts_recursive(nested, system, concepts);
            }
        }
    }

    /// Look up CodeSystem content mode for a given system URL.
    /// Returns None if CodeSystem not found, or Some("complete"/"fragment"/"not-present"/"example").
    fn get_codesystem_content(&self, system: &str) -> Option<String> {
        let cs = self.context.get_resource_by_url(system, None).ok()??;
        if cs.get("resourceType").and_then(|v| v.as_str()) != Some("CodeSystem") {
            return None;
        }
        cs.get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Check if a code exists directly in a CodeSystem's concept hierarchy.
    fn find_code_in_codesystem(&self, system: &str, code: &str) -> Option<ExpandedConcept> {
        let cs = self.context.get_resource_by_url(system, None).ok()??;
        if cs.get("resourceType").and_then(|v| v.as_str()) != Some("CodeSystem") {
            return None;
        }
        let concepts = cs.get("concept")?.as_array()?;
        self.find_in_concept_tree(concepts, system, code)
    }

    fn find_in_concept_tree(
        &self,
        concepts: &[Value],
        system: &str,
        code: &str,
    ) -> Option<ExpandedConcept> {
        for concept in concepts {
            let c = concept.get("code").and_then(|v| v.as_str())?;
            if c == code {
                return Some(ExpandedConcept {
                    system: system.to_string(),
                    code: code.to_string(),
                    display: concept
                        .get("display")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }
            if let Some(nested) = concept.get("concept").and_then(|v| v.as_array()) {
                if let Some(found) = self.find_in_concept_tree(nested, system, code) {
                    return Some(found);
                }
            }
        }
        None
    }
}

impl<C: FhirContext> TerminologyProvider for InMemoryTerminologyProvider<C> {
    fn validate_code(
        &self,
        system: &str,
        code: &str,
        display: Option<&str>,
        value_set_url: &str,
    ) -> Result<Option<CodeValidationResult>, Box<dyn std::error::Error>> {
        let expanded = match self.expand_value_set(value_set_url)? {
            Some(e) => e,
            None => return Ok(None), // ValueSet not known
        };

        // Empty expansion usually means we couldn't expand (e.g., external CodeSystem)
        // In that case, check the CodeSystem content mode
        if expanded.codes.is_empty() {
            if system.is_empty() {
                return Ok(None); // Can't validate bare code against unknown ValueSet
            }
            return self.validate_code_with_content_mode(system, code);
        }

        // For bare code types (empty system), search by code only
        let (is_member, matched_concept) = if system.is_empty() {
            let concept = expanded.concepts.iter().find(|c| c.code == code);
            (concept.is_some(), concept)
        } else {
            let concept = expanded
                .concepts
                .iter()
                .find(|c| c.system == system && c.code == code);
            (concept.is_some(), concept)
        };

        if is_member {
            let concept_display = matched_concept.and_then(|c| c.display.clone());

            let message = if let Some(provided_display) = display {
                if let Some(ref correct) = concept_display {
                    if provided_display != correct {
                        Some(format!(
                            "Display mismatch: provided '{}', expected '{}'",
                            provided_display, correct
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Display mismatch is a warning, not an error
            let has_message = message.is_some();
            Ok(Some(CodeValidationResult {
                valid: true,
                display: concept_display,
                message,
                severity_override: if has_message {
                    Some(IssueSeverity::Warning)
                } else {
                    None
                },
            }))
        } else {
            // Code not in ValueSet — but check content mode for severity
            let severity_override = self
                .get_codesystem_content(system)
                .and_then(|content| match content.as_str() {
                    "fragment" => Some(IssueSeverity::Warning),
                    "not-present" | "example" => Some(IssueSeverity::Information),
                    _ => None,
                });

            Ok(Some(CodeValidationResult {
                valid: false,
                display: None,
                message: Some(format!(
                    "Code '{}' from system '{}' is not in the ValueSet '{}'",
                    code, system, value_set_url
                )),
                severity_override,
            }))
        }
    }

    fn validate_code_in_system(
        &self,
        system: &str,
        code: &str,
    ) -> Result<Option<CodeValidationResult>, Box<dyn std::error::Error>> {
        self.validate_code_with_content_mode(system, code)
    }
}

impl<C: FhirContext> InMemoryTerminologyProvider<C> {
    fn validate_code_with_content_mode(
        &self,
        system: &str,
        code: &str,
    ) -> Result<Option<CodeValidationResult>, Box<dyn std::error::Error>> {
        let content = match self.get_codesystem_content(system) {
            Some(c) => c,
            None => return Ok(None), // CodeSystem not known
        };

        match content.as_str() {
            "not-present" | "example" => {
                // Can't validate — no concept data available
                Ok(Some(CodeValidationResult {
                    valid: true,
                    display: None,
                    message: None,
                    severity_override: None,
                }))
            }
            "fragment" => {
                // Incomplete — try to find, but missing code is only a warning
                match self.find_code_in_codesystem(system, code) {
                    Some(concept) => Ok(Some(CodeValidationResult {
                        valid: true,
                        display: concept.display,
                        message: None,
                        severity_override: None,
                    })),
                    None => Ok(Some(CodeValidationResult {
                        valid: false,
                        display: None,
                        message: Some(format!(
                            "Code '{}' not found in fragment CodeSystem '{}'",
                            code, system
                        )),
                        severity_override: Some(IssueSeverity::Warning),
                    })),
                }
            }
            _ => {
                // Complete — code must exist
                match self.find_code_in_codesystem(system, code) {
                    Some(concept) => Ok(Some(CodeValidationResult {
                        valid: true,
                        display: concept.display,
                        message: None,
                        severity_override: None,
                    })),
                    None => Ok(Some(CodeValidationResult {
                        valid: false,
                        display: None,
                        message: Some(format!(
                            "Unknown code '{}' in CodeSystem '{}'",
                            code, system
                        )),
                        severity_override: None,
                    })),
                }
            }
        }
    }
}
