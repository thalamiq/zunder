//! Value representation for FHIRPath evaluation
//!
//! This module provides efficient, zero-copy value representation using Arc for cheap cloning.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use serde_json::Value as JsonValue;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// JSON navigation token for lazy JSON-backed values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JsonPathToken {
    Key(Arc<str>),
    Index(usize),
}

fn resolve_json_at<'a>(
    mut current: &'a JsonValue,
    path: &[JsonPathToken],
) -> Option<&'a JsonValue> {
    for token in path {
        match token {
            JsonPathToken::Key(key) => {
                current = current.as_object()?.get(key.as_ref())?;
            }
            JsonPathToken::Index(idx) => {
                current = current.as_array()?.get(*idx)?;
            }
        }
    }
    Some(current)
}

/// Time precision levels according to FHIRPath spec
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimePrecision {
    Hour,        // @T10
    Minute,      // @T10:30
    Second,      // @T10:30:00
    Millisecond, // @T10:30:00.000
}

impl TimePrecision {
    /// Compare precision levels (returns true if same, false if different)
    pub fn is_compatible_with(self, other: TimePrecision) -> bool {
        self == other
            || matches!(
                (self, other),
                (TimePrecision::Second, TimePrecision::Millisecond)
                    | (TimePrecision::Millisecond, TimePrecision::Second)
            )
    }
}

/// Date precision levels according to FHIRPath spec
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatePrecision {
    Year,  // @2014
    Month, // @2014-01
    Day,   // @2014-01-01
}

impl DatePrecision {
    pub fn is_compatible_with(self, other: DatePrecision) -> bool {
        self == other
    }
}

/// DateTime precision levels according to FHIRPath spec
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateTimePrecision {
    Year,        // @2015T
    Month,       // @2015-02T
    Day,         // @2015-02-04T
    Hour,        // @2015-02-04T14
    Minute,      // @2015-02-04T14:30
    Second,      // @2015-02-04T14:30:00
    Millisecond, // @2015-02-04T14:30:00.000
}

impl DateTimePrecision {
    /// Compare precision levels (returns true if same, false if different)
    pub fn is_compatible_with(self, other: DateTimePrecision) -> bool {
        self == other
            || matches!(
                (self, other),
                (DateTimePrecision::Second, DateTimePrecision::Millisecond)
                    | (DateTimePrecision::Millisecond, DateTimePrecision::Second)
            )
    }
}

/// A FHIRPath value - cheap to clone via Arc
#[derive(Clone, Debug)]
pub struct Value(Arc<ValueData>);

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if self.ptr_eq(other) {
            return true;
        }
        match (self.data(), other.data()) {
            (ValueData::Empty, ValueData::Empty) => true,
            (ValueData::Boolean(l), ValueData::Boolean(r)) => l == r,
            (ValueData::Integer(l), ValueData::Integer(r)) => l == r,
            (ValueData::Decimal(l), ValueData::Decimal(r)) => l == r,
            (ValueData::String(l), ValueData::String(r)) => l == r,
            (
                ValueData::Date {
                    value: lv,
                    precision: lp,
                },
                ValueData::Date {
                    value: rv,
                    precision: rp,
                },
            ) => lv == rv && lp == rp,
            (
                ValueData::DateTime {
                    value: lv,
                    precision: lp,
                    timezone_offset: lt,
                },
                ValueData::DateTime {
                    value: rv,
                    precision: rp,
                    timezone_offset: rt,
                },
            ) => lv == rv && lp == rp && lt == rt,
            (
                ValueData::Time {
                    value: lv,
                    precision: lp,
                },
                ValueData::Time {
                    value: rv,
                    precision: rp,
                },
            ) => lv == rv && lp == rp,
            (
                ValueData::Quantity {
                    value: lv,
                    unit: lu,
                },
                ValueData::Quantity {
                    value: rv,
                    unit: ru,
                },
            ) => lv == rv && lu == ru,
            (ValueData::Object(l), ValueData::Object(r)) => Arc::ptr_eq(l, r),
            (
                ValueData::LazyJson { root: lr, path: lp },
                ValueData::LazyJson { root: rr, path: rp },
            ) => Arc::ptr_eq(lr, rr) && lp == rp,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.data() {
            ValueData::Empty => {
                0u8.hash(state);
            }
            ValueData::Boolean(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            ValueData::Integer(i) => {
                2u8.hash(state);
                i.hash(state);
            }
            ValueData::Decimal(d) => {
                3u8.hash(state);
                // Hash decimal as canonical string to handle precision correctly
                d.to_string().hash(state);
            }
            ValueData::String(s) => {
                4u8.hash(state);
                s.hash(state);
            }
            ValueData::Date { value, precision } => {
                5u8.hash(state);
                value.hash(state);
                precision.hash(state);
            }
            ValueData::DateTime {
                value,
                precision,
                timezone_offset,
            } => {
                6u8.hash(state);
                value.hash(state);
                precision.hash(state);
                timezone_offset.hash(state);
            }
            ValueData::Time { value, precision } => {
                7u8.hash(state);
                value.hash(state);
                precision.hash(state);
            }
            ValueData::Quantity { value, unit } => {
                8u8.hash(state);
                // Hash decimal as string for consistency
                value.to_string().hash(state);
                unit.hash(state);
            }
            ValueData::Object(map) => {
                9u8.hash(state);
                // Hash the pointer for Object (same as PartialEq logic)
                Arc::as_ptr(map).hash(state);
            }
            ValueData::LazyJson { root, path } => {
                10u8.hash(state);
                // Hash the root pointer and path (same as PartialEq logic)
                Arc::as_ptr(root).hash(state);
                path.hash(state);
            }
        }
    }
}

