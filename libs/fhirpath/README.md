<!--
This README is intentionally technical: it documents internal design choices,
trade-offs, and implementation details for contributors.
-->

# `fhir-fhirpath`

FHIRPath compiler + VM with support for the HL7 R5 FHIRPath test suite. The crate is designed to:

- Parse and compile FHIRPath expressions into a compact bytecode `Plan`
- Evaluate a `Plan` against JSON-backed FHIR resources
- Support both **lenient** evaluation (pragmatic for JSON) and **strict** semantics (spec-style validation)
- Provide pipeline visualization (AST / HIR / VM Plan) for debugging and development

This repository also contains a CLI wrapper in `crates/cli` that exercises the engine.

## Quick Start (API)

```rust
use ferrum_fhirpath::{Context, Engine, Value};
use serde_json::json;

let engine = Engine::with_fhir_version("R5")?;
let resource = json!({"resourceType": "Patient", "gender": "male"});
let root = Value::from_json(resource);
let ctx = Context::new(root);

// Rooted expression
let result = engine.evaluate_expr("Patient.gender", &ctx, None)?;

// Unrooted expression with type-aware compilation
let result = engine.evaluate_expr("gender", &ctx, Some("Patient"))?;
# Ok::<(), ferrum_fhirpath::Error>(())
```

## CLI

From the workspace root:

```sh
# evaluate (resource file)
cargo run -p cli -- fp "Patient.gender" fhir-test-cases/r5/examples/patient-example.json --output fhirpath

# evaluate (stdin)
cat fhir-test-cases/r5/examples/observation-example.json \
  | cargo run -p cli -- fp "valueQuantity.value" - --output fhirpath

# strict semantics (invalid paths error instead of returning empty)
cargo run -p cli -- fp "valueQuantity.value" fhir-test-cases/r5/examples/observation-example.json --strict
```

## Architecture Overview

The engine implements a classic compiler pipeline:

1. **Lexer** (`src/lexer.rs`) → token stream
2. **Parser** (`src/parser.rs`) → AST (`src/ast.rs`)
3. **Semantic analysis** (`src/analyzer.rs`) → HIR (`src/hir.rs`)
4. **Type resolution pass** (`src/typecheck.rs`) → typed HIR
5. **Codegen** (`src/codegen.rs`) → bytecode `Plan` (`src/vm.rs`)
6. **VM execution** (`src/vm.rs`, `src/vm/operations.rs`, `src/vm/functions/*`) → `Collection`

The top-level orchestration lives in `src/engine.rs`.

### Engine and Caching

`Engine` (`src/engine.rs`) owns the “global” registries and caches:

- `TypeRegistry` (`src/types.rs`): System type IDs + helpers used throughout compilation.
- `FunctionRegistry` (`src/functions.rs`): compile-time PHF map from function name → `FunctionId` + signature metadata.
- `VariableRegistry` (`src/variables.rs`): assigns numeric IDs to external variables (e.g. `%resource`).
- `FhirContext` (`fhir-context` crate): StructureDefinition access for type inference and validation.
- `LruCache<String, Arc<Plan>>`: caches compiled bytecode, keyed by `(base_type, expr)` when a base type is provided.

Key property: compilation can be expensive; evaluation is intended to be cheap, so `Engine::compile()` + `Engine::evaluate()` is the “hot path” for repeated evaluation.

## Data Model

### Value and Collection

The runtime value model is in `src/value.rs`:

- `Value` is an `Arc<ValueData>` for cheap cloning.
- `Collection` is an optimized container for the “mostly singleton” nature of FHIRPath:
  - small collections use an inline `SmallVec`
  - larger ones switch to an `Arc`-backed representation for O(1) clones

`ValueData` variants cover:

- primitives: `Boolean`, `Integer`, `Decimal`, `String`
- temporals: `Date`, `DateTime`, `Time` (each with explicit precision)
- `Quantity` (value + unit)
- `Object` (JSON object as `HashMap<field, Collection>`)
- `Empty` (FHIRPath empty collection marker; the VM treats `{}` as an empty *collection*, not a singleton `Empty` value)

### Temporal Precision and Timezones

Temporals model two distinct aspects:

1. **Precision** (`DatePrecision`, `DateTimePrecision`, `TimePrecision`)
2. **Timezone presence** for `DateTime`:
   - `timezone_offset: None` → no timezone was specified (unknown/local)
   - `Some(0)` → explicit `Z`
   - `Some(n)` → fixed offset seconds east of UTC (e.g. `+08:00` is `28800`)

This matters for both equality/ordering (comparability rules) and boundary functions.

## Context and Variables

`Context` (`src/context.rs`) contains:

- `$this` (current focus), `$index` (iteration index)
- `resource` / `root` (the original evaluation input)
- `variables` for environment values (e.g. `%resource`, `%context`, `%rootResource` (and legacy `%root`), `%profile`, `%sct`, `%loinc`, …)
- `strict` flag for strict semantic validation

The VM also tracks `$total` for `aggregate()` and threads it through nested evaluation contexts.

## The Compiler Pipeline in Detail

### Lexer

The lexer (`src/lexer.rs`) produces tokens for:

- identifiers, string/number literals
- operators (`=`, `!=`, `<`, `<=`, `|`, `in`, `contains`, …)
- temporal literals (e.g. `@2014-01`, `@2014-01-01T08`, `@T10:30:00.000`)
- special invocations (`$this`, `$index`, `$total`, `%resource`, …)

### Parser (AST)

The parser (`src/parser.rs`) builds an AST (`src/ast.rs`) using precedence rules that match the FHIRPath spec.

Notable parsing details:

- `is` / `as` bind at the correct precedence relative to equality and union.
- Temporal literals preserve precision and timezone presence.
- DateTime literals with hour-only forms are normalized to minute precision (`T08` → `T08:00`) to match the FHIR constraints used by the HL7 suite.

### Analyzer (AST → HIR)

The analyzer (`src/analyzer.rs`) is the semantic lowering step:

- Assigns *expression types* (`ExprType`) and cardinality ranges where possible.
- Resolves function names to `FunctionId` using `FunctionRegistry`.
- Rewrites syntactic sugar:
  - collection literals `{ a, b, c }` become chained unions (`a | b | c`)
- Produces a compact HIR (`src/hir.rs`) better suited for codegen and execution.

HIR nodes include:

- literals, variables, unary/binary ops
- paths (`base + [segments]`) where a segment is a field, choice segment, or index
- function calls and method calls
- higher-order forms like `where`, `select`, `repeat`, `aggregate`, `exists(predicate)`, and `all(predicate)`

### TypePass (HIR Type Resolution)

After analysis, a second pass (`src/typecheck.rs`) uses `FhirContext` to:

- Resolve FHIR element types along paths
- Validate path segments when a base type is provided
- Improve output types for downstream operations

This pass is where “compile-time strictness” comes from: passing a base type (e.g. `Some("Patient")`) enables StructureDefinition-backed validation.

### Code Generation (HIR → Plan)

`CodeGenerator` (`src/codegen.rs`) emits a `Plan` (`src/vm.rs`):

- `opcodes`: the bytecode instruction stream
- constant pool: literal `Value`s
- segment pool: field names, stored as interned `Arc<str>`
- type specifier pool: for `is` / `as` operations
- function ID table and nested `subplans` for closures

Important design choices:

- Many higher-order functions compile to dedicated opcodes (`Where`, `Select`, `Repeat`, `Aggregate`, `Exists`, `All`, `Iif`) so the VM can evaluate subplans lazily and with correct scoping for `$this/$index/$total`.
- Standalone function calls compile with an implicit base of `$this` (rather than “empty”) to match FHIRPath behavior.

## VM Execution Model

The VM (`src/vm.rs`) is a stack machine:

- Each opcode consumes/produces `Collection`s.
- Binary operators delegate to `src/vm/operations.rs`.
- Functions dispatch through `src/vm/functions.rs` (split into `src/vm/functions/*` modules).

### Opcode Model (Mental Reference)

The VM stack holds `Collection`s (not single `Value`s). Most opcodes follow “push result collection”.

Core opcodes:

- `PushConst`: push a literal from the constant pool. `ValueData::Empty` is treated specially and becomes an *empty collection*.
- `LoadThis`, `LoadIndex`, `LoadTotal`: load `$this`, `$index`, `$total`.
- `Navigate(segment_id)`: field navigation (dot access).
- `Index(i)`: positional indexing (`[i]`).
- `CallBinary(impl_id)`: run a binary operator implementation (e.g. `Eq`, `Union`, `DivInt`, …).
- `CallFunction(func_id, argc)`: call a normal function with an explicit base (for method calls the base is the receiver collection).

Higher-order opcodes execute nested `Plan`s (“subplans”) with a derived `Context`:

- `Where(subplan_id)`: for each item in input, run predicate subplan with `$this=item`, `$index=index`, and retain matches.
- `Select(subplan_id)`: map each item via projection subplan and flatten.
- `Repeat(subplan_id)`: iterative projection with cycle detection.
- `Aggregate(subplan_id, init_subplan_id?)`: fold left with `$total` set and correct `$index` propagation.
- `Exists(subplan_id?)`: `exists()` optionally with predicate.
- `All(subplan_id)`: `all(predicate)` with spec behavior (`all({}) == true`).
- `Iif(true_plan, false_plan, else_plan?)`: lazy branch evaluation. Important: branch VMs preserve `$index` and `$total`.

If you’re debugging a behavioral difference, it’s often easiest to visualize the VM plan and then reason about stack effects.

### Navigation (`Navigate`)

`Navigate` is responsible for “dot access” across collections. Key behaviors:

- Navigating a field over a collection flattens results.
- Missing fields return empty.
- In strict semantic mode (`Context.strict == true`), unknown fields error *when the base collection was non-empty*.

### Choice Types

FHIR choice elements are represented as `[x]` in StructureDefinitions but appear as expanded properties in JSON (e.g. `valueQuantity`, `valueString`, …).

This implementation supports two modes:

- **Strict mode**: direct access to choice expansions like `valueQuantity` is rejected (encourages spec expressions like `value.ofType(Quantity)`).
- **Lenient mode**: the VM can still find JSON choice expansions at runtime when compile-time validation is not enforced.

This is why CLI behavior depends on whether it compiles with a base type and/or strict semantics.

## Operators and Semantics

`src/vm/operations.rs` implements the core operator semantics:

- arithmetic (`+`, `-`, `*`, `/`, `div`, `mod`)
- equality and equivalence (`=`, `!=`, `~`, `!~`)
- ordering (`<`, `<=`, `>`, `>=`)
- boolean logic and collection membership (`in`, `contains`)
- set-like behavior (`|` union deduplicates)

Some semantics are subtle:

- Many operators are defined on **singleton** inputs; otherwise they may return empty (or error) per spec rules.
- Temporal comparability depends on precision and timezone presence.
- Equivalence (`~`) includes special handling for strings (case/whitespace normalization), quantities (unit conversion + least-precise rounding), and complex types.

### Type Operations: `is`, `as`, `ofType`

Type operations are implemented across:

- compile-time parsing/type-op nodes (`src/parser.rs`, `src/hir.rs`)
- runtime checks (`src/vm.rs` for `TypeIs/TypeAs`, and `src/vm/functions/type_helpers.rs`)

Important behavior:

- Unqualified names can refer to **System** types (e.g. `Boolean`, `Integer`) or **FHIR** types depending on context.
- `is(T)` supports inheritance checks for FHIR types (e.g. `Age is Quantity`). Unknown types return `false` rather than erroring.
- `as(T)` enforces singleton input per spec and performs **exact** type matching. For multi-item type filtering, use `ofType(T)`.
- `ofType(T)` filters collections using **exact** type matching (no inheritance), which matches HL7 suite expectations for subtype-vs-supertype edge cases.
- The `as` **operator** (`expr as Type`) is intentionally lenient on multi-item collections for FHIR R4 search parameter compatibility.
- Type specifiers are validated against `FhirContext`; unknown types like `string1` produce execution errors in `as()`/`ofType()`, while `is()` gracefully returns `false`.

### Temporal Strings in FHIR JSON

FHIR JSON represents dates/dateTimes/times as strings. This engine keeps JSON strings as `ValueData::String` by default (no eager parsing).

To still support correct temporal comparisons in common cases (e.g. `Period.start <= Period.end`), `src/temporal_parse.rs` provides:

- `parse_temporal_pair()` which tries to interpret two strings as date/dateTime/time values
- lenient dateTime parsing with timezone handling

The comparison/equality operators consult this helper when comparing `String` vs `String`.

## Function System

### Registry

`src/functions.rs` provides a compile-time PHF map from function name → ID and metadata:

- `min_args`, `max_args`
- a conservative `return_type` (`TypeId::Unknown` for polymorphic functions)

### Dispatch

At runtime, `src/vm/functions.rs` dispatches by numeric `FunctionId` into modules:

- `vm/functions/existence.rs`: `empty`, `exists`, `all`, `allTrue`, `subsetOf`, …
- `vm/functions/filtering.rs`: `ofType`, `extension`, …
- `vm/functions/conversion.rs`: `toInteger`, `toDecimal`, `toString`, …
- `vm/functions/string.rs`, `math.rs`, `utility.rs`, …

Higher-order functions that require closures often compile to opcodes rather than “normal” functions, to preserve correct scope and laziness.

## `resolve()` and Reference Handling

`Engine` can be constructed with a custom `ResourceResolver` (`src/resolver.rs`), which is used by the `resolve()` function. This allows integration with:

- database-backed reference resolution
- bundle-contained resources
- custom lookup strategies

If no resolver is configured, `resolve()` returns empty (or errors, depending on strictness and function behavior).

## Feature Flags

From `crates/fhir-fhirpath/Cargo.toml`:

- Default features include `regex`, `base64`, `hex`, `html-escape` (enables parts of the string/function surface).
- `xml-support` adds XML evaluation helpers via the optional `fhir-format` dependency.

## Debugging and Visualization

The crate supports visualizing compiler IR stages via `src/visualize.rs`:

- AST / HIR / Plan renderers
- output formats: ASCII tree, Mermaid, DOT

The CLI subcommand `visualize` wraps this capability.

## Testing

This repo includes the HL7 R5 FHIRPath suite runner:

- Tests: `tests/hl7/test_hl7_suite.rs`
- Source suite: `fhir-test-cases/r5/fhirpath/tests-fhir-r5.xml`
- Current status: **996/1033 passing** (0 failures, 37 skips for unimplemented features)

Run the full suite:

```sh
cargo test -p ferrum-fhirpath --test test_hl7_suite -- --ignored --nocapture
```

Run a specific group:

```sh
HL7_TEST_GROUP=testPlus cargo test -p ferrum-fhirpath --test test_hl7_suite -- --ignored --nocapture
```

## Strengths

- **End-to-end pipeline**: full compiler + bytecode VM, not an AST interpreter.
- **Fast repeated evaluation**: `Plan` caching and a compact opcode set.
- **Good debuggability**: IR visualization (AST/HIR/Plan).
- **Practical JSON support**: runtime navigation works naturally over JSON objects/arrays.
- **Temporal correctness**: explicit precision and timezone presence enable spec-style comparability and boundary operations.
- **HIR/VM specializations**: dedicated opcodes for higher-order functions preserve correct `$this/$index/$total` scoping and laziness.

## Weaknesses / Trade-offs

- **Strict vs lenient split is nuanced**: compile-time base type validation and runtime strict semantics interact with JSON choice expansions; users must choose the right mode.
- **Not all FHIRPath features are implemented**: some spec functions and terminology-dependent operations may be missing or stubbed (terminology services are out of scope).
- **FHIR JSON temporals are strings**: temporal semantics for strings rely on heuristic parsing (`src/temporal_parse.rs`), which is pragmatic but not a full “typed JSON model”.
- **Type inference is best-effort**: the engine depends on `FhirContext` availability; with incomplete contexts, typing falls back to unknowns.
- **Complex type equivalence is hard**: deep equivalence across arbitrary FHIR datatypes can be expensive and subtle; current rules aim to match HL7 suite expectations but may need refinement for edge cases.

## Contributing Notes

- Start with `src/engine.rs` to follow the pipeline.
- Add/adjust functions by extending:
  - `src/functions.rs` (registry metadata)
  - `src/vm/functions.rs` and appropriate module under `src/vm/functions/`
- Use `visualize` output early when debugging parser precedence or HIR lowering.
