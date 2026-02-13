# CLAUDE.md - Zunder FHIR Server

## What is this?

Zunder is a high-performance FHIR R4/R5 server in Rust. Monorepo with server, CLI, admin UI, and shared FHIR libraries.

## Project Structure

```
apps/
  server/          # Axum HTTP server + background worker (Rust)
  cli/             # CLI tool: FHIRPath eval, snapshot gen, codegen
  admin-ui/        # Next.js admin dashboard
libs/
  fhir-models      # FHIR resource type definitions
  fhir-context     # FHIR metadata, structure definitions, caching
  fhirpath         # FHIRPath expression engine (lexer → parser → analyzer → VM)
  fhir-snapshot    # StructureDefinition snapshot generation
  fhir-validator   # Resource validation against profiles
  fhir-format      # JSON ↔ XML serialization
  fhir-package     # FHIR package (.tgz) handling
  fhir-registry-client  # simplifier.net / packages.fhir.org client
  fhir-codegen     # Strongly-typed Rust codegen from FHIR definitions
  ucum             # UCUM units of measure
  fhir-client-ts   # TypeScript FHIR client (@thalamiq/fhir-client)
docs/              # Mintlify documentation site
quickstart/        # Quickstart bundle (compose.yaml, config.yaml, install.sh)
docker/            # Docker compose files for local/hetzner deployments
```

## Commands

```bash
# Build & check
cargo check                        # Type-check workspace
cargo build                        # Build all
cargo build -p zunder              # Build server only
cargo clippy                       # Lint

# Run
cargo run --bin fhir-server        # Start API server
cargo run --bin fhir-worker        # Start background worker

# Test
cargo test                         # All tests (needs PostgreSQL)
cargo test -p zunder               # Server tests only
cargo test -p zunder-fhirpath      # FHIRPath tests only
cargo test --test search_tests     # Specific integration test
RUST_TEST_THREADS=4 cargo test     # Control parallelism

# Docker
docker compose -f docker/compose.local.yaml up --build   # Local dev stack
docker compose -f docker/compose.hetzner.yaml up -d       # Production

# Admin UI
pnpm dev:ui                        # Dev server (port 3000)
pnpm build:ui                      # Production build

# CLI
cargo run -p zunder-cli -- fp "Patient.name.family" --resource '{"resourceType":"Patient","name":[{"family":"Smith"}]}'
cargo run -p zunder-cli -- snap gen --base base.json --differential diff.json
cargo run -p zunder-cli -- codegen --output ./generated --fhir-version R4
```

## Architecture (Server)

Four deployable components: **API** (Axum HTTP), **Worker** (background jobs), **Admin UI** (Next.js), **DB** (PostgreSQL).

### Key Design Rules

1. **Services NEVER have PgPool** — only repositories (in `src/db/`) touch SQL
2. **SearchEngine is shared** — single `Arc<SearchEngine>` created in AppState, shared across all services
3. **Repository pattern** — `PostgresResourceStore`, `TerminologyRepository`, `AdminRepository`, `MetadataRepository`, etc.
4. **Job queue abstraction** — `PostgresJobQueue` in prod, `InlineJobQueue` in tests (immediate execution)

### Request Flow

```
HTTP Request → Axum Router → Handler (src/api/handlers/)
  → Service (src/services/) → Repository (src/db/) → PostgreSQL
  → Response formatting (content negotiation, ETag, Prefer header)
```

### Database Schema

- `resources` — JSONB storage for all FHIR resources (versioned, soft-deletable)
- `resource_history` — immutable version log
- `search_*` — 8 UNLOGGED index tables (string, token, date, number, quantity, uri, text, content)
- `compartment_memberships` — Patient/Encounter compartment rules
- `jobs` — background job queue
- `runtime_config` + `runtime_config_audit` — dynamic config with LISTEN/NOTIFY sync

### Config Precedence

Defaults → `config.yaml` → `FHIR__*` env vars → `DATABASE_URL` fallback → Runtime overrides (PostgreSQL)

## Testing

Tests require a running PostgreSQL instance. Each test gets its own schema for isolation.

Key test infrastructure in `apps/server/tests/support/`:
- `TestApp` — per-test DB schema, automatic cleanup
- `builders.rs` — fluent resource builders (PatientBuilder, ObservationBuilder, etc.)
- `fixtures.rs` — standard test data (minimal_patient, observation_with_loinc, etc.)
- `assertions.rs` — FHIR-specific assertions (assert_bundle, assert_version_id, etc.)

## FHIR Spec Compliance Status