impl Value {
    pub fn data(&self) -> &ValueData {
        &self.0
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn from_json(json: JsonValue) -> Self {
        Self::from_json_root(Arc::new(json))
    }

    pub fn from_json_root(root: Arc<JsonValue>) -> Self {
        Self::from_json_node(root.clone(), SmallVec::new(), root.as_ref())
    }

    /// Create a Value pointing at a specific sub-path within a JSON root.
    ///
    /// `keys` navigates through object keys, `index` optionally selects an array element.
    /// This is useful for creating FHIRPath context nodes pointing at specific elements
    /// within a resource (e.g., Patient.name[0]).
    pub fn from_json_at(root: Arc<JsonValue>, keys: &[&str], index: Option<usize>) -> Self {
        let mut path: SmallVec<[JsonPathToken; 4]> = SmallVec::new();

        // Build the path tokens and verify each segment exists
        {
            let mut current: &JsonValue = root.as_ref();

            for key in keys {
                match current.get(*key) {
                    Some(child) => {
                        path.push(JsonPathToken::Key(Arc::from(*key)));
                        current = child;
                    }
                    None => return Self::empty(),
                }
            }

            if let Some(idx) = index {
                match current.as_array().and_then(|arr| arr.get(idx)) {
                    Some(_) => {
                        path.push(JsonPathToken::Index(idx));
                    }
                    None => return Self::empty(),
                }
            }
        }

        // Now resolve the node via the path (borrow of root is released)
        let node_ref = resolve_json_at(root.as_ref(), &path);
        match node_ref {
            Some(node) => Self::from_json_node(root.clone(), path, node),
            None => Self::empty(),
        }
    }

    pub fn boolean(b: bool) -> Self {
        Self(Arc::new(ValueData::Boolean(b)))
    }

    pub fn integer(i: i64) -> Self {
        Self(Arc::new(ValueData::Integer(i)))
    }

    pub fn decimal(d: Decimal) -> Self {
        Self(Arc::new(ValueData::Decimal(d)))
    }

    pub fn string(s: impl Into<Arc<str>>) -> Self {
        Self(Arc::new(ValueData::String(s.into())))
    }

    pub fn empty() -> Self {
        Self(Arc::new(ValueData::Empty))
    }

    pub fn date(d: NaiveDate) -> Self {
        Self(Arc::new(ValueData::Date {
            value: d,
            precision: DatePrecision::Day,
        }))
    }

    pub fn date_with_precision(d: NaiveDate, precision: DatePrecision) -> Self {
        Self(Arc::new(ValueData::Date {
            value: d,
            precision,
        }))
    }

    pub fn datetime(dt: DateTime<Utc>) -> Self {
        Self(Arc::new(ValueData::DateTime {
            value: dt,
            precision: DateTimePrecision::Second,
            timezone_offset: Some(0), // Default to Z/UTC
        }))
    }

    pub fn datetime_with_precision(dt: DateTime<Utc>, precision: DateTimePrecision) -> Self {
        Self(Arc::new(ValueData::DateTime {
            value: dt,
            precision,
            timezone_offset: Some(0), // Default to Z/UTC
        }))
    }

    pub fn datetime_with_precision_and_offset(
        dt: DateTime<Utc>,
        precision: DateTimePrecision,
        offset_seconds: Option<i32>,
    ) -> Self {
        Self(Arc::new(ValueData::DateTime {
            value: dt,
            precision,
            timezone_offset: offset_seconds,
        }))
    }

    pub fn time(t: NaiveTime) -> Self {
        Self(Arc::new(ValueData::Time {
            value: t,
            precision: TimePrecision::Second,
        }))
    }

    pub fn time_with_precision(t: NaiveTime, precision: TimePrecision) -> Self {
        Self(Arc::new(ValueData::Time {
            value: t,
            precision,
        }))
    }

    pub fn quantity(value: Decimal, unit: Arc<str>) -> Self {
        Self(Arc::new(ValueData::Quantity { value, unit }))
    }

    pub fn object(map: HashMap<Arc<str>, Collection>) -> Self {
        Self(Arc::new(ValueData::Object(Arc::new(map))))
    }

    /// Create Value from JSON using eager conversion (for use in from_json_eager)
    fn from_json_eager_value(json: &JsonValue) -> Self {
        Self(Arc::new(ValueData::from_json_eager(json)))
    }

    pub(crate) fn from_json_node(
        root: Arc<JsonValue>,
        path: SmallVec<[JsonPathToken; 4]>,
        node: &JsonValue,
    ) -> Self {
        match node {
            JsonValue::Null => Self::empty(),
            JsonValue::Bool(b) => Self::boolean(*b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::integer(i)
                } else if let Some(f) = n.as_f64() {
                    Self::decimal(Decimal::from_f64_retain(f).unwrap_or_default())
                } else {
                    Self::empty()
                }
            }
            JsonValue::String(s) => Self::string(Arc::from(s.as_str())),
            JsonValue::Object(_) => Self(Arc::new(ValueData::LazyJson { root, path })),
            JsonValue::Array(_) => Self::empty(),
        }
    }

