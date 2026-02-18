# Ferrum Validator — Implementation Plan

## Current State (updated 2026-02-18)

**168/391 tests passing (43.0%)**, 522 ignored, ~7.5 seconds

### What works today:
- Schema validation (resourceType, cardinality, data types, unknown elements, choice types)
  - Choice type variant detection at all nesting levels via `is_choice_variant_name()`
  - Extension objects auto-detected and children skipped (no false "Unknown element" on `url`/`value[x]`)
  - Nested/contained resources auto-detected — skips unknown element checks (defers to re-validation)
  - `fhir_comments` treated as special element (ignored)
- FHIRPath constraint/invariant evaluation (base + profile constraints)
  - Array-element constraint iteration (ele-1 evaluated per-item via `from_json_at()`)
  - `as()` on non-singleton collections returns empty (not error) per FHIRPath spec
  - Type navigation case-sensitivity fix (prevents `extension` ≠ `Extension` confusion)
- Profile validation (stricter cardinality, fixed/pattern values, type restrictions, mustSupport)
- Slicing (value + exists discriminators, slice cardinality, open/closed rules)
- Deep snapshot expansion via `ExpandedFhirContext` with `MaterializedView` pattern
- Terminology validation (in-memory, local mode):
  - `TerminologyProvider` trait with `InMemoryTerminologyProvider` impl
  - ValueSet expansion (pre-expanded, compose-based, include/exclude)
  - Binding strength → severity mapping (required/extensible/preferred/example)
  - CodeSystem content modes (complete/fragment/not-present/example)
  - Bare `code` type elements (no system, match by code only)
  - Expansion caching via `RwLock<HashMap>`
- Test harness:
  - `OverlayFhirContext` for layering test fixture resources over base context
  - Profile-specific test expectations (`profile.source`, `profile.java`)
  - Supporting resource loading from test fixtures

### What's stubbed but not implemented:
- Reference validation (`validate_references` → TODO)
- Bundle validation (`validate_bundles` → TODO)
- Contained/nested resource re-validation (detected but not recursively validated)
- Full extension validation against Extension SDs (children are skipped, not validated)

### Known Validator Gaps (by test failure category)

**1. Contained/nested resource re-validation (~20 failures)** — When the schema walker encounters a nested resource (Bundle.entry.resource, Resource.contained), it now correctly skips unknown element checking for that subtree. However, it does not yet re-enter validation against the nested resource's own StructureDefinition. HAPI handles this via `validateContains()` which re-enters validation with the nested resource's own SD. Fix: implement recursive re-validation for objects with `resourceType`.

**2. Full extension validation (~15 failures)** — Extension objects are now auto-detected (by `url` key + path ending in `.extension`/`.modifierExtension`) and their children are no longer flagged as unknown. However, children are not validated against the Extension's StructureDefinition. HAPI resolves the Extension SD by URL and validates `value[x]` against the declared types. Fix: resolve Extension SD inline during schema walk.

**3. QuestionnaireResponse validation (63 failures)** — Not implemented. QuestionnaireResponse validation is a specialized validator that cross-references the linked Questionnaire resource to validate answer types, required items, enableWhen conditions, etc. This is a standalone feature, not a general validation gap.

**4. Reference validation (~12 failures)** — Reference elements are not validated for type correctness or target profiles. HAPI validates reference format, target types against `type.targetProfile`, and optionally resolves the referenced resource.

**5. Bundle validation (~16 failures)** — Bundle-level constraints (entry structure, fullUrl uniqueness, type-specific rules like "document must start with Composition") are not implemented. Bundle entries are not individually re-validated.

**6. Missing schema checks (~10 failures)** — Several fine-grained schema validations not yet implemented:
   - Resource ID format validation (regex `[A-Za-z0-9\-\.]{1,64}`)
   - Empty array detection (`[]` is invalid in FHIR JSON)
   - Attachment size/hash validation
   - XHTML narrative validation (only basic structure, no content validation)

**7. Profile resolution from filenames (~15 failures)** — Some test cases reference profiles by filename (e.g., `'inactive-sd-inactive.json'` in `meta.profile`). HAPI resolves these via its test infrastructure. We treat them as canonical URLs and fail to resolve.

**8. `fhir_comments` handling** — Now treated as a special element (ignored). Note: HAPI only accepts `fhir_comments` in R2/R2B and errors on R4+. Our current behavior is more lenient.

---

## Phase 1: Test Harness Completeness

