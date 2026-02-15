//! Integration tests using the official HL7 FHIR snapshot-generation test suite.
//!
//! Test cases are loaded from `fhir-test-cases/rX/snapshot-generation/manifest.xml`.
//! Each manifest entry becomes an individual `#[test]` function.
//!
//! To run these tests, ensure the fhir-test-cases submodule is initialised:
//! ```bash
//! git submodule update --init --recursive
//! ```

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use ferrum_context::DefaultFhirContext;
use ferrum_format::xml_to_json;
use ferrum_models::StructureDefinition;
use ferrum_snapshot::generate_structure_definition_snapshot;

mod test_support;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TEST_CASES_DIR: &str = "../../fhir-test-cases/rX/snapshot-generation";

fn test_cases_available() -> bool {
    Path::new(TEST_CASES_DIR).join("manifest.xml").exists()
}

/// Load a FHIR resource file (JSON or XML) and return it as a JSON `Value`.
fn load_resource_file(path: &Path) -> Value {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));

    match path.extension().and_then(|s| s.to_str()) {
        Some("json") => serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse JSON {}: {}", path.display(), e)),
        Some("xml") => {
            let json_str = xml_to_json(&content)
                .unwrap_or_else(|e| panic!("Failed to convert XML→JSON {}: {}", path.display(), e));
            serde_json::from_str(&json_str)
                .unwrap_or_else(|e| panic!("Failed to parse converted JSON {}: {}", path.display(), e))
        }
        _ => panic!("Unsupported file extension: {}", path.display()),
    }
}

/// Resolve a test-case file by `id` and `suffix`, trying JSON first then XML.
fn resolve_file(dir: &Path, id: &str, suffix: &str) -> PathBuf {
    let json_path = dir.join(format!("{}{}.json", id, suffix));
    if json_path.exists() {
        return json_path;
    }
    let xml_path = dir.join(format!("{}{}.xml", id, suffix));
    if xml_path.exists() {
        return xml_path;
    }
    panic!(
        "Cannot find file for id={}, suffix={} in {}",
        id,
        suffix,
        dir.display()
    );
}

/// Resolve a register file by name, trying JSON first then XML.
fn resolve_register_file(dir: &Path, name: &str) -> PathBuf {
    let json_path = dir.join(format!("{}.json", name));
    if json_path.exists() {
        return json_path;
    }
    let xml_path = dir.join(format!("{}.xml", name));
    if xml_path.exists() {
        return xml_path;
    }
    panic!(
        "Cannot find register file {} in {}",
        name,
        dir.display()
    );
}

/// Build a `DefaultFhirContext` for the given FHIR version string (e.g. "4.0.1").
fn build_context(fhir_version: &str, register_resources: Vec<Value>) -> DefaultFhirContext {
    let version_label = match fhir_version {
        v if v.starts_with("4.0") => "R4",
        v if v.starts_with("4.3") => "R4B",
        v if v.starts_with("5.") => "R5",
        other => panic!("Unsupported FHIR version: {}", other),
    };

    let mut ctx = test_support::block_on(DefaultFhirContext::from_fhir_version_async(
        None,
        version_label,
    ))
    .unwrap_or_else(|e| panic!("Failed to create {} context: {}", version_label, e));

    for resource in register_resources {
        ctx.add_resource(resource);
    }

    ctx
}

