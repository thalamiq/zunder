#![allow(dead_code)]

use ferrum_context::FhirContext;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::runtime::Runtime;

// ---------------------------------------------------------------------------
// Async runtime helpers
// ---------------------------------------------------------------------------

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static BLOCK_ON_GUARD: OnceLock<std::sync::Mutex<()>> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("failed to create Tokio runtime for tests"))
}

pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let guard = BLOCK_ON_GUARD.get_or_init(|| std::sync::Mutex::new(()));
    let _lock = guard.lock().expect("failed to lock block_on guard");
    runtime().block_on(future)
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

pub const VALIDATOR_DIR: &str = "../../fhir-test-cases/validator";

pub fn validator_dir() -> PathBuf {
    PathBuf::from(VALIDATOR_DIR)
}

// ---------------------------------------------------------------------------
// Manifest types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Manifest {
    #[serde(rename = "test-cases")]
    pub test_cases: Vec<TestCase>,
}

#[derive(Debug, Deserialize)]
pub struct TestCase {
    pub name: String,
    pub file: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default, rename = "use-test")]
    pub use_test: Option<bool>,
    #[serde(default)]
    pub java: Option<JavaExpectation>,
    #[serde(default)]
    pub supporting: Option<Vec<String>>,
    #[serde(default)]
    pub profiles: Option<Vec<String>>,
    #[serde(default)]
    pub profile: Option<ProfileTest>,
    #[serde(default)]
    pub packages: Option<Vec<String>>,
    #[serde(default)]
    pub logical: Option<Value>,
    #[serde(flatten)]
    _extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct ProfileTest {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub java: Option<JavaExpectation>,
    #[serde(default)]
    pub supporting: Option<Vec<String>>,
    #[serde(flatten)]
    _extra: serde_json::Map<String, Value>,
}

/// The `java` field is polymorphic: either a string path to an outcome file
/// or an inline object with `errorCount` / `output`.
#[derive(Debug, Clone)]
pub enum JavaExpectation {
    FilePath(String),
    Inline { error_count: u32 },
}

impl<'de> Deserialize<'de> for JavaExpectation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(s) => Ok(JavaExpectation::FilePath(s)),
            Value::Object(map) => {
                if let Some(count) = map.get("errorCount") {
                    let count = count.as_u64().unwrap_or(0) as u32;
                    Ok(JavaExpectation::Inline { error_count: count })
                } else if let Some(outcome) = map.get("outcome") {
                    let count = count_errors_in_outcome(outcome);
                    Ok(JavaExpectation::Inline { error_count: count })
                } else {
                    Ok(JavaExpectation::Inline { error_count: 0 })
                }
            }
            _ => Err(serde::de::Error::custom("expected string or object for java")),
        }
    }
}

// ---------------------------------------------------------------------------
// Expected error count resolution
// ---------------------------------------------------------------------------

pub fn resolve_expected_errors(expectation: &JavaExpectation) -> Option<u32> {
    match expectation {
        JavaExpectation::Inline { error_count } => Some(*error_count),
        JavaExpectation::FilePath(path) => {
            let outcome_path = validator_dir().join("outcomes").join(path);
            let content = fs::read_to_string(&outcome_path).ok()?;
            let outcome: Value = serde_json::from_str(&content).ok()?;
            Some(count_errors_in_outcome(&outcome))
        }
    }
}

/// Count issues with severity "error" or "fatal" in an OperationOutcome.
fn count_errors_in_outcome(outcome: &Value) -> u32 {
    let issues = match outcome.get("issue").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return 0,
    };
    issues
        .iter()
        .filter(|issue| {
            matches!(
                issue.get("severity").and_then(|s| s.as_str()),
                Some("error" | "fatal")
            )
        })
        .count() as u32
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Modules we skip (require external terminology server, unsupported formats, etc.).
const SKIP_MODULES: &[&str] = &[
    "tx",
    "tx-advanced",
    "cda",
    "cdshooks",
    "shc",
    "logical",
    "json5",
];

fn is_supported_version(version: Option<&str>) -> bool {
    matches!(version, None | Some("4.0") | Some("5.0"))
}

pub fn fhir_version_label(version: Option<&str>) -> &str {
    match version {
        Some("4.0") => "R4",
        _ => "R5",
    }
}

/// Determine why a test case is skipped, or `None` if eligible.
pub fn skip_reason(tc: &TestCase) -> Option<&'static str> {
    if tc.use_test == Some(false) {
        return Some("disabled via use-test");
    }
    if !tc.file.ends_with(".json") {
        return Some("non-JSON file");
    }
    if !is_supported_version(tc.version.as_deref()) {
        return Some("unsupported FHIR version");
    }
    if let Some(ref module) = tc.module {
        if SKIP_MODULES.contains(&module.as_str()) {
            return Some("unsupported module");
        }
    }
    if tc.java.is_none() {
        return Some("no java expectations");
    }
    if tc.packages.is_some() {
        return Some("requires external packages");
    }
    None
}

pub fn is_eligible(tc: &TestCase) -> bool {
    skip_reason(tc).is_none()
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

pub fn load_manifest() -> Option<Manifest> {
    let manifest_path = validator_dir().join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("Failed to read manifest: {}", e));
    let manifest: Manifest = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse manifest: {}", e));
    Some(manifest)
}

pub fn load_test_resource(file: &str) -> Option<Value> {
    let path = validator_dir().join(file);
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

// ---------------------------------------------------------------------------
// Overlay context for supporting resources
// ---------------------------------------------------------------------------

/// A FhirContext that layers test fixture resources over a base context.
/// Resources loaded from test fixtures take precedence over the base context.
pub struct OverlayFhirContext<C: FhirContext> {
    base: Arc<C>,
    overrides: HashMap<String, Arc<Value>>,
}

impl<C: FhirContext> OverlayFhirContext<C> {
    pub fn new(base: Arc<C>) -> Self {
        Self {
            base,
            overrides: HashMap::new(),
        }
    }

    /// Add a resource to the overlay, indexed by its canonical URL.
    pub fn add_resource(&mut self, url: String, resource: Value) {
        self.overrides.insert(url, Arc::new(resource));
    }
}

impl<C: FhirContext> FhirContext for OverlayFhirContext<C> {
    fn get_resource_by_url(
        &self,
        canonical_url: &str,
        version: Option<&str>,
    ) -> ferrum_context::Result<Option<Arc<Value>>> {
        // Check overrides first (version-unaware — test fixtures typically don't version)
        if let Some(resource) = self.overrides.get(canonical_url) {
            return Ok(Some(resource.clone()));
        }
        self.base.get_resource_by_url(canonical_url, version)
    }
}

/// Load supporting files into an overlay context.
/// Each file is read from the validator test directory, its canonical URL extracted,
/// and added to the overlay.
pub fn load_supporting_resources<C: FhirContext>(
    base: Arc<C>,
    files: &[String],
) -> OverlayFhirContext<C> {
    let mut overlay = OverlayFhirContext::new(base);
    for file in files {
        let Some(resource) = load_test_resource(file) else {
            continue;
        };
        // Extract canonical URL from the resource
        if let Some(url) = resource.get("url").and_then(|v| v.as_str()) {
            overlay.add_resource(url.to_string(), resource);
        }
    }
    overlay
}
