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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- VersionAlgorithm::from_str ---

    #[test]
    fn from_str_recognises_all_variants() {
        assert_eq!(VersionAlgorithm::from_str("semver"), Some(VersionAlgorithm::Semver));
        assert_eq!(VersionAlgorithm::from_str("integer"), Some(VersionAlgorithm::Integer));
        assert_eq!(VersionAlgorithm::from_str("alpha"), Some(VersionAlgorithm::Alpha));
        assert_eq!(VersionAlgorithm::from_str("date"), Some(VersionAlgorithm::Date));
        assert_eq!(VersionAlgorithm::from_str("natural"), Some(VersionAlgorithm::Natural));
    }

    #[test]
    fn from_str_is_case_insensitive() {
        assert_eq!(VersionAlgorithm::from_str("Semver"), Some(VersionAlgorithm::Semver));
        assert_eq!(VersionAlgorithm::from_str("INTEGER"), Some(VersionAlgorithm::Integer));
    }

    #[test]
    fn from_str_returns_none_for_unknown() {
        assert_eq!(VersionAlgorithm::from_str("unknown"), None);
        assert_eq!(VersionAlgorithm::from_str(""), None);
    }

    // --- VersionAlgorithm::compare ---

    #[test]
    fn semver_compare_valid_versions() {
        let alg = VersionAlgorithm::Semver;
        assert_eq!(alg.compare("1.0.0", "2.0.0"), Ordering::Less);
        assert_eq!(alg.compare("2.0.0", "1.0.0"), Ordering::Greater);
        assert_eq!(alg.compare("1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(alg.compare("1.0.0-alpha", "1.0.0"), Ordering::Less);
    }

    #[test]
    fn semver_compare_invalid_fallback() {
        let alg = VersionAlgorithm::Semver;
        // Valid > invalid
        assert_eq!(alg.compare("1.0.0", "not-semver"), Ordering::Greater);
        // Invalid < valid
        assert_eq!(alg.compare("not-semver", "1.0.0"), Ordering::Less);
        // Both invalid: string comparison
        assert_eq!(alg.compare("abc", "def"), Ordering::Less);
    }

    #[test]
    fn integer_compare() {
        let alg = VersionAlgorithm::Integer;
        assert_eq!(alg.compare("1", "10"), Ordering::Less);
        assert_eq!(alg.compare("10", "2"), Ordering::Greater);
        assert_eq!(alg.compare("5", "5"), Ordering::Equal);
        // Valid > invalid
        assert_eq!(alg.compare("5", "abc"), Ordering::Greater);
        assert_eq!(alg.compare("abc", "5"), Ordering::Less);
    }

    #[test]
    fn alpha_compare_case_insensitive() {
        let alg = VersionAlgorithm::Alpha;
        assert_eq!(alg.compare("abc", "ABC"), Ordering::Equal);
        assert_eq!(alg.compare("alpha", "beta"), Ordering::Less);
    }

    #[test]
    fn date_compare() {
        let alg = VersionAlgorithm::Date;
        assert_eq!(alg.compare("2024-01-01", "2024-06-15"), Ordering::Less);
        assert_eq!(alg.compare("2025-01-01", "2024-12-31"), Ordering::Greater);
    }

    #[test]
    fn natural_compare() {
        let alg = VersionAlgorithm::Natural;
        assert_eq!(alg.compare("v2", "v10"), Ordering::Less);
        assert_eq!(alg.compare("file1", "file1"), Ordering::Equal);
    }

    // --- natural_cmp ---

    #[test]
    fn natural_cmp_numeric_segments() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("item20", "item3"), Ordering::Greater);
        assert_eq!(natural_cmp("v1.2", "v1.10"), Ordering::Less);
    }

    #[test]
    fn natural_cmp_case_insensitive() {
        assert_eq!(natural_cmp("ABC", "abc"), Ordering::Equal);
        assert_eq!(natural_cmp("Foo1", "foo1"), Ordering::Equal);
    }

    #[test]
    fn natural_cmp_prefix_ordering() {
        assert_eq!(natural_cmp("a", "ab"), Ordering::Less);
        assert_eq!(natural_cmp("abc", "ab"), Ordering::Greater);
    }

    #[test]
    fn natural_cmp_empty_strings() {
        assert_eq!(natural_cmp("", ""), Ordering::Equal);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
        assert_eq!(natural_cmp("a", ""), Ordering::Greater);
    }

    // --- extract_version_algorithm ---

    #[test]
    fn extract_algorithm_from_string() {
        let resource = json!({"versionAlgorithmString": "semver"});
        assert_eq!(extract_version_algorithm(&resource), Some(VersionAlgorithm::Semver));
    }

    #[test]
    fn extract_algorithm_from_coding() {
        let resource = json!({"versionAlgorithmCoding": {"code": "integer"}});
        assert_eq!(extract_version_algorithm(&resource), Some(VersionAlgorithm::Integer));
    }

    #[test]
    fn extract_algorithm_prefers_string_over_coding() {
        let resource = json!({
            "versionAlgorithmString": "alpha",
            "versionAlgorithmCoding": {"code": "integer"}
        });
        assert_eq!(extract_version_algorithm(&resource), Some(VersionAlgorithm::Alpha));
    }

    #[test]
    fn extract_algorithm_returns_none_when_absent() {
        let resource = json!({"url": "http://example.org"});
        assert_eq!(extract_version_algorithm(&resource), None);
    }

    // --- VersionKey ordering ---

    #[test]
    fn version_key_orders_by_semver() {
        let a = VersionKey::new("1.0.0", None);
        let b = VersionKey::new("2.0.0", None);
        assert_eq!(a.cmp(&b), Ordering::Less);
    }

    #[test]
    fn version_key_orders_by_integer_when_not_semver() {
        let a = VersionKey::new("5", None);
        let b = VersionKey::new("12", None);
        assert_eq!(a.cmp(&b), Ordering::Less);
    }

    #[test]
    fn version_key_falls_back_to_string() {
        let a = VersionKey::new("draft-a", None);
        let b = VersionKey::new("draft-b", None);
        assert_eq!(a.cmp(&b), Ordering::Less);
    }

    #[test]
    fn version_key_uses_explicit_algorithm() {
        let a = VersionKey::new("10", Some(VersionAlgorithm::Integer));
        let b = VersionKey::new("2", Some(VersionAlgorithm::Integer));
        assert_eq!(a.cmp(&b), Ordering::Greater);
    }

    // --- select_from_version_index ---

    #[test]
    fn select_exact_version() {
        let mut versions = BTreeMap::new();
        let v1 = Arc::new(json!({"version": "1.0.0"}));
        let v2 = Arc::new(json!({"version": "2.0.0"}));
        versions.insert(VersionKey::new("1.0.0", None), v1);
        versions.insert(VersionKey::new("2.0.0", None), v2);

        let result = select_from_version_index(&versions, Some("1.0.0")).unwrap();
        assert_eq!(result.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
    }

    #[test]
    fn select_latest_prefers_stable() {
        let mut versions = BTreeMap::new();
        let stable = Arc::new(json!({"version": "1.0.0"}));
        let pre = Arc::new(json!({"version": "2.0.0-beta"}));
        versions.insert(VersionKey::new("1.0.0", None), stable);
        versions.insert(VersionKey::new("2.0.0-beta", None), pre);

        let result = select_from_version_index(&versions, None).unwrap();
        assert_eq!(result.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
    }

    #[test]
    fn select_latest_falls_back_to_prerelease() {
        let mut versions = BTreeMap::new();
        let pre1 = Arc::new(json!({"version": "1.0.0-alpha"}));
        let pre2 = Arc::new(json!({"version": "1.0.0-beta"}));
        versions.insert(VersionKey::new("1.0.0-alpha", None), pre1);
        versions.insert(VersionKey::new("1.0.0-beta", None), pre2);

        let result = select_from_version_index(&versions, None).unwrap();
        // BTreeMap highest = last = beta (comes after alpha in semver)
        assert_eq!(result.get("version").and_then(|v| v.as_str()), Some("1.0.0-beta"));
    }

    #[test]
    fn select_returns_none_for_empty() {
        let versions: BTreeMap<VersionKey, Arc<Value>> = BTreeMap::new();
        assert!(select_from_version_index(&versions, None).is_none());
        assert!(select_from_version_index(&versions, Some("1.0.0")).is_none());
    }
}
