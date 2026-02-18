# FHIR Validation: HAPI Deep Dive & Ferrum Comparison

## HAPI Architecture Overview

### Core Components

**InstanceValidator** (`org.hl7.fhir.validation.instance.InstanceValidator`)
- Monolithic recursive walker (~8k lines) that traverses a resource element-by-element
- Validates each element against its ElementDefinition from the StructureDefinition snapshot
- Uses a "slicing context" stack to track which slices are active during traversal
- Calls out to terminology services, FHIRPath engine, and resource fetchers as needed

**ValidatorWrapper** (`FhirInstanceValidator` → `ValidatorWrapper`)
- Bridge between HAPI's `IValidationSupport` chain and the HL7 core `InstanceValidator`
- Configures: best practice level, extension handling, terminology checks, policy advisor
- Creates `WorkerContextValidationSupportAdapter` which wraps `IValidationSupport` as an `IWorkerContext`

**WorkerContextValidationSupportAdapter**
- Adapts HAPI's `IValidationSupport` to the HL7 `IWorkerContext` interface
- **Lazy snapshot generation**: calls `SnapshotGeneratingValidationSupport` on demand
- **R5 canonicalization**: internally converts all resources to R5 model regardless of input version
- Caches fetched/converted resources via `fetchedResourceCache`

**ValidationSupportChain** (Chain of Responsibility)
- Stacks multiple `IValidationSupport` implementations in order:
  1. `DefaultProfileValidationSupport` — bundled conformance resources
  2. `SnapshotGeneratingValidationSupport` — generates snapshots on demand
  3. `InMemoryTerminologyServerValidationSupport` — ValueSet expansion + code validation
  4. `CommonCodeSystemsTerminologyService` — well-known code systems (UCUM, MIME, languages)
  5. User-provided custom supports (e.g., `NpmPackageValidationSupport`, `RemoteTerminologyServiceValidationSupport`)
- Each link tries to answer; if it returns null, the next link is tried

### Key Design Decisions in HAPI

**1. Lazy Snapshot Generation (not pre-expanded)**
- Snapshots are generated on first access via `SnapshotGeneratingValidationSupport`
- Uses a `userData` flag (`SnapshotGeneratingValidationSupport.CURRENTLY_GENERATING`) to detect cycles
- If a cycle is detected, returns the SD without a snapshot (graceful degradation)
- No deep expansion — the validator resolves child types inline during traversal

