# zunder-snapshot

FHIR StructureDefinition snapshot generation, expansion, and differential computation.

## Architecture

The crate is split into two phases that mirror the FHIR profiling pipeline:

1. **Snapshot generation** (`generate_structure_definition_snapshot`) — merges a profile's differential onto its base snapshot, handling element inheritance, slicing, cardinality constraints, and normalization. This produces a *shallow* snapshot: every element from the differential is correctly merged into the base, but complex-type children are **not** recursively inlined.

2. **Deep expansion** (`generate_deep_snapshot`, `SnapshotExpander`) — takes a shallow snapshot and recursively inlines children of complex types, resolves contentReferences, and expands choice-type elements. This produces the fully-expanded snapshot that the FHIR reference implementation generates in a single pass.

Most use cases (validation, search parameter extraction, UI rendering) need the shallow snapshot. Deep expansion is useful for schema generation and complete element enumeration.

## Features

- **Snapshot generation** from base + differential
- **Recursive profile-on-profile** resolution (differential-only bases are generated on the fly)
- **Deep snapshot expansion** resolving complex types, choice types, and contentReferences
- **Differential generation** from base + snapshot (`generate_structure_definition_differential`)
- **Snapshot/differential validation** (`validate_snapshot`, `validate_differential`)
- **ContentReference expansion** — converts fragment references to fully qualified canonicals
- **Differential sorting** — sorts differential elements into canonical order before merging
- **Type-based element resolution** — looks up parent element types to find child definitions
- Element inheritance, merging, slicing, and normalization

## Usage

```rust
use zunder_snapshot::{generate_structure_definition_snapshot, generate_deep_snapshot, SnapshotExpander};
use zunder_context::{DefaultFhirContext, FhirContext};

// Build a FHIR context (requires a loaded package)
// let ctx = DefaultFhirContext::from_fhir_version_async(None, "R4").await?;

// Generate snapshot from a differential profile
// let result = generate_structure_definition_snapshot(None, &profile_sd, &ctx)?;

// Deep-expand a snapshot (inline complex types, resolve contentReferences)
// let deep = generate_deep_snapshot(&snapshot, &ctx)?;
```

## Testing

### Unit / integration tests

```bash
cargo test -p zunder-snapshot
```

### Official FHIR test suite

The crate includes a test harness that runs the official HL7 FHIR snapshot-generation test cases from `fhir-test-cases/rX/snapshot-generation/`. These tests require the `fhir-test-cases` git submodule:

```bash
# Initialise the submodule (one-time)
git submodule update --init --recursive

# Run the official test suite
cargo test -p zunder-snapshot --test test_fhir_test_cases -- --nocapture

# Run a single test case
cargo test -p zunder-snapshot --test test_fhir_test_cases fhir_test_case_simple_quantity -- --nocapture
```

When a test fails, detailed diffs are written to `libs/fhir-snapshot/tests/test_output/`:
- `<test-id>-diff.txt` -- summary of differences
- `<test-id>-generated.json` -- the snapshot we produced
- `<test-id>-expected.json` -- the reference snapshot from the test suite

If the submodule is not present, the tests skip gracefully rather than failing.

#### Test cases (from manifest.xml)

| Test | FHIR Version | Register SDs | Status |
|------|-------------|--------------|--------|
| obs-perf | 4.0.1 | reference-rest-or-logical | Active |
| location-qicore | 4.0.1 | location-uscore | Active |
| StructureDefinition-ratio-measure-cqfm | 4.0.1 | StructureDefinition-measure-cqfm | Active |
| simple-quantity | 4.0.1 | -- | Active |
| simple-quantity-2 | 4.0.1 | -- | Active |
| simple-quantity-3 | 4.0.1 | -- | Active |
| nl-core-NursingIntervention | 4.0.1 | zib-NursingIntervention-input, pattern-Zib... | Active |
| zib-NursingIntervention | 4.0.1 | zib-NursingIntervention-input, pattern-Zib..., pattern-NlCore... | Active |
| slice-cardinality-derived | 4.0.1 | slice-cardinality-base | Active |
| ch-location | 4.0.1 | ch-phone, ch-email, ch-internet | Active |
| prov-fi | 4.0.1 | -- | Active |
| bc-UterusActivity | 3.0.2 | bc-MaternalObservation, nl-core-observation | Skipped (R3) |
| encounter-legalStatus | 5.0.0 -> 4.0.1 | -- | Skipped (cross-version) |