**Goal**: Unlock ~20-30 more passing tests by properly loading test fixtures.

### 1.1 Supporting Resource Loading

Many test cases declare `"supporting": ["sd-myprofile.json", "vs-codes.json"]` — additional conformance resources that must be available in the context during validation.

**Approach**:
- Before validation, load each file from `fhir-test-cases/validator/`
- Parse as JSON, check `resourceType`
- Inject into the validator's `FhirContext` so `get_resource_by_url` can find them

**Implementation**: Extend `FhirContext` or create a `CompositeContext` that layers test fixtures over the base context:

```rust
/// Wraps a base FhirContext with additional resources loaded from test fixtures.
struct TestContext<C: FhirContext> {
    base: C,
    overrides: HashMap<String, Arc<Value>>,  // canonical_url → resource
}
```

This needs a way to extract `url` from arbitrary FHIR conformance resources (StructureDefinition, ValueSet, CodeSystem, etc.) — just read the `"url"` field from the JSON.

### 1.2 Profile-Specific Validation

Test cases with a `"profile"` field expect validation against a specific profile:

```json
{
  "name": "patient-us-core",
  "file": "patient-us-core.json",
  "profile": {
    "source": "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient",
    "java": { "errorCount": 2 }
  }
}
```

**Approach**:
- When `tc.profile` is present, use `profile.source` as an explicit profile URL
- Use `profile.java` expectations instead of the top-level `java`
- Load `profile.supporting` files into context

### 1.3 Expected Result: ~125-135 tests passing

---

## Phase 2: Terminology Validation

**Goal**: Validate coded elements against ValueSet bindings. This is the single largest source of test failures.

### 2.1 The Terminology Provider Trait

The validator lives in `libs/` and must not depend on PostgreSQL or the server. We need a trait that abstracts terminology operations:

```rust
/// Provides terminology validation capabilities to the validator.
///
/// Implementations range from in-memory (package-based) to remote
/// (HTTP terminology server). The validator calls this trait during
/// the terminology validation step.
pub trait TerminologyProvider: Send + Sync {
    /// Validate a code against a ValueSet.
    ///
    /// Returns `Ok(None)` if the ValueSet cannot be resolved (provider doesn't know it).
    /// Returns `Ok(Some(result))` with validation outcome if the ValueSet is known.
    fn validate_code(
        &self,
        system: &str,
        code: &str,
        display: Option<&str>,
        value_set_url: &str,
    ) -> Result<Option<CodeValidationResult>>;

    /// Check if a code exists in a CodeSystem (without ValueSet context).
    fn validate_code_in_system(
        &self,
        system: &str,
        code: &str,
    ) -> Result<Option<CodeValidationResult>>;
}
```

```rust
pub struct CodeValidationResult {
    pub valid: bool,
    pub display: Option<String>,
    pub message: Option<String>,
    /// For fragment CodeSystems: "warning" instead of "error"
    pub severity_override: Option<IssueSeverity>,
}
```

### 2.2 In-Memory Terminology Provider

An `InMemoryTerminologyProvider` that works with any `FhirContext`:

**Capabilities**:
1. **Resolve ValueSet** by canonical URL from `FhirContext::get_resource_by_url`
2. **Expand ValueSet** in memory:
   - If `ValueSet.expansion` exists → use pre-expanded codes directly
   - If `ValueSet.compose` exists → expand from include/exclude rules:
     - `include.concept` → explicit code list (most common case)
     - `include.system` without concepts → include all codes from CodeSystem
     - `include.valueSet` → recursive expansion of referenced ValueSets
     - `include.filter` → filter CodeSystem concepts by property (basic support)
     - `exclude` → remove matched codes from the included set
3. **Validate code** against expanded set:
   - Match by system + code
   - Respect `CodeSystem.caseSensitive` (default: case-sensitive)
   - Check display name (warning on mismatch, not error)
   - Handle `CodeSystem.content` modes:
     - `complete` → missing code = error
     - `fragment` → missing code = warning (incomplete CodeSystem)
     - `not-present` → skip validation (no concept data available)
     - `example` → skip validation

**What we explicitly do NOT support in Phase 2**:
- Remote terminology server calls (Phase 4)
- Complex filter operations beyond `is-a` and `=`
- SNOMED CT / LOINC specific logic (these need external terminology servers)
- Subsumption-based validation

