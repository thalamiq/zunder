//! Binary operations for FHIRPath VM
//!
//! Implements all binary operators following FHIRPath specification semantics.

use crate::error::{Error, Result};
use crate::hir::HirBinaryOperator;
use crate::value::{Collection, Value, ValueData};
use chrono::{Duration, Months};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Days,
    Weeks,
    Months,
    Years,
    Hours,
    Minutes,
    Seconds,
    Milliseconds,
    Dimensionless,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitDimension {
    Mass,
    Length,
    Time,
    CalendarDuration,
    Dimensionless,
    Unknown,
}

/// Calendar to UCUM equivalence mapping
fn get_calendar_ucum_equivalent(unit: &str) -> Option<&'static str> {
    let u = unit.trim().to_ascii_lowercase();
    match u.as_str() {
        "year" | "years" => Some("a"),
        "month" | "months" => Some("mo"),
        "week" | "weeks" => Some("wk"),
        "day" | "days" => Some("d"),
        "hour" | "hours" => Some("h"),
        "minute" | "minutes" => Some("min"),
        "second" | "seconds" => Some("s"),
        "millisecond" | "milliseconds" => Some("ms"),
        _ => None,
    }
}

fn calendar_is_strict_equal_to_ucum(unit: &str) -> bool {
    matches!(
        get_calendar_ucum_equivalent(unit),
        Some("wk" | "d" | "h" | "min" | "s" | "ms")
    )
}

fn is_pure_time_dimension(dim: &zunder_ucum::DimensionVector) -> bool {
    dim.0[0] == 0
        && dim.0[1] == 0
        && dim.0[3] == 0
        && dim.0[4] == 0
        && dim.0[5] == 0
        && dim.0[6] == 0
        && dim.0[7] == 0
        && dim.0[2] != 0
}

fn try_ucum_compare(lv: &Decimal, lu: &str, rv: &Decimal, ru: &str) -> Option<std::cmp::Ordering> {
    zunder_ucum::compare_decimal_quantities(lv, lu, rv, ru).ok()
}

/// Get unit dimension for dimensional analysis
fn get_unit_dimension(unit: &str) -> UnitDimension {
    let u = unit.trim().to_ascii_lowercase();
    match u.as_str() {
        // Mass units
        "g" | "kg" | "mg" | "lb" | "lbs" | "[lb_av]" => UnitDimension::Mass,
        // Length units
        "m" | "cm" | "mm" | "km" | "in" | "[in_i]" | "ft" => UnitDimension::Length,
        // Time units (definite durations)
        "s" | "sec" | "second" | "seconds" | "min" | "minute" | "minutes" | "h" | "hour"
        | "hours" | "d" | "day" | "days" | "wk" | "week" | "weeks" | "ms" | "millisecond"
        | "milliseconds" => UnitDimension::Time,
        // Calendar durations
        "year" | "years" | "month" | "months" => UnitDimension::CalendarDuration,
        // Dimensionless
        "" | "1" => UnitDimension::Dimensionless,
        _ => UnitDimension::Unknown,
    }
}

/// Convert unit value to base unit (for dimensional equivalence)
/// Returns (base_value, base_unit_name)
fn convert_to_base_unit(value: &Decimal, unit: &str) -> Option<(Decimal, &'static str)> {
    let u = unit.trim().to_ascii_lowercase();
    match u.as_str() {
        // Mass: convert to grams
        "g" => Some((*value, "g")),
        "kg" => Some((*value * Decimal::from(1000), "g")),
        "mg" => Some((*value / Decimal::from(1000), "g")),
        "lb" | "lbs" => Decimal::from_str("453.592").ok().map(|f| (*value * f, "g")),
        "[lb_av]" => Decimal::from_str("453.59237")
            .ok()
            .map(|f| (*value * f, "g")),
        // Length: convert to meters
        "m" => Some((*value, "m")),
        "cm" => Decimal::from_str("0.01")
            .ok()
            .map(|factor| (*value * factor, "m")),
        "mm" => Decimal::from_str("0.001")
            .ok()
            .map(|factor| (*value * factor, "m")),
        "km" => Some((*value * Decimal::from(1000), "m")),
        "in" | "[in_i]" => Decimal::from_str("0.0254")
            .ok()
            .map(|factor| (*value * factor, "m")),
        "ft" => Decimal::from_str("0.3048")
            .ok()
            .map(|factor| (*value * factor, "m")),
        // Time: convert to seconds
        "s" | "sec" | "second" | "seconds" => Some((*value, "s")),
        "ms" | "millisecond" | "milliseconds" => Some((*value / Decimal::from(1000), "s")),
        "min" | "minute" | "minutes" => Some((*value * Decimal::from(60), "s")),
        "h" | "hour" | "hours" => Some((*value * Decimal::from(3600), "s")),
        "d" | "day" | "days" => Some((*value * Decimal::from(86400), "s")),
        "wk" | "week" | "weeks" => Some((*value * Decimal::from(604800), "s")),
        // Dimensionless
        "" | "1" => Some((*value, "1")),
        _ => None,
    }
}

/// Check if two units are equivalent (same unit or calendar/UCUM equivalent)
fn units_equivalent(unit1: &str, unit2: &str) -> bool {
    let u1 = unit1.trim().to_ascii_lowercase();
    let u2 = unit2.trim().to_ascii_lowercase();

    // Exact match
    if u1 == u2 {
        return true;
    }

    // Special case: lbs and [lb_av] are equivalent (both represent pounds)
    if (u1 == "lbs" && u2 == "[lb_av]") || (u1 == "[lb_av]" && u2 == "lbs") {
        return true;
    }
    if (u1 == "lb" && u2 == "[lb_av]") || (u1 == "[lb_av]" && u2 == "lb") {
        return true;
    }

    // Check calendar/UCUM equivalence
    if let Some(ucum1) = get_calendar_ucum_equivalent(&u1) {
        if ucum1 == u2 {
            return true;
        }
    }
    if let Some(ucum2) = get_calendar_ucum_equivalent(&u2) {
        if ucum2 == u1 {
            return true;
        }
    }

    // Check dimensional equivalence (same dimension, convert to base and compare)
    let dim1 = get_unit_dimension(&u1);
    let dim2 = get_unit_dimension(&u2);

    if dim1 == dim2 && dim1 != UnitDimension::Unknown && dim1 != UnitDimension::CalendarDuration {
        // Same dimension - for equivalence, we need to check if they represent the same base value
        // This is handled at the value level, so we just return true if dimensions match
        // The actual value comparison will be done by converting both to base units
        return true;
    }

    false
}

fn normalize_unit(unit: &str) -> UnitKind {
    let u = unit.trim().to_ascii_lowercase();
    match u.as_str() {
        "" | "1" => UnitKind::Dimensionless,
        "d" | "day" | "days" => UnitKind::Days,
        "wk" | "week" | "weeks" => UnitKind::Weeks,
        "mo" | "month" | "months" => UnitKind::Months,
        "a" | "year" | "years" => UnitKind::Years,
        "h" | "hour" | "hours" => UnitKind::Hours,
        "min" | "minute" | "minutes" => UnitKind::Minutes,
        "s" | "sec" | "second" | "seconds" => UnitKind::Seconds,
        "ms" | "millisecond" | "milliseconds" => UnitKind::Milliseconds,
        _ => UnitKind::Unknown,
    }
}

enum DurationOrMonths {
    Duration(Duration),
    Months(i32),
}

fn quantity_to_duration(value: &Decimal, unit: &str) -> Result<DurationOrMonths> {
    let kind = normalize_unit(unit);
    match kind {
        UnitKind::Milliseconds => {
            let millis = value.to_i64().ok_or_else(|| {
                Error::InvalidOperation("Quantity milliseconds out of range".into())
            })?;
            Ok(DurationOrMonths::Duration(Duration::milliseconds(millis)))
        }
        UnitKind::Seconds => {
            // Handle fractional seconds by converting to milliseconds
            // Decimal value * 1000 gives milliseconds
            let millis = (*value * Decimal::from(1000))
                .to_i64()
                .ok_or_else(|| Error::InvalidOperation("Quantity seconds out of range".into()))?;
            Ok(DurationOrMonths::Duration(Duration::milliseconds(millis)))
        }
        UnitKind::Minutes => {
            let mins = value
                .to_i64()
                .ok_or_else(|| Error::InvalidOperation("Quantity minutes out of range".into()))?;
            Ok(DurationOrMonths::Duration(Duration::minutes(mins)))
        }
        UnitKind::Hours => {
            let hrs = value
                .to_i64()
                .ok_or_else(|| Error::InvalidOperation("Quantity hours out of range".into()))?;
            Ok(DurationOrMonths::Duration(Duration::hours(hrs)))
        }
        UnitKind::Days => {
            let days = value
                .to_i64()
                .ok_or_else(|| Error::InvalidOperation("Quantity days out of range".into()))?;
            Ok(DurationOrMonths::Duration(Duration::days(days)))
        }
        UnitKind::Weeks => {
            let weeks = value
                .to_i64()
                .ok_or_else(|| Error::InvalidOperation("Quantity weeks out of range".into()))?;
            Ok(DurationOrMonths::Duration(Duration::days(weeks * 7)))
        }
        UnitKind::Months => {
            let months = value
                .to_i32()
                .ok_or_else(|| Error::InvalidOperation("Quantity months out of range".into()))?;
            Ok(DurationOrMonths::Months(months))
        }
        UnitKind::Years => {
            let years = value
                .to_i32()
                .ok_or_else(|| Error::InvalidOperation("Quantity years out of range".into()))?;
            Ok(DurationOrMonths::Months(years * 12))
        }
        _ => Err(Error::InvalidOperation(
            "Unsupported quantity unit for temporal arithmetic".into(),
        )),
    }
}

/// Execute a binary operation
pub fn execute_binary_op(
    op: HirBinaryOperator,
    left: Collection,
    right: Collection,
) -> Result<Collection> {
    match op {
        // Arithmetic
        HirBinaryOperator::Add => add(left, right),
        HirBinaryOperator::Sub => subtract(left, right),
        HirBinaryOperator::Mul => multiply(left, right),
        HirBinaryOperator::Div => divide(left, right),
        HirBinaryOperator::DivInt => divide_int(left, right),
        HirBinaryOperator::Mod => modulo(left, right),

        // Comparison
        HirBinaryOperator::Eq => equals(left, right),
        HirBinaryOperator::Ne => not_equals(left, right),
        HirBinaryOperator::Equivalent => equivalent(left, right),
        HirBinaryOperator::NotEquivalent => not_equivalent(left, right),
        HirBinaryOperator::Lt => less_than(left, right),
        HirBinaryOperator::Le => less_or_equal(left, right),
        HirBinaryOperator::Gt => greater_than(left, right),
        HirBinaryOperator::Ge => greater_or_equal(left, right),

        // Boolean
        HirBinaryOperator::And => boolean_and(left, right),
        HirBinaryOperator::Or => boolean_or(left, right),
        HirBinaryOperator::Xor => boolean_xor(left, right),
        HirBinaryOperator::Implies => boolean_implies(left, right),

        // Collection
        HirBinaryOperator::Union => union(left, right),
        HirBinaryOperator::In => membership_in(left, right),
        HirBinaryOperator::Contains => membership_contains(left, right),

        // String
        HirBinaryOperator::Concat => concatenate(left, right),
    }
}

// ============================================
// Arithmetic Operations
// ============================================

