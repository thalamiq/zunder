//! FHIRPath Value to JSON conversion
//!
//! This module provides conversion from FHIRPath `Value` types back to JSON
//! representation, complementing the existing `Value::from_json()` function.
//!
//! # Design
//!
//! Follows the trait-based extension pattern used in `visualize.rs`:
//! - Trait `ToJson` provides conversion capability
//! - Implemented on `Value` type
//! - Handles all FHIRPath value types including temporal values with precision
//!
//! # Example
//!
//! ```rust,ignore
//! use zunder_fhirpath::Value;
//! use zunder_fhirpath::conversion::ToJson;
//!
//! let value = Value::string("hello");
//! assert_eq!(value.to_json(), Some(serde_json::json!("hello")));
//! ```

use crate::value::{DatePrecision, DateTimePrecision, TimePrecision, Value, ValueData};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde_json::Value as JsonValue;

/// Trait for converting FHIRPath values to JSON
///
/// Implemented for `Value` to provide `.to_json()` method
pub trait ToJson {
    /// Convert to JSON representation
    ///
    /// Returns `None` for Empty values, otherwise returns JSON Value
    fn to_json(&self) -> Option<JsonValue>;
}

impl ToJson for Value {
    fn to_json(&self) -> Option<JsonValue> {
        zunder_fhirpath_value_to_json(self)
    }
}

/// Convert FHIRPath Value to JSON representation
///
/// This is the inverse of `Value::from_json()`, preserving:
/// - Temporal values with their precision formatting
/// - Quantity values as objects with value and unit
/// - Object structures with nested collections
///
/// # Returns
///
/// - `Some(JsonValue)` for all non-empty values
/// - `None` for `ValueData::Empty`
pub fn zunder_fhirpath_value_to_json(value: &Value) -> Option<JsonValue> {
    match value.data() {
        // OPTIMIZATION: Lazy JSON is already JSON - return it directly without conversion
        ValueData::LazyJson { .. } => value.data().resolved_json().cloned(),
        ValueData::String(s) => Some(JsonValue::String(s.to_string())),
        ValueData::Integer(i) => Some(serde_json::json!(i)),
        ValueData::Decimal(d) => d.to_f64().map(|f| serde_json::json!(f)),
        ValueData::Boolean(b) => Some(JsonValue::Bool(*b)),
        ValueData::Date { value, precision } => {
            Some(JsonValue::String(format_date_value(*value, *precision)))
        }
        ValueData::DateTime {
            value,
            precision,
            timezone_offset,
        } => Some(JsonValue::String(format_datetime_value(
            value,
            *precision,
            *timezone_offset,
        ))),
        ValueData::Time { value, precision } => {
            Some(JsonValue::String(format_time_value(*value, *precision)))
        }
        ValueData::Quantity { value, unit } => {
            let mut map = serde_json::Map::new();
            map.insert("value".to_string(), JsonValue::String(value.to_string()));
            map.insert("unit".to_string(), JsonValue::String(unit.to_string()));
            Some(JsonValue::Object(map))
        }
        ValueData::Object(obj_map) => {
            // Convert HashMap<Arc<str>, Collection> to serde_json::Map
            let mut json_map = serde_json::Map::new();
            for (key, collection) in obj_map.as_ref() {
                // Convert collection to JSON array
                let json_values: Vec<JsonValue> = collection
                    .iter()
                    .filter_map(zunder_fhirpath_value_to_json)
                    .collect();
                if !json_values.is_empty() {
                    json_map.insert(key.to_string(), JsonValue::Array(json_values));
                }
            }
            Some(JsonValue::Object(json_map))
        }
        ValueData::Empty => None,
    }
}

/// Format a date value with appropriate precision
///
/// # Precision Formatting
///
/// - `Year`: "YYYY"
/// - `Month`: "YYYY-MM"
/// - `Day`: "YYYY-MM-DD"
pub fn format_date_value(value: NaiveDate, precision: DatePrecision) -> String {
    match precision {
        DatePrecision::Year => value.format("%Y").to_string(),
        DatePrecision::Month => value.format("%Y-%m").to_string(),
        DatePrecision::Day => value.format("%Y-%m-%d").to_string(),
    }
}

