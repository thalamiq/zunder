//! Subsetting functions for FHIRPath.
//!
//! This module implements functions that extract subsets from collections like
//! `single()`, `first()`, `last()`, `tail()`, `skip()`, `take()`, `intersect()`, `exclude()`.

use crate::error::{Error, Result};
use crate::value::{Collection, Value};

pub fn single(collection: Collection) -> Result<Collection> {
    if collection.len() == 1 {
        Ok(collection)
    } else if collection.is_empty() {
        Ok(Collection::empty())
    } else {
        Err(Error::TypeError(format!(
            "single() requires singleton collection, got {} items",
            collection.len()
        )))
    }
}

pub fn first(collection: Collection) -> Result<Collection> {
    if collection.is_empty() {
        return Ok(Collection::empty());
    }

    let first_item = collection.iter().next().unwrap();
    Ok(Collection::singleton(first_item.clone()))
}

pub fn last(collection: Collection) -> Result<Collection> {
    if collection.is_empty() {
        return Ok(Collection::empty());
    }

    // Collect into Vec to get owned values
    let items: Vec<Value> = collection.iter().cloned().collect();
    let last_item = items.last().unwrap().clone();
    Ok(Collection::singleton(last_item))
}

pub fn tail(collection: Collection) -> Result<Collection> {
    if collection.is_empty() {
        return Ok(Collection::empty());
    }

    let mut result = Collection::empty();
    let mut iter = collection.iter();
    iter.next(); // Skip first

    for item in iter {
        result.push(item.clone());
    }

    Ok(result)
}

pub fn skip(collection: Collection, count_arg: Option<&Collection>) -> Result<Collection> {
    let count = count_arg
        .ok_or_else(|| Error::InvalidOperation("skip() requires 1 argument".into()))?
        .as_integer()?;

    if count < 0 {
        return Err(Error::InvalidOperation(
            "skip() count must be non-negative".into(),
        ));
    }

    let mut result = Collection::empty();
    let mut iter = collection.iter();

    // Skip first count items
    for _ in 0..count {
        if iter.next().is_none() {
            break;
        }
    }

    // Add remaining items
    for item in iter {
        result.push(item.clone());
    }

    Ok(result)
}

pub fn take(collection: Collection, count_arg: Option<&Collection>) -> Result<Collection> {
    let count = count_arg
        .ok_or_else(|| Error::InvalidOperation("take() requires 1 argument".into()))?
        .as_integer()?;

    if count < 0 {
        return Err(Error::InvalidOperation(
            "take() count must be non-negative".into(),
        ));
    }

    let mut result = Collection::empty();
    let mut iter = collection.iter();

    // Take first count items
    for _ in 0..count {
        if let Some(item) = iter.next() {
            result.push(item.clone());
        } else {
            break;
        }
    }

    Ok(result)
}

pub fn intersect(collection: Collection, other: Option<&Collection>) -> Result<Collection> {
    use std::collections::HashSet;

    let other =
        other.ok_or_else(|| Error::InvalidOperation("intersect() requires 1 argument".into()))?;

    // Short-circuit: if either side is empty, intersection is empty
    if collection.is_empty() || other.is_empty() {
        return Ok(Collection::empty());
    }

    // Build HashSet from other collection for O(1) lookups
    let other_set: HashSet<&Value> = other.iter().collect();

    let mut seen = HashSet::with_capacity(collection.len().min(other.len()));
    let mut result = Collection::empty();

    for item in collection.iter() {
        if other_set.contains(item) && seen.insert(item) {
            result.push(item.clone());
        }
    }

    Ok(result)
}

pub fn exclude(collection: Collection, other: Option<&Collection>) -> Result<Collection> {
    use std::collections::HashSet;

    let other =
        other.ok_or_else(|| Error::InvalidOperation("exclude() requires 1 argument".into()))?;

    // Short-circuit: if collection is empty, result is empty
    if collection.is_empty() {
        return Ok(Collection::empty());
    }
    // Short-circuit: if other is empty, return collection
    if other.is_empty() {
        return Ok(collection);
    }

    // Build HashSet from other collection for O(1) lookups
    let other_set: HashSet<&Value> = other.iter().collect();

    let mut result = Collection::empty();

    // Per FHIRPath spec, exclude does NOT deduplicate — it only removes
    // items present in other, preserving duplicates in the source collection.
    for item in collection.iter() {
        if !other_set.contains(item) {
            result.push(item.clone());
        }
    }

    Ok(result)
}