fn add(left: Collection, right: Collection) -> Result<Collection> {
    // If either operand is empty, return empty
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    // FHIRPath addition requires singleton collections
    // If not singleton, return empty per FHIRPath spec
    if left.len() != 1 || right.len() != 1 {
        return Ok(Collection::empty());
    }

    let left_val = &left.iter().next().unwrap().materialize();
    let right_val = &right.iter().next().unwrap().materialize();

    match (left_val.data(), right_val.data()) {
        // String concatenation
        (ValueData::String(l), ValueData::String(r)) => {
            let result = format!("{}{}", l.as_ref(), r.as_ref());
            Ok(Collection::singleton(Value::string(result)))
        }
        // Numeric addition
        (ValueData::Integer(l), ValueData::Integer(r)) => {
            match l.checked_add(*r) {
                Some(sum) => Ok(Collection::singleton(Value::integer(sum))),
                None => Ok(Collection::empty()), // Overflow results in empty collection per FHIRPath spec
            }
        }
        (ValueData::Integer(l), ValueData::Decimal(r)) => Ok(Collection::singleton(
            Value::decimal(Decimal::from(*l) + *r),
        )),
        (ValueData::Decimal(l), ValueData::Integer(r)) => Ok(Collection::singleton(
            Value::decimal(*l + Decimal::from(*r)),
        )),
        (ValueData::Decimal(l), ValueData::Decimal(r)) => {
            Ok(Collection::singleton(Value::decimal(*l + *r)))
        }
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            let lu = lu.as_ref().trim();
            let ru = ru.as_ref().trim();

            // Calendar duration keywords are only strictly equal to UCUM for seconds/milliseconds.
            let lu_eff = if let Some(code) = get_calendar_ucum_equivalent(lu) {
                if calendar_is_strict_equal_to_ucum(lu) {
                    code
                } else {
                    return Ok(Collection::empty());
                }
            } else {
                lu
            };
            let ru_eff = if let Some(code) = get_calendar_ucum_equivalent(ru) {
                if calendar_is_strict_equal_to_ucum(ru) {
                    code
                } else {
                    return Ok(Collection::empty());
                }
            } else {
                ru
            };

            let (l_unit, r_unit) =
                match (zunder_ucum::Unit::parse(lu_eff), zunder_ucum::Unit::parse(ru_eff)) {
                    (Ok(lu), Ok(ru)) => (lu, ru),
                    _ => return Ok(Collection::empty()),
                };

            if l_unit.dimensions != r_unit.dimensions {
                return Ok(Collection::empty());
            }

            let (
                zunder_ucum::UnitKind::Multiplicative { factor: lf },
                zunder_ucum::UnitKind::Multiplicative { factor: rf },
            ) = (&l_unit.kind, &r_unit.kind)
            else {
                return Ok(Collection::empty());
            };

            let target = match lf.cmp(rf) {
                std::cmp::Ordering::Greater => ru_eff,
                _ => lu_eff,
            };

            let lv_t = if target == lu_eff {
                *lv
            } else {
                match zunder_ucum::convert_decimal(*lv, lu_eff, target) {
                    Ok(v) => v,
                    Err(_) => return Ok(Collection::empty()),
                }
            };
            let rv_t = if target == ru_eff {
                *rv
            } else {
                match zunder_ucum::convert_decimal(*rv, ru_eff, target) {
                    Ok(v) => v,
                    Err(_) => return Ok(Collection::empty()),
                }
            };

            Ok(Collection::singleton(Value::quantity(
                lv_t + rv_t,
                Arc::from(target),
            )))
        }
        // Temporal arithmetic with quantities
        (
            ValueData::Date {
                value: d,
                precision: date_prec,
            },
            ValueData::Quantity { value, unit },
        )
        | (
            ValueData::Quantity { value, unit },
            ValueData::Date {
                value: d,
                precision: date_prec,
            },
        ) => {
            // Check for invalid UCUM units per FHIRPath spec
            // 'mo' and 'a' are calendar durations and should be rejected
            if unit.as_ref() == "mo" || unit.as_ref() == "a" {
                return Err(Error::InvalidOperation(format!(
                    "Invalid UCUM unit '{}' for date arithmetic - use calendar duration units instead",
                    unit.as_ref()
                )));
            }

            let unit_norm = normalize_unit(unit.as_ref());
            match unit_norm {
                UnitKind::Days => {
                    let days = value.to_i64().unwrap_or(0);
                    let duration = chrono::Duration::days(days);
                    match d.checked_add_signed(duration) {
                        Some(shifted) => Ok(Collection::singleton(Value::date_with_precision(
                            shifted, *date_prec,
                        ))),
                        None => Err(Error::InvalidOperation(
                            "Date arithmetic resulted in out of range date".into(),
                        )),
                    }
                }
                UnitKind::Weeks => {
                    let days = value * Decimal::from(7);
                    let days_i = days.to_i64().unwrap_or(0);
                    let duration = chrono::Duration::days(days_i);
                    match d.checked_add_signed(duration) {
                        Some(shifted) => Ok(Collection::singleton(Value::date_with_precision(
                            shifted, *date_prec,
                        ))),
                        None => Err(Error::InvalidOperation(
                            "Date arithmetic resulted in out of range date".into(),
                        )),
                    }
                }
                UnitKind::Months => {
                    if let Some(months_i) = value.to_i32() {
                        // Handle negative months by converting to subtraction
                        let shifted = if months_i >= 0 {
                            d.checked_add_months(Months::new(months_i as u32))
                        } else {
                            d.checked_sub_months(Months::new((-months_i) as u32))
                        };

                        match shifted {
                            Some(date) => Ok(Collection::singleton(Value::date_with_precision(
                                date, *date_prec,
                            ))),
                            None => Err(Error::InvalidOperation(
                                "Date arithmetic resulted in out of range date".into(),
                            )),
                        }
                    } else {
                        Err(Error::InvalidOperation(
                            "Date + fractional months not supported".into(),
                        ))
                    }
                }
                UnitKind::Years => {
                    if let Some(years_i) = value.to_i32() {
                        let months = years_i * 12;
                        // Handle negative years by converting to subtraction
                        let shifted = if months >= 0 {
                            d.checked_add_months(Months::new(months as u32))
                        } else {
                            d.checked_sub_months(Months::new((-months) as u32))
                        };

                        match shifted {
                            Some(date) => Ok(Collection::singleton(Value::date_with_precision(
                                date, *date_prec,
                            ))),
                            None => Err(Error::InvalidOperation(
                                "Date arithmetic resulted in out of range date".into(),
                            )),
                        }
                    } else {
                        Err(Error::InvalidOperation(
                            "Date + fractional years not supported".into(),
                        ))
                    }
                }
                _ => Err(Error::InvalidOperation(
                    "Date arithmetic requires day/week/month/year units".into(),
                )),
            }
        }
        (
            ValueData::DateTime {
                value: dt,
                precision: prec,
                timezone_offset,
            },
            ValueData::Quantity { value, unit },
        )
        | (
            ValueData::Quantity { value, unit },
            ValueData::DateTime {
                value: dt,
                precision: prec,
                timezone_offset,
            },
        ) => {
            // Check for invalid UCUM units per FHIRPath spec
            if unit.as_ref() == "mo" || unit.as_ref() == "a" {
                return Err(Error::InvalidOperation(format!(
                    "Invalid UCUM unit '{}' for datetime arithmetic - use calendar duration units instead",
                    unit.as_ref()
                )));
            }

            match quantity_to_duration(value, unit.as_ref()) {
                Ok(DurationOrMonths::Duration(dur)) => {
                    // Use checked_add for Duration to handle overflow
                    match dt.checked_add_signed(dur) {
                        Some(result) => Ok(Collection::singleton(
                            Value::datetime_with_precision_and_offset(
                                result,
                                *prec,
                                *timezone_offset,
                            ),
                        )),
                        None => Err(Error::InvalidOperation(
                            "DateTime arithmetic resulted in out of range datetime".into(),
                        )),
                    }
                }
                Ok(DurationOrMonths::Months(months)) => {
                    // Handle negative months by converting to subtraction
                    let shifted = if months >= 0 {
                        dt.checked_add_months(Months::new(months as u32))
                    } else {
                        dt.checked_sub_months(Months::new((-months) as u32))
                    };

                    match shifted {
                        Some(datetime) => Ok(Collection::singleton(
                            Value::datetime_with_precision_and_offset(
                                datetime,
                                *prec,
                                *timezone_offset,
                            ),
                        )),
                        None => Err(Error::InvalidOperation(
                            "DateTime arithmetic resulted in out of range datetime".into(),
                        )),
                    }
                }
                Err(e) => Err(e),
            }
        }
        (
            ValueData::Time {
                value: t,
                precision: prec,
            },
            ValueData::Quantity { value, unit },
        )
        | (
            ValueData::Quantity { value, unit },
            ValueData::Time {
                value: t,
                precision: prec,
            },
        ) => match quantity_to_duration(value, unit.as_ref()) {
            Ok(DurationOrMonths::Duration(dur)) => {
                let base = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
                    .unwrap()
                    .and_time(*t);
                match base.checked_add_signed(dur) {
                    Some(shifted) => Ok(Collection::singleton(Value::time_with_precision(
                        shifted.time(),
                        *prec,
                    ))),
                    None => Err(Error::InvalidOperation(
                        "Time arithmetic resulted in out of range time".into(),
                    )),
                }
            }
            _ => Err(Error::InvalidOperation(
                "Time arithmetic requires time-based units".into(),
            )),
        },
        // Date/Time + non-quantity number is an error
        (ValueData::Date { .. }, ValueData::Integer(_))
        | (ValueData::Integer(_), ValueData::Date { .. })
        | (ValueData::Date { .. }, ValueData::Decimal(_))
        | (ValueData::Decimal(_), ValueData::Date { .. })
        | (
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
            ValueData::Integer(_),
        )
        | (
            ValueData::Integer(_),
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
        )
        | (
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
            ValueData::Decimal(_),
        )
        | (
            ValueData::Decimal(_),
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
        )
        | (
            ValueData::Time {
                value: _,
                precision: _,
            },
            ValueData::Integer(_),
        )
        | (
            ValueData::Integer(_),
            ValueData::Time {
                value: _,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: _,
                precision: _,
            },
            ValueData::Decimal(_),
        )
        | (
            ValueData::Decimal(_),
            ValueData::Time {
                value: _,
                precision: _,
            },
        ) => Err(Error::InvalidOperation(
            "Date/Time arithmetic requires quantity with unit".into(),
        )),
        _ => {
            // Type mismatch - return empty collection per FHIRPath spec
            Ok(Collection::empty())
        }
    }
}