/// Format a datetime value with timezone and precision
///
/// Handles timezone offsets and precision levels according to FHIR spec
pub fn format_datetime_value(
    value: &DateTime<Utc>,
    precision: DateTimePrecision,
    timezone_offset: Option<i32>,
) -> String {
    let format = match precision {
        DateTimePrecision::Year => "%Y",
        DateTimePrecision::Month => "%Y-%m",
        DateTimePrecision::Day => "%Y-%m-%d",
        DateTimePrecision::Hour => "%Y-%m-%dT%H",
        DateTimePrecision::Minute => "%Y-%m-%dT%H:%M",
        DateTimePrecision::Second => "%Y-%m-%dT%H:%M:%S",
        DateTimePrecision::Millisecond => "%Y-%m-%dT%H:%M:%S%.3f",
    };

    let (formatted, offset_suffix) = match timezone_offset {
        Some(offset_seconds)
            if matches!(
                precision,
                DateTimePrecision::Hour
                    | DateTimePrecision::Minute
                    | DateTimePrecision::Second
                    | DateTimePrecision::Millisecond
            ) =>
        {
            let offset = FixedOffset::east_opt(offset_seconds)
                .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
            let shifted = value.with_timezone(&offset);
            (
                shifted.format(format).to_string(),
                Some(format_offset(offset_seconds)),
            )
        }
        _ => (value.naive_utc().format(format).to_string(), None),
    };

    match offset_suffix {
        Some(suffix) => format!("{formatted}{suffix}"),
        None => formatted,
    }
}

/// Format a time value with precision
///
/// # Precision Formatting
///
/// - `Hour`: "HH"
/// - `Minute`: "HH:MM"
/// - `Second`: "HH:MM:SS"
/// - `Millisecond`: "HH:MM:SS.fff"
pub fn format_time_value(value: NaiveTime, precision: TimePrecision) -> String {
    match precision {
        TimePrecision::Hour => value.format("%H").to_string(),
        TimePrecision::Minute => value.format("%H:%M").to_string(),
        TimePrecision::Second => value.format("%H:%M:%S").to_string(),
        TimePrecision::Millisecond => value.format("%H:%M:%S%.3f").to_string(),
    }
}

/// Format a timezone offset in FHIR format
///
/// # Returns
///
/// - "Z" for UTC (offset = 0)
/// - "+HH:MM" for positive offsets
/// - "-HH:MM" for negative offsets
///
/// # Arguments
///
/// - `offset_seconds`: Timezone offset in seconds
pub fn format_offset(offset_seconds: i32) -> String {
    if offset_seconds == 0 {
        return "Z".to_string();
    }

    let sign = if offset_seconds >= 0 { '+' } else { '-' };
    let total_minutes = offset_seconds.abs() / 60;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    format!("{sign}{hours:02}:{minutes:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_conversion() {
        let value = Value::string("test");
        assert_eq!(value.to_json(), Some(serde_json::json!("test")));
    }

    #[test]
    fn test_integer_conversion() {
        let value = Value::integer(42);
        assert_eq!(value.to_json(), Some(serde_json::json!(42)));
    }

    #[test]
    fn test_boolean_conversion() {
        let value = Value::boolean(true);
        assert_eq!(value.to_json(), Some(serde_json::json!(true)));
    }

    #[test]
    fn test_empty_conversion() {
        let value = Value::empty();
        assert_eq!(value.to_json(), None);
    }

    #[test]
    fn test_offset_formatting() {
        assert_eq!(format_offset(0), "Z");
        assert_eq!(format_offset(3600), "+01:00");
        assert_eq!(format_offset(-3600), "-01:00");
        assert_eq!(format_offset(19800), "+05:30"); // India
        assert_eq!(format_offset(-28800), "-08:00"); // PST
    }
}
