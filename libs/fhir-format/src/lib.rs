//! FHIR JSON ↔ XML conversion following the official HL7 mapping rules.
//!
//! Uses pre-computed type metadata from FHIR R4 StructureDefinitions to
//! correctly handle array cardinality and primitive type coercion during
//! XML → JSON conversion. Metadata is embedded at compile time from
//! `fhir_type_metadata.json` (generated via `zunder-cli gen-format-metadata`).

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::Writer;
use roxmltree::Document;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::LazyLock;
use thiserror::Error;

/// Pre-computed FHIR type metadata for determining array cardinality.
/// Structure: { type_name: { property_name: { "type": String, "multiple": bool } } }
static FHIR_TYPE_METADATA: LazyLock<HashMap<String, HashMap<String, PropMeta>>> =
    LazyLock::new(|| {
        let json = include_str!("fhir_type_metadata.json");
        let raw: HashMap<String, HashMap<String, Value>> =
            serde_json::from_str(json).expect("failed to parse embedded fhir_type_metadata.json");
        raw.into_iter()
            .map(|(type_name, props)| {
                let prop_map = props
                    .into_iter()
                    .map(|(prop_name, v)| {
                        let multiple = v
                            .get("multiple")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        let type_name = v
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("string")
                            .to_string();
                        (prop_name, PropMeta { type_name, multiple })
                    })
                    .collect();
                (type_name, prop_map)
            })
            .collect()
    });

#[derive(Debug)]
struct PropMeta {
    type_name: String,
    multiple: bool,
}

/// Look up property metadata for a given parent type and property name.
fn lookup_prop_meta<'a>(parent_type: Option<&str>, prop_name: &str) -> Option<&'a PropMeta> {
    let parent = parent_type?;
    FHIR_TYPE_METADATA
        .get(parent)
        .and_then(|props| props.get(prop_name))
}