    /// Materialize a lazy value into an eagerly-evaluated structure.
    ///
    /// This forces conversion of LazyJson to Object. For non-lazy values, returns the same value.
    /// Use this when you need to ensure a value is fully materialized (e.g., for functions
    /// that iterate all fields of an object).
    pub fn materialize(&self) -> Self {
        match self.data() {
            ValueData::LazyJson { .. } => Self(Arc::new(self.0.materialize())),
            _ => self.clone(),
        }
    }
}

/// Internal value data representation
#[derive(Debug, Clone)]
pub enum ValueData {
    // Primitives (inline)
    Boolean(bool),
    Integer(i64),
    Decimal(Decimal),

    // Heap-allocated
    String(Arc<str>),
    Date {
        value: NaiveDate,
        precision: DatePrecision,
    },
    DateTime {
        value: DateTime<Utc>,
        precision: DateTimePrecision,
        /// Timezone offset in seconds east of UTC.
        /// - `None` means no timezone was specified (local/unknown offset).
        /// - `Some(0)` means `Z`/UTC.
        /// - `Some(n)` means a fixed offset `+/-HH:MM`.
        timezone_offset: Option<i32>,
    },
    Time {
        value: NaiveTime,
        precision: TimePrecision,
    },
    Quantity {
        value: Decimal,
        unit: Arc<str>,
    },

    // Structured (shared)
    Object(Arc<HashMap<Arc<str>, Collection>>),

    // Lazy JSON - defers conversion until field access (major performance optimization)
    /// References a node inside a shared JSON tree. Navigation extends `path` without cloning JSON.
    LazyJson {
        root: Arc<JsonValue>,
        path: SmallVec<[JsonPathToken; 4]>,
    },

    // Special
    Empty,
    // TypeInfo will be added later when types module is ready
}

impl ValueData {
    pub(crate) fn resolved_json(&self) -> Option<&JsonValue> {
        match self {
            ValueData::LazyJson { root, path } => resolve_json_at(root.as_ref(), path.as_slice()),
            _ => None,
        }
    }

