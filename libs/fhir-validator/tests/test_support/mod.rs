#![allow(dead_code)]

use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::runtime::Runtime;

// ---------------------------------------------------------------------------
// Async runtime helpers (shared with fhir-snapshot test pattern)
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
// Manifest types
// ---------------------------------------------------------------------------

pub const VALIDATOR_DIR: &str = "../../fhir-test-cases/validator";

/// Top-level manifest.json
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
    /// Can be a string URL or a complex object — we only care about presence, not content.
    #[serde(default)]
    pub logical: Option<Value>,
    /// Catch all other fields we don't care about.
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

/// The `java` field is polymorphic: either a string path or an inline object.
#[derive(Debug, Clone)]
pub enum JavaExpectation {
    /// Path to an outcome file, e.g. "java/R4.allergy-base.json"
    FilePath(String),
    /// Inline expectation with errorCount
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

/// Resolve a JavaExpectation to an expected error count.
pub fn resolve_expected_errors(expectation: &JavaExpectation) -> u32 {
    match expectation {
        JavaExpectation::Inline { error_count } => *error_count,
        JavaExpectation::FilePath(path) => {
            let outcome_path = PathBuf::from(VALIDATOR_DIR).join("outcomes").join(path);
            if !outcome_path.exists() {
                return u32::MAX;
            }
            let content = fs::read_to_string(&outcome_path).unwrap_or_else(|e| {
                panic!("Failed to read outcome file {}: {}", outcome_path.display(), e)
            });
            let outcome: Value = serde_json::from_str(&content).unwrap_or_else(|e| {
                panic!("Failed to parse outcome file {}: {}", outcome_path.display(), e)
            });
            count_errors_in_outcome(&outcome)
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
// Filtering helpers
// ---------------------------------------------------------------------------

/// Modules we skip (require external services or unsupported formats).
const SKIP_MODULES: &[&str] = &[
    "tx",
    "tx-advanced",
    "cda",
    "cdshooks",
    "shc",
    "logical",
    "json5",
];

/// FHIR versions we support.
fn is_supported_version(version: Option<&str>) -> bool {
    match version {
        None => true,        // absent = R5
        Some("4.0") => true, // R4
        Some("5.0") => true, // R5
        _ => false,          // R3, R4B, etc. — skip for now
    }
}

/// Determine which FHIR version label to use for context creation.
pub fn fhir_version_label(version: Option<&str>) -> &str {
    match version {
        Some("4.0") => "R4",
        _ => "R5",
    }
}

/// Check if a test case is eligible for our harness.
pub fn is_eligible(tc: &TestCase) -> bool {
    // Skip if explicitly disabled
    if tc.use_test == Some(false) {
        return false;
    }

    // Skip non-JSON files
    if !tc.file.ends_with(".json") {
        return false;
    }

    // Skip unsupported FHIR versions
    if !is_supported_version(tc.version.as_deref()) {
        return false;
    }

    // Skip unsupported modules
    if let Some(ref module) = tc.module {
        if SKIP_MODULES.contains(&module.as_str()) {
            return false;
        }
    }

    // Skip tests without java expectations
    if tc.java.is_none() {
        return false;
    }

    // Skip tests requiring external packages
    if tc.packages.is_some() {
        return false;
    }

    true
}

// ---------------------------------------------------------------------------
// Manifest loading
// ---------------------------------------------------------------------------

pub fn load_manifest() -> Option<Manifest> {
    let manifest_path = Path::new(VALIDATOR_DIR).join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("Failed to read manifest: {}", e));
    let manifest: Manifest = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse manifest: {}", e));
    Some(manifest)
}

/// Load a test resource file from the validator test cases directory.
pub fn load_test_resource(file: &str) -> Option<Value> {
    let path = PathBuf::from(VALIDATOR_DIR).join(file);
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}