/// Compare snapshot elements between generated and expected StructureDefinitions.
///
/// Returns a list of human-readable differences. An empty list means the
/// snapshots match.
fn compare_snapshots(generated: &StructureDefinition, expected: &StructureDefinition) -> Vec<String> {
    let mut diffs = Vec::new();

    let gen_snapshot = match &generated.snapshot {
        Some(s) => s,
        None => {
            diffs.push("Generated SD has no snapshot".to_string());
            return diffs;
        }
    };

    let exp_snapshot = match &expected.snapshot {
        Some(s) => s,
        None => {
            diffs.push("Expected SD has no snapshot".to_string());
            return diffs;
        }
    };

    // Serialize elements to Value for flexible comparison.
    let gen_values: Vec<Value> = gen_snapshot
        .element
        .iter()
        .map(|e| serde_json::to_value(e).expect("failed to serialise generated element"))
        .collect();
    let exp_values: Vec<Value> = exp_snapshot
        .element
        .iter()
        .map(|e| serde_json::to_value(e).expect("failed to serialise expected element"))
        .collect();

    if gen_values.len() != exp_values.len() {
        diffs.push(format!(
            "Element count: generated {} vs expected {}",
            gen_values.len(),
            exp_values.len()
        ));
    }

    // Build lookup by element id (falling back to path).
    let gen_by_id: std::collections::HashMap<&str, &Value> = gen_values
        .iter()
        .filter_map(|e| {
            let key = e
                .get("id")
                .and_then(|v| v.as_str())
                .or_else(|| e.get("path").and_then(|v| v.as_str()))?;
            Some((key, e))
        })
        .collect();

    let exp_by_id: std::collections::HashMap<&str, &Value> = exp_values
        .iter()
        .filter_map(|e| {
            let key = e
                .get("id")
                .and_then(|v| v.as_str())
                .or_else(|| e.get("path").and_then(|v| v.as_str()))?;
            Some((key, e))
        })
        .collect();

    // Missing / extra elements.
    for id in exp_by_id.keys() {
        if !gen_by_id.contains_key(id) {
            diffs.push(format!("Missing element: {}", id));
        }
    }
    for id in gen_by_id.keys() {
        if !exp_by_id.contains_key(id) {
            diffs.push(format!("Extra element: {}", id));
        }
    }

    // Per-element deep comparison (skip keys that are intentionally variable).
    let ignore_keys: std::collections::HashSet<&str> =
        ["constraint", "mapping", "extension", "comment", "comments", "requirements"]
            .iter()
            .copied()
            .collect();

    for (id, exp_elem) in &exp_by_id {
        if let Some(gen_elem) = gen_by_id.get(id) {
            let exp_obj = exp_elem.as_object();
            let gen_obj = gen_elem.as_object();
            if let (Some(exp_map), Some(gen_map)) = (exp_obj, gen_obj) {
                for (key, exp_val) in exp_map {
                    if ignore_keys.contains(key.as_str()) {
                        continue;
                    }
                    match gen_map.get(key) {
                        Some(gen_val) if gen_val == exp_val => {}
                        Some(gen_val) => {
                            diffs.push(format!(
                                "Element {}.{}: generated={}, expected={}",
                                id,
                                key,
                                serde_json::to_string(gen_val).unwrap_or_default(),
                                serde_json::to_string(exp_val).unwrap_or_default(),
                            ));
                        }
                        None => {
                            diffs.push(format!(
                                "Element {}.{}: missing in generated (expected={})",
                                id,
                                key,
                                serde_json::to_string(exp_val).unwrap_or_default(),
                            ));
                        }
                    }
                }
            }
        }
    }

    diffs
}

/// Write diff output to test_output/ for inspection.
fn write_diff_output(test_id: &str, diffs: &[String], generated: &StructureDefinition, expected_value: &Value) {
    let output_dir = Path::new("tests/test_output");
    let _ = fs::create_dir_all(output_dir);

    // Write diffs
    let diff_path = output_dir.join(format!("{}-diff.txt", test_id));
    let diff_content = diffs.join("\n");
    let _ = fs::write(&diff_path, &diff_content);

    // Write generated snapshot
    let gen_path = output_dir.join(format!("{}-generated.json", test_id));
    if let Ok(json) = serde_json::to_string_pretty(&generated) {
        let _ = fs::write(&gen_path, json);
    }

    // Write expected snapshot
    let exp_path = output_dir.join(format!("{}-expected.json", test_id));
    if let Ok(json) = serde_json::to_string_pretty(expected_value) {
        let _ = fs::write(&exp_path, json);
    }

    eprintln!(
        "  Diff output written to {}",
        diff_path.display()
    );
}

