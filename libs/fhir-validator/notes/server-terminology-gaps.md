# Server as Remote Terminology Service for Validation

## What the Validator's RemoteTerminologyProvider Needs

The `RemoteTerminologyProvider` (inside `libs/fhir-validator`) will call the server's HTTP endpoints. It needs exactly two operations:

### 1. `POST /fhir/ValueSet/$validate-code`

**Request** (Parameters):
```json
{
  "resourceType": "Parameters",
  "parameter": [
    { "name": "url", "valueUri": "http://hl7.org/fhir/ValueSet/languages" },
    { "name": "system", "valueUri": "urn:ietf:bcp:47" },
    { "name": "code", "valueString": "en" },
    { "name": "display", "valueString": "English" }
  ]
}
```

**Response** (Parameters):
```json
{
  "resourceType": "Parameters",
  "parameter": [
    { "name": "result", "valueBoolean": true },
    { "name": "display", "valueString": "English" },
    { "name": "message", "valueString": "..." }
  ]
}
```

### 2. `POST /fhir/CodeSystem/$validate-code`

Same shape but validates against a CodeSystem directly (no ValueSet context).

---

## What the Server Already Has

| Capability | Status | Notes |
|-----------|--------|-------|
| `ValueSet/$validate-code` | **Works** | Accepts `url`, `system`, `code`, `coding` |
| `CodeSystem/$validate-code` | **Works** | Direct concept lookup |
| `ValueSet/$expand` | **Works** | Full compose expansion with caching |
| `$lookup` | **Works** | Concept details with properties |
| `$subsumes` | **Works** | Hierarchy traversal |
| `$translate` | **Works** | ConceptMap-based translation |
| Expansion caching | **Works** | 24h TTL, parameter-hashed |
| GET + POST input | **Works** | Query params auto-converted to Parameters |

The server is already 80% ready. The gaps below are what's needed to make it a reliable validation backend.

---

## Gaps to Fix

### Critical (blocks validation correctness)

#### 1. Missing OperationDefinitions in internal package

**Problem**: Terminology operations ($expand, $validate-code, $lookup, $subsumes, $translate) have no OperationDefinition resources in `apps/server/fhir_packages/ferrum.fhir.server#1.0.0/`. The operation registry won't find them unless they're loaded from external FHIR packages.

**Fix**: Add OperationDefinition JSON files to the internal package for:
- `ValueSet-expand` (type-level + instance-level)
- `ValueSet-validate-code` (type-level + instance-level)
- `CodeSystem-validate-code` (type-level + instance-level)
- `CodeSystem-lookup` (type-level)
- `CodeSystem-subsumes` (type-level + instance-level)

These are standard FHIR operations — their OperationDefinitions are in the spec. We just need to bundle them.

**Alternative**: The operations may already work if the HL7 FHIR core package is loaded (it includes these OperationDefinitions). Verify this and document the requirement.

#### 2. `$validate-code` missing `display` parameter

**Problem**: The validator needs to check whether a display string matches the expected display for a code. The server's `$validate-code` doesn't accept or validate the `display` parameter.

**Fix** in `apps/server/src/services/terminology.rs`:
- Accept `display` (string) parameter
- After validating code membership, compare provided display against the concept's display
- If mismatch: `result: true` but include `message: "Display mismatch: expected 'X', got 'Y'"`
- Return the correct display in the `display` output parameter regardless

#### 3. `$validate-code` missing `abstract` handling

**Problem**: Some ValueSets include abstract codes (grouping concepts not meant for direct use). The validator needs to know if a matched code is abstract.

**Fix**:
- Check `ValueSetExpansionContains.abstract` flag
- If code is abstract and used in a concrete context, return `result: false` with appropriate message
- Add `abstract` output parameter to response

### Important (improves coverage)

#### 4. CodeSystem.content mode awareness in $validate-code response

**Problem**: When a CodeSystem has `content: "fragment"` (e.g., SNOMED CT loaded partially), a missing code doesn't necessarily mean it's invalid — the CodeSystem is just incomplete. The validator needs this signal to downgrade errors to warnings.

**Fix**:
- Look up the CodeSystem's `content` mode during validation
- Include a `codeSystem.content` output parameter in the response (or a boolean `unknown` parameter)
- When `content: "fragment"` and code not found: `result: false` + `message: "Code not found in fragment CodeSystem (may exist in full version)"`

The validator's `RemoteTerminologyProvider` uses this to emit a warning instead of an error.