fn subtract(left: Collection, right: Collection) -> Result<Collection> {
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    // FHIRPath subtraction requires singleton collections
    // If not singleton, return empty per FHIRPath spec
    if left.len() != 1 || right.len() != 1 {
        return Ok(Collection::empty());
    }

    let left_val = &left.iter().next().unwrap().materialize();
    let right_val = &right.iter().next().unwrap().materialize();

    match (left_val.data(), right_val.data()) {
        (ValueData::Integer(l), ValueData::Integer(r)) => {
            match l.checked_sub(*r) {
                Some(diff) => Ok(Collection::singleton(Value::integer(diff))),
                None => Ok(Collection::empty()), // Overflow results in empty collection per FHIRPath spec
            }
        }
        (ValueData::Integer(l), ValueData::Decimal(r)) => Ok(Collection::singleton(
            Value::decimal(Decimal::from(*l) - *r),
        )),
        (ValueData::Decimal(l), ValueData::Integer(r)) => Ok(Collection::singleton(
            Value::decimal(*l - Decimal::from(*r)),
        )),
        (ValueData::Decimal(l), ValueData::Decimal(r)) => {
            Ok(Collection::singleton(Value::decimal(*l - *r)))
        }
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            let lu = lu.as_ref().trim();
            let ru = ru.as_ref().trim();

            let lu_eff = if let Some(code) = get_calendar_ucum_equivalent(lu) {
                if calendar_is_strict_equal_to_ucum(lu) {
                    code
                } else {
                    return Ok(Collection::empty());
                }
            } else {
                lu
            };
            let ru_eff = if let Some(code) = get_calendar_ucum_equivalent(ru) {
                if calendar_is_strict_equal_to_ucum(ru) {
                    code
                } else {
                    return Ok(Collection::empty());
                }
            } else {
                ru
            };

            let (l_unit, r_unit) =
                match (zunder_ucum::Unit::parse(lu_eff), zunder_ucum::Unit::parse(ru_eff)) {
                    (Ok(lu), Ok(ru)) => (lu, ru),
                    _ => return Ok(Collection::empty()),
                };

            if l_unit.dimensions != r_unit.dimensions {
                return Ok(Collection::empty());
            }

            let (
                zunder_ucum::UnitKind::Multiplicative { factor: lf },
                zunder_ucum::UnitKind::Multiplicative { factor: rf },
            ) = (&l_unit.kind, &r_unit.kind)
            else {
                return Ok(Collection::empty());
            };

            let target = match lf.cmp(rf) {
                std::cmp::Ordering::Greater => ru_eff,
                _ => lu_eff,
            };

            let lv_t = if target == lu_eff {
                *lv
            } else {
                match zunder_ucum::convert_decimal(*lv, lu_eff, target) {
                    Ok(v) => v,
                    Err(_) => return Ok(Collection::empty()),
                }
            };
            let rv_t = if target == ru_eff {
                *rv
            } else {
                match zunder_ucum::convert_decimal(*rv, ru_eff, target) {
                    Ok(v) => v,
                    Err(_) => return Ok(Collection::empty()),
                }
            };

            Ok(Collection::singleton(Value::quantity(
                lv_t - rv_t,
                Arc::from(target),
            )))
        }
        (
            ValueData::Date {
                value: d,
                precision: date_prec,
            },
            ValueData::Quantity { value, unit },
        ) => add(
            Collection::singleton(Value::date_with_precision(*d, *date_prec)),
            Collection::singleton(Value::quantity(-(*value), unit.clone())),
        ),
        (
            ValueData::DateTime {
                value: dt,
                precision: prec,
                timezone_offset,
            },
            ValueData::Quantity { value, unit },
        ) => add(
            Collection::singleton(Value::datetime_with_precision_and_offset(
                *dt,
                *prec,
                *timezone_offset,
            )),
            Collection::singleton(Value::quantity(-(*value), unit.clone())),
        ),
        (
            ValueData::Time {
                value: t,
                precision: _,
            },
            ValueData::Quantity { value, unit },
        ) => add(
            Collection::singleton(Value::time(*t)),
            Collection::singleton(Value::quantity(-(*value), unit.clone())),
        ),
        _ => Err(Error::TypeError(
            "Subtraction requires numeric types".into(),
        )),
    }
}

fn multiply(left: Collection, right: Collection) -> Result<Collection> {
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    // FHIRPath multiplication requires singleton collections
    // If not singleton, return empty per FHIRPath spec
    if left.len() != 1 || right.len() != 1 {
        return Ok(Collection::empty());
    }

    let left_val = &left.iter().next().unwrap().materialize();
    let right_val = &right.iter().next().unwrap().materialize();

    match (left_val.data(), right_val.data()) {
        (ValueData::Integer(l), ValueData::Integer(r)) => {
            match l.checked_mul(*r) {
                Some(product) => Ok(Collection::singleton(Value::integer(product))),
                None => Ok(Collection::empty()), // Overflow results in empty collection per FHIRPath spec
            }
        }
        (ValueData::Integer(l), ValueData::Decimal(r)) => Ok(Collection::singleton(
            Value::decimal(Decimal::from(*l) * *r),
        )),
        (ValueData::Decimal(l), ValueData::Integer(r)) => Ok(Collection::singleton(
            Value::decimal(*l * Decimal::from(*r)),
        )),
        (ValueData::Decimal(l), ValueData::Decimal(r)) => {
            Ok(Collection::singleton(Value::decimal(*l * *r)))
        }
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            let lu = lu.as_ref().trim();
            let ru = ru.as_ref().trim();

            let lu_eff = if let Some(code) = get_calendar_ucum_equivalent(lu) {
                if calendar_is_strict_equal_to_ucum(lu) {
                    code
                } else {
                    return Ok(Collection::empty());
                }
            } else {
                lu
            };
            let ru_eff = if let Some(code) = get_calendar_ucum_equivalent(ru) {
                if calendar_is_strict_equal_to_ucum(ru) {
                    code
                } else {
                    return Ok(Collection::empty());
                }
            } else {
                ru
            };

            let (l_unit, r_unit) =
                match (zunder_ucum::Unit::parse(lu_eff), zunder_ucum::Unit::parse(ru_eff)) {
                    (Ok(lu), Ok(ru)) => (lu, ru),
                    _ => return Ok(Collection::empty()),
                };
            if matches!(l_unit.kind, zunder_ucum::UnitKind::NonLinear)
                || matches!(r_unit.kind, zunder_ucum::UnitKind::NonLinear)
            {
                return Ok(Collection::empty());
            }

            if l_unit.dimensions == zunder_ucum::DimensionVector::ZERO {
                return Ok(Collection::singleton(Value::quantity(
                    *lv * *rv,
                    Arc::from(ru_eff),
                )));
            }
            if r_unit.dimensions == zunder_ucum::DimensionVector::ZERO {
                return Ok(Collection::singleton(Value::quantity(
                    *lv * *rv,
                    Arc::from(lu_eff),
                )));
            }

            // Same dimension: normalize to the most granular unit and square it (e.g., m * cm -> cm2).
            if l_unit.dimensions == r_unit.dimensions {
                let (
                    zunder_ucum::UnitKind::Multiplicative { factor: lf },
                    zunder_ucum::UnitKind::Multiplicative { factor: rf },
                ) = (&l_unit.kind, &r_unit.kind)
                else {
                    return Ok(Collection::empty());
                };

                let target = match lf.cmp(rf) {
                    std::cmp::Ordering::Greater => ru_eff,
                    _ => lu_eff,
                };

                let lv_t = if target == lu_eff {
                    *lv
                } else {
                    match zunder_ucum::convert_decimal(*lv, lu_eff, target) {
                        Ok(v) => v,
                        Err(_) => return Ok(Collection::empty()),
                    }
                };
                let rv_t = if target == ru_eff {
                    *rv
                } else {
                    match zunder_ucum::convert_decimal(*rv, ru_eff, target) {
                        Ok(v) => v,
                        Err(_) => return Ok(Collection::empty()),
                    }
                };

                let result_unit = format!("{target}2");
                return Ok(Collection::singleton(Value::quantity(
                    lv_t * rv_t,
                    Arc::from(result_unit.as_str()),
                )));
            }

            // Fallback: concatenate unit representations
            let result_value = *lv * *rv;
            let result_unit = format!("{lu_eff}.{ru_eff}");
            Ok(Collection::singleton(Value::quantity(
                result_value,
                Arc::from(result_unit.as_str()),
            )))
        }
        _ => Err(Error::TypeError(
            "Multiplication requires numeric types".into(),
        )),
    }
}

fn divide(left: Collection, right: Collection) -> Result<Collection> {
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    // FHIRPath division requires singleton collections
    // If not singleton, return empty per FHIRPath spec
    if left.len() != 1 || right.len() != 1 {
        return Ok(Collection::empty());
    }

    let left_val = &left.iter().next().unwrap().materialize();
    let right_val = &right.iter().next().unwrap().materialize();

    match (left_val.data(), right_val.data()) {
        (ValueData::Integer(l), ValueData::Integer(r)) => {
            if *r == 0 {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::decimal(
                Decimal::from(*l) / Decimal::from(*r),
            )))
        }
        (ValueData::Integer(l), ValueData::Decimal(r)) => {
            if *r == Decimal::ZERO {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::decimal(
                Decimal::from(*l) / *r,
            )))
        }
        (ValueData::Decimal(l), ValueData::Integer(r)) => {
            if *r == 0 {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::decimal(
                *l / Decimal::from(*r),
            )))
        }
        (ValueData::Decimal(l), ValueData::Decimal(r)) => {
            if *r == Decimal::ZERO {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::decimal(*l / *r)))
        }
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            if *rv == Decimal::ZERO {
                return Ok(Collection::empty());
            }
            let lu = lu.as_ref().trim();
            let ru = ru.as_ref().trim();

            let lu_eff = if let Some(code) = get_calendar_ucum_equivalent(lu) {
                if calendar_is_strict_equal_to_ucum(lu) {
                    code
                } else {
                    return Ok(Collection::empty());
                }
            } else {
                lu
            };
            let ru_eff = if let Some(code) = get_calendar_ucum_equivalent(ru) {
                if calendar_is_strict_equal_to_ucum(ru) {
                    code
                } else {
                    return Ok(Collection::empty());
                }
            } else {
                ru
            };

            let (l_unit, r_unit) =
                match (zunder_ucum::Unit::parse(lu_eff), zunder_ucum::Unit::parse(ru_eff)) {
                    (Ok(lu), Ok(ru)) => (lu, ru),
                    _ => return Ok(Collection::empty()),
                };
            if matches!(l_unit.kind, zunder_ucum::UnitKind::NonLinear)
                || matches!(r_unit.kind, zunder_ucum::UnitKind::NonLinear)
            {
                return Ok(Collection::empty());
            }

            if r_unit.dimensions == zunder_ucum::DimensionVector::ZERO {
                return Ok(Collection::singleton(Value::quantity(
                    *lv / *rv,
                    Arc::from(lu_eff),
                )));
            }

            // If the dimensions match, result is dimensionless (unit "1")
            if l_unit.dimensions == r_unit.dimensions {
                let lv_in_ru = match zunder_ucum::convert_decimal(*lv, lu_eff, ru_eff) {
                    Ok(v) => v,
                    Err(_) => return Ok(Collection::empty()),
                };
                let result_value = lv_in_ru / *rv;
                return Ok(Collection::singleton(Value::quantity(
                    result_value,
                    Arc::from("1"),
                )));
            }

            // Fallback: divide values and create compound unit
            let result_value = *lv / *rv;
            let result_unit = format!("{lu_eff}/{ru_eff}");
            Ok(Collection::singleton(Value::quantity(
                result_value,
                Arc::from(result_unit.as_str()),
            )))
        }
        _ => Err(Error::TypeError("Division requires numeric types".into())),
    }
}

fn divide_int(left: Collection, right: Collection) -> Result<Collection> {
    // FHIRPath `div` operator: integer division (truncate toward zero).
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    // If not singleton, return empty per FHIRPath spec
    if left.len() != 1 || right.len() != 1 {
        return Ok(Collection::empty());
    }

    let left_val = &left.iter().next().unwrap().materialize();
    let right_val = &right.iter().next().unwrap().materialize();

    match (left_val.data(), right_val.data()) {
        (ValueData::Integer(l), ValueData::Integer(r)) => {
            if *r == 0 {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::integer(l / r)))
        }
        (ValueData::Integer(l), ValueData::Decimal(r)) => {
            if *r == Decimal::ZERO {
                return Ok(Collection::empty());
            }
            let q = Decimal::from(*l) / *r;
            Ok(q.trunc()
                .to_i64()
                .map(Value::integer)
                .map(Collection::singleton)
                .unwrap_or_else(Collection::empty))
        }
        (ValueData::Decimal(l), ValueData::Integer(r)) => {
            if *r == 0 {
                return Ok(Collection::empty());
            }
            let q = *l / Decimal::from(*r);
            Ok(q.trunc()
                .to_i64()
                .map(Value::integer)
                .map(Collection::singleton)
                .unwrap_or_else(Collection::empty))
        }
        (ValueData::Decimal(l), ValueData::Decimal(r)) => {
            if *r == Decimal::ZERO {
                return Ok(Collection::empty());
            }
            let q = *l / *r;
            Ok(q.trunc()
                .to_i64()
                .map(Value::integer)
                .map(Collection::singleton)
                .unwrap_or_else(Collection::empty))
        }
        _ => Err(Error::TypeError("Division requires numeric types".into())),
    }
}

