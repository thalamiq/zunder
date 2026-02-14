//! Official FHIR Validator Test Suite Harness
//!
//! Runs our validator against the HL7 fhir-test-cases suite and compares
//! error counts with the Java reference implementation expectations.
//!
//! This test is `#[ignore]` — run explicitly:
//! ```bash
//! cargo test -p zunder-validator --test official_suite -- --ignored --nocapture
//! ```

mod test_support;

use std::fmt::Write;
use std::panic;
use test_support::{
    block_on, fhir_version_label, is_eligible, load_manifest, load_test_resource,
    resolve_expected_errors,
};
use zunder_context::DefaultFhirContext;
use zunder_validator::{Preset, ValidationOutcome, Validator, ValidatorConfig};

#[test]
#[ignore]
fn official_test_suite_baseline() {
    // Run in a thread with a large stack to handle deeply nested resources.
    let builder = std::thread::Builder::new()
        .name("suite-runner".to_string())
        .stack_size(64 * 1024 * 1024); // 64 MB
    let handler = builder
        .spawn(official_test_suite_inner)
        .expect("failed to spawn suite runner thread");
    handler.join().expect("suite runner thread panicked");
}

fn official_test_suite_inner() {
    let manifest = match load_manifest() {
        Some(m) => m,
        None => {
            eprintln!("fhir-test-cases/validator/manifest.json not found - skipping");
            return;
        }
    };

    let total = manifest.test_cases.len();
    let eligible: Vec<_> = manifest.test_cases.iter().filter(|tc| is_eligible(tc)).collect();
    let skipped = total - eligible.len();

    let config = ValidatorConfig::preset(Preset::Authoring);

    eprintln!("Loading R4 context...");
    let ctx_r4 = block_on(DefaultFhirContext::from_fhir_version_async(None, "R4"))
        .expect("Failed to create R4 context");
    let validator_r4 = Validator::from_config(&config, ctx_r4)
        .expect("Failed to create R4 validator");

    eprintln!("Loading R5 context...");
    let ctx_r5 = block_on(DefaultFhirContext::from_fhir_version_async(None, "R5"))
        .expect("Failed to create R5 context");
    let validator_r5 = Validator::from_config(&config, ctx_r5)
        .expect("Failed to create R5 validator");

    eprintln!("Running {} eligible tests...", eligible.len());
    eprintln!("First 5 eligible: {:?}", eligible.iter().take(5).map(|t| &t.name).collect::<Vec<_>>());

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut panicked = 0u32;
    let mut load_errors = 0u32;
    let mut failures: Vec<(String, Option<String>, u32, u32)> = Vec::new();

    for tc in &eligible {
        let java = tc.java.as_ref().unwrap();
        let expected_errors = resolve_expected_errors(java);
        if expected_errors == u32::MAX {
            load_errors += 1;
            continue;
        }

        let resource = match load_test_resource(&tc.file) {
            Some(r) => r,
            None => {
                load_errors += 1;
                continue;
            }
        };

        let version_label = fhir_version_label(tc.version.as_deref());
        eprintln!("  validating: {} ({})", tc.name, version_label);

        // Catch panics (e.g. stack overflow on deeply recursive structures)
        let result: Result<ValidationOutcome, _> = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            match version_label {
                "R4" => validator_r4.validate(&resource),
                _ => validator_r5.validate(&resource),
            }
        }));

        let actual_errors = match result {
            Ok(outcome) => outcome.error_count() as u32,
            Err(_) => {
                eprintln!("  PANIC: {}", tc.name);
                panicked += 1;
                continue;
            }
        };

        if actual_errors == expected_errors {
            passed += 1;
        } else {
            failed += 1;
            failures.push((
                tc.name.clone(),
                tc.version.clone(),
                expected_errors,
                actual_errors,
            ));
        }
    }

    // Print report
    let sep = "=".repeat(60);
    let mut report = String::new();
    writeln!(report, "\n{sep}").unwrap();
    writeln!(report, "  FHIR Validator - Official Test Suite Baseline").unwrap();
    writeln!(report, "{sep}").unwrap();
    writeln!(report, "  Total in manifest : {total}").unwrap();
    writeln!(report, "  Skipped (filtered): {skipped}").unwrap();
    writeln!(report, "  Load errors       : {load_errors}").unwrap();
    writeln!(report, "  Panicked          : {panicked}").unwrap();
    writeln!(report, "  Ran               : {}", passed + failed).unwrap();
    writeln!(report, "  PASS              : {passed}").unwrap();
    writeln!(report, "  FAIL              : {failed}").unwrap();
    let ran = passed + failed;
    let pct = if ran > 0 {
        (passed as f64 / ran as f64) * 100.0
    } else {
        0.0
    };
    writeln!(report, "  Pass rate         : {pct:.1}%").unwrap();
    writeln!(report, "{sep}").unwrap();

    if !failures.is_empty() {
        writeln!(report, "\n  Failures (first 50):").unwrap();
        writeln!(
            report,
            "  {:<50} {:>6} {:>8} {:>8}",
            "Test Name", "Ver", "Expected", "Actual"
        )
        .unwrap();
        writeln!(report, "  {:-<78}", "").unwrap();
        for (name, version, expected, actual) in failures.iter().take(50) {
            let ver = version.as_deref().unwrap_or("R5");
            writeln!(report, "  {:<50} {:>6} {:>8} {:>8}", name, ver, expected, actual).unwrap();
        }
        if failures.len() > 50 {
            writeln!(report, "  ... and {} more", failures.len() - 50).unwrap();
        }
    }

    eprintln!("{report}");
}
