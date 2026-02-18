//! Boolean logic functions for FHIRPath.
//!
//! This module implements boolean operations like `not()` and type casting with `as()`.

use crate::context::Context;
use crate::error::{Error, Result};
use crate::value::{Collection, Value};
use ferrum_context::FhirContext;

use super::type_helpers::{matches_type_specifier_exact, validate_type_specifier};

pub fn not(collection: Collection) -> Result<Collection> {
    if collection.is_empty() {
        return Ok(Collection::empty());
    }

    if collection.len() > 1 {
        return Err(Error::TypeError(
            "not() requires singleton or empty collection".into(),
        ));
    }

    // Per FHIRPath spec: singleton collections evaluate to true when boolean is expected
    // Try to get as boolean, or treat singleton as true
    let bool_val = collection.as_boolean().unwrap_or(true);
    Ok(Collection::singleton(Value::boolean(!bool_val)))
}

pub fn as_type(
    collection: Collection,
    type_arg: Option<&Collection>,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
) -> Result<Collection> {
    if collection.is_empty() {
        return Ok(Collection::empty());
    }

    // Extract type specifier from argument
    let type_spec = match type_arg {
        Some(arg) => {
            if arg.is_empty() {
                return Ok(Collection::empty());
            }
            if arg.len() > 1 {
                return Err(Error::TypeError(
                    "as() type specifier must be singleton".into(),
                ));
            }
            // Type specifier should be a string
            arg.as_string()
                .map_err(|_| Error::TypeError("as() type specifier must be a string".into()))?
        }
        None => {
            return Err(Error::InvalidOperation(
                "as() requires 1 argument (type specifier)".into(),
            ));
        }
    };

    validate_type_specifier(type_spec.as_ref(), fhir_context)?;

    // Per FHIRPath spec, as() operates on a single-item collection.
    // If the collection has more than one item, return empty (not an error).
    if collection.len() > 1 {
        return Ok(Collection::empty());
    }

    let item = collection.iter().next().unwrap();
    if matches_type_specifier_exact(item, type_spec.as_ref(), path_hint, fhir_context, ctx) {
        Ok(Collection::singleton(item.clone()))
    } else {
        Ok(Collection::empty())
    }
}