    /// Get string value if this is a String
    pub fn as_string(&self) -> Option<Arc<str>> {
        match self {
            ValueData::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Force conversion of lazy JSON to eagerly-evaluated Object structure.
    ///
    /// This is used when we need the full object structure (e.g., for functions
    /// that iterate all keys), but most path navigation avoids this.
    pub(crate) fn materialize(&self) -> ValueData {
        match self {
            ValueData::LazyJson { .. } => self
                .resolved_json()
                .map(Self::from_json_eager)
                .unwrap_or(ValueData::Empty),
            other => other.clone(),
        }
    }

    /// Eagerly convert JSON to Object (old behavior) - only used when materializing
    fn from_json_eager(json: &JsonValue) -> Self {
        match json {
            JsonValue::Null => ValueData::Empty,
            JsonValue::Bool(b) => ValueData::Boolean(*b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    ValueData::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    ValueData::Decimal(Decimal::from_f64_retain(f).unwrap_or_default())
                } else {
                    ValueData::Empty
                }
            }
            JsonValue::String(s) => ValueData::String(Arc::from(s.as_str())),
            JsonValue::Array(_arr) => {
                // Arrays become empty - they're handled at Collection level
                ValueData::Empty
            }
            JsonValue::Object(obj) => {
                let mut map = HashMap::new();
                for (k, v) in obj {
                    if let JsonValue::Array(arr) = v {
                        let mut coll = Collection::empty();
                        for item in arr {
                            coll.push(Value::from_json_eager_value(item));
                        }
                        map.insert(Arc::from(k.as_str()), coll);
                    } else {
                        let mut coll = Collection::empty();
                        coll.push(Value::from_json_eager_value(v));
                        map.insert(Arc::from(k.as_str()), coll);
                    }
                }
                ValueData::Object(Arc::new(map))
            }
        }
    }
}

/// Threshold for switching from SmallVec to Arc<SmallVec> for cloning optimization.
/// Collections with more than this many items will use Arc to make cloning O(1).
const COLLECTION_ARC_THRESHOLD: usize = 4;

/// Collection optimized for singleton case (90% of FHIRPath collections are single-element)
///
/// For small collections (≤4 items), uses SmallVec directly.
/// For large collections (>4 items), wraps the vector in Arc to make cloning O(1).
#[derive(Clone, Debug)]
pub struct Collection {
    inner: CollectionInner,
}

#[derive(Clone, Debug)]
enum CollectionInner {
    /// Small collections stored directly (≤4 items)
    Small(SmallVec<[Value; 4]>),
    /// Large collections wrapped in Arc (>4 items)
    /// Cloning is O(1) - just increments the reference count
    Large(Arc<SmallVec<[Value; 4]>>),
}

impl Collection {
    pub fn empty() -> Self {
        Self {
            inner: CollectionInner::Small(SmallVec::new()),
        }
    }