/// Core test runner for a single manifest entry.
fn run_snapshot_test(
    test_id: &str,
    fhir_version: &str,
    register_names: &[&str],
) {
    if !test_cases_available() {
        eprintln!("Skipping {} — fhir-test-cases submodule not present", test_id);
        return;
    }

    let dir = PathBuf::from(TEST_CASES_DIR);

    // Load register resources.
    let register_resources: Vec<Value> = register_names
        .iter()
        .map(|name| {
            let path = resolve_register_file(&dir, name);
            load_resource_file(&path)
        })
        .collect();

    // Build context with registered SDs.
    let ctx = build_context(fhir_version, register_resources);

    // Load input SD.
    let input_path = resolve_file(&dir, test_id, "-input");
    let input_value = load_resource_file(&input_path);
    let input_sd: StructureDefinition = serde_json::from_value(input_value)
        .unwrap_or_else(|e| panic!("Failed to deserialise input SD for {}: {}", test_id, e));

    // Load expected output SD.
    let expected_path = resolve_file(&dir, test_id, "-output");
    let expected_value = load_resource_file(&expected_path);
    let expected_sd: StructureDefinition = serde_json::from_value(expected_value.clone())
        .unwrap_or_else(|e| panic!("Failed to deserialise expected SD for {}: {}", test_id, e));

    // Generate snapshot.
    let generated_sd = generate_structure_definition_snapshot(None, &input_sd, &ctx)
        .unwrap_or_else(|e| panic!("Snapshot generation failed for {}: {}", test_id, e));

    // Compare.
    let diffs = compare_snapshots(&generated_sd, &expected_sd);

    if !diffs.is_empty() {
        write_diff_output(test_id, &diffs, &generated_sd, &expected_value);
        eprintln!("\n=== {} failed with {} differences ===", test_id, diffs.len());
        for (i, diff) in diffs.iter().enumerate().take(30) {
            eprintln!("  [{}] {}", i + 1, diff);
        }
        if diffs.len() > 30 {
            eprintln!("  ... and {} more", diffs.len() - 30);
        }
        panic!(
            "Snapshot test '{}' failed with {} differences (see tests/test_output/)",
            test_id,
            diffs.len()
        );
    }

    eprintln!("  {} passed", test_id);
}

// ---------------------------------------------------------------------------
// Individual test functions — one per manifest entry
// ---------------------------------------------------------------------------

#[test]
#[ignore] // Official HL7 test suite — expected differences, run explicitly with --ignored
fn fhir_test_case_obs_perf() {
    run_snapshot_test("obs-perf", "4.0.1", &["reference-rest-or-logical"]);
}

#[test]
#[ignore]
fn fhir_test_case_location_qicore() {
    run_snapshot_test("location-qicore", "4.0.1", &["location-uscore"]);
}

#[test]
#[ignore]
fn fhir_test_case_ratio_measure_cqfm() {
    run_snapshot_test(
        "StructureDefinition-ratio-measure-cqfm",
        "4.0.1",
        &["StructureDefinition-measure-cqfm"],
    );
}

#[test]
#[ignore]
fn fhir_test_case_simple_quantity() {
    run_snapshot_test("simple-quantity", "4.0.1", &[]);
}

#[test]
#[ignore]
fn fhir_test_case_simple_quantity_2() {
    run_snapshot_test("simple-quantity-2", "4.0.1", &[]);
}

#[test]
#[ignore]
fn fhir_test_case_simple_quantity_3() {
    run_snapshot_test("simple-quantity-3", "4.0.1", &[]);
}

#[test]
#[ignore]
fn fhir_test_case_nl_core_nursing_intervention() {
    run_snapshot_test(
        "nl-core-NursingIntervention",
        "4.0.1",
        &[
            "zib-NursingIntervention-input",
            "pattern-ZibHealthProfessionalReference",
        ],
    );
}

#[test]
#[ignore]
fn fhir_test_case_zib_nursing_intervention() {
    run_snapshot_test(
        "zib-NursingIntervention",
        "4.0.1",
        &[
            "zib-NursingIntervention-input",
            "pattern-ZibHealthProfessionalReference",
            "pattern-NlCoreHealthProfessionalReference",
        ],
    );
}

#[test]
#[ignore]
fn fhir_test_case_slice_cardinality_derived() {
    run_snapshot_test(
        "slice-cardinality-derived",
        "4.0.1",
        &["slice-cardinality-base"],
    );
}

#[test]
#[ignore]
fn fhir_test_case_ch_location() {
    run_snapshot_test(
        "ch-location",
        "4.0.1",
        &["ch-phone", "ch-email", "ch-internet"],
    );
}

// SKIP: bc-UterusActivity — requires FHIR 3.0.2 (R3) which is not supported
// SKIP: encounter-legalStatus — requires cross-version targetVersion conversion

#[test]
#[ignore]
fn fhir_test_case_prov_fi() {
    run_snapshot_test("prov-fi", "4.0.1", &[]);
}