const FHIR_NS: &str = "http://hl7.org/fhir";
const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("expected a JSON object for the resource")]
    ExpectedObject,
    #[error("missing resourceType property")]
    MissingResourceType,
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("XML parse error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("XML write error: {0}")]
    XmlWrite(#[from] quick_xml::Error),
}

/// Convert a FHIR JSON payload into its XML representation.
pub fn json_to_xml(input: &str) -> Result<String, FormatError> {
    let value: Value = serde_json::from_str(input)?;
    let obj = value.as_object().ok_or(FormatError::ExpectedObject)?;
    let resource_type = obj
        .get("resourceType")
        .and_then(Value::as_str)
        .ok_or(FormatError::MissingResourceType)?;

    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    let mut root = BytesStart::new(resource_type);
    root.push_attribute(("xmlns", FHIR_NS));
    writer.write_event(Event::Start(root.clone()))?;

    let mut meta = HashMap::new();
    for (k, v) in obj {
        if k.starts_with('_') {
            meta.insert(k.trim_start_matches('_').to_string(), v.clone());
        }
    }

    for (k, v) in obj {
        if k == "resourceType" || k.starts_with('_') {
            continue;
        }
        let meta_entry = meta.get(k);
        write_json_value(&mut writer, k, v, meta_entry)?;
    }

    // Handle metadata fields that don't have a corresponding value field
    // (e.g., _active with extensions but no active field)
    for (k, v) in &meta {
        if !obj.contains_key(k) {
            // This metadata has no corresponding value, write it as a primitive with no value
            write_json_value(&mut writer, k, &Value::Null, Some(v))?;
        }
    }

    writer.write_event(Event::End(BytesEnd::new(resource_type)))?;
    let bytes = writer.into_inner().into_inner();
    Ok(String::from_utf8(bytes)?)
}

/// Convert a FHIR XML payload into its JSON representation.
pub fn xml_to_json(input: &str) -> Result<String, FormatError> {
    let doc = Document::parse(input)?;
    let root = doc.root_element();

    let resource_type = root.tag_name().name().to_string();

    let mut map = Map::new();
    map.insert(
        "resourceType".to_string(),
        Value::String(resource_type.clone()),
    );

    let mut accumulator = Map::new();
    for child in root.children().filter(|n| n.is_element()) {
        process_xml_child(input, &mut accumulator, &child, Some(&resource_type))?;
    }

    map.extend(accumulator);
    let json = Value::Object(map);
    Ok(serde_json::to_string_pretty(&json)?)
}

fn write_json_value(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    name: &str,
    value: &Value,
    meta: Option<&Value>,
) -> Result<(), FormatError> {
    match value {
        Value::Array(items) => {
            let meta_array = meta.and_then(Value::as_array);
            for (idx, item) in items.iter().enumerate() {
                let item_meta = meta_array.and_then(|m| m.get(idx));
                write_json_value(writer, name, item, item_meta)?;
            }
        }
        Value::Object(obj) => write_complex(writer, name, obj)?,
        Value::Null => {}
        primitive => write_primitive(writer, name, primitive, meta)?,
    }
    Ok(())
}

fn write_complex(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    name: &str,
    obj: &Map<String, Value>,
) -> Result<(), FormatError> {
    let mut meta = HashMap::new();
    for (k, v) in obj {
        if k.starts_with('_') {
            meta.insert(k.trim_start_matches('_').to_string(), v.clone());
        }
    }

    let mut start = BytesStart::new(name);
    if let Some(Value::String(id)) = obj.get("id") {
        start.push_attribute(("id", id.as_str()));
    }

    writer.write_event(Event::Start(start))?;

    for (k, v) in obj {
        if k.starts_with('_') || k == "id" {
            continue;
        }
        let meta_entry = meta.get(k);
        write_json_value(writer, k, v, meta_entry)?;
    }

    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

fn write_primitive(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    name: &str,
    value: &Value,
    meta: Option<&Value>,
) -> Result<(), FormatError> {
    let mut elem = BytesStart::new(name);

    // Only add value attribute if the value is not null
    let has_value = !matches!(value, Value::Null);
    if has_value {
        elem.push_attribute(("value", primitive_to_string(value).as_str()));
    }

    let mut has_children = false;
    if let Some(Value::Object(m)) = meta {
        if let Some(Value::String(id)) = m.get("id") {
            elem.push_attribute(("id", id.as_str()));
        }
        if m.get("extension").is_some() {
            has_children = true;
        }
    }

    // If we have neither a value nor children, skip writing this element
    if !has_value && !has_children {
        return Ok(());
    }

    if has_children {
        writer.write_event(Event::Start(elem.clone()))?;
        if let Some(Value::Object(m)) = meta {
            if let Some(ext) = m.get("extension") {
                write_json_value(writer, "extension", ext, None)?;
            }
        }
        writer.write_event(Event::End(BytesEnd::new(name)))?;
    } else {
        writer.write_event(Event::Empty(elem))?;
    }
    Ok(())
}

fn primitive_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "".to_string(),
        other => other.to_string(),
    }
}

fn process_xml_child(
    source: &str,
    target: &mut Map<String, Value>,
    node: &roxmltree::Node,
    parent_type: Option<&str>,
) -> Result<(), FormatError> {
    let name = node.tag_name().name().to_string();

    // Look up metadata to determine if this property is an array and what its type is.
    let prop_meta = lookup_prop_meta(parent_type, &name);
    let force_array = prop_meta.map(|m| m.multiple).unwrap_or(false);
    let element_type = prop_meta.map(|m| m.type_name.as_str());

    let (value, meta) = xml_element_to_value(source, node, element_type)?;

    insert_json_property(target, &name, value, meta, force_array);
    Ok(())
}

