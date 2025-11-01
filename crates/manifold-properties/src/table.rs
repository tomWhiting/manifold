//! PropertyTable for CRUD operations with composite keys.
//!
//! This module provides the main API for property storage and retrieval.
//!
//! IMPLEMENTATION REQUIRED:
//! - PropertyTable struct wrapping Manifold table with composite key (Uuid, &str)
//! - PropertyTableRead for read-only access with zero-copy guards
//! - PropertyGuard for zero-copy access to property values
//! - Methods: set(), get(), get_all(), delete(), batch_set()
//! - Integration with Manifold transactions

// Implementation goes here