**Data flow**:
```
ElementDefinition.binding
  ├── strength: required | extensible | preferred | example
  └── valueSet: "http://hl7.org/fhir/ValueSet/languages"
                          │
                          ▼
              TerminologyProvider.validate_code(system, code, display, valueSet)
                          │
                          ▼
              InMemoryTerminologyProvider
                ├── FhirContext.get_resource_by_url(valueSet) → ValueSet JSON
                ├── Parse ValueSet → expand compose or use expansion
                ├── Match system+code against expanded set
                └── Return CodeValidationResult
```

### 2.3 Terminology Validation Step

```rust
/// Walks the resource, finds coded elements, validates against bindings.
pub fn validate_terminology(
    resource: &Value,
    plan: &TerminologyPlan,
    context: &dyn FhirContext,
    terminology: &dyn TerminologyProvider,
    issues: &mut Vec<ValidationIssue>,
)
```

**Algorithm**:
1. Resolve the resource's StructureDefinition (base type)
2. Walk the snapshot elements looking for elements with `binding`
3. For each binding:
   - Skip if `strength == Example`
   - Skip if `strength == Preferred` and `extensible_handling == Ignore`
   - Extract the actual value from the resource at that path
   - For `code` type: validate the string directly against the ValueSet
   - For `Coding` type: validate `system` + `code` + optional `display`
   - For `CodeableConcept` type: validate each `coding` entry; at least one must be valid for `required` bindings
4. Map binding strength to issue severity:
   - `required` → Error
   - `extensible` → controlled by `plan.extensible_handling` (Ignore / Warn / Error)
   - `preferred` → Information

**Coded element types and how to extract values**:

| Element Type | JSON Shape | Extraction |
|-------------|-----------|------------|
| `code` | `"status": "active"` | String value, no system (use binding's implicit system) |
| `Coding` | `{"system": "...", "code": "..."}` | Object with system + code |
| `CodeableConcept` | `{"coding": [{"system": "...", "code": "..."}]}` | Array of Codings |
| `Quantity` | `{"system": "http://unitsofmeasure.org", "code": "mg"}` | system + code from Quantity |
| `string` with binding | `"en"` | Treat like `code` |

### 2.4 Wiring Into the Validator

The `Validator<C>` needs a `TerminologyProvider`:

```rust
pub struct Validator<C: FhirContext> {
    plan: ValidationPlan,
    context: Arc<C>,
    fhirpath_engine: Arc<FhirPathEngine>,
    terminology: Option<Arc<dyn TerminologyProvider>>,  // NEW
}
```

When `TerminologyMode::Local` → create `InMemoryTerminologyProvider` from the same `FhirContext`.
When `TerminologyMode::Off` → `terminology = None`, skip step entirely.
When `TerminologyMode::Remote` / `Hybrid` → user must provide a `TerminologyProvider` impl.

### 2.5 Caching

ValueSet expansion can be expensive (walking CodeSystem concept trees). Cache expanded ValueSets:

```rust
struct InMemoryTerminologyProvider<C: FhirContext> {
    context: Arc<C>,
    expansion_cache: RwLock<HashMap<String, Arc<ExpandedValueSet>>>,
}

struct ExpandedValueSet {
    /// Flattened set of (system, code) pairs for O(1) lookup
    codes: HashSet<(String, String)>,
    /// Original concepts with display names for display validation
    concepts: Vec<ExpandedConcept>,
}
```

### 2.6 Expected Result: ~165-195 tests passing (~42-50%)

---

## Phase 3: Reference Validation

**Goal**: Validate Reference elements for type correctness and target profiles.

### 3.1 Reference Modes

Already defined in config:

```rust
pub enum ReferenceMode {
    Off,        // Skip entirely
    TypeOnly,   // Check reference.type matches allowed target types
    Existence,  // TypeOnly + verify the resource exists (server mode)
    Full,       // Existence + validate referenced resource against targetProfile
}
```

### 3.2 Reference Validation Step

```rust
pub fn validate_references(
    resource: &Value,
    plan: &ReferencesPlan,
    context: &dyn FhirContext,
    issues: &mut Vec<ValidationIssue>,
)
```

**Algorithm**:
1. Walk snapshot elements looking for `type.code == "Reference"`
2. For each Reference element in the resource:
   - Parse the reference string (`"Patient/123"`, `"#contained-1"`, `"http://example.com/Patient/123"`)
   - **TypeOnly**: Check `reference.type` or inferred type from URL matches `type.targetProfile`
   - **Existence**: Resolve the reference (contained, bundled, or external)
   - **Full**: Validate the resolved resource against the targetProfile SD
3. Handle `allow_external`:
   - If `false` and reference is absolute URL → error
   - If `true` → skip existence check for external references

### 3.3 Reference Types

| Reference Format | Example | Handling |
|-----------------|---------|---------|
| Relative | `"Patient/123"` | Extract type from first segment |
| Contained | `"#bp-measurement"` | Look up in `resource.contained[]` |
| Absolute | `"http://example.com/fhir/Patient/123"` | Extract type from URL pattern |
| Logical | `{"identifier": {...}}` | Check `type` field if present |
| Bundle entry | `"urn:uuid:..."` | Resolve within Bundle.entry[].fullUrl |

### 3.4 Expected Result: ~175-205 tests passing

---

## Phase 4: Bundle Validation

**Goal**: Validate Bundle resources — entry structure, internal references, type-specific rules.

### 4.1 Bundle Validation Step

```rust
pub fn validate_bundles(
    resource: &Value,
    plan: &BundlePlan,
    context: &dyn FhirContext,
    terminology: Option<&dyn TerminologyProvider>,
    fhirpath_engine: &Arc<FhirPathEngine>,
    issues: &mut Vec<ValidationIssue>,
)
```

**Checks**:
1. **Entry validation**: Each `Bundle.entry.resource` validated independently
2. **fullUrl uniqueness**: No duplicate fullUrls within the Bundle
3. **Internal reference resolution**: References between entries must resolve
4. **Bundle type rules**:
   - `document` → first entry must be Composition
   - `message` → first entry must be MessageHeader
   - `transaction` / `batch` → entries must have `request` with method + url
   - `transaction-response` / `batch-response` → entries must have `response`
   - `searchset` → entries may have `search.mode`
5. **Request consistency** (transaction):
   - PUT requires `request.url` matching resource type
   - Conditional references must be valid search URLs

### 4.2 Expected Result: ~180-210 tests passing

---

## Phase 5: Slicing Completeness

**Goal**: Implement remaining discriminator types.

### 5.1 Type Discriminator

Match elements by their `resourceType` or polymorphic type:

```rust
fn matches_type_discriminator(element: &Value, expected_type: &str) -> bool {
    // For polymorphic types: check which value[x] variant is present
    // For contained resources: check resourceType
}
```

### 5.2 Profile Discriminator

Match elements by conformance to a specific profile:

```rust
fn matches_profile_discriminator(
    element: &Value,
    profile_url: &str,
    context: &dyn FhirContext,
) -> bool {
    // Validate element against profile, return true if no errors
}
```

### 5.3 Position Discriminator

Match elements by their index in the array. Rarely used but needed for completeness.

### 5.4 Expected Result: ~185-215 tests passing

---

## Phase 6: Remote Terminology

**Goal**: Support external terminology server for large code systems (SNOMED, LOINC).

### 6.1 HTTP Terminology Provider

```rust
pub struct RemoteTerminologyProvider {
    base_url: String,
    client: reqwest::Client,
    timeout: Duration,
    cache: RwLock<HashMap<String, Arc<ExpandedValueSet>>>,
}
```

Calls `$validate-code` on the remote server:
```
POST /ValueSet/$validate-code
{
  "resourceType": "Parameters",
  "parameter": [
    {"name": "url", "valueUri": "http://hl7.org/fhir/ValueSet/languages"},
    {"name": "system", "valueUri": "urn:ietf:bcp:47"},
    {"name": "code", "valueString": "en"}
  ]
}
```

### 6.2 Hybrid Terminology Provider

Tries in-memory first, falls back to remote:

```rust
pub struct HybridTerminologyProvider<C: FhirContext> {
    local: InMemoryTerminologyProvider<C>,
    remote: RemoteTerminologyProvider,
}
```

### 6.3 Timeout Handling

Controlled by `TerminologyConfig`:
- `on_timeout: Skip` → silently skip the check
- `on_timeout: Warn` → emit warning issue
- `on_timeout: Error` → emit error issue

---

## Configuration Reference

### Full YAML Example

```yaml
preset: Authoring

fhir:
  version: R4
  allow_version_mismatch: false

exec:
  fail_fast: false
  max_issues: 1000

report:
  include_warnings: true
  include_information: false

schema:
  mode: On
  allow_unknown_elements: false
  allow_modifier_extensions: false

profiles:
  mode: On
  explicit_profiles:
    - http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient

constraints:
  mode: Full                    # Off | InvariantsOnly | Full
  best_practice: Warn           # Ignore | Warn | Error
  suppress:
    - dom-6                     # Skip specific constraint IDs
  level_overrides:
    - id: ele-1
      level: Warning            # Downgrade from Error to Warning

terminology:
  mode: Local                   # Off | Local | Remote | Hybrid
  extensible_handling: Warn     # Ignore | Warn | Error
  timeout: 1500                 # ms, for Remote/Hybrid
  on_timeout: Warn              # Skip | Warn | Error
  cache: Memory                 # None | Memory

references:
  mode: TypeOnly                # Off | TypeOnly | Existence | Full
  allow_external: true

bundles:
  mode: On                      # Off | On
```

### Preset Defaults

| Setting | Ingestion | Authoring | Server | Publication |
|---------|-----------|-----------|--------|-------------|
| Schema | On | On | On | On |
| Profiles | Off | On | On | On |
| Constraints | Off | Full | Full | Full |
| Best Practice | Ignore | Ignore | Ignore | Warn |
| Terminology | Off | Local | Hybrid | Remote |
| References | Off | Off | Existence | Full |
| Bundles | Off | Off | Off | Off |

### Terminology Mode Behavior

| Mode | In-Memory | Remote Server | Use Case |
|------|-----------|---------------|----------|
| `Off` | - | - | Skip all terminology checks |
| `Local` | Yes | No | CLI, offline validation, package-only ValueSets |
| `Remote` | No | Yes | Full validation with SNOMED/LOINC |
| `Hybrid` | Try first | Fallback | Server mode — fast for common cases, complete for complex |

### Binding Strength → Issue Severity Matrix

| Binding Strength | `extensible_handling: Ignore` | `extensible_handling: Warn` | `extensible_handling: Error` |
|-----------------|-------------------------------|-----------------------------|-----------------------------|
| `required` | Error | Error | Error |
| `extensible` | (skip) | Warning | Error |
| `preferred` | (skip) | Information | Information |
| `example` | (skip) | (skip) | (skip) |

---

## Architecture Decisions

### 1. TerminologyProvider is NOT on FhirContext

The `FhirContext` trait provides raw resource access (`get_resource_by_url`). Terminology validation is business logic (expand ValueSet, match codes) that belongs in the validator, not the context layer. This keeps the dependency graph clean:

```
FhirContext (raw resource access)
    ↓
TerminologyProvider (uses FhirContext for ValueSet/CodeSystem lookup)
    ↓
Validator (uses both)
```

### 2. Synchronous API

The validator API is synchronous (`fn validate(&self, resource: &Value) -> ValidationOutcome`). The `TerminologyProvider` trait is also synchronous. For `RemoteTerminologyProvider`, we'll use `tokio::runtime::Handle::block_on` internally or require the caller to use `validate_async`.

Alternative: Add `async fn validate_async` alongside the sync version. The sync version would create a runtime internally for remote calls.

### 3. No Separate Expansion Step

HAPI has a separate `$expand` operation that the validator calls. We inline expansion into `validate_code` — the provider expands and validates in one call, caching the expansion for reuse. This avoids exposing expansion as a public API on the provider (simpler interface).

### 4. Validator Owns TerminologyProvider

The `Validator<C>` struct holds an `Option<Arc<dyn TerminologyProvider>>`. When `TerminologyMode::Local`, the `Validator::from_config` constructor creates an `InMemoryTerminologyProvider` from the same `FhirContext`. Users can also inject a custom provider.

### 5. Walk-Based Terminology Validation

Rather than the validator's schema step collecting bindings and passing them to a separate terminology step, the terminology step independently walks the snapshot and resource. This keeps steps decoupled — each step is self-contained and can be enabled/disabled independently.

---

## Implementation Order

### Completed

| # | Task | Files | Result |
|---|------|-------|--------|
| 1 | Test harness: supporting resources + profile tests | `tests/official_suite.rs`, `tests/test_support/mod.rs` | Done — `OverlayFhirContext`, profile-specific expectations |
| 2 | `TerminologyProvider` trait | `src/terminology/provider.rs` | Done |
| 3 | `InMemoryTerminologyProvider` | `src/terminology/in_memory.rs` | Done — expansion, caching, content modes |
| 4 | Terminology validation step | `src/steps/terminology.rs` | Done — binding walk, coded element extraction |
| 5 | Wire terminology into Validator | `src/validator.rs`, `src/lib.rs` | Done |
| 6 | FHIRPath fixes | `libs/fhirpath/src/value.rs`, `libs/fhirpath/src/vm/functions/boolean.rs` | Done — `from_json_at()` for array items, `as()` non-singleton returns empty |
| 7 | Constraint array iteration | `src/steps/constraints.rs` | Done — ele-1 evaluated per array item |
| 8 | Schema choice type detection | `src/steps/schema.rs` | Done — `is_choice_variant_name()` in unknown element check |
| 9 | Extension object detection | `src/steps/schema.rs` | Done — skip unknown element checks for extension objects (`url` key + extension path) |
| 10 | Nested resource detection | `src/steps/schema.rs` | Done — skip unknown element checks for objects with `resourceType` (not root) |
| 11 | FHIRPath type navigation fix | `libs/fhirpath/src/vm.rs` | Done — case-sensitive type navigation prevents `extension` → `Extension` confusion |

### Remaining (priority order)

| # | Task | Files | Est. Tests |
|---|------|-------|------------|
| 12 | Contained/nested resource re-validation | `src/steps/schema.rs` or new step | ~15-20 |
| 13 | Extension SD resolution + child validation | `src/steps/schema.rs` or `src/steps/extensions.rs` | ~10-15 |
| 14 | Reference validation step (TypeOnly) | `src/steps/references.rs` | ~5-10 |
| 15 | Bundle entry validation | `src/steps/bundles.rs` | ~5-10 |
| 16 | QuestionnaireResponse validation | `src/steps/questionnaire.rs` (new) | ~20-40 |
| 17 | Slicing: type + profile discriminators | `src/steps/slicing.rs` | ~5-10 |
| 18 | Schema: ID format, empty arrays, attachment checks | `src/steps/schema.rs` | ~5-10 |
| 19 | Remote/Hybrid terminology provider | `src/terminology/remote.rs` | ~10-20 |

**Trajectory so far**: 105 → 133 (ele-1) → 134 (as()) → 138 (choice types + fhir_comments) → 163 (extension + nested resource detection) → 168 (FHIRPath type navigation fix)

**Projected**: 168 → ~185 (re-validation) → ~200 (extension SD + references) → ~215 (bundle) → ~255 (questionnaire) → ~265 (slicing+schema)

### Architectural decisions for remaining work

**Contained/nested resource re-validation (#12)**: Detection is done — objects with `resourceType` (not at the root path) are identified and their children are excluded from the parent's unknown element checks. Next step: recursively invoke validation against the nested resource's own StructureDefinition. HAPI does this via `validateContains()` → `validateResource()`. Need to handle: Bundle.entry.resource, Resource.contained[], Parameters.parameter.resource. Key challenge: creating the right FHIRPath context for constraint evaluation on the nested resource.

**Extension SD resolution (#13)**: Extension objects are now auto-detected (by `url` key + path ending in `.extension`/`.modifierExtension`) and children are silently skipped. Next step: resolve the Extension's StructureDefinition by its `url` field from the FhirContext, then validate `value[x]` against the declared types. HAPI does this via `checkExtension()`. Note: deep snapshot expansion of Extension.value[x] was attempted but abandoned due to combinatorial blowup (~50 type variants). Inline SD resolution during schema walk (approach (a)) is the correct path forward.

**QuestionnaireResponse (#16)**: This is a completely separate validation mode that cross-references Questionnaire definitions. HAPI has dedicated `QuestionnaireValidator`. Deprioritized — it's a specialized feature, not a core validation gap. However, it accounts for ~63 of the 223 remaining failures.

---

## Open Questions

1. **Should `validate` become async?** Remote terminology needs async HTTP. Options:
   - Keep sync, use `block_on` internally (simpler API, potential deadlock risk)
   - Add `validate_async`, keep sync as convenience wrapper
   - Stay sync-only, require `Local` mode for sync callers

2. **How to handle missing CodeSystems**: When a ValueSet references a CodeSystem not in the context (e.g., SNOMED), should we:
   - Skip validation (current HAPI behavior for `not-present` content)
   - Emit a warning ("could not validate — CodeSystem not available")
   - Emit an error (strict mode)
   - Current behavior: warning by default.

3. **Extension validation depth**: Should we validate extension children at arbitrary nesting depth (complex extensions with sub-extensions)? HAPI does this recursively. For now, we skip extension children entirely — this is a known gap.
