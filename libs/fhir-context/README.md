# ferrum-context

FHIR conformance resource access, version resolution, and caching. This is the shared foundation that `fhir-validator`, `fhir-snapshot`, `fhirpath`, and the server use to look up StructureDefinitions, ValueSets, and other conformance resources.

## Core Abstractions

### `FhirContext` (trait)

The main interface for accessing FHIR metadata. All consumers depend on this trait, never on a concrete implementation.

```rust
use ferrum_context::FhirContext;

// Look up a StructureDefinition by type name
let sd = ctx.get_core_structure_definition_by_type("Patient")?;

// Resolve element type info
let info = ctx.get_element_type("Observation", "value[x]")?;

// Navigate nested paths across StructureDefinitions
let info = ctx.resolve_path_type("Patient", "name.given")?;

// Get choice type expansions
let types = ctx.get_choice_expansions("Observation", "value")?;
```

### `DefaultFhirContext`

In-memory implementation backed by loaded FHIR packages. Resources are indexed by canonical URL and version, with automatic version selection (stable releases preferred over prereleases).

```rust
use ferrum_context::DefaultFhirContext;

// From a loaded package
let ctx = DefaultFhirContext::new(package);

// From multiple packages (e.g. core + profiles)
let ctx = DefaultFhirContext::from_packages(vec![core, us_core]);

// From the registry (async, downloads with transitive deps)
let ctx = DefaultFhirContext::from_fhir_version_async(None, "R4").await?;
```

### `FlexibleFhirContext`

Async-capable context with two-level LRU caching (canonical URL + exact version). Wraps any `ConformanceResourceProvider` — use this when conformance resources come from a database or external service rather than static packages.

```rust
use ferrum_context::FlexibleFhirContext;

let ctx = FlexibleFhirContext::new(provider)
    .with_cache_capacity(8192)
    .with_ttl(Some(Duration::from_secs(120)));
```

### `ConformanceResourceProvider` (trait)

Async trait for sourcing conformance resources. Implement this to back a `FlexibleFhirContext` with a custom store (database, API, etc.).

### `FallbackConformanceProvider`

Composite provider with primary/fallback semantics — tries the primary source first, falls back on empty results or errors.

## Version Resolution

The crate implements FHIR version algorithm support (`semver`, `integer`, `alpha`, `date`, `natural`) and uses `versionAlgorithmString`/`versionAlgorithmCoding` from resources when present. When no algorithm is specified, it auto-detects semver > integer > string ordering.

## Lock Files

`PackageLock` pins exact package versions for reproducible builds:

```rust
use ferrum_context::PackageLock;

// Generate from loaded packages
let lock = PackageLock::from_packages("my.ig", "1.0.0", &packages);
lock.save("fhir.lock.json")?;

// Load and validate
let lock = PackageLock::load("fhir.lock.json")?;
lock.validate_packages(&packages)?;
```

## Feature Flags

- **`registry-loader`** (default) — enables async package loading from the Simplifier registry via `ferrum-registry-client`

## Testing

```bash
cargo test -p ferrum-context
```