fn modulo(left: Collection, right: Collection) -> Result<Collection> {
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    if left.len() != 1 || right.len() != 1 {
        return Err(Error::TypeError(
            "Modulo requires singleton collections".into(),
        ));
    }

    let left_val = &left.iter().next().unwrap().materialize();
    let right_val = &right.iter().next().unwrap().materialize();

    match (left_val.data(), right_val.data()) {
        (ValueData::Integer(l), ValueData::Integer(r)) => {
            if *r == 0 {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::integer(l % r)))
        }
        (ValueData::Integer(l), ValueData::Decimal(r)) => {
            if *r == Decimal::ZERO {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::decimal(
                Decimal::from(*l) % *r,
            )))
        }
        (ValueData::Decimal(l), ValueData::Integer(r)) => {
            if *r == 0 {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::decimal(
                *l % Decimal::from(*r),
            )))
        }
        (ValueData::Decimal(l), ValueData::Decimal(r)) => {
            if *r == Decimal::ZERO {
                return Ok(Collection::empty());
            }
            Ok(Collection::singleton(Value::decimal(*l % *r)))
        }
        _ => Err(Error::TypeError("Modulo requires numeric types".into())),
    }
}

// ============================================
// Equality Operations
// ============================================

fn equals(left: Collection, right: Collection) -> Result<Collection> {
    // If either operand is empty, return empty collection
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    // If collections have different lengths, they're not equal
    if left.len() != right.len() {
        return Ok(Collection::singleton(Value::boolean(false)));
    }

    // Compare each item in order
    let left_items: Vec<_> = left.iter().collect();
    let right_items: Vec<_> = right.iter().collect();

    for (l, r) in left_items.iter().zip(right_items.iter()) {
        match items_equal(l, r) {
            Some(false) => return Ok(Collection::singleton(Value::boolean(false))),
            None => return Ok(Collection::empty()), // Incomparable - return empty
            Some(true) => continue,                 // Equal, check next item
        }
    }

    Ok(Collection::singleton(Value::boolean(true)))
}

fn equivalent(left: Collection, right: Collection) -> Result<Collection> {
    // Empty collections are equivalent (return true, not empty)
    if left.is_empty() && right.is_empty() {
        return Ok(Collection::singleton(Value::boolean(true)));
    }

    // If only one is empty, they're not equivalent (return false, not empty)
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::singleton(Value::boolean(false)));
    }

    // If collections have different lengths, they're not equivalent
    if left.len() != right.len() {
        return Ok(Collection::singleton(Value::boolean(false)));
    }

    let left_items: Vec<_> = left.iter().collect();
    let right_items: Vec<_> = right.iter().collect();

    // For single items, use direct equivalence comparison
    if left_items.len() == 1 && right_items.len() == 1 {
        if items_equivalent(left_items[0], right_items[0]) {
            return Ok(Collection::singleton(Value::boolean(true)));
        } else {
            return Ok(Collection::singleton(Value::boolean(false)));
        }
    }

    // For multiple items, use order-independent comparison
    // Convert Vec<&Value> to Vec<Value> for comparison
    let left_vals: Vec<Value> = left_items.iter().map(|v| (*v).clone()).collect();
    let right_vals: Vec<Value> = right_items.iter().map(|v| (*v).clone()).collect();
    if lists_equivalent(&left_vals, &right_vals) {
        Ok(Collection::singleton(Value::boolean(true)))
    } else {
        Ok(Collection::singleton(Value::boolean(false)))
    }
}

fn not_equivalent(left: Collection, right: Collection) -> Result<Collection> {
    let equiv_result = equivalent(left, right)?;
    let equiv_bool = equiv_result.as_boolean()?;
    Ok(Collection::singleton(Value::boolean(!equiv_bool)))
}

fn not_equals(left: Collection, right: Collection) -> Result<Collection> {
    let left_clone = left.clone();
    let right_clone = right.clone();
    let eq_result = equals(left, right)?;
    if eq_result.is_empty() {
        // Special-case cross-precision temporal comparisons:
        // Date vs Time (or vice versa) are considered not equal in HL7 tests.
        if left_clone.len() == 1 && right_clone.len() == 1 {
            let l = left_clone.iter().next().unwrap();
            let r = right_clone.iter().next().unwrap();
            match (l.data(), r.data()) {
                (
                    ValueData::Date { .. },
                    ValueData::Time {
                        value: _,
                        precision: _,
                    },
                )
                | (
                    ValueData::Time {
                        value: _,
                        precision: _,
                    },
                    ValueData::Date { .. },
                ) => {
                    return Ok(Collection::singleton(Value::boolean(true)));
                }
                (
                    ValueData::DateTime {
                        value: _,
                        precision: _,
                        timezone_offset: _,
                    },
                    ValueData::Time {
                        value: _,
                        precision: _,
                    },
                )
                | (
                    ValueData::Time {
                        value: _,
                        precision: _,
                    },
                    ValueData::DateTime {
                        value: _,
                        precision: _,
                        timezone_offset: _,
                    },
                ) => {
                    return Ok(Collection::singleton(Value::boolean(true)));
                }
                (
                    ValueData::String(_),
                    ValueData::Time {
                        value: _,
                        precision: _,
                    },
                )
                | (
                    ValueData::Time {
                        value: _,
                        precision: _,
                    },
                    ValueData::String(_),
                ) => {
                    return Ok(Collection::singleton(Value::boolean(true)));
                }
                _ => {}
            }
        }
        return Ok(Collection::empty());
    }
    let eq_bool = eq_result.as_boolean()?;
    Ok(Collection::singleton(Value::boolean(!eq_bool)))
}

