//! Type checking functions for FHIRPath.
//!
//! This module implements the `is()` function for type checking.

use crate::context::Context;
use crate::error::{Error, Result};
use crate::value::{Collection, Value};
use ferrum_context::FhirContext;

use super::type_helpers::{matches_type_specifier, validate_type_specifier};

pub fn is_type(
    collection: Collection,
    type_arg: Option<&Collection>,
    path_hint: Option<&str>,
    fhir_context: Option<&dyn FhirContext>,
    ctx: &Context,
) -> Result<Collection> {
    if collection.is_empty() {
        return Ok(Collection::empty());
    }

    let type_spec = match type_arg {
        Some(arg) => {
            if arg.is_empty() {
                return Ok(Collection::empty());
            }
            if arg.len() > 1 {
                return Err(Error::TypeError(
                    "is() type specifier must be singleton".into(),
                ));
            }
            arg.as_string()
                .map_err(|_| Error::TypeError("is() type specifier must be a string".into()))?
        }
        None => {
            return Err(Error::InvalidOperation(
                "is() requires 1 argument (type specifier)".into(),
            ));
        }
    };

    // Per FHIRPath spec, is() with an unknown type returns false (not an error).
    if validate_type_specifier(type_spec.as_ref(), fhir_context).is_err() {
        return Ok(Collection::singleton(Value::boolean(false)));
    }

    if collection.len() > 1 {
        return Err(Error::TypeError(
            "is() requires singleton collection".into(),
        ));
    }

    let item = collection.iter().next().unwrap();
    let matches = matches_type_specifier(item, type_spec.as_ref(), path_hint, fhir_context, ctx);
    Ok(Collection::singleton(Value::boolean(matches)))
}