    pub fn singleton(value: Value) -> Self {
        let mut inner = SmallVec::new();
        inner.push(value);
        Self {
            inner: CollectionInner::Small(inner),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let inner = SmallVec::with_capacity(capacity);
        if capacity > COLLECTION_ARC_THRESHOLD {
            Self {
                inner: CollectionInner::Large(Arc::new(inner)),
            }
        } else {
            Self {
                inner: CollectionInner::Small(inner),
            }
        }
    }

    /// Get a mutable reference to the underlying SmallVec.
    /// If the collection is Arc-wrapped, this will clone it to make it mutable.
    fn get_mut(&mut self) -> &mut SmallVec<[Value; 4]> {
        // If we have an Arc-wrapped collection, convert it to SmallVec first
        if let CollectionInner::Large(arc) = &self.inner {
            let vec = (**arc).clone();
            self.inner = CollectionInner::Small(vec);
        }

        // Now we can safely get a mutable reference
        match &mut self.inner {
            CollectionInner::Small(vec) => vec,
            CollectionInner::Large(_) => unreachable!(),
        }
    }

    /// Ensure the collection is in the appropriate representation based on its size.
    /// If it's large and currently Small, convert to Arc. If it's small and currently Arc, convert back.
    fn ensure_representation(&mut self) {
        let len = self.len();
        match &self.inner {
            CollectionInner::Small(vec) if len > COLLECTION_ARC_THRESHOLD => {
                // Convert to Arc-wrapped
                let vec = vec.clone();
                self.inner = CollectionInner::Large(Arc::new(vec));
            }
            CollectionInner::Large(arc) if len <= COLLECTION_ARC_THRESHOLD => {
                // Convert back to SmallVec (unlikely but possible if items are removed)
                let vec = (**arc).clone();
                self.inner = CollectionInner::Small(vec);
            }
            _ => {
                // Already in correct representation
            }
        }
    }

    pub fn push(&mut self, value: Value) {
        self.get_mut().push(value);
        self.ensure_representation();
    }

    pub fn iter(&self) -> impl Iterator<Item = &Value> {
        // Both SmallVec and Arc<SmallVec> can be converted to slices
        match &self.inner {
            CollectionInner::Small(vec) => vec.iter(),
            CollectionInner::Large(arc) => arc.iter(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.inner {
            CollectionInner::Small(vec) => vec.is_empty(),
            CollectionInner::Large(arc) => arc.is_empty(),
        }
    }

    pub fn len(&self) -> usize {
        match &self.inner {
            CollectionInner::Small(vec) => vec.len(),
            CollectionInner::Large(arc) => arc.len(),
        }
    }

    /// Get a value by index
    pub fn get(&self, index: usize) -> Option<&Value> {
        match &self.inner {
            CollectionInner::Small(vec) => vec.get(index),
            CollectionInner::Large(arc) => arc.get(index),
        }
    }

    pub fn as_boolean(&self) -> Result<bool> {
        if self.is_empty() {
            return Ok(false);
        }
        if self.len() > 1 {
            return Err(Error::TypeError("Expected singleton boolean".into()));
        }
        match self.get(0).unwrap().data() {
            ValueData::Boolean(b) => Ok(*b),
            _ => Err(Error::TypeError("Expected boolean value".into())),
        }
    }

    pub fn as_string(&self) -> Result<Arc<str>> {
        if self.is_empty() {
            return Err(Error::TypeError("Empty collection".into()));
        }
        if self.len() > 1 {
            return Err(Error::TypeError("Expected singleton string".into()));
        }
        match self.get(0).unwrap().data() {
            ValueData::String(s) => Ok(s.clone()),
            _ => Err(Error::TypeError("Expected string value".into())),
        }
    }

    pub fn as_integer(&self) -> Result<i64> {
        if self.is_empty() {
            return Err(Error::TypeError("Empty collection".into()));
        }
        if self.len() > 1 {
            return Err(Error::TypeError("Expected singleton integer".into()));
        }
        match self.get(0).unwrap().data() {
            ValueData::Integer(i) => Ok(*i),
            _ => Err(Error::TypeError("Expected integer value".into())),
        }
    }
}

use crate::error::{Error, Result};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn is_lazy_json(v: &Value) -> bool {
        matches!(v.data(), ValueData::LazyJson { .. })
    }

    #[test]
    fn test_from_json_at_object_key() {
        let resource = json!({
            "resourceType": "Patient",
            "active": true
        });
        let root = Arc::new(resource);
        let val = Value::from_json_at(root, &["active"], None);
        assert!(matches!(val.data(), ValueData::Boolean(true)));
    }

    #[test]
    fn test_from_json_at_array_item() {
        let resource = json!({
            "resourceType": "Patient",
            "name": [
                {"family": "Smith", "given": ["John"]},
                {"family": "Doe"}
            ]
        });
        let root = Arc::new(resource);

        // Access name[0] — should be a LazyJson object
        let val = Value::from_json_at(root.clone(), &["name"], Some(0));
        assert!(is_lazy_json(&val));

        // Access name[1]
        let val = Value::from_json_at(root.clone(), &["name"], Some(1));
        assert!(is_lazy_json(&val));

        // Access name[2] — out of bounds, should be empty
        let val = Value::from_json_at(root, &["name"], Some(2));
        assert!(matches!(val.data(), ValueData::Empty));
    }

    #[test]
    fn test_from_json_at_missing_key() {
        let resource = json!({"resourceType": "Patient"});
        let root = Arc::new(resource);
        let val = Value::from_json_at(root, &["nonexistent"], None);
        assert!(matches!(val.data(), ValueData::Empty));
    }

    #[test]
    fn test_from_json_at_nested_keys() {
        let resource = json!({
            "resourceType": "Patient",
            "meta": {"versionId": "1"}
        });
        let root = Arc::new(resource);
        let val = Value::from_json_at(root, &["meta", "versionId"], None);
        assert_eq!(val.data().as_string().unwrap().as_ref(), "1");
    }

    #[test]
    fn test_from_json_at_children_available() {
        // Verify that from_json_at creates a proper LazyJson value
        // that supports children() navigation (critical for ele-1 constraint)
        let resource = json!({
            "resourceType": "Patient",
            "name": [
                {"family": "Smith", "given": ["John"]}
            ]
        });
        let root = Arc::new(resource);
        let val = Value::from_json_at(root, &["name"], Some(0));
        // Should be a LazyJson-backed object
        assert!(is_lazy_json(&val));
        // Materializing should produce an Object with fields
        let materialized = val.materialize();
        assert!(matches!(materialized.data(), ValueData::Object { .. }));
    }
}