fn items_equal(left: &Value, right: &Value) -> Option<bool> {
    // Returns Some(true) if equal, Some(false) if different, None if incomparable (empty result)

    match (left.data(), right.data()) {
        // Materialize LazyJson before comparison
        (ValueData::LazyJson { .. }, _) | (_, ValueData::LazyJson { .. }) => {
            let left_mat = left.materialize();
            let right_mat = right.materialize();
            items_equal(&left_mat, &right_mat)
        }
        (ValueData::Boolean(l), ValueData::Boolean(r)) => Some(l == r),
        (ValueData::Integer(l), ValueData::Integer(r)) => Some(l == r),
        (ValueData::Integer(l), ValueData::Decimal(r)) => {
            Some(Decimal::from(*l).normalize() == r.normalize())
        }
        (ValueData::Decimal(l), ValueData::Integer(r)) => {
            Some(l.normalize() == Decimal::from(*r).normalize())
        }
        (ValueData::Decimal(l), ValueData::Decimal(r)) => Some(l.normalize() == r.normalize()),
        (ValueData::String(l), ValueData::String(r)) => {
            if let Some((l_temporal, r_temporal)) =
                crate::temporal_parse::parse_temporal_pair(l.as_ref(), r.as_ref())
            {
                return items_equal(&l_temporal, &r_temporal);
            }
            Some(l == r)
        }
        (
            ValueData::Date {
                value: l,
                precision: l_prec,
            },
            ValueData::Date {
                value: r,
                precision: r_prec,
            },
        ) => {
            if !l_prec.is_compatible_with(*r_prec) {
                return None;
            }
            Some(l == r)
        }
        (
            ValueData::DateTime {
                value: l,
                precision: l_prec,
                timezone_offset: l_tz,
            },
            ValueData::DateTime {
                value: r,
                precision: r_prec,
                timezone_offset: r_tz,
            },
        ) => {
            // Per FHIRPath spec: DateTimes with different precisions are incomparable
            if !l_prec.is_compatible_with(*r_prec) {
                return None; // Incomparable
            }
            // Timezone must either be specified for both, or for neither.
            if l_tz.is_some() != r_tz.is_some() {
                return None;
            }
            Some(l == r)
        }
        (
            ValueData::Time {
                value: l,
                precision: l_prec,
            },
            ValueData::Time {
                value: r,
                precision: r_prec,
            },
        ) => {
            // Per FHIRPath spec: Times with different precisions are incomparable
            if !l_prec.is_compatible_with(*r_prec) {
                return None; // Incomparable
            }
            Some(l == r)
        }
        // Date vs DateTime - incomparable when datetime has time precision
        (
            ValueData::Date {
                value: l,
                precision: l_prec,
            },
            ValueData::DateTime {
                value: r,
                precision: r_prec,
                timezone_offset: r_tz,
            },
        ) => {
            if !l_prec.is_compatible_with(match *r_prec {
                crate::value::DateTimePrecision::Year => crate::value::DatePrecision::Year,
                crate::value::DateTimePrecision::Month => crate::value::DatePrecision::Month,
                crate::value::DateTimePrecision::Day => crate::value::DatePrecision::Day,
                _ => return None,
            }) {
                return None;
            }

            let local_date = if let Some(offset_secs) = r_tz {
                let offset = chrono::FixedOffset::east_opt(*offset_secs)?;
                r.with_timezone(&offset).date_naive()
            } else {
                r.date_naive()
            };
            Some(*l == local_date)
        }
        (
            ValueData::DateTime {
                value: l,
                precision: l_prec,
                timezone_offset: l_tz,
            },
            ValueData::Date {
                value: r,
                precision: r_prec,
            },
        ) => {
            if !r_prec.is_compatible_with(match *l_prec {
                crate::value::DateTimePrecision::Year => crate::value::DatePrecision::Year,
                crate::value::DateTimePrecision::Month => crate::value::DatePrecision::Month,
                crate::value::DateTimePrecision::Day => crate::value::DatePrecision::Day,
                _ => return None,
            }) {
                return None;
            }

            let local_date = if let Some(offset_secs) = l_tz {
                let offset = chrono::FixedOffset::east_opt(*offset_secs)?;
                l.with_timezone(&offset).date_naive()
            } else {
                l.date_naive()
            };
            Some(local_date == *r)
        }
        // Date/DateTime vs Time are incomparable
        (
            ValueData::Date { .. },
            ValueData::Time {
                value: _,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: _,
                precision: _,
            },
            ValueData::Date { .. },
        )
        | (
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
            ValueData::Time {
                value: _,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: _,
                precision: _,
            },
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
        ) => None,
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            let lu = lu.as_ref().trim();
            let ru = ru.as_ref().trim();

            if lu == ru {
                return Some(lv == rv);
            }

            let l_cal = get_calendar_ucum_equivalent(lu);
            let r_cal = get_calendar_ucum_equivalent(ru);

            match (l_cal, r_cal) {
                (Some(lc), Some(rc)) => {
                    try_ucum_compare(lv, lc, rv, rc)
                        .map(|ord| ord == std::cmp::Ordering::Equal)
                }
                (Some(lc), None) => {
                    let Ok(other) = zunder_ucum::Unit::parse(ru) else {
                        return None;
                    };
                    if !is_pure_time_dimension(&other.dimensions) {
                        return None;
                    }
                    if calendar_is_strict_equal_to_ucum(lu) {
                        try_ucum_compare(lv, lc, rv, ru)
                            .map(|ord| ord == std::cmp::Ordering::Equal)
                    } else {
                        Some(false)
                    }
                }
                (None, Some(rc)) => {
                    let Ok(other) = zunder_ucum::Unit::parse(lu) else {
                        return None;
                    };
                    if !is_pure_time_dimension(&other.dimensions) {
                        return None;
                    }
                    if calendar_is_strict_equal_to_ucum(ru) {
                        try_ucum_compare(lv, lu, rv, rc)
                            .map(|ord| ord == std::cmp::Ordering::Equal)
                    } else {
                        Some(false)
                    }
                }
                (None, None) => {
                    try_ucum_compare(lv, lu, rv, ru)
                        .map(|ord| ord == std::cmp::Ordering::Equal)
                }
            }
        }
        // Cross-type: String vs Date - try to parse string as date
        (
            ValueData::String(s),
            ValueData::Date {
                value: d,
                precision: d_prec,
            },
        )
        | (
            ValueData::Date {
                value: d,
                precision: d_prec,
            },
            ValueData::String(s),
        ) => {
            use chrono::NaiveDate;
            let s = s.as_ref();
            let (parsed, parsed_prec) = match s.len() {
                4 => (
                    NaiveDate::parse_from_str(&format!("{}-01-01", s), "%Y-%m-%d").ok()?,
                    crate::value::DatePrecision::Year,
                ),
                7 => (
                    NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d").ok()?,
                    crate::value::DatePrecision::Month,
                ),
                10 => (
                    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?,
                    crate::value::DatePrecision::Day,
                ),
                _ => return None,
            };
            if !d_prec.is_compatible_with(parsed_prec) {
                return None;
            }
            Some(parsed == *d)
        }
        // Cross-type: String vs DateTime - try to parse string as datetime
        (
            ValueData::String(s),
            ValueData::DateTime {
                value: dt,
                precision: _,
                timezone_offset: _,
            },
        )
        | (
            ValueData::DateTime {
                value: dt,
                precision: _,
                timezone_offset: _,
            },
            ValueData::String(s),
        ) => {
            use chrono::DateTime;
            DateTime::parse_from_rfc3339(s.as_ref())
                .ok()
                .map(|parsed| parsed.with_timezone(&chrono::Utc) == *dt)
        }
        // Cross-type: String vs Time - try to parse string as time
        (
            ValueData::String(s),
            ValueData::Time {
                value: t,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: t,
                precision: _,
            },
            ValueData::String(s),
        ) => {
            use chrono::NaiveTime;
            NaiveTime::parse_from_str(s.as_ref(), "%H:%M:%S")
                .or_else(|_| NaiveTime::parse_from_str(s.as_ref(), "%H:%M:%S%.f"))
                .or_else(|_| NaiveTime::parse_from_str(s.as_ref(), "%H:%M"))
                .ok()
                .map(|parsed| &parsed == t)
        }
        // Empty values are equivalent
        (ValueData::Empty, ValueData::Empty) => Some(true),
        // FHIR Quantity object vs SystemQuantity comparison
        (
            ValueData::Object(l_obj),
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            if let (Some(l_val), Some(l_unit)) = (
                l_obj.get(&Arc::from("value")),
                l_obj
                    .get(&Arc::from("unit"))
                    .or_else(|| l_obj.get(&Arc::from("code"))),
            ) {
                if let (Some(l_val_v), Some(l_unit_v)) = (l_val.iter().next(), l_unit.iter().next())
                {
                    // Materialize LazyJson before matching
                    let l_val_v = l_val_v.materialize();
                    let l_unit_v = l_unit_v.materialize();

                    let lv_decimal = match l_val_v.data() {
                        ValueData::Decimal(d) => *d,
                        ValueData::Integer(i) => Decimal::from(*i),
                        _ => return Some(false),
                    };
                    let lu_str = match l_unit_v.data() {
                        ValueData::String(s) => s,
                        _ => return Some(false),
                    };

                    if !units_equivalent(lu_str.as_ref(), ru.as_ref()) {
                        return Some(false);
                    }

                    let lu_lower = lu_str.as_ref().trim().to_ascii_lowercase();
                    let ru_lower = ru.as_ref().trim().to_ascii_lowercase();
                    let same_unit = lu_lower == ru_lower
                        || (lu_lower == "lbs" && ru_lower == "[lb_av]")
                        || (lu_lower == "[lb_av]" && (ru_lower == "lbs" || ru_lower == "lb"))
                        || (lu_lower == "lb" && ru_lower == "[lb_av]");

                    if same_unit {
                        Some(lv_decimal == *rv)
                    } else if let (Some((lv_base, _)), Some((rv_base, _))) = (
                        convert_to_base_unit(&lv_decimal, lu_str.as_ref()),
                        convert_to_base_unit(rv, ru.as_ref()),
                    ) {
                        Some(lv_base == rv_base)
                    } else {
                        Some(lv_decimal == *rv)
                    }
                } else {
                    Some(false)
                }
            } else {
                Some(false)
            }
        }
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Object(r_obj),
        ) => {
            if let (Some(r_val), Some(r_unit)) = (
                r_obj.get(&Arc::from("value")),
                r_obj
                    .get(&Arc::from("unit"))
                    .or_else(|| r_obj.get(&Arc::from("code"))),
            ) {
                if let (Some(r_val_v), Some(r_unit_v)) = (r_val.iter().next(), r_unit.iter().next())
                {
                    // Materialize LazyJson before matching
                    let r_val_v = r_val_v.materialize();
                    let r_unit_v = r_unit_v.materialize();

                    let rv_decimal = match r_val_v.data() {
                        ValueData::Decimal(d) => *d,
                        ValueData::Integer(i) => Decimal::from(*i),
                        _ => return Some(false),
                    };
                    let ru_str = match r_unit_v.data() {
                        ValueData::String(s) => s,
                        _ => return Some(false),
                    };

                    if !units_equivalent(lu.as_ref(), ru_str.as_ref()) {
                        return Some(false);
                    }

                    let lu_lower = lu.as_ref().trim().to_ascii_lowercase();
                    let ru_lower = ru_str.as_ref().trim().to_ascii_lowercase();
                    let same_unit = lu_lower == ru_lower
                        || (lu_lower == "lbs" && ru_lower == "[lb_av]")
                        || (lu_lower == "[lb_av]" && (ru_lower == "lbs" || ru_lower == "lb"))
                        || (lu_lower == "lb" && ru_lower == "[lb_av]");

                    if same_unit {
                        Some(*lv == rv_decimal)
                    } else if let (Some((lv_base, _)), Some((rv_base, _))) = (
                        convert_to_base_unit(lv, lu.as_ref()),
                        convert_to_base_unit(&rv_decimal, ru_str.as_ref()),
                    ) {
                        Some(lv_base == rv_base)
                    } else {
                        Some(*lv == rv_decimal)
                    }
                } else {
                    Some(false)
                }
            } else {
                Some(false)
            }
        }
        // Complex types (objects) - recursive equivalence mapped to boolean
        (ValueData::Object(_), ValueData::Object(_)) => Some(items_equivalent(left, right)),
        _ => Some(false),
    }
}

/// Normalize whitespace in a string (collapse multiple spaces to single space, trim)
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Get decimal scale (number of decimal places), ignoring trailing zeros.
fn decimal_scale_ignoring_trailing_zeros(d: &Decimal) -> u32 {
    d.normalize().scale()
}

/// Round decimal to specified precision
fn round_to_precision(d: &Decimal, precision: u32) -> Decimal {
    d.round_dp(precision)
}

fn round_to_step(value: Decimal, step: Decimal) -> Decimal {
    if step.is_zero() {
        return value;
    }
    (value / step).round() * step
}