fn xml_element_to_value(
    source: &str,
    node: &roxmltree::Node,
    element_type: Option<&str>,
) -> Result<(Value, Option<Value>), FormatError> {
    if node.tag_name().namespace().is_some_and(|ns| ns == XHTML_NS) {
        let snippet = &source[node.range()];
        return Ok((Value::String(snippet.to_string()), None));
    }

    let mut meta_map = Map::new();
    if let Some(id) = node.attribute("id") {
        meta_map.insert("id".to_string(), Value::String(id.to_string()));
    }

    if let Some(val) = node.attribute("value") {
        let mut extensions = Vec::new();
        for child in node.children().filter(|c| c.is_element()) {
            if child.tag_name().name() == "extension" {
                let (ext_val, _ext_meta) =
                    xml_element_to_value(source, &child, Some("Extension"))?;
                extensions.push(ext_val);
            }
        }
        if !extensions.is_empty() {
            meta_map.insert("extension".to_string(), Value::Array(extensions));
        }
        let prim = parse_primitive(val, element_type);
        let meta = if meta_map.is_empty() {
            None
        } else {
            Some(Value::Object(meta_map))
        };
        return Ok((prim, meta));
    }

    let mut obj = Map::new();
    if let Some(id) = node.attribute("id") {
        obj.insert("id".to_string(), Value::String(id.to_string()));
    }

    for child in node.children().filter(|c| c.is_element()) {
        process_xml_child(source, &mut obj, &child, element_type)?;
    }

    Ok((Value::Object(obj), None))
}

fn insert_json_property(
    map: &mut Map<String, Value>,
    name: &str,
    value: Value,
    meta: Option<Value>,
    force_array: bool,
) {
    let entry = map.entry(name.to_string());
    match entry {
        serde_json::map::Entry::Vacant(v) => {
            if force_array {
                v.insert(Value::Array(vec![value]));
            } else {
                v.insert(value);
            }
        }
        serde_json::map::Entry::Occupied(mut o) => match o.get_mut() {
            Value::Array(arr) => arr.push(value),
            existing => {
                let old = existing.take();
                *existing = Value::Array(vec![old, value]);
            }
        },
    }

    if meta.is_none() && !map.contains_key(&format!("_{}", name)) {
        return;
    }

    let meta_key = format!("_{}", name);
    let value_is_array = matches!(map.get(name), Some(Value::Array(_)));
    let value_count = match map.get(name) {
        Some(Value::Array(arr)) => arr.len(),
        Some(_) => 1,
        None => 0,
    };

    match map.entry(meta_key) {
        serde_json::map::Entry::Vacant(v) => {
            if let Some(m) = meta {
                if value_is_array {
                    let mut arr = Vec::new();
                    if value_count > 1 {
                        arr.resize(value_count - 1, Value::Null);
                    }
                    arr.push(m);
                    v.insert(Value::Array(arr));
                } else {
                    v.insert(m);
                }
            }
        }
        serde_json::map::Entry::Occupied(mut o) => match o.get_mut() {
            Value::Array(arr) => {
                if let Some(m) = meta {
                    if arr.len() + 1 < value_count {
                        arr.resize(value_count - 1, Value::Null);
                    }
                    arr.push(m);
                } else {
                    arr.push(Value::Null);
                }
            }
            existing => {
                if value_is_array {
                    let first = existing.take();
                    let mut arr = Vec::new();
                    arr.push(first);
                    if value_count > 1 {
                        arr.resize(value_count - 1, Value::Null);
                    }
                    if let Some(m) = meta {
                        arr.push(m);
                    } else {
                        arr.push(Value::Null);
                    }
                    *existing = Value::Array(arr);
                } else if let Some(m) = meta {
                    *existing = m;
                }
            }
        },
    }
}

/// FHIR types that map to JSON numbers.
const FHIR_NUMBER_TYPES: &[&str] = &[
    "integer",
    "positiveInt",
    "unsignedInt",
    "integer64",
];

/// FHIR types that map to JSON booleans.
const FHIR_BOOLEAN_TYPES: &[&str] = &["boolean"];

/// FHIR types that map to JSON numbers (decimal).
const FHIR_DECIMAL_TYPES: &[&str] = &["decimal"];