### Fully Implemented
- CRUD operations (create, read, update, patch, delete) with conditional variants
- Search: string, token, date, number, quantity, reference, URI, text/content parameters
- Search: single-level chaining, _include/_revinclude, _summary, _elements, _sort (by _id, _lastUpdated)
- Batch and Transaction bundles (atomic, with URL rewriting and reference resolution)
- History (instance, type, system level) with _since, _count
- Versioning: ETag, If-Match, If-None-Match, If-Modified-Since
- Pagination: cursor-based keyset + offset
- CapabilityStatement generation from live search parameters
- Terminology: $expand, $lookup, $validate-code, $subsumes, $translate, $closure
- Compartment search (Patient, Encounter)
- SMART on FHIR / OIDC authentication (resource server mode)
- JSON format (application/fhir+json)

### Known Spec Gaps (Priority Order)

**Critical — spec violations:**
1. Token search is case-insensitive (spec requires case-sensitive) — `tests/search/parameters/token.rs:277`
2. Date period overlap logic missing — `tests/search/parameters/date.rs:495`
3. ID-only reference search returns 400 instead of matching — `tests/search/parameters/reference.rs:164`

**High — missing features that matter:**
4. Composite search parameters not supported
5. Reference `:identifier` modifier not implemented — `tests/search/parameters/reference.rs:383`
6. Token modifiers `:in`, `:not-in`, `:above`, `:below` not supported
7. XML format parsed but disabled (`content_negotiation.rs:105`)
8. Sorting only by `_id` and `_lastUpdated` (no custom parameter sort)
9. History `_at` and `_list` parameters not supported
10. Validator missing: terminology validation, reference validation, bundle validation — `libs/fhir-validator/src/validator.rs:168-176`
11. Slicing discriminators incomplete: position, type, profile — `libs/fhir-validator/src/steps/slicing.rs`

**Medium — nice to have:**
12. `$everything` operation not implemented
13. Recursive chaining not supported (single-level only)
14. PATCH only supports JSON Patch (no FHIRPath Patch)
15. Only Patient/Encounter compartments (missing Device, Practitioner, RelatedPerson)
16. Metadata normative mode not implemented — `src/api/handlers/metadata.rs:46`
17. `$subsumes` missing codingA/codingB support — `src/services/terminology.rs:897`
18. `$reindex` doesn't enqueue background job — `src/services/operation_executor.rs:181`
19. Date chaining with prefixes incorrect — `tests/search/chaining.rs:611`
20. SMART patient compartment enforcement not implemented

### Test Coverage Gaps

Well-covered: CRUD (60+ tests), Search (50+ tests), Batch/Transaction, FHIRPath (20 test files + HL7 suite)

Not tested: Admin endpoints, /metadata, /metrics, background workers, SMART authorization, runtime config, package management endpoints

## Crate Naming

| Crate | Cargo name | Import name | Directory |
|-------|-----------|-------------|-----------|
| Server | `zunder` | `zunder` | `apps/server` |
| CLI | `zunder-cli` | `zunder_cli` | `apps/cli` |
| Models | `zunder-models` | `zunder_models` | `libs/fhir-models` |
| Context | `zunder-context` | `zunder_context` | `libs/fhir-context` |
| FHIRPath | `zunder-fhirpath` | `zunder_fhirpath` | `libs/fhirpath` |
| Snapshot | `zunder-snapshot` | `zunder_snapshot` | `libs/fhir-snapshot` |
| Validator | `zunder-validator` | `zunder_validator` | `libs/fhir-validator` |
| Format | `zunder-format` | `zunder_format` | `libs/fhir-format` |
| Package | `zunder-package` | `zunder_package` | `libs/fhir-package` |
| Registry | `zunder-registry-client` | `zunder_registry_client` | `libs/fhir-registry-client` |
| Codegen | `zunder-codegen` | `zunder_codegen` | `libs/fhir-codegen` |
| UCUM | `zunder-ucum` | `zunder_ucum` | `libs/ucum` |

## Docker Images

- `ghcr.io/thalamiq/zunder` — server + worker (select binary via command)
- `ghcr.io/thalamiq/zunder-ui` — admin UI
- Container names: `zunder-db`, `zunder-server`, `zunder-worker`, `zunder-ui`, `zunder-caddy`

## Deployment

- **Local**: `docker compose -f docker/compose.local.yaml up --build`
- **Production**: `docker compose -f docker/compose.hetzner.yaml up -d` (with Caddy TLS)
- **Fly.io**: `fly deploy` from `apps/server/` (app: `zunder`, db: `zunder-db`)
- **Distribution**: `./scripts/release-dist.sh` creates `zunder.tar.gz`

## CI/CD

- `.github/workflows/docker-build.yml` — builds + pushes Docker images on tags, creates GitHub releases
- `.github/workflows/deploy-fly.yml` — deploys to Fly.io on tags

## Internal FHIR Package

`apps/server/fhir_packages/zunder.fhir.server#1.0.0/` contains custom OperationDefinitions ($reindex, $install-package). Loaded automatically at startup when `install_internal_packages: true`.