#### Current test status (as of 2026-02-14)

All 11 active test cases generate snapshots successfully. Remaining differences against the reference output fall into three categories:

**1. Version-tagged canonical URLs** (affects all tests)

The reference implementation appends `|<fhir-version>` to canonical URLs in `binding.valueSet` and `type[].profile` during snapshot generation. Our generator preserves URLs as-is. Both forms are valid FHIR — we chose not to implement version tagging since it adds complexity without functional value (terminology servers resolve both forms identically).

**2. Inline type expansion under slices** (major — affects complex profiles)

The reference implementation inlines all children of complex types under slice entries during basic snapshot generation. For example, a slice `Location.telecom:phone` gets all ContactPoint children expanded underneath it. Our generator handles this as a **separate step** via `SnapshotExpander` / `generate_deep_snapshot` — see "Architecture" above.

**3. Choice-type slice naming** (affects obs-perf)

The reference implementation uses slice notation for choice-type expansions (`Observation.effective[x]:effectivePeriod`) while our generator uses the direct expansion form (`Observation.effectivePeriod`). Both are valid FHIR representations.

| Test | Diff count | Primary cause |
|------|-----------|---------------|
| simple-quantity | 7 | Version-tagged URLs |
| simple-quantity-2 | 6 | Version-tagged URLs |
| simple-quantity-3 | 6 | Version-tagged URLs |
| slice-cardinality-derived | 15 | Version-tagged URLs, contentReference |
| location-qicore | 25 | Type expansion, version URLs |
| obs-perf | 51 | Choice-type naming, type expansion |
| ch-location | 58 | Slice child type expansion |
| ratio-measure-cqfm | 94 | Type expansion, version URLs |
| nl-core-NursingIntervention | 106 | Type expansion |
| zib-NursingIntervention | 106 | Type expansion |
| prov-fi | 278 | Slice child type expansion |

## Fixes applied

### Phase 1 (2026-02-14)

1. **`#[serde(default)]` on `StructureDefinition.name`** (`fhir-models`) — allows deserializing SDs that omit the `name` field.
2. **Recursive snapshot generation** — when the base SD is itself a differential-only profile, its snapshot is generated on the fly before merging.
3. **Relaxed differential validation** — `validate_hierarchy` is now lenient about slice-grouped differentials where children appear before parents in different slice groups. Closed slicing rules are treated as warnings during generation (they restrict further derivation, not the defining profile itself). Cardinality conflicts (child min > 0 with parent max = 0) are warnings for unreachable inherited children.
4. **Choice-type parent path validation** — `validate_paths_against_base` and `validate_snapshot_hierarchy` now recognize intermediate paths through choice-type elements (e.g., `scheduled[x].repeat` is valid if `scheduled[x]` exists in the base).
5. **Canonical element ordering** — `find_insertion_position` places new elements after their closest ancestor and all its descendants, correctly handling choice-type expansions.

### Phase 2 (2026-02-14)

6. **ContentReference expansion** — fragment references like `#Observation.referenceRange` are expanded to fully qualified canonicals (`http://hl7.org/fhir/StructureDefinition/Observation#Observation.referenceRange`).
7. **Differential sorting** — differential elements are sorted into canonical FHIR order (by base snapshot position) before merging, matching the reference implementation's `sortDifferential()` pre-processing step.
8. **Type-based child element resolution** — `find_base_element` now resolves child elements through the parent element's type. For example, `Location.address.postalCode` resolves via the parent `Location.address` (type Address) to find `Address.postalCode` in the Address StructureDefinition.
9. **Slicing `ordered` default** — slicing entries that omit `ordered` now default to `ordered: false` for consistent serialization.
10. **Merge path preservation** — `merge_element` always uses the differential's path, ensuring elements resolved from type SDs (e.g., `Address.city`) get the correct context path (e.g., `Location.address.city`).