fn parse_primitive(input: &str, fhir_type: Option<&str>) -> Value {
    if let Some(ft) = fhir_type {
        if FHIR_BOOLEAN_TYPES.contains(&ft) {
            return match input {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => Value::String(input.to_string()),
            };
        }
        if FHIR_NUMBER_TYPES.contains(&ft) {
            if let Ok(int) = input.parse::<i64>() {
                return Value::Number(int.into());
            }
            return Value::String(input.to_string());
        }
        if FHIR_DECIMAL_TYPES.contains(&ft) {
            // FHIR decimals must preserve precision, so keep as string in JSON
            // unless it's a simple integer value
            if let Ok(n) = input.parse::<serde_json::Number>() {
                return Value::Number(n);
            }
            return Value::String(input.to_string());
        }
        // For all other known types (string, code, uri, etc.), keep as string
        return Value::String(input.to_string());
    }

    // No type info available — use heuristic (legacy behavior)
    match input {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => {
            if let Ok(int) = input.parse::<i64>() {
                Value::Number(int.into())
            } else {
                Value::String(input.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_to_xml_basic_patient() {
        let json = r#"
        {
            "resourceType": "Patient",
            "id": "pat-1",
            "active": true,
            "name": [
                { "family": "Everyman", "given": ["Adam"] }
            ]
        }
        "#;

        let xml = json_to_xml(json).expect("conversion failed");
        assert!(xml.contains("<Patient"));
        assert!(xml.contains(r#"<id value="pat-1"/>"#));
        assert!(xml.contains(r#"<active value="true"/>"#));
        assert!(xml.contains(r#"<family value="Everyman"/>"#));
    }

    #[test]
    fn xml_to_json_round_trip() {
        let xml = r#"
        <Patient xmlns="http://hl7.org/fhir">
            <id value="p1"/>
            <active value="true"/>
            <name>
                <family value="Everyman"/>
                <given value="Adam"/>
            </name>
        </Patient>
        "#;

        let json = xml_to_json(xml).expect("xml->json failed");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["resourceType"], "Patient");
        assert_eq!(value["id"], "p1");
        assert_eq!(value["active"], true);
        // name is an array field — even a single element must be wrapped
        assert!(value["name"].is_array(), "name should be an array");
        assert_eq!(value["name"][0]["family"], "Everyman");
        // given is also an array field
        assert!(value["name"][0]["given"].is_array(), "given should be an array");
        assert_eq!(value["name"][0]["given"][0], "Adam");
    }

    #[test]
    fn xml_to_json_single_element_array() {
        // StructureDefinition with a single differential element should produce an array
        let xml = r#"
        <StructureDefinition xmlns="http://hl7.org/fhir">
            <url value="http://example.org/fhir/StructureDefinition/test"/>
            <name value="Test"/>
            <status value="active"/>
            <kind value="resource"/>
            <abstract value="false"/>
            <type value="Patient"/>
            <differential>
                <element>
                    <path value="Patient"/>
                </element>
            </differential>
        </StructureDefinition>
        "#;

        let json = xml_to_json(xml).expect("xml->json failed");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["resourceType"], "StructureDefinition");
        // differential.element must be an array even with a single element
        assert!(
            value["differential"]["element"].is_array(),
            "differential.element should be an array, got: {}",
            value["differential"]["element"]
        );
        assert_eq!(value["differential"]["element"][0]["path"], "Patient");
    }

    #[test]
    fn xml_to_json_scalar_stays_scalar() {
        // Verify scalar fields are NOT wrapped in arrays
        let xml = r#"
        <Patient xmlns="http://hl7.org/fhir">
            <id value="p1"/>
            <active value="true"/>
            <birthDate value="1990-01-01"/>
        </Patient>
        "#;

        let json = xml_to_json(xml).expect("xml->json failed");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert!(!value["active"].is_array(), "active should be scalar");
        assert!(!value["birthDate"].is_array(), "birthDate should be scalar");
        assert!(!value["id"].is_array(), "id should be scalar");
    }

    #[test]
    fn primitive_metadata_survives_roundtrip() {
        let json = r#"
        {
            "resourceType": "Patient",
            "birthDate": "1974-12-25",
            "_birthDate": { "id": "bd1" }
        }
        "#;

        let xml = json_to_xml(json).unwrap();
        assert!(xml.contains("<birthDate"));
        assert!(xml.contains(r#"value="1974-12-25""#));
        assert!(xml.contains(r#"id="bd1""#));

        let back = xml_to_json(&xml).unwrap();
        let val: Value = serde_json::from_str(&back).unwrap();
        assert_eq!(val["birthDate"], "1974-12-25");
        assert_eq!(val["_birthDate"]["id"], "bd1");
    }
}
