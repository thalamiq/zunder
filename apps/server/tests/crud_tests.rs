#![allow(unused)]
//! Integration tests for CRUD operations
//!
//! This module organizes tests by FHIR CRUD operations according to the spec:
//! - CREATE (POST /{resourceType})
//! - READ (GET /{resourceType}/{id})
//! - UPDATE (PUT /{resourceType}/{id})
//! - DELETE (DELETE /{resourceType}/{id})
//!
//! Each operation has its own submodule with comprehensive tests.

mod crud;
mod support;