#### 5. ValueSet compose.include filter expansion

**Problem**: Some ValueSets use property-based filters (e.g., `is-a`, `descendent-of`, `=`) to include codes. The server's `$expand` only handles explicit concept lists and full system includes, not filter-based includes.

**Fix** in expansion logic:
- Support `filter.op = "="` — match concepts where property equals value
- Support `filter.op = "is-a"` — include concept and all descendants
- Support `filter.op = "descendent-of"` — include descendants only (not the concept itself)
- Support `filter.op = "in"` — match concepts where property is in comma-separated list

This requires the `codesystem_concepts` table to have properties indexed, which it already does (JSONB `properties` column with GIN index).

#### 6. Automatic CodeSystem concept extraction

**Problem**: `$validate-code` against a CodeSystem works by looking up concepts in the `codesystem_concepts` table. But concepts are only extracted when explicitly indexed (e.g., via `$reindex` or package install). If a CodeSystem is uploaded via CRUD without extraction, validation silently fails to find codes.

**Fix**:
- On CodeSystem create/update, extract concepts to `codesystem_concepts` as a background job
- Or: fall back to in-memory CodeSystem.concept hierarchy scan (already partially implemented)
- The in-memory fallback exists but is slow for large CodeSystems

### Nice to Have (not blocking)

#### 7. Batch $validate-code

**Problem**: The validator checks many codes per resource (every coded element). Making one HTTP call per code is slow.

**Fix**: Add a batch endpoint or accept multiple codes in a single call:
```
POST /fhir/ValueSet/$validate-code
{
  "resourceType": "Parameters",
  "parameter": [
    { "name": "url", "valueUri": "http://hl7.org/fhir/ValueSet/languages" },
    { "name": "coding", "valueCoding": { "system": "...", "code": "en" } },
    { "name": "coding", "valueCoding": { "system": "...", "code": "fr" } },
    { "name": "coding", "valueCoding": { "system": "...", "code": "de" } }
  ]
}
```

Alternative: The `RemoteTerminologyProvider` can batch internally — collect all codes from a resource, deduplicate by (system, code, valueSet), then make one call per unique ValueSet.

#### 8. Cache invalidation endpoint

**Problem**: Expansion cache has fixed 24h TTL with no manual invalidation.

**Fix**: Add `DELETE /fhir/ValueSet/$expand-cache` or a parameter to `$expand` like `cache=no-cache`.

#### 9. $validate-code `inferSystem` parameter

**Problem**: Some bindings don't have an explicit system — the validator passes just the code and ValueSet URL. If the ValueSet only draws from one system, the server should infer it.

**Fix**: When `system` is not provided but the ValueSet's compose has exactly one include with a system, use that system automatically.

---

## Implementation Priority

```
Phase 1 (unblocks RemoteTerminologyProvider):
  [1] Bundle OperationDefinitions for terminology ops
  [2] Add display parameter to $validate-code
  [4] Add CodeSystem.content mode to $validate-code response

Phase 2 (improves accuracy):
  [3] Abstract code handling
  [5] Filter-based ValueSet expansion (is-a, =, descendent-of)
  [6] Auto-extract CodeSystem concepts on upload

Phase 3 (performance):
  [7] Batch $validate-code
  [8] Cache invalidation
  [9] inferSystem support
```

---

## RemoteTerminologyProvider ↔ Server Contract

The `RemoteTerminologyProvider` in `libs/fhir-validator` will:

```rust
pub struct RemoteTerminologyProvider {
    base_url: String,        // e.g., "http://localhost:8080/fhir"
    client: reqwest::Client,
    timeout: Duration,
    /// Cache expanded results to avoid repeated HTTP calls
    cache: RwLock<HashMap<(String, String, String), CodeValidationResult>>,
}

impl TerminologyProvider for RemoteTerminologyProvider {
    fn validate_code(
        &self,
        system: &str,
        code: &str,
        display: Option<&str>,
        value_set_url: &str,
    ) -> Result<Option<CodeValidationResult>> {
        // 1. Check local cache
        // 2. POST to {base_url}/ValueSet/$validate-code
        // 3. Parse Parameters response
        // 4. Map to CodeValidationResult
        // 5. Cache result
    }
}
```

The server needs to return enough information for the validator to make the right decision:
- `result` (bool) — is the code valid?
- `display` (string) — correct display for the code
- `message` (string) — human-readable explanation
- `codeSystem.content` (code) — "complete" / "fragment" / "not-present" (for severity mapping)
