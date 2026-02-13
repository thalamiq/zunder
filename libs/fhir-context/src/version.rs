use semver::Version;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Version algorithm types as defined in FHIR spec
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionAlgorithm {
    /// Semantic versioning (semver.org)
    Semver,
    /// Integer ordering (numeric)
    Integer,
    /// Alphabetical ordering (case-insensitive, accent-insensitive)
    Alpha,
    /// Date/time ordering (ISO date/time syntax)
    Date,
    /// Natural ordering (naturalordersort.org)
    Natural,
}

impl VersionAlgorithm {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "semver" => Some(VersionAlgorithm::Semver),
            "integer" => Some(VersionAlgorithm::Integer),
            "alpha" => Some(VersionAlgorithm::Alpha),
            "date" => Some(VersionAlgorithm::Date),
            "natural" => Some(VersionAlgorithm::Natural),
            _ => None,
        }
    }

    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        match self {
            VersionAlgorithm::Semver => {
                match (Version::parse(a).ok(), Version::parse(b).ok()) {
                    (Some(va), Some(vb)) => va.cmp(&vb),
                    (Some(_), None) => Ordering::Greater,
                    (None, Some(_)) => Ordering::Less,
                    (None, None) => a.cmp(b), // Fallback to string comparison
                }
            }
            VersionAlgorithm::Integer => {
                match (a.parse::<i64>(), b.parse::<i64>()) {
                    (Ok(ia), Ok(ib)) => ia.cmp(&ib),
                    (Ok(_), Err(_)) => Ordering::Greater,
                    (Err(_), Ok(_)) => Ordering::Less,
                    (Err(_), Err(_)) => a.cmp(b), // Fallback to string comparison
                }
            }
            VersionAlgorithm::Alpha => {
                // Case-insensitive comparison
                a.to_lowercase().cmp(&b.to_lowercase())
            }
            VersionAlgorithm::Date => {
                // Try parsing as ISO 8601 dates
                // Simple implementation - can be enhanced with proper date parsing
                a.cmp(b)
            }
            VersionAlgorithm::Natural => {
                // Natural order sort - simplified implementation
                // Full implementation would follow naturalordersort.org
                natural_cmp(a, b)
            }
        }
    }
}

/// Extract version algorithm from a resource JSON Value
pub fn extract_version_algorithm(resource: &Value) -> Option<VersionAlgorithm> {
    // Try versionAlgorithmString first
    if let Some(alg_str) = resource
        .get("versionAlgorithmString")
        .and_then(|v| v.as_str())
    {
        return VersionAlgorithm::from_str(alg_str);
    }

    // Try versionAlgorithmCoding
    if let Some(coding) = resource.get("versionAlgorithmCoding") {
        if let Some(code) = coding.get("code").and_then(|v| v.as_str()) {
            return VersionAlgorithm::from_str(code);
        }
    }

    None
}

/// Natural order comparison (simplified implementation)
/// For full implementation, see naturalordersort.org
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let mut a_idx = 0;
    let mut b_idx = 0;

    while a_idx < a_chars.len() && b_idx < b_chars.len() {
        let a_char = a_chars[a_idx];
        let b_char = b_chars[b_idx];

        if a_char.is_ascii_digit() && b_char.is_ascii_digit() {
            // Extract numeric sequences
            let a_num_start = a_idx;
            let b_num_start = b_idx;

            while a_idx < a_chars.len() && a_chars[a_idx].is_ascii_digit() {
                a_idx += 1;
            }
            while b_idx < b_chars.len() && b_chars[b_idx].is_ascii_digit() {
                b_idx += 1;
            }

            let a_num: i64 = a[a_num_start..a_idx].parse().unwrap_or(0);
            let b_num: i64 = b[b_num_start..b_idx].parse().unwrap_or(0);

            let num_cmp = a_num.cmp(&b_num);
            if num_cmp != Ordering::Equal {
                return num_cmp;
            }
        } else {
            let a_lower = a_char.to_lowercase().next().unwrap_or(a_char);
            let b_lower = b_char.to_lowercase().next().unwrap_or(b_char);
            let char_cmp = a_lower.cmp(&b_lower);
            if char_cmp != Ordering::Equal {
                return char_cmp;
            }
            a_idx += 1;
            b_idx += 1;
        }
    }

    a_chars.len().cmp(&b_chars.len())
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct VersionKey {
    pub(crate) original: String,
    pub(crate) algorithm: Option<VersionAlgorithm>,
    pub(crate) semver: Option<Version>,
    pub(crate) integer: Option<i64>,
}

impl VersionKey {
    pub(crate) fn new(version: &str, algorithm: Option<VersionAlgorithm>) -> Self {
        let semver = Version::parse(version).ok();
        let integer = version.parse::<i64>().ok();

        Self {
            original: version.to_string(),
            algorithm,
            semver,
            integer,
        }
    }
}

impl Ord for VersionKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // If both have the same algorithm, use it.
        if let (Some(alg_a), Some(alg_b)) = (self.algorithm, other.algorithm) {
            if alg_a == alg_b {
                return alg_a.compare(&self.original, &other.original);
            }
        }

        // If one has an algorithm, prefer it.
        if let Some(alg) = self.algorithm {
            return alg.compare(&self.original, &other.original);
        }
        if let Some(alg) = other.algorithm {
            return alg.compare(&self.original, &other.original).reverse();
        }

        // Fallback: try semver first, then integer, then string.
        match (&self.semver, &other.semver) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => match (&self.integer, &other.integer) {
                (Some(a), Some(b)) => a.cmp(b),
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (None, None) => self.original.cmp(&other.original),
            },
        }
    }
}

impl PartialOrd for VersionKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(crate) fn select_from_version_index<'a>(
    versions: &'a BTreeMap<VersionKey, Arc<Value>>,
    version: Option<&str>,
) -> Option<&'a Arc<Value>> {
    match version {
        Some(v) => {
            // For exact version lookup, find by version string regardless of algorithm.
            versions
                .iter()
                .find(|(key, _)| key.original == v)
                .map(|(_, value)| value)
        }
        None => {
            // Prefer stable release (no prerelease suffix). If none, return highest prerelease.
            let stable = versions
                .iter()
                .rev()
                .find(|(k, _)| k.semver.as_ref().is_some_and(|v| v.pre.is_empty()))
                .map(|(_, v)| v);

            stable.or_else(|| versions.iter().next_back().map(|(_, v)| v))
        }
    }
}
