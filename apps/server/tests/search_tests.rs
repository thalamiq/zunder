#![allow(unused)]
//! FHIR Search Tests
//!
//! Comprehensive tests for FHIR search functionality organized by the spec:
//! - Parameter types (token, string, reference, date, number, quantity, uri)
//! - Modifiers (:missing, :exact, :contains, :text, :not, :above, :below, etc.)
//! - Result parameters (_sort, _count, _include, _revinclude, _summary, _elements)
//! - Prefixes (eq, ne, gt, ge, lt, le, sa, eb, ap)
//! - Advanced features (chaining, composite parameters)
//!
//! Note: Background workers are disabled in tests, but indexing runs inline via the
//! test job queue, so tests should not write to `search_*` tables directly.

mod search;
mod support;
