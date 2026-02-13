//! Shared referential integrity utilities.
//!
//! Used by CrudService and TransactionService to validate references.

use serde_json::Value as JsonValue;
use std::collections::HashSet;

/// Walk the entire JSON tree and collect relative references of the form `Type/id`.
///
/// Skips fragments (`#...`), absolute URLs (`http://...`), canonical URLs, and URN references.
pub(crate) fn collect_relative_refs(
    value: &JsonValue,
    out: &mut HashSet<(String, String)>,
) {
    match value {
        JsonValue::Array(items) => {
            for item in items {
                collect_relative_refs(item, out);
            }
        }
        JsonValue::Object(obj) => {
            if let Some(ref_str) = obj.get("reference").and_then(|v| v.as_str()) {
                let trimmed = ref_str.trim();
                if !trimmed.is_empty()
                    && !trimmed.starts_with('#')
                    && !trimmed.contains("://")
                    && !trimmed.starts_with("urn:")
                {
                    let parts: Vec<&str> = trimmed.splitn(3, '/').collect();
                    if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                        out.insert((parts[0].to_string(), parts[1].to_string()));
                    }
                }
            }
            for child in obj.values() {
                collect_relative_refs(child, out);
            }
        }
        _ => {}
    }
}
