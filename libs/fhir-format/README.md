# zunder-format

FHIR JSON ↔ XML conversion following the official HL7 mapping rules.

## Features

- **JSON → XML** (`json_to_xml`) — converts FHIR JSON resources to XML with proper namespace, primitive encoding, and metadata handling
- **XML → JSON** (`xml_to_json`) — converts FHIR XML resources to JSON with correct array cardinality and type coercion
- Pre-computed type metadata from FHIR R4 StructureDefinitions ensures single-element arrays are correctly wrapped (e.g. `"name": [{ ... }]` instead of `"name": { ... }`)
- Type-aware primitive parsing produces correct JSON types (boolean, integer, decimal, string) based on the FHIR element type

## Usage

```rust
use zunder_format::{json_to_xml, xml_to_json};

// JSON → XML
let xml = json_to_xml(r#"{"resourceType":"Patient","id":"p1","active":true}"#)?;

// XML → JSON
let json = xml_to_json(r#"<Patient xmlns="http://hl7.org/fhir"><id value="p1"/></Patient>"#)?;
```

## Type Metadata

The embedded `fhir_type_metadata.json` is generated from FHIR R4 core StructureDefinitions via the CLI:

```bash
cargo run -p zunder-cli -- gen-format-metadata --fhir-version R4 \
  --output libs/fhir-format/src/fhir_type_metadata.json
```

This file maps `(parent_type, property_name) → { type, multiple }` for all 699 FHIR R4 types. It is committed to the repo and embedded at compile time via `include_str!`.

Regenerate after FHIR version upgrades or if new types need support.

## Testing

```bash
cargo test -p zunder-format
```