**2. PolicyAdvisor Controls Strictness**
- `IValidationPolicyAdvisor` determines how strictly to validate different contexts
- `FhirDefaultPolicyAdvisor`:
  - `CHECK_VALID` for contained resources
  - `IGNORE` for external references (don't fetch/validate them)
  - `CHECK_VALID_NO_CACHE` for Bundle entries
- This is pluggable — users can customize per use case

**3. Two-Phase Profile Loading**
- Phase 1: Load the base resource type's StructureDefinition
- Phase 2: Load any profiles declared in `meta.profile`, validate against each
- Unknown profiles controlled by `errorForUnknownProfiles` flag (default: error)

**4. InMemory Terminology Validation**
- Expands ValueSets in-memory using set operations on `include`/`exclude` definitions
- Supports `compose.include` with system + concept enumeration and filters
- Falls back to `RemoteTerminologyServiceValidationSupport` for complex expansions
- `$validate-code` works against the expanded set

**5. Extension Handling**
- `anyExtensionsAllowed` flag (default: true) — unknown extensions produce info, not errors
- Custom extension domains can be registered
- Known extensions (hl7.org, example.org, nema.org, acme.com) always allowed

## Ferrum vs HAPI: What We Do Similarly

| Aspect | HAPI | Ferrum |
|--------|------|--------|
| Snapshot materialization | Lazy, on-demand | Eager, cached in `ExpandedFhirContext` |
| Cycle detection | `userData` flag per SD | `HashSet<String>` stack per materialization call |
| Caching | `fetchedResourceCache` in WorkerContext | `RwLock<HashMap<SdCacheKey, Arc<SD>>>` in ExpandedFhirContext |
| Profile validation | Validates base type + declared profiles | Validates against resolved SD (profile support WIP) |
| Configurable strictness | `BestPracticeWarningLevel` + `PolicyAdvisor` | `ValidatorConfig` with presets (Ingestion/Authoring/Server/Publication) |
| Step-based validation | Monolithic walker with inline checks | Composable steps (schema, cardinality, fixed values, FHIRPath, slicing) |

## Where Ferrum Can Do Better

### 1. Pre-expanded Snapshots (Performance Win)
HAPI resolves child types lazily during validation traversal. Ferrum's `ExpandedFhirContext` pre-expands snapshots with deep child type inlining. This means:
- **One-time cost**: expansion happens once per SD, cached for reuse
- **Simpler validator logic**: the validator sees a fully-expanded tree, no need to resolve types mid-walk
- **Faster repeated validation**: subsequent validations against the same profile are pure lookups

The `MaterializedView` pattern ensures the expander doesn't double-expand, keeping the pre-expansion safe.

### 2. Composable Validation Steps (vs Monolithic Walker)
HAPI's `InstanceValidator` is a single ~8k line class. Ferrum uses composable steps:
- `SchemaStep` — type/structure validation
- `CardinalityStep` — min/max constraints
- `FixedValueStep` — fixed/pattern matching
- `FhirPathStep` — constraint expressions
- `SlicingStep` — discriminator-based slicing

Benefits:
- Each step is independently testable
- Steps can be enabled/disabled per config preset
- New validation rules = new step, not modifying a giant file
- Parallel step execution possible in the future

### 3. Presets Over Flags
HAPI has ~15 individual boolean/enum settings. Ferrum groups these into semantic presets:
- `Ingestion` — lenient, accept what's reasonable
- `Authoring` — strict, catch mistakes early
- `Server` — balanced for API responses
- `Publication` — strictest, conformance-grade

This is more user-friendly while still allowing per-step customization.

### 4. Rust Performance Characteristics
- No GC pauses during validation of large bundles
- `Arc<StructureDefinition>` for zero-copy sharing across threads
- `RwLock` caches allow concurrent reads with infrequent writes
- Potential for `rayon`-based parallel bundle entry validation

## Gaps to Close (What HAPI Does That We Don't Yet)

### Priority 1: Terminology Validation
HAPI validates coded elements against ValueSets using `InMemoryTerminologyServerValidationSupport`. Ferrum's validator currently skips terminology checks entirely (noted in CLAUDE.md gap #10).

**Plan**: Add a `TerminologyStep` that:
- Resolves `binding.valueSet` from ElementDefinition
- Validates codes against expanded ValueSets (we already have `$expand` and `$validate-code` in the server)
- Respects binding strength (required → error, extensible → warning, preferred → info)

### Priority 2: Reference Validation
HAPI resolves and optionally validates referenced resources. Ferrum skips reference validation.

**Plan**: Add a `ReferenceStep` that:
- Validates reference format (relative, absolute, contained)
- Checks `targetProfile` constraints on Reference types
- Optionally resolves contained references
- PolicyAdvisor-style control over external reference handling

### Priority 3: Bundle Validation
HAPI validates Bundle entries with `CHECK_VALID_NO_CACHE` policy and handles `fullUrl` resolution.

**Plan**: Add a `BundleStep` that:
- Validates each entry against its resource type
- Resolves internal Bundle references
- Validates Bundle-level constraints (unique fullUrls, transaction consistency)

### Priority 4: Profile Support in Tests
The official test suite uses `profiles` and `supporting` fields to load additional SDs. Current test harness ignores these — we only validate against the base resource type.

**Plan**: Extend `run_single_test` to:
- Load `supporting` SDs into the context
- Apply `profiles` as validation targets
- Handle `profile.source` for profile-specific test expectations

### Priority 5: Multi-Profile Validation
HAPI validates against both the base type SD and all profiles in `meta.profile`. Ferrum currently only validates against the resolved SD.

**Plan**: After loading a resource:
1. Validate against base type SD
2. For each profile in `meta.profile`, resolve and validate separately
3. Merge issues, tracking which profile produced which error

### Priority 6: Slicing Completeness
Ferrum has basic slicing support but is missing discriminators for: `type`, `profile`, `position`. HAPI handles all discriminator types.

## Test Suite Status (updated 2026-02-18)

**Current baseline**: 168/391 passing (43.0%), 522 ignored, ~7.5 seconds

### Changelog (105 → 168)

| Fix | Tests | Description |
|-----|-------|-------------|
| ele-1 false positives | 105 → 133 (+28) | Constraint evaluation now iterates over array items via `from_json_at()` instead of evaluating on raw JSON arrays |
| `as()` non-singleton | 133 → 134 (+1) | `as()` on multi-item collections returns empty per FHIRPath spec, not error |
| Choice type detection | 134 → 138 (+4) | `is_choice_variant_name()` in unknown element check + `fhir_comments` as special element |
| Extension object detection | 138 → 163 (+25) | Skip unknown element checking for extension objects (detected by `url` key + path ending in `.extension`/`.modifierExtension`) and nested resources (detected by `resourceType` key) |
| FHIRPath type navigation | 163 → 168 (+5) | Case-sensitive type navigation prevents `extension` field being treated as `Extension` type navigation, fixing ext-1 constraint false positives |

### Skip categories (522 tests)
- Non-JSON files (XML not supported yet)
- Unsupported FHIR versions (only R4/R5)
- Modules: tx, tx-advanced (terminology), cda, cdshooks, shc, logical, json5
- External packages required
- No Java expectations defined

### Failure breakdown (223 remaining)

| Category | Count | Root Cause | Effort |
|----------|-------|------------|--------|
| questionnaire | ~63 | QR validation not implemented | Large (new feature) |
| profile | ~30 | Mix: extension SD validation, contained resource re-validation, missing profiles | Medium |
| general | ~25 | ID validation, empty arrays, contained re-validation, attachment checks | Medium |
| bundle | ~16 | Bundle entry validation not implemented | Medium |
| references | ~12 | Reference validation not implemented | Medium |
| extensions | ~12 | Extension children skipped but not validated against Extension SD | Medium |
| base | ~10 | R5-specific, contained resource re-validation | Low priority |
| sd | ~9 | StructureDefinition meta-validation | Low priority |
| measure/dsig/fmt | ~18 | Specialized resource types, signatures | Low priority |
| xver/xhtml/v2 | ~13 | Cross-version, HTML, v2 messages | Out of scope |
| api/security | ~7 | SearchParameter/security-specific | Low priority |
| other | ~8 | Type matching, FHIRPath edge cases, misc | Low priority |

### Resolved false positive patterns
These patterns previously caused false failures and have been fixed:
1. ~~`Unknown element 'url'` / `Unknown element 'valueXxx'` in extension contexts~~ → Fixed: extension objects auto-detected and skipped
2. ~~`ext-1` constraint failing due to FHIRPath treating `extension` as `Extension` type navigation~~ → Fixed: case-sensitive type navigation
3. ~~`Unknown element 'status'` etc. in contained/nested resources~~ → Fixed: nested resources auto-detected and skipped
4. ~~ele-1 failing on arrays (e.g., `Patient.name`)~~ → Fixed: constraint evaluation iterates per array item

### Remaining false positive patterns
1. Extension children not validated against Extension SD — children are silently skipped, but some tests expect specific validation of extension values
2. Nested resource children not re-validated — objects with `resourceType` are skipped but not recursively validated against their own SD
3. Profile resolution failures — test profiles referenced by filename can't be resolved

### Projected trajectory
- Contained/nested resource re-validation: 168 → ~185
- Extension SD resolution + references: ~185 → ~200
- Bundle validation: ~200 → ~215
- Schema fine-grained checks: ~215 → ~225
- QuestionnaireResponse (if implemented): ~225 → ~265