fn items_equivalent(left: &Value, right: &Value) -> bool {
    // Returns true if equivalent, false otherwise (never returns None/empty)

    match (left.data(), right.data()) {
        // Materialize LazyJson before comparison
        (ValueData::LazyJson { .. }, _) | (_, ValueData::LazyJson { .. }) => {
            let left_mat = left.materialize();
            let right_mat = right.materialize();
            items_equivalent(&left_mat, &right_mat)
        }
        (ValueData::Boolean(l), ValueData::Boolean(r)) => l == r,
        (ValueData::Integer(l), ValueData::Integer(r)) => l == r,
        // Integer-Decimal equivalence: convert integer to decimal with 0 precision
        (ValueData::Integer(l), ValueData::Decimal(r)) => {
            let l_decimal = Decimal::from(*l);
            let min_precision = 0;
            let l_rounded = round_to_precision(&l_decimal, min_precision);
            let r_rounded = round_to_precision(r, min_precision);
            l_rounded == r_rounded
        }
        (ValueData::Decimal(l), ValueData::Integer(r)) => {
            let r_decimal = Decimal::from(*r);
            let min_precision = 0;
            let l_rounded = round_to_precision(l, min_precision);
            let r_rounded = round_to_precision(&r_decimal, min_precision);
            l_rounded == r_rounded
        }
        // Decimal equivalence: round to least precise operand's precision
        (ValueData::Decimal(l), ValueData::Decimal(r)) => {
            let min_precision = decimal_scale_ignoring_trailing_zeros(l)
                .min(decimal_scale_ignoring_trailing_zeros(r));
            let l_rounded = round_to_precision(l, min_precision);
            let r_rounded = round_to_precision(r, min_precision);
            l_rounded == r_rounded
        }
        // String equivalence: case-insensitive and whitespace-normalized
        (ValueData::String(l), ValueData::String(r)) => {
            let l_norm = normalize_whitespace(&l.to_lowercase());
            let r_norm = normalize_whitespace(&r.to_lowercase());
            l_norm == r_norm
        }
        (
            ValueData::Date {
                value: l,
                precision: l_prec,
            },
            ValueData::Date {
                value: r,
                precision: r_prec,
            },
        ) => l_prec.is_compatible_with(*r_prec) && l == r,
        (
            ValueData::DateTime {
                value: l,
                precision: l_prec,
                timezone_offset: l_tz,
            },
            ValueData::DateTime {
                value: r,
                precision: r_prec,
                timezone_offset: r_tz,
            },
        ) => {
            // For equivalence: different precisions return false (not empty)
            if !l_prec.is_compatible_with(*r_prec) {
                return false;
            }
            if l_tz.is_some() != r_tz.is_some() {
                return false;
            }
            l == r
        }
        (
            ValueData::Time {
                value: l,
                precision: l_prec,
            },
            ValueData::Time {
                value: r,
                precision: r_prec,
            },
        ) => {
            // For equivalence: different precisions return false (not empty)
            if !l_prec.is_compatible_with(*r_prec) {
                return false;
            }
            l == r
        }
        // Date vs DateTime: different precision returns false (not empty)
        (
            ValueData::Date {
                value: l,
                precision: l_prec,
            },
            ValueData::DateTime {
                value: r,
                precision: r_prec,
                timezone_offset: r_tz,
            },
        ) => {
            let expected = match *r_prec {
                crate::value::DateTimePrecision::Year => crate::value::DatePrecision::Year,
                crate::value::DateTimePrecision::Month => crate::value::DatePrecision::Month,
                crate::value::DateTimePrecision::Day => crate::value::DatePrecision::Day,
                _ => return false,
            };
            if !l_prec.is_compatible_with(expected) {
                return false;
            }

            let local_date = if let Some(offset_secs) = r_tz {
                let Some(offset) = chrono::FixedOffset::east_opt(*offset_secs) else {
                    return false;
                };
                r.with_timezone(&offset).date_naive()
            } else {
                r.date_naive()
            };
            *l == local_date
        }
        (
            ValueData::DateTime {
                value: l,
                precision: l_prec,
                timezone_offset: l_tz,
            },
            ValueData::Date {
                value: r,
                precision: r_prec,
            },
        ) => {
            let expected = match *l_prec {
                crate::value::DateTimePrecision::Year => crate::value::DatePrecision::Year,
                crate::value::DateTimePrecision::Month => crate::value::DatePrecision::Month,
                crate::value::DateTimePrecision::Day => crate::value::DatePrecision::Day,
                _ => return false,
            };
            if !r_prec.is_compatible_with(expected) {
                return false;
            }
            let local_date = if let Some(offset_secs) = l_tz {
                let Some(offset) = chrono::FixedOffset::east_opt(*offset_secs) else {
                    return false;
                };
                l.with_timezone(&offset).date_naive()
            } else {
                l.date_naive()
            };
            local_date == *r
        }
        // Quantity equivalence
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            let lu = lu.as_ref().trim();
            let ru = ru.as_ref().trim();

            let lu_eff = get_calendar_ucum_equivalent(lu).unwrap_or(lu);
            let ru_eff = get_calendar_ucum_equivalent(ru).unwrap_or(ru);

            if lu_eff == ru_eff {
                let min_precision = decimal_scale_ignoring_trailing_zeros(lv)
                    .min(decimal_scale_ignoring_trailing_zeros(rv));
                let lv_rounded = round_to_precision(lv, min_precision);
                let rv_rounded = round_to_precision(rv, min_precision);
                return lv_rounded == rv_rounded;
            }

            let (l_unit, r_unit) =
                match (zunder_ucum::Unit::parse(lu_eff), zunder_ucum::Unit::parse(ru_eff)) {
                    (Ok(lu), Ok(ru)) => (lu, ru),
                    _ => return false,
                };
            if l_unit.dimensions != r_unit.dimensions {
                return false;
            }
            if matches!(l_unit.kind, zunder_ucum::UnitKind::NonLinear)
                || matches!(r_unit.kind, zunder_ucum::UnitKind::NonLinear)
            {
                return false;
            }

            match (&l_unit.kind, &r_unit.kind) {
                // Affine units: normalize both to Kelvin and apply decimal equivalence.
                (zunder_ucum::UnitKind::Affine { .. }, _) | (_, zunder_ucum::UnitKind::Affine { .. }) => {
                    let lv_norm = match zunder_ucum::convert_decimal(*lv, lu_eff, "K") {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    let rv_norm = match zunder_ucum::convert_decimal(*rv, ru_eff, "K") {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    let min_precision = decimal_scale_ignoring_trailing_zeros(&lv_norm)
                        .min(decimal_scale_ignoring_trailing_zeros(&rv_norm));
                    round_to_precision(&lv_norm, min_precision)
                        == round_to_precision(&rv_norm, min_precision)
                }
                (
                    zunder_ucum::UnitKind::Multiplicative { factor: lf },
                    zunder_ucum::UnitKind::Multiplicative { factor: rf },
                ) => {
                    // Most granular unit of either input (smallest UCUM factor).
                    let target = match lf.cmp(rf) {
                        std::cmp::Ordering::Greater => ru_eff,
                        _ => lu_eff,
                    };

                    let lv_norm = match zunder_ucum::convert_decimal(*lv, lu_eff, target) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    let rv_norm = match zunder_ucum::convert_decimal(*rv, ru_eff, target) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };

                    // Compare using the least precise operand's implied resolution in the target unit.
                    // Precision is based on decimal scale *and* unit conversion granularity.
                    let lv_step = {
                        let p = decimal_scale_ignoring_trailing_zeros(lv);
                        let step = Decimal::new(1, p);
                        match zunder_ucum::convert_decimal(step, lu_eff, target) {
                            Ok(v) => v,
                            Err(_) => return false,
                        }
                    }
                    .abs();
                    let rv_step = {
                        let p = decimal_scale_ignoring_trailing_zeros(rv);
                        let step = Decimal::new(1, p);
                        match zunder_ucum::convert_decimal(step, ru_eff, target) {
                            Ok(v) => v,
                            Err(_) => return false,
                        }
                    }
                    .abs();
                    let step = if lv_step >= rv_step { lv_step } else { rv_step };

                    round_to_step(lv_norm, step) == round_to_step(rv_norm, step)
                }
                _ => false,
            }
        }
        // Cross-type: String vs Date - try to parse string as date
        (
            ValueData::String(s),
            ValueData::Date {
                value: d,
                precision: d_prec,
            },
        )
        | (
            ValueData::Date {
                value: d,
                precision: d_prec,
            },
            ValueData::String(s),
        ) => {
            use chrono::NaiveDate;
            let s = s.as_ref();
            let (parsed, parsed_prec) = match s.len() {
                4 => (
                    NaiveDate::parse_from_str(&format!("{}-01-01", s), "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Year),
                ),
                7 => (
                    NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Month),
                ),
                10 => (
                    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Day),
                ),
                _ => (None, None),
            };
            match (parsed, parsed_prec) {
                (Some(parsed), Some(parsed_prec)) => {
                    d_prec.is_compatible_with(parsed_prec) && parsed == *d
                }
                _ => false,
            }
        }
        // Cross-type: String vs DateTime - try to parse string as datetime
        (
            ValueData::String(s),
            ValueData::DateTime {
                value: dt,
                precision: _,
                timezone_offset: _,
            },
        )
        | (
            ValueData::DateTime {
                value: dt,
                precision: _,
                timezone_offset: _,
            },
            ValueData::String(s),
        ) => {
            use chrono::DateTime;
            DateTime::parse_from_rfc3339(s.as_ref())
                .map(|parsed| parsed.with_timezone(&chrono::Utc) == *dt)
                .unwrap_or(false)
        }
        // Cross-type: String vs Time - try to parse string as time
        (
            ValueData::String(s),
            ValueData::Time {
                value: t,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: t,
                precision: _,
            },
            ValueData::String(s),
        ) => {
            use chrono::NaiveTime;
            NaiveTime::parse_from_str(s.as_ref(), "%H:%M:%S")
                .or_else(|_| NaiveTime::parse_from_str(s.as_ref(), "%H:%M:%S%.f"))
                .map(|parsed| parsed == *t)
                .unwrap_or(false)
        }
        // Date vs Time: different types -> false
        (
            ValueData::Date { .. },
            ValueData::Time {
                value: _,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: _,
                precision: _,
            },
            ValueData::Date { .. },
        ) => false,
        (
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
            ValueData::Time {
                value: _,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: _,
                precision: _,
            },
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
        ) => false,
        // Empty values are equivalent
        (ValueData::Empty, ValueData::Empty) => true,
        // FHIR Quantity object vs SystemQuantity comparison
        (
            ValueData::Object(l_obj),
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            // Check if object is a FHIR Quantity (has "value" and ("unit" or "code"))
            if let (Some(l_val), Some(l_unit)) = (
                l_obj.get(&Arc::from("value")),
                l_obj
                    .get(&Arc::from("unit"))
                    .or_else(|| l_obj.get(&Arc::from("code"))),
            ) {
                // Extract value and unit from collections
                let l_val_item = l_val.iter().next();
                let l_unit_item = l_unit.iter().next();
                if let (Some(l_val_v), Some(l_unit_v)) = (l_val_item, l_unit_item) {
                    // Materialize LazyJson before matching
                    let l_val_v = l_val_v.materialize();
                    let l_unit_v = l_unit_v.materialize();

                    let lv_decimal = match l_val_v.data() {
                        ValueData::Decimal(d) => *d,
                        ValueData::Integer(i) => Decimal::from(*i),
                        _ => return false,
                    };
                    let lu_str = match l_unit_v.data() {
                        ValueData::String(s) => s,
                        _ => return false,
                    };
                    // Check if units are equivalent
                    if !units_equivalent(lu_str.as_ref(), ru.as_ref()) {
                        return false;
                    }
                    // Compare values (convert to base units if different units)
                    let lu_lower = lu_str.as_ref().trim().to_ascii_lowercase();
                    let ru_lower = ru.as_ref().trim().to_ascii_lowercase();
                    if lu_lower == ru_lower {
                        lv_decimal == *rv
                    } else if (lu_lower == "lbs" && ru_lower == "[lb_av]")
                        || (lu_lower == "[lb_av]" && ru_lower == "lbs")
                        || (lu_lower == "lb" && ru_lower == "[lb_av]")
                        || (lu_lower == "[lb_av]" && ru_lower == "lb")
                    {
                        // Special case: lbs/lb and [lb_av] are the same unit conceptually - compare values directly
                        lv_decimal == *rv
                    } else {
                        // Convert to base units and compare
                        if let (Some((lv_base, _)), Some((rv_base, _))) = (
                            convert_to_base_unit(&lv_decimal, lu_str.as_ref()),
                            convert_to_base_unit(rv, ru.as_ref()),
                        ) {
                            lv_base == rv_base
                        } else {
                            lv_decimal == *rv
                        }
                    }
                } else {
                    false
                }
            } else {
                false
            }
        }
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Object(r_obj),
        ) => {
            // Check if object is a FHIR Quantity (has "value" and ("unit" or "code"))
            if let (Some(r_val), Some(r_unit)) = (
                r_obj.get(&Arc::from("value")),
                r_obj
                    .get(&Arc::from("unit"))
                    .or_else(|| r_obj.get(&Arc::from("code"))),
            ) {
                // Extract value and unit from collections
                let r_val_item = r_val.iter().next();
                let r_unit_item = r_unit.iter().next();
                if let (Some(r_val_v), Some(r_unit_v)) = (r_val_item, r_unit_item) {
                    // Materialize LazyJson before matching
                    let r_val_v = r_val_v.materialize();
                    let r_unit_v = r_unit_v.materialize();

                    let rv_decimal = match r_val_v.data() {
                        ValueData::Decimal(d) => *d,
                        ValueData::Integer(i) => Decimal::from(*i),
                        _ => return false,
                    };
                    let ru_str = match r_unit_v.data() {
                        ValueData::String(s) => s,
                        _ => return false,
                    };
                    // Check if units are equivalent
                    if !units_equivalent(lu.as_ref(), ru_str.as_ref()) {
                        return false;
                    }
                    // Compare values (convert to base units if different units)
                    let lu_lower = lu.as_ref().trim().to_ascii_lowercase();
                    let ru_lower = ru_str.as_ref().trim().to_ascii_lowercase();
                    if lu_lower == ru_lower {
                        *lv == rv_decimal
                    } else if (lu_lower == "lbs" && ru_lower == "[lb_av]")
                        || (lu_lower == "[lb_av]" && ru_lower == "lbs")
                        || (lu_lower == "lb" && ru_lower == "[lb_av]")
                        || (lu_lower == "[lb_av]" && ru_lower == "lb")
                    {
                        // Special case: lbs/lb and [lb_av] are the same unit conceptually - compare values directly
                        *lv == rv_decimal
                    } else {
                        // Convert to base units and compare
                        if let (Some((lv_base, _)), Some((rv_base, _))) = (
                            convert_to_base_unit(lv, lu.as_ref()),
                            convert_to_base_unit(&rv_decimal, ru_str.as_ref()),
                        ) {
                            lv_base == rv_base
                        } else {
                            *lv == rv_decimal
                        }
                    }
                } else {
                    false
                }
            } else {
                false
            }
        }
        // Complex types (objects) - recursive equivalence
        (ValueData::Object(l_obj), ValueData::Object(r_obj)) => {
            if l_obj.len() != r_obj.len() {
                return false;
            }
            for (key, l_val) in l_obj.iter() {
                if let Some(r_val) = r_obj.get(key) {
                    // Compare collections recursively
                    let l_items: Vec<Value> = l_val.iter().cloned().collect();
                    let r_items: Vec<Value> = r_val.iter().cloned().collect();
                    if l_items.len() != r_items.len() {
                        return false;
                    }
                    if l_items.len() == 1 && r_items.len() == 1 {
                        if !items_equivalent(&l_items[0], &r_items[0]) {
                            return false;
                        }
                    } else if !lists_equivalent(&l_items, &r_items) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

/// Compare two lists for equivalence (order-independent)
fn lists_equivalent(left: &[Value], right: &[Value]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    // For each item in left, find an equivalent item in right
    let mut right_remaining: Vec<usize> = (0..right.len()).collect();

    for left_item in left {
        let mut found_equivalent = false;
        for i in (0..right_remaining.len()).rev() {
            let right_idx = right_remaining[i];
            if items_equivalent(left_item, &right[right_idx]) {
                right_remaining.remove(i);
                found_equivalent = true;
                break;
            }
        }
        if !found_equivalent {
            return false;
        }
    }

    right_remaining.is_empty()
}

// ============================================
// Comparison Operations
// ============================================

fn less_than(left: Collection, right: Collection) -> Result<Collection> {
    compare(left, right, |l, r| {
        compare_values(l, r, |ord| ord == std::cmp::Ordering::Less)
    })
}

fn less_or_equal(left: Collection, right: Collection) -> Result<Collection> {
    compare(left, right, |l, r| {
        compare_values(l, r, |ord| ord != std::cmp::Ordering::Greater)
    })
}

fn greater_than(left: Collection, right: Collection) -> Result<Collection> {
    compare(left, right, |l, r| {
        compare_values(l, r, |ord| ord == std::cmp::Ordering::Greater)
    })
}

fn greater_or_equal(left: Collection, right: Collection) -> Result<Collection> {
    compare(left, right, |l, r| {
        compare_values(l, r, |ord| ord != std::cmp::Ordering::Less)
    })
}

fn compare<F>(left: Collection, right: Collection, cmp: F) -> Result<Collection>
where
    F: FnOnce(&Value, &Value) -> Result<Option<bool>>,
{
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    if left.len() != 1 || right.len() != 1 {
        return Err(Error::TypeError(
            "Comparison requires singleton collections".into(),
        ));
    }

    let left_val = left.iter().next().unwrap();
    let right_val = right.iter().next().unwrap();

    match cmp(left_val, right_val) {
        Ok(Some(result)) => Ok(Collection::singleton(Value::boolean(result))),
        Ok(None) => Ok(Collection::empty()), // Incomparable types
        Err(Error::TypeError(msg)) if msg.contains("Incomparable types") => Ok(Collection::empty()),
        Err(e) => Err(e),
    }
}

fn datetime_precision_as_date_precision(
    precision: crate::value::DateTimePrecision,
) -> Option<crate::value::DatePrecision> {
    match precision {
        crate::value::DateTimePrecision::Year => Some(crate::value::DatePrecision::Year),
        crate::value::DateTimePrecision::Month => Some(crate::value::DatePrecision::Month),
        crate::value::DateTimePrecision::Day => Some(crate::value::DatePrecision::Day),
        _ => None,
    }
}

fn datetime_local_date(
    dt: &chrono::DateTime<chrono::Utc>,
    tz: Option<i32>,
) -> Option<chrono::NaiveDate> {
    if let Some(offset_secs) = tz {
        let offset = chrono::FixedOffset::east_opt(offset_secs)?;
        Some(dt.with_timezone(&offset).date_naive())
    } else {
        Some(dt.date_naive())
    }
}

// Helper to compare two values
fn compare_values<F>(left: &Value, right: &Value, op: F) -> Result<Option<bool>>
where
    F: FnOnce(std::cmp::Ordering) -> bool,
{
    match (left.data(), right.data()) {
        // Materialize LazyJson before comparison
        (ValueData::LazyJson { .. }, _) | (_, ValueData::LazyJson { .. }) => {
            let left_mat = left.materialize();
            let right_mat = right.materialize();
            compare_values(&left_mat, &right_mat, op)
        }
        (ValueData::Integer(l), ValueData::Integer(r)) => Ok(Some(op(l.cmp(r)))),
        (ValueData::Integer(l), ValueData::Decimal(r)) => Ok(Some(op(Decimal::from(*l).cmp(r)))),
        (ValueData::Decimal(l), ValueData::Integer(r)) => Ok(Some(op(l.cmp(&Decimal::from(*r))))),
        (ValueData::Decimal(l), ValueData::Decimal(r)) => Ok(Some(op(l.cmp(r)))),
        (ValueData::String(_), ValueData::Integer(_))
        | (ValueData::String(_), ValueData::Decimal(_))
        | (ValueData::Integer(_), ValueData::String(_))
        | (ValueData::Decimal(_), ValueData::String(_)) => Err(Error::InvalidOperation(
            "Comparison requires comparable types".into(),
        )),
        (ValueData::String(l), ValueData::String(r)) => {
            if let Some((l_temporal, r_temporal)) =
                crate::temporal_parse::parse_temporal_pair(l.as_ref(), r.as_ref())
            {
                return compare_values(&l_temporal, &r_temporal, op);
            }
            Ok(Some(op(l.cmp(r))))
        }
        (
            ValueData::Date {
                value: l,
                precision: l_prec,
            },
            ValueData::Date {
                value: r,
                precision: r_prec,
            },
        ) => {
            if !l_prec.is_compatible_with(*r_prec) {
                return Ok(None);
            }
            Ok(Some(op(l.cmp(r))))
        }
        (
            ValueData::DateTime {
                value: l,
                precision: l_prec,
                timezone_offset: l_tz,
            },
            ValueData::DateTime {
                value: r,
                precision: r_prec,
                timezone_offset: r_tz,
            },
        ) => {
            // Per FHIRPath spec: DateTimes with different precisions are incomparable for ordering
            if !l_prec.is_compatible_with(*r_prec) {
                return Ok(None); // Incomparable - return empty
            }
            if l_tz.is_some() != r_tz.is_some() {
                return Ok(None);
            }
            // Same precision - compare instants in UTC
            Ok(Some(op(l.cmp(r))))
        }
        (
            ValueData::Time {
                value: l,
                precision: l_prec,
            },
            ValueData::Time {
                value: r,
                precision: r_prec,
            },
        ) => {
            // Per FHIRPath spec: Times with different precisions are incomparable for ordering
            if !l_prec.is_compatible_with(*r_prec) {
                return Ok(None); // Incomparable - return empty
            }
            // Same precision - compare times
            Ok(Some(op(l.cmp(r))))
        }
        (
            ValueData::DateTime {
                value: l,
                precision: l_prec,
                timezone_offset: l_tz,
            },
            ValueData::Date {
                value: r,
                precision: r_prec,
            },
        ) => {
            let expected = datetime_precision_as_date_precision(*l_prec).ok_or_else(|| {
                Error::TypeError("Incomparable types: dateTime with time precision".into())
            })?;
            if !r_prec.is_compatible_with(expected) {
                return Ok(None);
            }
            let l_date = datetime_local_date(l, *l_tz).ok_or_else(|| {
                Error::InvalidOperation("Invalid datetime timezone offset".into())
            })?;
            Ok(Some(op(l_date.cmp(r))))
        }
        (
            ValueData::Date {
                value: l,
                precision: l_prec,
            },
            ValueData::DateTime {
                value: r,
                precision: r_prec,
                timezone_offset: r_tz,
            },
        ) => {
            let expected = datetime_precision_as_date_precision(*r_prec).ok_or_else(|| {
                Error::TypeError("Incomparable types: dateTime with time precision".into())
            })?;
            if !l_prec.is_compatible_with(expected) {
                return Ok(None);
            }
            let r_date = datetime_local_date(r, *r_tz).ok_or_else(|| {
                Error::InvalidOperation("Invalid datetime timezone offset".into())
            })?;
            Ok(Some(op(l.cmp(&r_date))))
        }
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            let lu = lu.as_ref().trim();
            let ru = ru.as_ref().trim();

            if lu == ru {
                return Ok(Some(op(lv.cmp(rv))));
            }

            let l_cal = get_calendar_ucum_equivalent(lu);
            let r_cal = get_calendar_ucum_equivalent(ru);

            match (l_cal, r_cal) {
                (Some(lc), Some(rc)) => Ok(try_ucum_compare(lv, lc, rv, rc).map(op)),
                (Some(lc), None) => {
                    if !calendar_is_strict_equal_to_ucum(lu) {
                        let Ok(other) = zunder_ucum::Unit::parse(ru) else {
                            return Ok(None);
                        };
                        if is_pure_time_dimension(&other.dimensions) {
                            return Ok(None);
                        }
                        return Ok(None);
                    }
                    Ok(try_ucum_compare(lv, lc, rv, ru).map(op))
                }
                (None, Some(rc)) => {
                    if !calendar_is_strict_equal_to_ucum(ru) {
                        let Ok(other) = zunder_ucum::Unit::parse(lu) else {
                            return Ok(None);
                        };
                        if is_pure_time_dimension(&other.dimensions) {
                            return Ok(None);
                        }
                        return Ok(None);
                    }
                    Ok(try_ucum_compare(lv, lu, rv, rc).map(op))
                }
                (None, None) => Ok(try_ucum_compare(lv, lu, rv, ru).map(op)),
            }
        }
        (
            ValueData::Object(l_obj),
            ValueData::Quantity {
                value: rv,
                unit: ru,
            },
        ) => {
            // Handle FHIR Quantity object vs System Quantity comparison
            let system = l_obj
                .get(&Arc::from("system"))
                .and_then(|c| c.iter().next())
                .and_then(|v| match v.data() {
                    ValueData::String(s) => Some(s.as_ref()),
                    _ => None,
                });

            let unit_or_code = if system == Some("http://unitsofmeasure.org") {
                l_obj
                    .get(&Arc::from("code"))
                    .or_else(|| l_obj.get(&Arc::from("unit")))
            } else {
                l_obj
                    .get(&Arc::from("unit"))
                    .or_else(|| l_obj.get(&Arc::from("code")))
            };

            if let (Some(l_val), Some(l_unit)) = (l_obj.get(&Arc::from("value")), unit_or_code) {
                if let (Some(l_val_item), Some(l_unit_item)) =
                    (l_val.iter().next(), l_unit.iter().next())
                {
                    let lv = match l_val_item.data() {
                        ValueData::Decimal(d) => *d,
                        ValueData::Integer(i) => Decimal::from(*i),
                        _ => return Ok(None),
                    };
                    let lu = match l_unit_item.data() {
                        ValueData::String(s) => s.as_ref(),
                        _ => return Ok(None),
                    };
                    // Recursively compare as Quantity vs Quantity
                    return compare_values(
                        &Value::quantity(lv, Arc::from(lu)),
                        &Value::quantity(*rv, ru.clone()),
                        op,
                    );
                }
            }
            Ok(None)
        }
        (
            ValueData::Quantity {
                value: lv,
                unit: lu,
            },
            ValueData::Object(r_obj),
        ) => {
            // Handle System Quantity vs FHIR Quantity object comparison
            let system = r_obj
                .get(&Arc::from("system"))
                .and_then(|c| c.iter().next())
                .and_then(|v| match v.data() {
                    ValueData::String(s) => Some(s.as_ref()),
                    _ => None,
                });

            let unit_or_code = if system == Some("http://unitsofmeasure.org") {
                r_obj
                    .get(&Arc::from("code"))
                    .or_else(|| r_obj.get(&Arc::from("unit")))
            } else {
                r_obj
                    .get(&Arc::from("unit"))
                    .or_else(|| r_obj.get(&Arc::from("code")))
            };

            if let (Some(r_val), Some(r_unit)) = (r_obj.get(&Arc::from("value")), unit_or_code) {
                if let (Some(r_val_item), Some(r_unit_item)) =
                    (r_val.iter().next(), r_unit.iter().next())
                {
                    let rv = match r_val_item.data() {
                        ValueData::Decimal(d) => *d,
                        ValueData::Integer(i) => Decimal::from(*i),
                        _ => return Ok(None),
                    };
                    let ru = match r_unit_item.data() {
                        ValueData::String(s) => s.as_ref(),
                        _ => return Ok(None),
                    };
                    // Recursively compare as Quantity vs Quantity
                    return compare_values(
                        &Value::quantity(*lv, lu.clone()),
                        &Value::quantity(rv, Arc::from(ru)),
                        op,
                    );
                }
            }
            Ok(None)
        }
        // Date vs Time or DateTime vs Time are incomparable for ordering
        (
            ValueData::Date { .. },
            ValueData::Time {
                value: _,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: _,
                precision: _,
            },
            ValueData::Date { .. },
        )
        | (
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
            ValueData::Time {
                value: _,
                precision: _,
            },
        )
        | (
            ValueData::Time {
                value: _,
                precision: _,
            },
            ValueData::DateTime {
                value: _,
                precision: _,
                timezone_offset: _,
            },
        ) => Ok(None),
        // String to Date/DateTime implicit conversion
        (
            ValueData::String(s),
            ValueData::Date {
                value: r,
                precision: r_prec,
            },
        ) => {
            // Try to parse string as date
            use chrono::NaiveDate;
            let s = s.as_ref();
            let (parsed, parsed_prec) = match s.len() {
                4 => (
                    NaiveDate::parse_from_str(&format!("{}-01-01", s), "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Year),
                ),
                7 => (
                    NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Month),
                ),
                10 => (
                    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Day),
                ),
                _ => (None, None),
            };
            match (parsed, parsed_prec) {
                (Some(date), Some(p)) if r_prec.is_compatible_with(p) => Ok(Some(op(date.cmp(r)))),
                _ => Ok(None),
            }
        }
        (
            ValueData::Date {
                value: l,
                precision: l_prec,
            },
            ValueData::String(s),
        ) => {
            // Try to parse string as date
            use chrono::NaiveDate;
            let s = s.as_ref();
            let (parsed, parsed_prec) = match s.len() {
                4 => (
                    NaiveDate::parse_from_str(&format!("{}-01-01", s), "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Year),
                ),
                7 => (
                    NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Month),
                ),
                10 => (
                    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok(),
                    Some(crate::value::DatePrecision::Day),
                ),
                _ => (None, None),
            };
            match (parsed, parsed_prec) {
                (Some(date), Some(p)) if l_prec.is_compatible_with(p) => Ok(Some(op(l.cmp(&date)))),
                _ => Ok(None),
            }
        }
        (
            ValueData::String(s),
            ValueData::DateTime {
                value: r,
                precision: _,
                timezone_offset: _,
            },
        ) => {
            use chrono::{DateTime, NaiveDate};
            if let Ok(dt) = DateTime::parse_from_rfc3339(s.as_ref()) {
                let dt_utc = dt.with_timezone(&chrono::Utc);
                return Ok(Some(op(dt_utc.cmp(r))));
            }
            if let Ok(date) = NaiveDate::parse_from_str(s.as_ref(), "%Y-%m-%d") {
                let ord = date.cmp(&r.date_naive());
                return Ok(Some(op(ord)));
            }
            Ok(None)
        }
        (
            ValueData::DateTime {
                value: l,
                precision: _,
                timezone_offset: _,
            },
            ValueData::String(s),
        ) => {
            use chrono::{DateTime, NaiveDate};
            if let Ok(dt) = DateTime::parse_from_rfc3339(s.as_ref()) {
                let dt_utc = dt.with_timezone(&chrono::Utc);
                return Ok(Some(op(l.cmp(&dt_utc))));
            }
            if let Ok(date) = NaiveDate::parse_from_str(s.as_ref(), "%Y-%m-%d") {
                let ord = l.date_naive().cmp(&date);
                return Ok(Some(op(ord)));
            }
            Ok(None)
        }
        (
            ValueData::String(s),
            ValueData::Time {
                value: t,
                precision: _,
            },
        ) => {
            use chrono::NaiveTime;
            if let Ok(parsed) = NaiveTime::parse_from_str(s.as_ref(), "%H:%M:%S").or_else(|_| {
                NaiveTime::parse_from_str(s.as_ref(), "%H:%M:%S%.f")
                    .or_else(|_| NaiveTime::parse_from_str(s.as_ref(), "%H:%M"))
            }) {
                Ok(Some(op(parsed.cmp(t))))
            } else {
                Ok(None)
            }
        }
        (
            ValueData::Time {
                value: t,
                precision: _,
            },
            ValueData::String(s),
        ) => {
            use chrono::NaiveTime;
            if let Ok(parsed) = NaiveTime::parse_from_str(s.as_ref(), "%H:%M:%S").or_else(|_| {
                NaiveTime::parse_from_str(s.as_ref(), "%H:%M:%S%.f")
                    .or_else(|_| NaiveTime::parse_from_str(s.as_ref(), "%H:%M"))
            }) {
                Ok(Some(op(t.cmp(&parsed))))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

// ============================================
// Boolean Operations
// ============================================

fn boolean_and(left: Collection, right: Collection) -> Result<Collection> {
    // Short-circuit: if left is false, return false without evaluating right
    if !left.is_empty() && left.len() == 1 {
        if let Ok(false) = left.as_boolean() {
            return Ok(Collection::singleton(Value::boolean(false)));
        }
    }

    // Get boolean values (None for empty, Some(bool) for non-empty)
    let left_bool = if left.is_empty() {
        None
    } else {
        Some(left.as_boolean().unwrap_or(true)) // Non-empty non-boolean = true
    };

    let right_bool = if right.is_empty() {
        None
    } else {
        Some(right.as_boolean().unwrap_or(true)) // Non-empty non-boolean = true
    };

    // Apply FHIRPath and logic per spec:
    // {} and true = {}
    // {} and false = false
    // {} and {} = {}
    // true and {} = {}
    // false and {} = false
    match (left_bool, right_bool) {
        (None, Some(false)) => Ok(Collection::singleton(Value::boolean(false))), // {} and false = false
        (None, Some(true)) => Ok(Collection::empty()),                           // {} and true = {}
        (None, None) => Ok(Collection::empty()),                                 // {} and {} = {}
        (Some(false), None) => Ok(Collection::singleton(Value::boolean(false))), // false and {} = false
        (Some(true), None) => Ok(Collection::empty()),                           // true and {} = {}
        (Some(l), Some(r)) => Ok(Collection::singleton(Value::boolean(l && r))),
    }
}

fn boolean_or(left: Collection, right: Collection) -> Result<Collection> {
    // Short-circuit: if left is true, return true
    if !left.is_empty() && left.len() == 1 {
        if let Ok(true) = left.as_boolean() {
            return Ok(Collection::singleton(Value::boolean(true)));
        }
    }

    // Get boolean values (None for empty, Some(bool) for non-empty)
    let left_bool = if left.is_empty() {
        None
    } else {
        Some(left.as_boolean().unwrap_or(true)) // Non-empty non-boolean = true
    };

    let right_bool = if right.is_empty() {
        None
    } else {
        Some(right.as_boolean().unwrap_or(true)) // Non-empty non-boolean = true
    };

    // Apply FHIRPath or logic per spec:
    // {} or true = true
    // {} or false = {}
    // {} or {} = {}
    // false or {} = {}
    // true or {} = true
    match (left_bool, right_bool) {
        (None, Some(true)) => Ok(Collection::singleton(Value::boolean(true))), // {} or true = true
        (None, Some(false)) => Ok(Collection::empty()),                        // {} or false = {}
        (None, None) => Ok(Collection::empty()),                               // {} or {} = {}
        (Some(false), None) => Ok(Collection::empty()),                        // false or {} = {}
        (Some(true), None) => Ok(Collection::singleton(Value::boolean(true))), // true or {} = true
        (Some(l), Some(r)) => Ok(Collection::singleton(Value::boolean(l || r))),
    }
}

fn boolean_xor(left: Collection, right: Collection) -> Result<Collection> {
    if left.is_empty() || right.is_empty() {
        return Ok(Collection::empty());
    }

    let left_bool = left.as_boolean().unwrap_or(false);
    let right_bool = right.as_boolean().unwrap_or(false);

    Ok(Collection::singleton(Value::boolean(
        left_bool ^ right_bool,
    )))
}

fn boolean_implies(left: Collection, right: Collection) -> Result<Collection> {
    // Get boolean values (None for empty, Some(bool) for non-empty)
    let left_bool = if left.is_empty() {
        None
    } else {
        Some(left.as_boolean().unwrap_or(true)) // Non-empty non-boolean = true
    };

    let right_bool = if right.is_empty() {
        None
    } else {
        Some(right.as_boolean().unwrap_or(true)) // Non-empty non-boolean = true
    };

    // Apply FHIRPath implies logic per spec:
    // {} implies true = true
    // {} implies false = {}
    // {} implies {} = {}
    // false implies X = true
    // true implies {} = {}
    // true implies true = true
    // true implies false = false
    match (left_bool, right_bool) {
        (None, Some(true)) => Ok(Collection::singleton(Value::boolean(true))), // {} implies true = true
        (None, Some(false)) => Ok(Collection::empty()), // {} implies false = {}
        (None, None) => Ok(Collection::empty()),        // {} implies {} = {}
        (Some(false), _) => Ok(Collection::singleton(Value::boolean(true))), // false implies X = true
        (Some(true), None) => Ok(Collection::empty()),                       // true implies {} = {}
        (Some(true), Some(r)) => Ok(Collection::singleton(Value::boolean(r))), // true implies r = r
    }
}

// ============================================
// Collection Operations
// ============================================

fn union(left: Collection, right: Collection) -> Result<Collection> {
    // Binary `|` operator: set union (deduplicated) preserving left-to-right order.
    use std::collections::HashSet;

    // Short-circuit: if either side is empty, return the other
    if left.is_empty() {
        return Ok(right);
    }
    if right.is_empty() {
        return Ok(left);
    }

    // Use HashSet for O(1) lookups instead of O(n) iteration
    let mut seen = HashSet::with_capacity(left.len() + right.len());
    let mut result = Collection::with_capacity(left.len() + right.len());

    for item in left.iter().chain(right.iter()) {
        // Try to insert into HashSet first
        // Only add to result if not seen before
        if seen.insert(item.clone()) {
            result.push(item.clone());
        }
    }

    Ok(result)
}

fn membership_in(left: Collection, right: Collection) -> Result<Collection> {
    use std::collections::HashSet;

    // If left is empty, return empty (empty in collection is empty)
    if left.is_empty() {
        return Ok(Collection::empty());
    }

    // If right is empty, nothing is in it - return false
    if right.is_empty() {
        return Ok(Collection::singleton(Value::boolean(false)));
    }

    if left.len() != 1 {
        return Err(Error::TypeError(
            "'in' requires singleton left operand".into(),
        ));
    }

    let left_val = left.iter().next().unwrap();

    // Build HashSet from right for O(1) lookup
    let right_set: HashSet<&Value> = right.iter().collect();

    // Check if left_val is in right collection
    let found = right_set.contains(left_val);

    Ok(Collection::singleton(Value::boolean(found)))
}

fn membership_contains(left: Collection, right: Collection) -> Result<Collection> {
    // 'contains' is the reverse of 'in'
    membership_in(right, left)
}

// ============================================
// String Operations
// ============================================

fn concatenate(left: Collection, right: Collection) -> Result<Collection> {
    // Per FHIRPath spec: if either operand is empty, return empty
    // But testConcatenate2 expects: '1' & {} = '1'
    // This suggests empty should be treated as empty string for concatenation
    // Actually, let's check the spec behavior: empty collection concatenation returns empty
    // But the test expects '1' & {} = '1', which suggests {} is treated as empty string

    // Handle empty collections as empty strings for concatenation
    let left_str = if left.is_empty() {
        Arc::from("")
    } else {
        left.as_string()?
    };

    let right_str = if right.is_empty() {
        Arc::from("")
    } else {
        right.as_string()?
    };

    let result = format!("{}{}", left_str, right_str);
    Ok(Collection::singleton(Value::string(result)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};

    #[test]
    fn date_vs_datetime_with_time_component_is_incomparable() {
        let date = Value::date(NaiveDate::from_ymd_opt(2020, 1, 1).unwrap());
        let datetime = Value::datetime(
            Utc.with_ymd_and_hms(2020, 1, 1, 1, 0, 0)
                .single()
                .expect("valid datetime"),
        );

        let result = execute_binary_op(
            HirBinaryOperator::Lt,
            Collection::singleton(date),
            Collection::singleton(datetime),
        )
        .unwrap();

        assert!(
            result.is_empty(),
            "date vs datetime with time precision should be incomparable"
        );
    }
}
